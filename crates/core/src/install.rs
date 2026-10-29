//! Erase and install: the release image (the port's
//! tools/build-release-image.sh) put on a phone that runs the port - its
//! userdata made anew, the image written in checked parts, the owner's ssh
//! key put in so item/grid can reach it, and its first start awaited (it grows
//! to fill userdata by itself). What userdata held goes: the whole system is
//! backed up first, unless a fresh backup is here already.

use std::io::Read;
use std::path::{Path, PathBuf};

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
    /// The compressed file's sha256 ("" in a manifest without one).
    pub compressed_sha256: String,
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
        compressed_sha256: s(&m["compressed"]["sha256"]),
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
    let serial = crate::backup::serial(host)?;
    if confirm.trim() != crate::android::confirm_word(&serial) {
        return Err(format!("the confirmation does not match ({} expected) - nothing is changed", crate::android::confirm_word(&serial)));
    }
    let key = public_key()?;
    let twrp = crate::full::recovery().ok_or("no recovery image (tools/build-recovery.sh, or a release with item-recovery.img)")?;
    crate::ramboot::check_image(&twrp)?;
    // The image checked here before anything.
    let size = check_release(release, say)?;

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

    say("starting the recovery".into());
    crate::ramboot::ram_boot(host, &twrp, crate::ramboot::Expect::Recovery, say)?;
    if !crate::full::ours(&serial) && !crate::full::fast_tools(&serial) {
        let _ = crate::ramboot::leave_recovery(host, &serial, say);
        return Err("this TWRP lacks pigz or nc".into());
    }
    say("testing the cable: 512 MB sent and checked".into());
    if let Err(e) = crate::android::try_the_way_back(&serial) {
        let _ = crate::ramboot::leave_recovery(host, &serial, say);
        return Err(format!("{e} - nothing is erased"));
    }

    put_on_userdata(&serial, release, size, &key, quick.as_ref(), say)?;

    say("starting item: the first start takes a minute longer".into());
    crate::rec::Rec::on(&serial).restart();
    let start = std::time::Instant::now();
    while !crate::phone::answers_fresh(host) {
        if start.elapsed() > std::time::Duration::from_secs(480) {
            return Err("the new system did not answer in 8 minutes - look at the phone's screen".into());
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    crate::ramboot::arm_brake_linux(host)?;
    crate::flash::log(&serial, "the new system answers, parking brake armed - itemgrid")?;
    say(if mode == Mode::KeepFiles { "item is up: unlock with your PIN".into() } else { "item is up: unlock with 1234, then choose your own PIN".into() });
    Ok(())
}


/// From TWRP on the phone `serial`: userdata made anew, the release image
/// (`size` decompressed) put on it in checked parts and checked whole, this
/// computer's ssh key put into it (and what `quick` kept, put back), misc
/// cleared.
fn put_on_userdata(serial: &str, release: &Release, size: u64, key: &str, quick: Option<&crate::backup::Backup>, say: crate::ramboot::Say) -> Result<(), String> {
    // Its steps on the phone's screen (item/grid's own recovery).
    let version = release.item.split(['~', '-', '+']).next().unwrap_or(&release.item).to_owned();
    crate::screen::begin(serial, &["clean", &format!("item {version}"), "check", "link", "restart"]);
    let r = put_on_userdata_steps(serial, release, size, key, quick, say);
    match &r {
        Ok(()) => crate::screen::done("item is on", "setup next"),
        Err(e) => crate::screen::error(e),
    }
    r
}

fn put_on_userdata_steps(serial: &str, release: &Release, size: u64, key: &str, quick: Option<&crate::backup::Backup>, say: crate::ramboot::Say) -> Result<(), String> {
    let serial = serial.to_owned();
    crate::flash::log(&serial, &format!("ERASING userdata for {} - itemgrid", release.name))?;
    say("erasing the phone's data partition".into());
    crate::screen::step(0, "cleaning", "old data goes");
    let blk = crate::android::BLK;
    let rec = crate::rec::Rec::on(&serial);
    rec.make_ext4(&format!("{blk}/userdata"), "userdata", "/tmp/ud")?;

    say(format!("writing item {}", release.item.split(['~', '-', '+']).next().unwrap_or(&release.item)));
    crate::screen::step(1, "writing item", "");
    crate::screen::total(size);
    if rec.ours() {
        // As it is, zstd (a third of the gzip): unpacked on the phone
        // (item-recovery's unzstd), checked whole from its flash below.
        put_compressed(&rec, release, say)?;
    } else {
        let (written, written_sha) = crate::android::put_file(&serial, "rootfs.img", &mut decompress(&release.compressed)?, "image", true, say)?;
        if written_sha.as_deref() != Some(release.sha256.as_str()) || written != size {
            return Err("the image read here changed while it was sent - stopping in the recovery; run it again".into());
        }
    }
    rec.run(&["meta", "/tmp/ud/rootfs.img", "644", "0", "0"], "chmod 644 /tmp/ud/rootfs.img")?;
    say("checking every byte on the phone".into());
    crate::screen::step(2, "checking", "every byte");
    if rec.hash_written("/tmp/ud/rootfs.img")? != release.sha256 {
        return Err("the image on the phone differs from the release - stopping in the recovery; run it again".into());
    }

    if key.is_empty() {
        say("no ssh key to put in: the phone is reached from here by USB only".into());
    } else {
        say("letting this computer in (its ssh key)".into());
    }
    crate::screen::step(3, "linking", "to item/grid");
    rec.mount("/tmp/ud/rootfs.img", "/tmp/r", false)?;
    let put_key = (|| -> Result<(), String> {
        if key.is_empty() {
            return Ok(());
        }
        for (home, id) in [("/tmp/r/root", 0), ("/tmp/r/home/droidian", 32011)] {
            let (dir, keys) = (format!("{home}/.ssh"), format!("{home}/.ssh/authorized_keys"));
            rec.mkdir(&dir)?;
            rec.write(&keys, key.as_bytes())?;
            rec.meta(&dir, 0o700, id, id)?;
            rec.meta(&keys, 0o600, id, id)?;
        }
        if !rec.read_text("/tmp/r/root/.ssh/authorized_keys")?.contains("ssh-") {
            return Err("the ssh key did not arrive in the image".into());
        }
        Ok(())
    })();
    let kept = match (quick, &put_key) {
        (Some(q), Ok(())) => put_kept(&rec, &serial, q, say),
        _ => Ok(()),
    };
    let _ = rec.umount("/tmp/r");
    // The Android container's data back (the fingerprints with it), when
    // the release's port is the one it was taken under: another port's
    // container may not take it.
    let android = match (quick, &put_key, &kept) {
        (Some(q), Ok(()), Ok(())) => put_android_data(&rec, &serial, q, release, say),
        _ => Ok(()),
    };
    let _ = rec.umount("/tmp/ud");
    put_key?;
    kept?;
    android?;

    say("clearing the bootloader's note".into());
    rec.zero(&format!("{blk}/misc"), Some(2048))?;
    let _ = std::fs::remove_file(crate::android::guest_path_of(&serial));
    crate::flash::log(&serial, &format!("{} installed (userdata anew) - itemgrid", release.name))?;

    Ok(())
}

/// The quick backup's Android container data onto the new userdata, if it
/// has it and the release runs the port it was taken under; said either way.
fn put_android_data(rec: &crate::rec::Rec, serial: &str, quick: &crate::backup::Backup, release: &Release, say: crate::ramboot::Say) -> Result<(), String> {
    let archive = quick.dir.join(crate::backup::ANDROID_DATA);
    let base = |v: &str| v.split(['+', '~']).next().unwrap_or(v).to_owned();
    if !archive.exists() {
        say("no Android data in the backup: the fingerprints are enrolled again on the phone".into());
        return Ok(());
    }
    if base(&quick.manifest.port) != base(&release.adaptation) {
        say(format!("the Android data was taken under port {}, the release runs {}: the fingerprints are enrolled again on the phone", quick.manifest.port, release.adaptation));
        return Ok(());
    }
    say("putting the fingerprints back".into());
    rec.rm("/tmp/ud/android-data")?;
    crate::android::put_archive(serial, &archive, "/tmp/ud", say)
}

/// The release's rootfs.img.zst sent as it is into item-recovery's unzstd,
/// which writes /tmp/ud/rootfs.img; sent again whole if the sending fails
/// (three times at most). Its progress on the phone's screen by the
/// compressed bytes sent.
fn put_compressed(rec: &crate::rec::Rec, release: &Release, say: crate::ramboot::Say) -> Result<(), String> {
    struct Counted<'a, R> {
        inner: R,
        sent: u64,
        len: u64,
        next: u64,
        say: &'a mut dyn FnMut(String),
    }
    impl<R: Read> Read for Counted<'_, R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.sent += n as u64;
            crate::screen::progress("writing item", self.sent);
            if self.sent >= self.next {
                (self.say)(format!("  {} of {} sent", crate::status::size_words(self.sent / 1024), crate::status::size_words(self.len / 1024)));
                self.next += 512 << 20;
            }
            Ok(n)
        }
    }
    let len = std::fs::metadata(&release.compressed).map(|m| m.len()).unwrap_or(0);
    crate::screen::total(len);
    let mut last = String::new();
    for _ in 0..3 {
        let file = std::fs::File::open(&release.compressed).map_err(|e| format!("{}: {e}", release.compressed.display()))?;
        let mut r = Counted { inner: std::io::BufReader::with_capacity(4 << 20, file), sent: 0, len, next: 512 << 20, say: &mut *say };
        match rec.send(&["unzstd", "/tmp/ud/rootfs.img"], "", &mut r) {
            Ok(_) => return Ok(()),
            Err(e) => {
                say(format!("  image: {e} - sending it again"));
                last = e;
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        }
    }
    Err(format!("the image did not arrive whole in three tries ({last}) - the phone stays in the recovery; run it again"))
}

/// The release image decompressed as it is read (zstd, a window of up to
/// 2^27 as the images are made: build-release-image.sh's --long=27).
fn decompress(path: &Path) -> Result<impl Read, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut d = zstd::stream::read::Decoder::new(std::io::BufReader::with_capacity(4 << 20, file)).map_err(|e| format!("zstd: {e}"))?;
    d.window_log_max(27).map_err(|e| format!("zstd: {e}"))?;
    Ok(d)
}

/// The release checked against its manifest before anything is changed:
/// its compressed file's sha256 (seconds), or - a manifest without it -
/// the image decompressed and hashed (half a minute to two here). The
/// image's size back.
fn check_release(release: &Release, say: crate::ramboot::Say) -> Result<u64, String> {
    say("checking the image here".into());
    if !release.compressed_sha256.is_empty() && release.size > 0 {
        let mut f = std::fs::File::open(&release.compressed).map_err(|e| format!("{}: {e}", release.compressed.display()))?;
        let (_, sha) = hash_stream(&mut f)?;
        if sha != release.compressed_sha256 {
            return Err("the release image does not match its manifest - nothing is changed".into());
        }
        return Ok(release.size);
    }
    let (size, sha) = hash_stream(&mut decompress(&release.compressed)?)?;
    if sha != release.sha256 || (release.size > 0 && size != release.size) {
        return Err("the release image does not match its manifest - nothing is changed".into());
    }
    Ok(size)
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
            Mode::KeepFiles => "your files, Wi-Fi networks, time zone, PIN and fingerprints kept",
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
fn put_kept(rec: &crate::rec::Rec, serial: &str, quick: &crate::backup::Backup, say: crate::ramboot::Say) -> Result<(), String> {
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
    say(format!("putting your files back ({} MB)", (small.len() >> 20) as u64 + big.iter().map(|b| b.1 >> 20).sum::<u64>()));
    rec.untar("/tmp/r", &small)?;
    for (path, size, mode, uid, gid) in &big {
        say(format!("putting back {path} ({} MB)", size >> 20));
        let mut a = open()?;
        let mut found = false;
        for entry in a.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            if entry.path().map(|p| p.to_string_lossy().trim_start_matches("./") == path.as_str()).unwrap_or(false) {
                crate::android::put_file(serial, &format!("/tmp/r/{path}"), &mut entry, path, false, say)?;
                rec.meta(&format!("/tmp/r/{path}"), *mode, *uid, *gid)?;
                found = true;
                break;
            }
        }
        if !found {
            return Err(format!("{path} went missing from the backup"));
        }
    }
    // Every small file there, at its size.
    let bad = rec.check_sizes(&sizes)?;
    if bad != 0 {
        return Err(format!("{bad} of your files did not arrive whole - stopping in the recovery"));
    }
    // The PIN: the owner's password hash in place of the image's (the
    // file rewritten in place: its mode and group stay).
    if let Some(line) = shadow {
        let now = rec.read_text("/tmp/r/etc/shadow")?;
        let mut new: String = now.lines().filter(|l| !l.starts_with("droidian:")).map(|l| format!("{l}\n")).collect();
        new.push_str(&format!("{line}\n"));
        rec.write("/tmp/r/etc/shadow", new.as_bytes())?;
        if rec.read_text("/tmp/r/etc/shadow")?.lines().filter(|l| l.starts_with("droidian:")).count() != 1 {
            return Err("the PIN did not go into the image".into());
        }
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
    // The computer's Wi-Fi taken now, while the computer is free (asked at
    // the end, with the image just written, it once did not come): kept in
    // memory only, given to the phone at the end.
    let wifi = crate::wifi::computer();
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
    // The computer's Wi-Fi given to it (wifi.rs): its setup finds the
    // network joined. Not having it is no failure of the install.
    match wifi.and_then(|net| crate::wifi::share_net(host, net)) {
        Ok((ssid, how)) => say(format!("the computer's Wi-Fi given to the phone: {ssid} ({})", match how {
            crate::wifi::Shared::Joined => "joined",
            crate::wifi::Shared::Kept => "kept, joined when in reach",
            crate::wifi::Shared::Known => "it knew it",
        })),
        Err(e) => say(format!("no Wi-Fi given to the phone: {e}")),
    }
    // And its time zone (a fresh phone is on UTC).
    match crate::wifi::share_zone(host) {
        Ok(zone) => {
            say(format!("the computer's time zone given to the phone: {zone}"));
            match crate::wifi::share_weather_place(host, &zone) {
                Ok(Some(place)) => say(format!("the weather's place: {place}")),
                Ok(None) => {}
                Err(e) => say(format!("no weather place given to the phone: {e}")),
            }
        }
        Err(e) => say(format!("no time zone given to the phone: {e}")),
    }
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
    say("starting the recovery".into());
    crate::ramboot::boot_in_fastboot(host, serial, slot, &twrp, &twrp_img, crate::ramboot::Expect::Recovery, say)?;
    let other = if slot == 'a' { 'b' } else { 'a' };
    for s in [slot, other] {
        crate::bootchain::write(serial, s, &[boot.clone(), vbmeta.clone()], say)?;
    }
    // misc cleared: the phone starts by itself now (no brake armed on the
    // way out - from TWRP a reboot might consume it and park in fastboot).
    say("clearing the bootloader's note, restarting from the phone's own boot".into());
    let rec = crate::rec::Rec::on(serial);
    rec.zero(&format!("{}/misc", crate::android::BLK), Some(2048))?;
    rec.restart();
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
    let twrp = crate::full::recovery().ok_or("no recovery image (tools/build-recovery.sh, or a release with item-recovery.img)")?;
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
    let size = check_release(release, say)?;

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
    say("starting the recovery".into());
    crate::ramboot::boot_in_fastboot(crate::link::CABLE, serial, p.slot, &p.twrp, &p.twrp_img, crate::ramboot::Expect::Recovery, say)?;
    // TWRP sends through nc; item/grid's own recovery takes the stream.
    if !crate::full::ours(serial) && !crate::full::fast_tools(serial) {
        return Err("this TWRP lacks pigz or nc".into());
    }
    without_windows(serial, say)?;
    put_on_userdata(serial, release, p.size, key, None, say)
}

/// Windows taken off the phone, if the WOA Project's guides put it there:
/// its partitions dropped and userdata grown back over their room
/// (item-recovery's `gpt`), before userdata is made anew. The install's
/// plan said so (the window's "erases windows too"). Nothing on a phone
/// without it.
fn without_windows(serial: &str, say: crate::ramboot::Say) -> Result<(), String> {
    let rec = crate::rec::Rec::on(serial);
    if !rec.ours() {
        let names = crate::full::adb_shell(serial, &format!("ls {}", crate::android::BLK)).unwrap_or_default();
        if names.split_whitespace().any(|n| n == "win") {
            return Err("Windows is on this phone, and only item/grid's own recovery takes it off: nothing is changed".into());
        }
        return Ok(());
    }
    let plan = rec.run(&["gpt", "/dev/block/sda"], "");
    let plan = match plan {
        Ok(p) => p,
        // 1: no Windows there.
        Err(e) if e.contains("nothing to do") => return Ok(()),
        Err(e) => return Err(format!("the phone's partition table: {e}")),
    };
    say("Windows is on this phone: taking it off (its partitions go, userdata takes their room)".into());
    let after = plan.lines().skip_while(|l| !l.starts_with("without Windows")).skip(1).map(str::trim).collect::<Vec<_>>().join("; ");
    crate::flash::log(serial, &format!("WINDOWS OFF: the table rewritten without esp and win ({after}) - itemgrid"))?;
    let done = rec.run(&["gpt", "--write", "/dev/block/sda"], "")?;
    say(done.lines().last().unwrap_or("written").to_owned());
    Ok(())
}
