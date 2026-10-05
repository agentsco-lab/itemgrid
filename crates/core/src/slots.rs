//! The phone's two boot slots, read without touching them: Qualcomm keeps
//! their state in the GPT attributes of boot_a and boot_b (priority, active,
//! retries left, booted successfully, unbootable - as ABL reads them), and
//! what is in each is told by matching its boot partition against images
//! this computer knows (Gridbay's backups, the port's out/), or else by the
//! kernel's version inside the image.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One slot.
#[derive(Debug, Clone, Default)]
pub struct Slot {
    pub name: char,
    pub priority: u8,
    pub active: bool,
    pub retries: u8,
    pub successful: bool,
    pub unbootable: bool,
    /// The boot partition's sha256.
    pub sha256: String,
    /// A known image it holds, if one matched.
    pub image: Option<String>,
    /// The kernel's version inside the image.
    pub kernel: String,
}

const SCRIPT: &str = r#"exec python3 - <<'PY'
import os, struct, hashlib, zlib, sys
for slot in "ab":
    dev = f"/dev/disk/by-partlabel/boot_{slot}"
    part = os.path.realpath(dev)
    name = os.path.basename(part)
    disk = "/dev/" + name.rstrip("0123456789")
    num = int(name[len(os.path.basename(disk)):])
    ss = int(open(f"/sys/block/{os.path.basename(disk)}/queue/logical_block_size").read())
    with open(disk, "rb") as d:
        d.seek(ss)
        hdr = d.read(92)
        lba, count, size = struct.unpack_from("<QII", hdr, 72)
        d.seek(lba * ss + (num - 1) * size)
        attr = struct.unpack_from("<Q", d.read(size), 48)[0]
    data = open(dev, "rb").read()
    sha = hashlib.sha256(data).hexdigest()
    kernel = ""
    if data[:8] == b"ANDROID!":
        ksize, page = struct.unpack_from("<I", data, 8)[0], struct.unpack_from("<I", data, 36)[0]
        k = data[page:page + ksize]
        if k[:2] == b"\x1f\x8b":
            try:
                k = zlib.decompressobj(31).decompress(k)
            except Exception:
                pass
        i = k.find(b"Linux version ")
        if i >= 0:
            kernel = k[i + 14:k.find(b" ", i + 14)].decode(errors="replace")
    print(f"slot={slot} attr={attr} sha={sha} kernel={kernel}")
PY
"#;

/// Reads both slots on the phone at `host`.
pub fn read(host: &str) -> Result<[Slot; 2], String> {
    let text = crate::phone::run(host, SCRIPT)?;
    let mut slots: [Slot; 2] = [Slot { name: 'a', ..Default::default() }, Slot { name: 'b', ..Default::default() }];
    for line in text.lines() {
        let kv: HashMap<&str, &str> = line.split_whitespace().filter_map(|w| w.split_once('=')).collect();
        let i = match kv.get("slot") {
            Some(&"a") => 0,
            Some(&"b") => 1,
            _ => continue,
        };
        let attr: u64 = kv.get("attr").and_then(|v| v.parse().ok()).unwrap_or(0);
        let s = &mut slots[i];
        s.priority = ((attr >> 48) & 3) as u8;
        s.active = (attr >> 50) & 1 == 1;
        s.retries = ((attr >> 51) & 7) as u8;
        s.successful = (attr >> 54) & 1 == 1;
        s.unbootable = (attr >> 55) & 1 == 1;
        s.sha256 = kv.get("sha").copied().unwrap_or_default().to_owned();
        s.kernel = kv.get("kernel").copied().unwrap_or_default().to_owned();
    }
    if slots.iter().any(|s| s.sha256.is_empty()) {
        return Err("the slots were not read".into());
    }
    let known = known_images();
    for s in slots.iter_mut() {
        s.image = known.get(&s.sha256).cloned().or_else(|| match_by_prefix(host, s));
    }
    Ok(slots)
}

/// Where boot images are known from: Gridbay's backups, and the port's tree
/// (GRIDBAY_PORT, ~/.config/gridbay/port, or ~/Desktop/projects/surfaceduo/
/// surfaceduo-droidian) - its out/ and out/backups/.
fn image_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![crate::backup::root()];
    if let Some(port) = crate::flash::port_tree() {
        dirs.push(port.join("out"));
    }
    dirs
}

fn images() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = image_dirs();
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "img") && p.file_name().is_some_and(|n| n.to_string_lossy().contains("boot")) {
                if let Ok(m) = p.metadata() {
                    if m.len() <= 128 << 20 {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

/// What the partitions' hashes are known to be: worked out once and kept
/// (~/.cache/gridbay/slots.tsv: partition hash, image).
fn cache_path() -> PathBuf {
    Path::new(&std::env::var("HOME").unwrap_or_default()).join(".cache/gridbay/slots.tsv")
}

fn known_images() -> HashMap<String, String> {
    std::fs::read_to_string(cache_path())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\t').map(|(a, b)| (a.to_owned(), b.to_owned())))
        .collect()
}

fn remember(sha: &str, image: &str) {
    let path = cache_path();
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    text.push_str(&format!("{sha}\t{image}\n"));
    let _ = std::fs::write(path, text);
}

/// An image shorter than the partition is in it if the partition starts
/// with it: the phone hashes its first bytes for each length known here.
fn match_by_prefix(host: &str, slot: &Slot) -> Option<String> {
    use sha2::{Digest, Sha256};
    let mut by_len: HashMap<u64, Vec<(String, PathBuf)>> = HashMap::new();
    for p in images() {
        let Ok(bytes) = std::fs::read(&p) else { continue };
        by_len.entry(bytes.len() as u64).or_default().push((format!("{:x}", Sha256::digest(&bytes)), p));
    }
    let lens: Vec<u64> = by_len.keys().copied().collect();
    if lens.is_empty() {
        return None;
    }
    let script: String = lens.iter().map(|n| format!("echo \"{n} $(head -c {n} /dev/disk/by-partlabel/boot_{} | sha256sum | cut -d' ' -f1)\"\n", slot.name)).collect();
    let text = crate::phone::run(host, &script).ok()?;
    // The shortest match: the image as built, not a whole partition's copy
    // (which says nothing of what is in it).
    let mut best: Option<(u64, String)> = None;
    for line in text.lines() {
        let Some((n, sha)) = line.split_once(' ') else { continue };
        let Ok(n) = n.parse::<u64>() else { continue };
        if let Some((_, path)) = by_len.get(&n).and_then(|v| v.iter().find(|(s, _)| s == sha)) {
            if best.as_ref().is_none_or(|(m, _)| n < *m) {
                best = Some((n, path.file_name()?.to_string_lossy().into_owned()));
            }
        }
    }
    let (_, name) = best?;
    remember(&slot.sha256, &name);
    Some(name)
}
