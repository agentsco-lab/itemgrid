//! Erase and install: the release image (the port's
//! tools/build-release-image.sh) put on a phone that runs the port - its
//! userdata made anew, the image written in checked parts, the owner's ssh
//! key put in so item/grid can reach it, and its first start awaited (it grows
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
    /// The port's boot image that goes with it (a phone coming from stock
    /// Android has none), and its sha256 - none in releases made before.
    pub boot: Option<(PathBuf, String)>,
    /// The vbmeta the port runs with (verification off), written with it.
    pub vbmeta: Option<(PathBuf, String)>,
}

/// Where release images are looked for: item/grid's own folder, and the port's
/// out/release when its tree is here.
pub fn places() -> Vec<PathBuf> {
    let mut dirs = vec![crate::paths::data().join("releases")];
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
        boot: m["boot"]["file"].as_str().map(|f| dir.join(f)).filter(|f| f.exists()).map(|f| (f, s(&m["boot"]["sha256"]))),
        vbmeta: m["vbmeta"]["file"].as_str().map(|f| dir.join(f)).filter(|f| f.exists()).map(|f| (f, s(&m["vbmeta"]["sha256"]))),
    })
}

/// The key item/grid reaches phones with.
pub fn public_key() -> Result<String, String> {
    let candidates = std::env::var_os("SFDUO_PUBKEY").map(PathBuf::from).into_iter().chain(["id_ed25519.pub", "id_ecdsa.pub", "id_rsa.pub"].iter().map(|n| crate::paths::ssh().join(n)));
    for c in candidates {
        if let Ok(k) = std::fs::read_to_string(&c) {
            let k = k.trim().to_owned();
            if k.starts_with("ssh-") || k.starts_with("ecdsa-") {
                return Ok(k);
            }
        }
    }
    Err("no ssh public key in ~/.ssh: item/grid could not reach the phone after installing (ssh-keygen -t ed25519 makes one)".into())
}

/// Erase and install, from Linux. `confirm` must be the phone's word
/// (android::confirm_word).
pub fn erase_and_install(host: &str, release: &Release, mode: Mode, confirm: &str, say: crate::ramboot::Say) -> Result<(), String> {
    crate::link::need_cable(host)?;
    let _ = crate::phone::keep_awake(host, true);
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

    // What is kept, taken first. The device data (not on userdata, never
    // touched here) is backed up once in any case.
    if !crate::backup::has_device_data(&serial) {
        say("backing up the device data first (radio calibration, IMEI)".into());
        crate::backup::take(host, crate::backup::Kind::Device, say)?;
    }
    let mut quick = None;
    match mode {
        Mode::Erase => say("nothing is kept: the phone starts afresh".into()),
        Mode::KeepFiles => {
            say("backing up your files and settings".into());
            quick = Some(crate::backup::take(host, crate::backup::Kind::Quick, say)?);
        }
        Mode::FullCopy => match crate::android::fresh_full(host, &serial)? {
            Some(b) => say(format!("the whole system was backed up {} - fresh", b.manifest.created)),
            None => {
                say("taking the whole system's backup first".into());
                crate::full::take(host, say)?;
            }
        },
    }
    crate::flash::log(&serial, &format!("erase and install {} begun ({}) - itemgrid", release.name, mode.words()))?;

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

    put_on_userdata(&serial, release, size, &key, quick.as_ref(), say)?;

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
    crate::flash::log(&serial, "the new system answers, parking brake armed - itemgrid")?;
    say(if mode == Mode::KeepFiles { "the new system is up: unlock with your PIN".into() } else { "the new system is up: unlock with 1234, then choose your own PIN".into() });
    Ok(())
}


/// From TWRP on the phone `serial`: userdata made anew, the release image
/// (`size` decompressed) put on it in checked parts and checked whole, this
/// computer's ssh key put into it (and what `quick` kept, put back), misc
/// cleared.
fn put_on_userdata(serial: &str, release: &Release, size: u64, key: &str, quick: Option<&crate::backup::Backup>, say: crate::ramboot::Say) -> Result<(), String> {
    use crate::full::adb_shell;
    let serial = serial.to_owned();
    crate::flash::log(&serial, &format!("ERASING userdata for {} - itemgrid", release.name))?;
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

    if key.is_empty() {
        say("no ssh key to put in: the phone is reached from here by USB only".into());
    } else {
        say("putting this computer's ssh key in".into());
    }
    adb_shell(&serial, "mkdir -p /tmp/r && mount -o loop,rw /tmp/ud/rootfs.img /tmp/r && echo ok")?;
    let put_key = (|| -> Result<(), String> {
        if key.is_empty() {
            return Ok(());
        }
        // A small text by adb push: through the socket it did not arrive
        // (2026-10-04) - the big transfers go that way, this need not.
        push_text(&serial, key, "/tmp/itemgrid.pub")?;
        adb_shell(
            &serial,
            "for d in /tmp/r/root /tmp/r/home/droidian; do mkdir -p $d/.ssh && cat /tmp/itemgrid.pub > $d/.ssh/authorized_keys && chmod 700 $d/.ssh && chmod 600 $d/.ssh/authorized_keys; done; \
             chown -R 0:0 /tmp/r/root/.ssh; chown -R 32011:32011 /tmp/r/home/droidian/.ssh; rm -f /tmp/itemgrid.pub; \
             grep -q ssh- /tmp/r/root/.ssh/authorized_keys && echo ok",
        )
        .map(|_| ())
    })();
    let kept = match (quick, &put_key) {
        (Some(q), Ok(())) => put_kept(&serial, q, say),
        _ => Ok(()),
    };
    adb_shell(&serial, "sync; umount /tmp/r; sync; umount /tmp/ud").ok();
    put_key?;
    kept?;

    say("clearing misc".into());
    adb_shell(&serial, &format!("dd if=/dev/zero of={blk}/misc bs=2048 count=1 2>/dev/null; sync"))?;
    let _ = std::fs::remove_file(crate::android::guest_path_of(&serial));
    crate::flash::log(&serial, &format!("{} installed (userdata anew) - itemgrid", release.name))?;

    Ok(())
}

/// The release image decompressed as it is read (zstd, a window of up to
/// 2^27 as the images are made: build-release-image.sh's --long=27).
fn decompress(path: &Path) -> Result<impl Read, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut d = zstd::stream::read::Decoder::new(std::io::BufReader::with_capacity(4 << 20, file)).map_err(|e| format!("zstd: {e}"))?;
    d.window_log_max(27).map_err(|e| format!("zstd: {e}"))?;
    Ok(d)
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

// ---- the modes -----------------------------------------------------------------

/// What happens to what the phone holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    /// Nothing kept: the fastest (minutes).
    Erase,
    /// The home folder, Wi-Fi networks, time zone and PIN kept, from a quick
    /// backup taken first, put into the new system before its first start.
    KeepFiles,
    /// The whole system backed up first, to be brought back exactly (Back to
    /// Linux) - for trying images; ~20 minutes more.
    FullCopy,
}

impl Mode {
    pub fn words(self) -> &'static str {
        match self {
            Mode::Erase => "nothing kept",
            Mode::KeepFiles => "your files, Wi-Fi networks, time zone and PIN kept",
            Mode::FullCopy => "a full copy of the current system kept on this computer",
        }
    }
}

/// From the quick backup, what a new system takes: the home folder, the
/// Wi-Fi networks, the time zone.
fn kept(path: &str) -> bool {
    path.starts_with("home/") || path == "home" || path.starts_with("etc/NetworkManager/system-connections") || path == "etc/localtime" || path == "etc/timezone"
}

/// Files bigger than this go part by part (TWRP's tar writes a big file from
/// a stream as empty); the rest in one tar stream.
const BIG: u64 = 32 << 20;

/// The kept part of a quick backup put into the new system, mounted at /tmp/r
/// in TWRP: small files through tar, big ones part by part, every file's size
/// checked after; the owner's password hash (the PIN) carried over.
fn put_kept(serial: &str, quick: &crate::backup::Backup, say: crate::ramboot::Say) -> Result<(), String> {
    use crate::full::adb_shell;
    let archive = quick.dir.join("home-etc.tar.gz");
    let open = || -> Result<tar::Archive<flate2::read::MultiGzDecoder<std::io::BufReader<std::fs::File>>>, String> {
        let f = std::fs::File::open(&archive).map_err(|e| format!("{}: {e}", archive.display()))?;
        Ok(tar::Archive::new(flate2::read::MultiGzDecoder::new(std::io::BufReader::new(f))))
    };
    // Pass one: the small entries into a tar of their own, the big ones and
    // the sizes noted, the PIN's line kept.
    let mut small = tar::Builder::new(Vec::new());
    let (mut big, mut sizes, mut shadow) = (Vec::new(), String::new(), None);
    let mut a = open()?;
    for entry in a.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.to_string_lossy().trim_start_matches("./").to_owned();
        if path == "etc/shadow" {
            let mut text = String::new();
            entry.read_to_string(&mut text).map_err(|e| e.to_string())?;
            shadow = text.lines().find(|l| l.starts_with("droidian:")).map(str::to_owned);
            continue;
        }
        if !kept(&path) || path.contains('\'') || path.contains('\n') {
            continue;
        }
        let header = entry.header().clone();
        let size = header.size().unwrap_or(0);
        if header.entry_type() == tar::EntryType::Regular && size > BIG {
            big.push((path, size, header.mode().unwrap_or(0o644), header.uid().unwrap_or(0), header.gid().unwrap_or(0)));
            continue;
        }
        if header.entry_type() == tar::EntryType::Regular {
            sizes.push_str(&format!("{size} /tmp/r/{path}\n"));
        }
        let mut h = header.clone();
        small.append_data(&mut h, &path, &mut entry).map_err(|e| format!("{path}: {e}"))?;
    }
    let small = small.into_inner().map_err(|e| e.to_string())?;
    say(format!("putting back your files ({} MB, {} big)", small.len() >> 20, big.len()));
    crate::full::push_file(serial, &small, "/tmp/itemgrid-small.tar")?;
    adb_shell(serial, "tar -C /tmp/r -xpf /tmp/itemgrid-small.tar; rc=$?; rm -f /tmp/itemgrid-small.tar; exit $rc")?;
    for (path, size, mode, uid, gid) in &big {
        say(format!("putting back {path} ({} MB)", size >> 20));
        let mut a = open()?;
        let mut found = false;
        for entry in a.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            if entry.path().map(|p| p.to_string_lossy().trim_start_matches("./") == path.as_str()).unwrap_or(false) {
                crate::android::put_file(serial, &format!("/tmp/r/{path}"), &mut entry, path, say)?;
                adb_shell(serial, &format!("chmod {mode:o} '/tmp/r/{path}'; chown {uid}:{gid} '/tmp/r/{path}'"))?;
                found = true;
                break;
            }
        }
        if !found {
            return Err(format!("{path} went missing from the backup"));
        }
    }
    // Every small file there, at its size.
    push_text(serial, &sizes, "/tmp/kept-sizes")?;
    let bad = adb_shell(serial, "n=0; while read s p; do [ \"$(stat -c %s \"$p\" 2>/dev/null)\" = \"$s\" ] || n=$((n+1)); done < /tmp/kept-sizes; rm -f /tmp/kept-sizes; echo $n")?;
    if bad.trim() != "0" {
        return Err(format!("{} of your files did not arrive whole - stopping in TWRP", bad.trim()));
    }
    // The PIN: the owner's password hash in place of the image's.
    if let Some(line) = shadow {
        push_text(serial, &line, "/tmp/kept-shadow")?;
        adb_shell(serial, "l=$(cat /tmp/kept-shadow); rm -f /tmp/kept-shadow; grep -v '^droidian:' /tmp/r/etc/shadow > /tmp/shadow.new && echo \"$l\" >> /tmp/shadow.new && cat /tmp/shadow.new > /tmp/r/etc/shadow && rm -f /tmp/shadow.new && grep -c '^droidian:' /tmp/r/etc/shadow")?;
    }
    Ok(())
}

/// A small text put at `to` in TWRP, by adb push.
fn push_text(serial: &str, text: &str, to: &str) -> Result<(), String> {
    let tmp = std::env::temp_dir().join(format!("itemgrid-push-{}", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    let out = Command::new(crate::programs::adb()).args(["-s", serial, "push"]).arg(&tmp).arg(to).stdin(Stdio::null()).output().map_err(|e| format!("adb: {e}"))?;
    let _ = std::fs::remove_file(&tmp);
    if !out.status.success() {
        return Err(format!("adb push: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

/// item put on a Duo that runs its stock Android (adb on, this computer
/// allowed) or sits in its bootloader - a whole new start: the bootloader
/// unlocked if it is not (the owner says so on the phone: Android is erased
/// then), TWRP from RAM, userdata made anew and the release put on it, the
/// port's kernel booted from RAM until Linux answers (SAFETY.md: only an
/// image that booted on this phone is written), then written to both slots
/// from the running Linux and read back, and the phone restarted from its
/// slot. `confirm`: the phone's word (android::confirm_word).
pub fn from_stock(serial: &str, release: &Release, confirm: &str, say: crate::ramboot::Say) -> Result<(), String> {
    let host = crate::link::CABLE;
    if confirm.trim() != crate::android::confirm_word(serial) {
        return Err(format!("the confirmation does not match ({} expected) - nothing is changed", crate::android::confirm_word(serial)));
    }
    let key = public_key()?;
    let p = prepare_from_stock(serial, release, say)?;
    image_on_from_stock(serial, release, &p, &key, say)?;
    let Prepared { slot, boot_file, boot_img, boot, vbmeta, .. } = p;

    // The port's kernel from RAM: it proves itself on this phone first.
    say("into the bootloader".into());
    crate::ramboot::adb_to_bootloader(serial)?;
    say("starting item from the computer - its first start grows it to fill the phone".into());
    crate::ramboot::boot_in_fastboot_within(host, serial, slot, &boot_file, &boot_img, crate::ramboot::Expect::Linux, 600, say)?;
    // Then written for good, both slots, from it.
    let other = if slot == 'a' { 'b' } else { 'a' };
    for s in [slot, other] {
        crate::bootchain::write_from_linux(host, serial, s, &boot, say)?;
        crate::bootchain::write_from_linux(host, serial, s, &vbmeta, say)?;
    }
    // And started from its own slot: it starts by itself now.
    say("restarting from the phone's own boot".into());
    crate::ramboot::arm_brake_linux(host)?;
    crate::phone::reboot(host, &mut |b| say(b.words().to_owned()))?;
    crate::flash::log(serial, &format!("{} installed from stock, started from slot {slot} - itemgrid", release.name))?;
    say("item is up: unlock with 1234, then choose your own PIN".into());
    Ok(())
}

/// Install from stock Android on a computer with no ssh to the phone (the
/// Windows installer): the same way in, Linux known to be up by its USB
/// gadget, and the boot chain written from TWRP instead of from Linux -
/// after the owner's forced restart (the power button held: an unattended
/// reset, which the parking brake parks in fastboot). `key`: an ssh public
/// key to put in, or none.
pub fn from_stock_by_usb(serial: &str, release: &Release, key: &str, say: crate::ramboot::Say) -> Result<(), String> {
    use std::time::{Duration, Instant};
    let host = crate::link::CABLE;
    let p = prepare_from_stock(serial, release, say)?;
    image_on_from_stock(serial, release, &p, key, say)?;
    let Prepared { slot, boot_file, boot_img, boot, vbmeta, twrp, twrp_img, .. } = p;

    // The port's kernel from RAM: it proves itself on this phone first.
    say("into the bootloader".into());
    crate::ramboot::adb_to_bootloader(serial)?;
    say("starting item from the computer - its first start grows it to fill the phone".into());
    crate::ramboot::boot_in_fastboot_within(host, serial, slot, &boot_file, &boot_img, crate::ramboot::Expect::LinuxOnUsb, 600, say)?;

    // Back into fastboot by the owner's hand: the brake flashed before the
    // boot is still in misc, and a forced restart is the unattended reset
    // it parks.
    say("item is up from the computer. Now, on the phone: hold the power button until the screen goes dark (about 10 s) - it comes back in its bootloader".into());
    let start = Instant::now();
    while !crate::ramboot::in_fastboot(serial) {
        if start.elapsed() > Duration::from_secs(900) {
            return Err("the phone did not come back in its bootloader in 15 minutes: hold the power button until the screen goes dark, then run the install again".into());
        }
        std::thread::sleep(Duration::from_secs(3));
    }
    crate::flash::log(serial, "back in fastboot after the RAM boot (the owner's forced restart) - itemgrid")?;

    // The boot chain from TWRP, both slots.
    say("into TWRP".into());
    crate::ramboot::boot_in_fastboot(host, serial, slot, &twrp, &twrp_img, crate::ramboot::Expect::Recovery, say)?;
    let other = if slot == 'a' { 'b' } else { 'a' };
    for s in [slot, other] {
        crate::bootchain::write(serial, s, &[boot.clone(), vbmeta.clone()], say)?;
    }
    // misc cleared: the phone starts by itself now (no brake armed on the
    // way out - from TWRP a reboot might consume it and park in fastboot).
    say("clearing misc and restarting from the phone's own boot".into());
    crate::full::adb_shell(serial, &format!("dd if=/dev/zero of={}/misc bs=2048 count=1 2>/dev/null; sync", crate::android::BLK))?;
    crate::full::adb_shell(serial, "reboot").ok();
    let start = Instant::now();
    while !crate::ramboot::linux_on_usb(serial) {
        if start.elapsed() > Duration::from_secs(600) {
            return Err("the phone did not come up in 10 minutes: if it shows its bootloader, run the install again".into());
        }
        std::thread::sleep(Duration::from_secs(3));
    }
    crate::flash::log(serial, &format!("{} installed from stock by USB, started from slot {slot} - itemgrid", release.name))?;
    say("item is up: unlock with 1234, then choose your own PIN".into());
    Ok(())
}

/// What a from-stock install has checked and found before the phone is
/// changed.
struct Prepared {
    slot: char,
    twrp: PathBuf,
    twrp_img: crate::ramboot::Image,
    boot_file: PathBuf,
    boot_img: crate::ramboot::Image,
    boot: crate::bootchain::Part,
    vbmeta: crate::bootchain::Part,
    /// The image's size, decompressed and hashed here.
    size: u64,
}

/// The release and TWRP checked here, the phone into its bootloader and
/// unlocked (the owner says so on the phone), its slot read.
fn prepare_from_stock(serial: &str, release: &Release, say: crate::ramboot::Say) -> Result<Prepared, String> {
    use std::time::{Duration, Instant};
    let twrp = crate::full::twrp().ok_or("no TWRP image")?;
    let twrp_img = crate::ramboot::check_image(&twrp)?;
    let (boot_file, boot_sha) = release.boot.clone().ok_or("this release has no boot image for a phone coming from Android")?;
    let boot_img = crate::ramboot::check_image(&boot_file)?;
    let boot = crate::bootchain::Part::of("boot", &boot_file)?;
    if !boot_sha.is_empty() && boot_sha != boot.sha256 {
        return Err("the release's boot image does not match its manifest - nothing is changed".into());
    }
    // The port's vbmeta: verification off (bit 1), as the port runs.
    let (vb_file, vb_sha) = release.vbmeta.clone().ok_or("this release has no vbmeta for a phone coming from Android")?;
    if crate::bootchain::vbmeta_flags(&vb_file).is_none_or(|f| f & 2 == 0) {
        return Err("the release's vbmeta does not turn verification off - nothing is changed".into());
    }
    let vbmeta = crate::bootchain::Part::of("vbmeta", &vb_file)?;
    if !vb_sha.is_empty() && vb_sha != vbmeta.sha256 {
        return Err("the release's vbmeta does not match its manifest - nothing is changed".into());
    }
    say(format!("checking the image {} here", release.name));
    let (size, sha) = hash_stream(&mut decompress(&release.compressed)?)?;
    if sha != release.sha256 || (release.size > 0 && size != release.size) {
        return Err("the release image does not match its manifest - nothing is changed".into());
    }

    // Into the bootloader, unlocked.
    if !crate::programs::present("fastboot") || !crate::programs::present("adb") {
        return Err("adb and fastboot are needed here and were not found".into());
    }
    if !crate::ramboot::in_fastboot(serial) {
        say("into the bootloader".into());
        crate::ramboot::adb_to_bootloader(serial)?;
    }
    let fb = crate::ramboot::probe()?;
    if fb.serial != serial {
        return Err(format!("the bootloader shows another phone ({}): stopping", fb.serial));
    }
    // Before the unlock (it erases the phone), not after: a Duo 2 or any
    // other phone is left as it is.
    if fb.product != "surfaceduo" {
        return Err(format!("the bootloader shows a '{}', not a Duo 1: nothing is changed", fb.product));
    }
    if fb.unlocked != "yes" {
        say("unlocking the bootloader: on the phone, choose Unlock with the volume keys, then press Power".into());
        crate::flash::log(serial, "bootloader unlock asked (Android is erased by it) - itemgrid")?;
        let _ = crate::ramboot::fastboot(&["flashing", "unlock"], Duration::from_secs(120));
        let start = Instant::now();
        loop {
            if crate::ramboot::in_fastboot(serial) && crate::ramboot::probe().is_ok_and(|f| f.unlocked == "yes") {
                break;
            }
            if start.elapsed() > Duration::from_secs(180) {
                return Err("the bootloader was not unlocked: if the phone restarted into Android's setup, turn USB debugging on again there, then install again".into());
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        crate::flash::log(serial, "bootloader unlocked - itemgrid")?;
    }
    let fb = crate::ramboot::probe()?;
    let slot = fb.slot.chars().next().ok_or("the bootloader did not say its slot")?;
    crate::flash::log(serial, &format!("install of {} from stock begun (slot {slot}) - itemgrid", release.name))?;
    Ok(Prepared { slot, twrp, twrp_img, boot_file, boot_img, boot, vbmeta, size })
}

/// TWRP from RAM, and the image onto userdata from it (with `key`, if any).
fn image_on_from_stock(serial: &str, release: &Release, p: &Prepared, key: &str, say: crate::ramboot::Say) -> Result<(), String> {
    say("into TWRP".into());
    crate::ramboot::boot_in_fastboot(crate::link::CABLE, serial, p.slot, &p.twrp, &p.twrp_img, crate::ramboot::Expect::Recovery, say)?;
    if !crate::full::fast_tools(serial) {
        return Err("this TWRP lacks pigz or nc".into());
    }
    put_on_userdata(serial, release, p.size, key, None, say)
}
