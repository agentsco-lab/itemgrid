//! Microsoft's own package for the Duo (a full A/B OTA: payload.bin in a
//! signed zip, from support.microsoft.com/surface-recovery-image): read, and
//! the partitions item/grid needs taken out of it - boot, dtbo, vbmeta - each
//! checked against the hash the package gives. The stock kernel then comes
//! from Microsoft, not from anyone's backup. The package as a whole is
//! applied only by the stock recovery (sideload), never written by item/grid.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Where packages are kept, and what is taken out of them.
pub fn packages_dir() -> PathBuf {
    crate::paths::cache().join("stock")
}

pub fn extracted_dir() -> PathBuf {
    crate::paths::data().join("stock")
}

/// A package on this computer.
#[derive(Debug, Clone)]
pub struct Package {
    pub path: PathBuf,
    /// "2022.902.48".
    pub build: String,
    /// The fingerprints it installs ("a|b" for the carrier and unlocked).
    pub fingerprint: String,
    /// When it was built (UTC seconds).
    pub timestamp: i64,
    pub security_patch: String,
}

/// The packages here, newest build last.
pub fn packages() -> Vec<Package> {
    let Ok(dir) = std::fs::read_dir(packages_dir()) else { return Vec::new() };
    let mut found: Vec<Package> = dir.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "zip")).filter_map(|e| read(&e.path()).ok()).collect();
    found.sort_by_key(|p| p.timestamp);
    found
}

/// A package's facts, from its metadata.
pub fn read(path: &Path) -> Result<Package, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{}: not a zip ({e})", path.display()))?;
    let mut meta = String::new();
    zip.by_name("META-INF/com/android/metadata").map_err(|_| format!("{}: not an Android OTA package", path.display()))?.read_to_string(&mut meta).map_err(|e| e.to_string())?;
    let get = |key: &str| meta.lines().find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('='))).unwrap_or("").to_owned();
    if get("ota-type") != "AB" || get("pre-device") != "duo" {
        return Err(format!("{}: not a Surface Duo A/B package", path.display()));
    }
    let fingerprint = get("post-build");
    let build = fingerprint.split('/').nth(3).unwrap_or("").split(':').next().unwrap_or("").to_owned();
    Ok(Package { path: path.to_owned(), build, fingerprint, timestamp: get("post-timestamp").parse().unwrap_or(0), security_patch: get("post-security-patch-level") })
}

// ---- payload.bin ------------------------------------------------------------

struct Op {
    kind: u64,
    data_offset: u64,
    data_length: u64,
    /// (start block, blocks)
    dst: Vec<(u64, u64)>,
}

struct Partition {
    name: String,
    size: u64,
    hash: Vec<u8>,
    ops: Vec<Op>,
}

struct Payload {
    file: std::fs::File,
    /// Where the data blobs begin, in the zip file.
    data_start: u64,
    block_size: u64,
    partitions: Vec<Partition>,
}

fn varint(b: &[u8], i: &mut usize) -> Result<u64, String> {
    let (mut r, mut s) = (0u64, 0);
    loop {
        let c = *b.get(*i).ok_or("the package's manifest ends early")?;
        *i += 1;
        r |= ((c & 0x7f) as u64) << s;
        s += 7;
        if c < 0x80 {
            return Ok(r);
        }
        if s > 63 {
            return Err("the package's manifest is malformed".into());
        }
    }
}

/// A protobuf message's fields: (number, varint value, bytes).
fn fields(b: &[u8]) -> Result<Vec<(u64, u64, &[u8])>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let key = varint(b, &mut i)?;
        let (num, wire) = (key >> 3, key & 7);
        match wire {
            0 => out.push((num, varint(b, &mut i)?, &b[0..0])),
            1 => {
                i += 8;
                out.push((num, 0, &b[0..0]));
            }
            2 => {
                let len = varint(b, &mut i)? as usize;
                let end = i.checked_add(len).filter(|e| *e <= b.len()).ok_or("the package's manifest is malformed")?;
                out.push((num, 0, &b[i..end]));
                i = end;
            }
            5 => {
                i += 4;
                out.push((num, 0, &b[0..0]));
            }
            _ => return Err("the package's manifest is malformed".into()),
        }
    }
    Ok(out)
}

fn open_payload(path: &Path) -> Result<Payload, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let (start, stored) = {
        let entry = zip.by_name("payload.bin").map_err(|_| "the package has no payload.bin")?;
        (entry.data_start(), entry.compression() == zip::CompressionMethod::Stored)
    };
    if !stored {
        return Err("payload.bin is compressed in the zip: not a package as Microsoft makes them".into());
    }
    let mut file = zip.into_inner();
    file.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
    let mut head = [0u8; 24];
    file.read_exact(&mut head).map_err(|e| e.to_string())?;
    if &head[..4] != b"CrAU" || u64::from_be_bytes(head[4..12].try_into().unwrap()) != 2 {
        return Err("payload.bin is not a version 2 payload".into());
    }
    let manifest_len = u64::from_be_bytes(head[12..20].try_into().unwrap());
    let signature_len = u32::from_be_bytes(head[20..24].try_into().unwrap()) as u64;
    if manifest_len > 64 << 20 {
        return Err("payload.bin's manifest is implausibly large".into());
    }
    let mut manifest = vec![0u8; manifest_len as usize];
    file.read_exact(&mut manifest).map_err(|e| e.to_string())?;
    let mut block_size = 4096;
    let mut partitions = Vec::new();
    for (num, value, bytes) in fields(&manifest)? {
        match num {
            3 => block_size = value,
            13 => {
                let mut p = Partition { name: String::new(), size: 0, hash: Vec::new(), ops: Vec::new() };
                for (n, _, b) in fields(bytes)? {
                    match n {
                        1 => p.name = String::from_utf8_lossy(b).into_owned(),
                        7 => {
                            for (m, v, h) in fields(b)? {
                                match m {
                                    1 => p.size = v,
                                    2 => p.hash = h.to_vec(),
                                    _ => {}
                                }
                            }
                        }
                        8 => {
                            let mut op = Op { kind: 0, data_offset: 0, data_length: 0, dst: Vec::new() };
                            for (m, v, e) in fields(b)? {
                                match m {
                                    1 => op.kind = v,
                                    2 => op.data_offset = v,
                                    3 => op.data_length = v,
                                    6 => {
                                        let (mut s, mut c) = (0, 0);
                                        for (k, x, _) in fields(e)? {
                                            match k {
                                                1 => s = x,
                                                2 => c = x,
                                                _ => {}
                                            }
                                        }
                                        op.dst.push((s, c));
                                    }
                                    _ => {}
                                }
                            }
                            p.ops.push(op);
                        }
                        _ => {}
                    }
                }
                partitions.push(p);
            }
            _ => {}
        }
    }
    Ok(Payload { file, data_start: start + 24 + manifest_len + signature_len, block_size, partitions })
}

/// The partitions a package holds, with their sizes.
pub fn partitions(path: &Path) -> Result<Vec<(String, u64)>, String> {
    Ok(open_payload(path)?.partitions.iter().map(|p| (p.name.clone(), p.size)).collect())
}

/// One partition taken out of a package into `out`, its sha256 checked
/// against the package's. Full packages only: a delta's operations (copies
/// from the old partition) are refused.
pub fn extract(path: &Path, name: &str, out: &Path) -> Result<String, String> {
    let mut payload = open_payload(path)?;
    let bs = payload.block_size;
    let part = payload.partitions.iter().find(|p| p.name == name).ok_or_else(|| format!("the package has no {name}"))?;
    if part.size > 512 << 20 {
        return Err(format!("{name} is {} MB: item/grid takes out only the boot chain", part.size >> 20));
    }
    let mut image = vec![0u8; part.size as usize];
    for op in &part.ops {
        let mut data = vec![0u8; op.data_length as usize];
        if op.data_length > 0 {
            payload.file.seek(SeekFrom::Start(payload.data_start + op.data_offset)).map_err(|e| e.to_string())?;
            payload.file.read_exact(&mut data).map_err(|e| e.to_string())?;
        }
        let blocks: Vec<u8> = match op.kind {
            0 => data,
            1 => {
                let mut v = Vec::new();
                bzip2::read::BzDecoder::new(&data[..]).read_to_end(&mut v).map_err(|e| format!("{name}: bzip2: {e}"))?;
                v
            }
            8 => {
                let mut v = Vec::new();
                lzma_rs::xz_decompress(&mut &data[..], &mut v).map_err(|e| format!("{name}: xz: {e:?}"))?;
                v
            }
            // ZERO and DISCARD: the image starts zeroed.
            6 | 7 => continue,
            other => return Err(format!("{name}: operation {other} needs the old partition - not a full package")),
        };
        // The data fills the destination extents in order.
        let mut at = 0usize;
        for (start, count) in &op.dst {
            let (from, len) = ((start * bs) as usize, (count * bs) as usize);
            let end = from.checked_add(len).filter(|e| *e <= image.len()).ok_or_else(|| format!("{name}: an extent outside the partition"))?;
            let take = len.min(blocks.len().saturating_sub(at));
            image[from..from + take].copy_from_slice(&blocks[at..at + take]);
            let _ = end;
            at += len;
        }
    }
    let sha = Sha256::digest(&image);
    if !part.hash.is_empty() && sha.as_slice() != part.hash.as_slice() {
        return Err(format!("{name} taken out of the package does not match the package's hash - not kept"));
    }
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = out.with_extension("part");
    std::fs::write(&tmp, &image).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, out).map_err(|e| e.to_string())?;
    Ok(format!("{sha:x}"))
}

/// The boot chain of a package taken out, once, into
/// ~/.local/share/itemgrid/stock/<build>/ - where the return to Android finds
/// its stock kernel.
pub fn boot_chain(pkg: &Package, say: crate::ramboot::Say) -> Result<PathBuf, String> {
    let dir = extracted_dir().join(&pkg.build);
    for name in ["boot", "dtbo", "vbmeta", "vbmeta_system"] {
        let out = dir.join(format!("{name}.img"));
        if out.exists() {
            continue;
        }
        say(format!("taking {name} out of Microsoft's {} package", pkg.build));
        let sha = extract(&pkg.path, name, &out)?;
        say(format!("{name}: {} MB, sha {}…, as the package says", out.metadata().map(|m| m.len() >> 20).unwrap_or(0), &sha[..16]));
    }
    Ok(dir)
}

// ---- getting a package from Microsoft ----------------------------------------

/// Microsoft's page: signed in there, a product and a serial give a link.
pub const RECOVERY_PAGE: &str = "https://support.microsoft.com/en-us/surface-recovery-image";

/// The page's answer read for the download link and what it names
/// ("Surface Duo 128 - Android 12 - ATT - 2022.902.48").
pub fn link_in(html: &str) -> Option<(String, String)> {
    let at = html.find("https://surface.downloads.prss.microsoft.com/")?;
    let end = at + html[at..].find('"')?;
    let url = html[at..end].replace("&amp;", "&");
    // The row's product cell, before the link.
    let row = &html[..at];
    let label = row.rsplit("<td").nth(1).map(|c| strip_tags(c).trim().to_owned()).filter(|s| !s.is_empty()).unwrap_or_default();
    Some((url, label))
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in s.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out.trim_start_matches(|c: char| c != ' ' && !c.is_alphanumeric()).to_owned()
}

/// The file a link downloads to, by its name in the link.
pub fn file_for(url: &str) -> Result<PathBuf, String> {
    let name = url.split('?').next().and_then(|u| u.rsplit('/').next()).filter(|n| n.ends_with(".zip") && !n.contains(['/', '\\'])).ok_or("the link does not name a zip")?;
    Ok(packages_dir().join(name))
}

/// A package downloaded from a link (resumed if a part is here already),
/// its size checked, then read as a Duo package. `progress` hears bytes so
/// far and the whole.
pub fn download(url: &str, progress: &mut dyn FnMut(u64, u64)) -> Result<Package, String> {
    use std::io::Write;
    let path = file_for(url)?;
    if let Ok(pkg) = read(&path) {
        return Ok(pkg);
    }
    std::fs::create_dir_all(packages_dir()).map_err(|e| e.to_string())?;
    let part = path.with_extension("zip.part");
    let have = part.metadata().map(|m| m.len()).unwrap_or(0);
    let agent = ureq::Agent::config_builder().timeout_global(None).build().new_agent();
    let mut req = agent.get(url);
    if have > 0 {
        req = req.header("Range", &format!("bytes={have}-"));
    }
    let resp = req.call().map_err(|e| format!("Microsoft's server: {e} - the link may have expired (they last ten minutes); ask for it again"))?;
    let resumed = resp.status() == 206;
    let rest: u64 = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(0);
    let (mut done, whole) = if resumed { (have, have + rest) } else { (0, rest) };
    let mut file = std::fs::OpenOptions::new().create(true).write(true).append(resumed).truncate(!resumed).open(&part).map_err(|e| e.to_string())?;
    let mut body = resp.into_body().into_reader();
    let mut buf = vec![0u8; 1 << 20];
    let mut last = std::time::Instant::now();
    loop {
        let n = body.read(&mut buf).map_err(|e| format!("downloading: {e} - run it again to go on from here"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if last.elapsed().as_millis() > 300 {
            progress(done, whole);
            last = std::time::Instant::now();
        }
    }
    file.sync_all().map_err(|e| e.to_string())?;
    progress(done, whole);
    if whole > 0 && done != whole {
        return Err(format!("the download stopped at {} of {} MB - run it again to go on", done >> 20, whole >> 20));
    }
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    read(&path).map_err(|e| {
        let _ = std::fs::remove_file(&path);
        format!("what came is not a Duo package: {e}")
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_link_is_found_in_microsofts_answer() {
        let html = r#"<table><tr><td>Surface Duo 128 - Android 12 - ATT - 2022.902.48</td><td><a href="https://surface.downloads.prss.microsoft.com/dbazure/ota_b1-12-customer_att_2022.902.48.zip?t=x&amp;P1=1">Download image</a></td></tr></table>"#;
        let (url, label) = super::link_in(html).unwrap();
        assert_eq!(url, "https://surface.downloads.prss.microsoft.com/dbazure/ota_b1-12-customer_att_2022.902.48.zip?t=x&P1=1");
        assert_eq!(label, "Surface Duo 128 - Android 12 - ATT - 2022.902.48");
        assert!(super::file_for(&url).unwrap().ends_with("ota_b1-12-customer_att_2022.902.48.zip"));
    }
}
