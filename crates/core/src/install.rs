//! Erase and install: the release image (the port's
//! tools/build-release-image.sh) put on a phone that runs the port - its
//! userdata made anew, the image written in checked parts, the owner's ssh
//! key put in so Cradle can reach it, and its first start awaited (it grows
//! to fill userdata by itself). What userdata held goes: the whole system is
//! backed up first, unless a fresh backup is here already.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A release image on this computer.
#[derive(Debug, Clone)]
pub struct Release {
    pub dir: PathBuf,
    pub name: String,
    pub adaptation: String,
    pub item: String,
    /// The image's size and sha256, decompressed.
    pub size: u64,
    pub sha256: String,
    pub compressed: PathBuf,
}

/// Where release images are looked for: Cradle's own folder, and the port's
/// out/release when its tree is here.
pub fn places() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let mut dirs = vec![home.join(".local/share/cradle/releases")];
    if let Some(port) = crate::flash::port_tree() {
        dirs.push(port.join("out/release"));
    }
    dirs
}

/// The release images here, newest last.
pub fn releases() -> Vec<Release> {
    let mut found: Vec<Release> = places()
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .flatten()
        .filter_map(|e| read(&e.path()).ok())
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

pub fn read(dir: &Path) -> Result<Release, String> {
    let text = std::fs::read_to_string(dir.join("manifest.json")).map_err(|_| format!("{}: no manifest.json", dir.display()))?;
    let m: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", dir.display()))?;
    let s = |v: &serde_json::Value| v.as_str().unwrap_or("").to_owned();
    let compressed = dir.join(m["compressed"]["file"].as_str().unwrap_or("rootfs.img.zst"));
    if !compressed.exists() {
        return Err(format!("{}: the image is missing", dir.display()));
    }
    Ok(Release {
        dir: dir.to_owned(),
        name: s(&m["name"]),
        adaptation: s(&m["adaptation"]),
        item: s(&m["item"]),
        size: m["image"]["size"].as_u64().unwrap_or(0),
        sha256: s(&m["image"]["sha256"]),
        compressed,
    })
}

/// The key Cradle reaches phones with.
pub fn public_key() -> Result<String, String> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let candidates = std::env::var_os("SFDUO_PUBKEY").map(PathBuf::from).into_iter().chain(["id_ed25519.pub", "id_ecdsa.pub", "id_rsa.pub"].iter().map(|n| home.join(".ssh").join(n)));
    for c in candidates {
        if let Ok(k) = std::fs::read_to_string(&c) {
            let k = k.trim().to_owned();
            if k.starts_with("ssh-") || k.starts_with("ecdsa-") {
                return Ok(k);
            }
        }
    }
    Err("no ssh public key in ~/.ssh: Cradle could not reach the phone after installing (ssh-keygen -t ed25519 makes one)".into())
}

/// Erase and install, from Linux. `confirm` must be the phone's word
/// (android::confirm_word).
pub fn erase_and_install(host: &str, release: &Release, confirm: &str, say: crate::ramboot::Say) -> Result<(), String> {
    use crate::full::adb_shell;
    let serial = crate::backup::serial(host)?;
    if confirm.trim() != crate::android::confirm_word(&serial) {
        return Err(format!("the confirmation does not match ({} expected) - nothing is changed", crate::android::confirm_word(&serial)));
    }
    let key = public_key()?;
    let twrp = crate::full::twrp().ok_or("no TWRP image")?;
    crate::ramboot::check_image(&twrp)?;
    // The image checked here before anything: decompressed, its sha.
    say(format!("checking the image {} here", release.name));
    let (size, sha) = hash_stream(&mut decompress(&release.compressed)?)?;
    if sha != release.sha256 || (release.size > 0 && size != release.size) {
        return Err("the release image does not match its manifest - nothing is changed".into());
    }

    let full = match crate::android::fresh_full(host, &serial)? {
        Some(b) => {
            say(format!("the whole system was backed up {} - fresh", b.manifest.created));
            b
        }
        None => {
            say("taking the whole system's backup first".into());
            crate::full::take(host, say)?
        }
    };
    crate::flash::log(&serial, &format!("erase and install {} begun: the system backed up in {} - cradle", release.name, full.dir.display()))?;

    say("into TWRP".into());
    crate::ramboot::ram_boot(host, &twrp, crate::ramboot::Expect::Recovery, say)?;
    if !crate::full::fast_tools(&serial) {
        let _ = crate::ramboot::leave_recovery(host, &serial, say);
        return Err("this TWRP lacks pigz or nc".into());
    }
    say("trying the way in (512 MB onto the phone, checked)".into());
    if let Err(e) = crate::android::try_the_way_back(&serial) {
        let _ = crate::ramboot::leave_recovery(host, &serial, say);
        return Err(format!("{e} - nothing is erased"));
    }

    crate::flash::log(&serial, &format!("ERASING userdata for {} - cradle", release.name))?;
    say("making userdata anew (what it held goes)".into());
    let blk = crate::android::BLK;
    adb_shell(&serial, &format!("umount /tmp/ud 2>/dev/null; mke2fs -F -t ext4 -L userdata {blk}/userdata >/dev/null && mkdir -p /tmp/ud && mount -t ext4 {blk}/userdata /tmp/ud && echo ok"))?;

    say(format!("putting {} on the phone", release.name));
    let (written, written_sha) = crate::android::put_file(&serial, "rootfs.img", &mut decompress(&release.compressed)?, "image", say)?;
    if written_sha != release.sha256 || written != size {
        return Err("the image read here changed while it was sent - stopping in TWRP; run it again".into());
    }
    adb_shell(&serial, "chmod 644 /tmp/ud/rootfs.img")?;
    say("checking the whole image on the phone".into());
    let there = adb_shell(&serial, "sha256sum /tmp/ud/rootfs.img | cut -d' ' -f1")?;
    if there.trim() != release.sha256 {
        return Err("the image on the phone differs from the release - stopping in TWRP; run it again".into());
    }

    say("putting this computer's ssh key in".into());
    adb_shell(&serial, "mkdir -p /tmp/r && mount -o loop,rw /tmp/ud/rootfs.img /tmp/r && echo ok")?;
    let put_key = (|| -> Result<(), String> {
        crate::full::adb_send(&serial, "cat > /tmp/cradle.pub", &mut key.as_bytes())?;
        adb_shell(
            &serial,
            "for d in /tmp/r/root /tmp/r/home/droidian; do mkdir -p $d/.ssh && cat /tmp/cradle.pub > $d/.ssh/authorized_keys && chmod 700 $d/.ssh && chmod 600 $d/.ssh/authorized_keys; done; \
             chown -R 0:0 /tmp/r/root/.ssh; chown -R 32011:32011 /tmp/r/home/droidian/.ssh; rm -f /tmp/cradle.pub; \
             grep -q ssh- /tmp/r/root/.ssh/authorized_keys && echo ok",
        )
        .map(|_| ())
    })();
    adb_shell(&serial, "sync; umount /tmp/r; sync; umount /tmp/ud").ok();
    put_key?;

    say("clearing misc".into());
    adb_shell(&serial, &format!("dd if=/dev/zero of={blk}/misc bs=2048 count=1 2>/dev/null; sync"))?;
    let _ = std::fs::remove_file(crate::android::guest_path_of(&serial));
    crate::flash::log(&serial, &format!("{} installed (userdata anew) - cradle", release.name))?;

    say("starting the new system - its first start grows it to fill userdata".into());
    adb_shell(&serial, "reboot").ok();
    let start = std::time::Instant::now();
    while !crate::phone::answers_fresh(host) {
        if start.elapsed() > std::time::Duration::from_secs(480) {
            return Err("the new system did not answer in 8 minutes - look at the phone's screen".into());
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    crate::ramboot::arm_brake_linux(host)?;
    crate::flash::log(&serial, "the new system answers, parking brake armed - cradle")?;
    say("the new system is up: unlock with 1234, then choose your own PIN".into());
    Ok(())
}

fn decompress(path: &Path) -> Result<impl Read, String> {
    let child = Command::new("zstd")
        .args(["-dc", "--long=27"])
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("zstd: {e} - is zstd installed?"))?;
    Ok(child.stdout.expect("piped"))
}

fn hash_stream(r: &mut dyn Read) -> Result<(u64, String), String> {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut n = 0u64;
    loop {
        let k = r.read(&mut buf).map_err(|e| e.to_string())?;
        if k == 0 {
            break;
        }
        h.update(&buf[..k]);
        n += k as u64;
    }
    Ok((n, format!("{:x}", h.finalize())))
}
