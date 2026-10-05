//! Back to the phone's stock Android, and from it back to Linux - the port's
//! docs/STOCK-ANDROID.md (done on this Duo on 2026-09-26) as code.
//!
//! Android never left the phone: its system and vendor stay in `super` while
//! the port runs. It needs only its own kernel - from the same build as the
//! vendor in super - and an empty userdata, which it formats on its first
//! boot. This module's first part only reads: what is in super, which stock
//! kernel belongs to it, whether the whole system's backup is fresh, what
//! userdata holds that no backup has.

use std::path::{Path, PathBuf};

use crate::backup::{Backup, Kind};

const MIN_BATTERY: u32 = 50;
/// A full backup older than this, or older than the phone's last boot, is
/// taken again first.
const FRESH_SECS: i64 = 3600;
/// The stock kernel is built with its vendor: after it, the same day.
const SAME_BUILD_SECS: i64 = 24 * 3600;

/// Android's build in super, per slot.
#[derive(Debug, Clone)]
pub struct SuperBuild {
    pub slot: char,
    pub fingerprint: String,
    /// The vendor's build time (UTC seconds).
    pub vendor_utc: i64,
}

/// A stock kernel found on this computer.
#[derive(Debug, Clone)]
pub struct Kernel {
    pub path: PathBuf,
    pub version: String,
    /// Its build time (UTC seconds), from "Linux version ... #1 SMP PREEMPT <date>".
    pub built_utc: i64,
}

/// What a return to Android would do, and what stops it.
#[derive(Debug, Clone)]
pub struct Plan {
    pub builds: Vec<SuperBuild>,
    pub kernel: Option<(Kernel, SuperBuild)>,
    pub kernels_seen: Vec<Kernel>,
    pub battery: Option<u32>,
    pub full: Option<Backup>,
    /// The full backup is fresh: no new one needs taking first.
    pub full_fresh: bool,
    pub device_data: bool,
    /// What userdata holds that the full backup does not: (name, bytes).
    pub losses: Vec<(String, u64)>,
    /// What stops the return, in words; empty when it may go.
    pub stops: Vec<String>,
}

fn props_script() -> &'static str {
    r#"for s in a b; do d=/tmp/hythe-super-$s; mkdir -p $d
  if mount -o ro /dev/mapper/dynpart-vendor_$s $d 2>/dev/null; then
    echo "slot=$s $(grep -h '^ro.vendor.build.date.utc=\|^ro.vendor.build.fingerprint=' $d/build.prop | tr '\n' ' ')"
    umount $d
  fi; rmdir $d; done
"#
}

/// What super holds, read on the phone (read-only mounts).
pub fn super_builds(host: &str) -> Result<Vec<SuperBuild>, String> {
    let text = crate::phone::run(host, props_script())?;
    let mut out = Vec::new();
    for line in text.lines() {
        let mut slot = None;
        let (mut fp, mut utc) = (String::new(), 0i64);
        for w in line.split_whitespace() {
            if let Some(v) = w.strip_prefix("slot=") {
                slot = v.chars().next();
            } else if let Some(v) = w.strip_prefix("ro.vendor.build.date.utc=") {
                utc = v.parse().unwrap_or(0);
            } else if let Some(v) = w.strip_prefix("ro.vendor.build.fingerprint=") {
                fp = v.to_owned();
            }
        }
        if let (Some(slot), true) = (slot, utc > 0) {
            out.push(SuperBuild { slot, fingerprint: fp, vendor_utc: utc });
        }
    }
    Ok(out)
}

/// "Mon Jul 10 11:35:04 UTC 2023" as UTC seconds.
fn utc_of(date: &str) -> Option<i64> {
    let w: Vec<&str> = date.split_whitespace().collect();
    let (mon, day, time, year) = match w.as_slice() {
        [_, mon, day, time, _tz, year, ..] => (*mon, *day, *time, *year),
        _ => return None,
    };
    let m = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"].iter().position(|x| *x == mon)? as i64 + 1;
    let (d, y): (i64, i64) = (day.parse().ok()?, year.parse().ok()?);
    let t: Vec<i64> = time.split(':').filter_map(|x| x.parse().ok()).collect();
    if t.len() != 3 {
        return None;
    }
    // Days since 1970 (civil from days, inverted).
    let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + t[0] * 3600 + t[1] * 60 + t[2])
}

/// A boot image's kernel version line and build time, if it has them.
pub fn kernel_of(path: &Path) -> Option<Kernel> {
    use std::io::Read;
    let b = std::fs::read(path).ok()?;
    if b.get(..8)? != b"ANDROID!" {
        return None;
    }
    let u = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as usize;
    let (ksize, page) = (u(8), u(36));
    let k = b.get(page..page + ksize)?;
    let mut text = Vec::new();
    if k[..2] == [0x1f, 0x8b] {
        let _ = flate2::read::GzDecoder::new(k).take(64 << 20).read_to_end(&mut text);
    } else {
        text = k.to_vec();
    }
    let i = text.windows(14).position(|w| w == b"Linux version ")?;
    let end = text[i..].iter().position(|c| *c == b'\n' || *c == 0).map(|e| i + e)?;
    let line = String::from_utf8_lossy(&text[i..end]).into_owned();
    let date = line.rsplit("PREEMPT").next()?.trim().to_owned();
    Some(Kernel { path: path.to_owned(), version: line, built_utc: utc_of(&date)? })
}

/// Where stock kernels are looked for: Hythe's own stock folder, its
/// backups, and the port's out/backups/.
fn kernel_places() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let mut dirs = vec![home.join(".local/share/hythe/stock"), crate::backup::root()];
    if let Some(port) = crate::flash::port_tree() {
        dirs.push(port.join("out/backups"));
    }
    dirs
}

/// The stock kernels on this computer (a "duo-p" kernel is Microsoft's;
/// the port's are "microsoft-surfaceduo"), one per build.
pub fn stock_kernels() -> Vec<Kernel> {
    let mut found: Vec<Kernel> = Vec::new();
    let mut stack = kernel_places();
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "img") && p.metadata().is_ok_and(|m| m.len() <= 128 << 20) {
                if let Some(k) = kernel_of(&p) {
                    if k.version.contains("duo-p") && !found.iter().any(|f| f.built_utc == k.built_utc) {
                        found.push(k);
                    }
                }
            }
        }
    }
    found.sort_by_key(|k| k.built_utc);
    found
}

/// The plan for returning to Android - reading only.
pub fn plan(host: &str) -> Result<Plan, String> {
    let serial = crate::backup::serial(host)?;
    let builds = super_builds(host)?;
    let kernels_seen = stock_kernels();
    // A kernel built with a vendor in super: after it, within a day.
    let kernel = builds.iter().find_map(|b| {
        kernels_seen
            .iter()
            .filter(|k| k.built_utc >= b.vendor_utc && k.built_utc - b.vendor_utc <= SAME_BUILD_SECS)
            .min_by_key(|k| k.built_utc - b.vendor_utc)
            .map(|k| (k.clone(), b.clone()))
    });
    let st = crate::status::read(host)?;
    let all = crate::backup::list(Some(&serial));
    let full = all.iter().find(|b| b.manifest.kind == Kind::Full).cloned();
    let device_data = all.iter().any(|b| b.manifest.kind == Kind::Device);
    // Fresh: taken within the hour, and after the phone's last boot.
    let times: Vec<i64> = crate::phone::run(host, "date +%s; awk '/^btime/ {print $2}' /proc/stat\n")?.lines().filter_map(|l| l.trim().parse().ok()).collect();
    let (phone_now, booted) = (times.first().copied().unwrap_or(0), times.get(1).copied().unwrap_or(0));
    let full_fresh = full.as_ref().is_some_and(|b| {
        let taken = local_secs(&b.manifest.created);
        taken > booted && phone_now - taken < FRESH_SECS
    });
    // What userdata holds besides what the full backup takes.
    let listing = crate::phone::run(host, "cd /userdata && for f in *; do [ -L \"$f\" ] && continue; s=$(du -sb \"$f\" 2>/dev/null | cut -f1); echo \"$s $f\"; done\n")?;
    // A whole-system backup takes the rest too; one taken before it did not.
    let rest_kept = !full_fresh || full.as_ref().is_some_and(|b| b.manifest.items.iter().any(|i| i.file == crate::full::REST));
    let losses: Vec<(String, u64)> = if rest_kept { Vec::new() } else { listing
        .lines()
        .filter_map(|l| l.split_once(' '))
        .filter(|(_, name)| !matches!(*name, "rootfs.img" | "android-data" | "lost+found"))
        .map(|(s, name)| (name.to_owned(), s.parse().unwrap_or(0)))
        .collect() };
    let mut stops = Vec::new();
    if builds.is_empty() {
        stops.push("no Android build found in super - nothing to return to".into());
    }
    match &kernel {
        None => stops.push("no stock kernel from the build in super on this computer (~/.local/share/hythe/stock/, or the backup taken before the port went on)".into()),
        Some((k, _)) => {
            if let Err(e) = crate::ramboot::check_image(&k.path) {
                stops.push(format!("the stock kernel fails the image check: {e}"));
            }
        }
    }
    match st.battery {
        Some(b) if b >= MIN_BATTERY => {}
        Some(b) => stops.push(format!("the battery is at {b} %: {MIN_BATTERY} % is needed")),
        None => stops.push("the battery was not read".into()),
    }
    if !device_data {
        stops.push("the device data (radio calibration, IMEI) is not backed up: Back Up Now first".into());
    }
    if crate::full::twrp().is_none() {
        stops.push("no TWRP image: it is needed to erase and to come back".into());
    }
    Ok(Plan { builds, kernel, kernels_seen, battery: st.battery, full, full_fresh, device_data, losses, stops })
}

/// A manifest's local "YYYY-MM-DD HH:MM:SS" as UTC seconds (by date(1)).
fn local_secs(stamp: &str) -> i64 {
    std::process::Command::new("date")
        .args(["-d", stamp, "+%s"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(0)
}

/// UTC seconds in words.
pub fn when(utc: i64) -> String {
    std::process::Command::new("date")
        .args(["-u", "-d", &format!("@{utc}"), "+%Y-%m-%d %H:%M UTC"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_a_kernels_date() {
        // The vendor in super on our Duo: 1688987527 = 2023-07-10 11:12:07 UTC.
        assert_eq!(super::utc_of("Mon Jul 10 11:12:07 UTC 2023"), Some(1688987527));
    }
}

// ---- going: from Linux to stock Android -------------------------------------

/// The partitions by name in TWRP.
pub(crate) const BLK: &str = "/dev/block/platform/soc/1d84000.ufshc/by-name";
/// The way back is tried with this much before anything is erased.
const TEST_BYTES: u64 = 512 << 20;
/// sha256 of 1 MiB of zeros.
const ZERO_MB: &str = "30e14955ebf1352266dc2ff8067e68104607e750abb9d3b36582b8af909fcb58";

/// Which stock kernel and slot the guest Android runs from: kept per phone
/// so a restart picks the same, not a guess.
pub(crate) fn guest_path_of(serial: &str) -> PathBuf {
    guest_path(serial)
}

fn guest_path(serial: &str) -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/hythe/android").join(format!("{serial}.json"))
}

fn remember_guest(serial: &str, kernel: &Path, slot: char) -> Result<(), String> {
    let path = guest_path(serial);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let body = serde_json::json!({"kernel": kernel, "slot": slot.to_string()});
    std::fs::write(&path, body.to_string()).map_err(|e| format!("{}: {e}", path.display()))
}

/// The guest's kernel and slot, if Hythe started Android on this phone.
pub fn guest(serial: &str) -> Option<(PathBuf, char)> {
    let d: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(guest_path(serial)).ok()?).ok()?;
    Some((PathBuf::from(d["kernel"].as_str()?), d["slot"].as_str()?.chars().next()?))
}

/// The word that opens the point of no return: the phone's number in the
/// club, or its serial's last five digits.
pub fn confirm_word(serial: &str) -> String {
    crate::club::known(serial).map(|d| d.number).unwrap_or_else(|| serial.chars().rev().take(5).collect::<Vec<_>>().into_iter().rev().collect())
}

/// A full backup's files checked against its manifest (the image
/// decompressed and hashed: a few minutes).
pub fn verify_full(b: &Backup, say: crate::ramboot::Say) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    for item in &b.manifest.items {
        let path = b.dir.join(&item.file);
        say(format!("checking {} in the backup", item.file));
        let file = std::fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut hash = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        // The image's sha is of what it decompresses to; the tar's of the file.
        let mut reader: Box<dyn Read> = if item.file == "rootfs.img.gz" { Box::new(flate2::read::MultiGzDecoder::new(std::io::BufReader::new(file))) } else { Box::new(file) };
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
        }
        if format!("{:x}", hash.finalize()) != item.sha256 {
            return Err(format!("{} does not match its manifest: the backup is damaged - nothing is erased", path.display()));
        }
    }
    Ok(())
}

/// The way back tried in TWRP: data sent onto the phone, its sha256 there
/// compared. (adb dropped mid-transfer once on this phone; better found out
/// before userdata is gone.)
pub fn try_the_way_back(serial: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    // Not zeros - those would prove little: a hash chain, cheap and varied.
    let mut data = Vec::with_capacity(TEST_BYTES as usize);
    let mut block = Sha256::digest(b"hythe way back").to_vec();
    while (data.len() as u64) < TEST_BYTES {
        block = Sha256::digest(&block).to_vec();
        data.extend_from_slice(&block);
    }
    let want = format!("{:x}", Sha256::digest(&data));
    crate::full::adb_send(serial, "cat > /tmp/hythe-way-back", &mut data.as_slice())?;
    let there = crate::full::adb_shell(serial, "sha256sum /tmp/hythe-way-back | cut -d' ' -f1; rm -f /tmp/hythe-way-back")?;
    if there.trim() != want {
        return Err("the way back failed its trial (what arrived differs) - nothing is erased".into());
    }
    Ok(())
}

/// metadata and userdata zeroed whole, from TWRP - a zeroed start is not
/// enough: ext4's backup superblocks could bring the port's filesystem back
/// under Android's first boot, and Android would poison misc
/// (STOCK-ANDROID.md). Then a few places read back: all zeros.
fn erase(serial: &str, say: crate::ramboot::Say) -> Result<(), String> {
    let _ = crate::full::adb_shell(serial, "umount /tmp/ud 2>/dev/null; true");
    for (name, bs) in [("metadata", 1048576u64), ("userdata", 4194304)] {
        let dev = crate::full::adb_shell(serial, &format!("readlink -f {BLK}/{name}"))?.trim().to_owned();
        if !dev.starts_with("/dev/block/sd") {
            return Err(format!("{name} is not where it should be ({dev}): stopping"));
        }
        say(format!("zeroing {name} ({dev}) whole{}", if name == "userdata" { " - about 7 minutes" } else { "" }));
        crate::full::adb_shell(serial, &format!("dd if=/dev/zero of={dev} bs={bs} 2>/dev/null; sync; true"))?;
        // Read back at the start, the end and between.
        let size: u64 = crate::full::adb_shell(serial, &format!("blockdev --getsize64 {dev}"))?.trim().parse().map_err(|_| format!("{name}'s size was not read"))?;
        let mb = size >> 20;
        for k in 0..14u64 {
            let at = if k == 13 { mb.saturating_sub(1) } else { mb * k / 13 };
            let sha = crate::full::adb_shell(serial, &format!("dd if={dev} bs=1048576 skip={at} count=1 2>/dev/null | sha256sum | cut -d' ' -f1"))?;
            if sha.trim() != ZERO_MB {
                return Err(format!("{name} is not all zeros at {at} MB - stopping before Android boots"));
            }
        }
    }
    Ok(())
}

/// The return to Android, by the plan. `confirm` must be confirm_word's
/// word; `accept_losses`, the owner's word that what no backup has may go.
pub fn go(host: &str, plan: &Plan, confirm: &str, accept_losses: bool, say: crate::ramboot::Say) -> Result<(), String> {
    crate::link::need_cable(host)?;
    let _ = crate::phone::keep_awake(host, true);
    if !plan.stops.is_empty() {
        return Err(format!("stopped before anything: {}", plan.stops.join("; ")));
    }
    let serial = crate::backup::serial(host)?;
    if confirm.trim() != confirm_word(&serial) {
        return Err(format!("the confirmation does not match ({} expected) - nothing is changed", confirm_word(&serial)));
    }
    if !plan.losses.is_empty() && !accept_losses {
        return Err(format!("userdata holds what no backup has ({}) - say it may go, or back it up", plan.losses.iter().map(|l| l.0.as_str()).collect::<Vec<_>>().join(", ")));
    }
    let (kernel, build) = plan.kernel.clone().ok_or("no stock kernel")?;
    let stock = crate::ramboot::check_image(&kernel.path)?;
    let twrp = crate::full::twrp().ok_or("no TWRP image")?;
    let _ = crate::ramboot::check_image(&twrp)?;

    // The whole system, fresh and whole.
    let full = if plan.full_fresh {
        let b = plan.full.clone().expect("fresh");
        verify_full(&b, say)?;
        b
    } else {
        say("taking the whole system's backup first".into());
        crate::full::take(host, say)?
    };
    crate::flash::log(&serial, &format!("return to Android begun: the system backed up in {} - hythe", full.dir.display()))?;

    // Into TWRP, where the way back is tried and userdata erased.
    say("into TWRP".into());
    crate::ramboot::ram_boot(host, &twrp, crate::ramboot::Expect::Recovery, say)?;
    say("trying the way back (512 MB onto the phone, checked)".into());
    if let Err(e) = try_the_way_back(&serial) {
        let _ = crate::ramboot::leave_recovery(host, &serial, say);
        return Err(e);
    }
    crate::flash::log(&serial, &format!("ERASING metadata and userdata for stock Android ({}) - hythe", build.fingerprint))?;
    erase(&serial, say)?;
    crate::flash::log(&serial, "metadata and userdata zeroed and read back - hythe")?;

    // The stock kernel from RAM, on its slot.
    say("into the bootloader".into());
    crate::ramboot::adb_to_bootloader(&serial)?;
    remember_guest(&serial, &kernel.path, build.slot)?;
    crate::ramboot::boot_in_fastboot(host, &serial, build.slot, &kernel.path, &stock, crate::ramboot::Expect::Android, say)?;
    crate::flash::log(&serial, &format!("stock Android started from RAM ({}) - hythe", kernel.path.display()))?;
    say("Android is setting itself up on the phone. For Hythe later: Settings - About phone - tap Build number seven times - Developer options - USB debugging. Don't restart it plainly: the port's kernel is still on the slot and finds no system. For the bootloader: Volume Down + Power from off (or Hythe, with USB debugging on).".into());
    Ok(())
}

// ---- running: the guest Android started again ---------------------------------

/// Stock Android started again from RAM - it runs as a guest: the port's
/// kernel is still on the slot. The parking brake does not hold here: stock
/// Android clears misc as it boots (seen 2026-10-04), so a plain restart
/// starts the port's kernel, which finds no system on the erased userdata
/// and stops in its initramfs (USB 18d1:d001). Volume Down + Power from off
/// reaches the bootloader; this starts Android from there.
pub fn start(host: &str, serial: &str, say: crate::ramboot::Say) -> Result<(), String> {
    let (kernel, slot) = guest(serial).ok_or("Android was never started by Hythe on this phone: return to it first")?;
    let img = crate::ramboot::check_image(&kernel)?;
    if !crate::ramboot::in_fastboot(serial) {
        say("into the bootloader".into());
        crate::ramboot::adb_to_bootloader(serial)?;
    }
    crate::ramboot::boot_in_fastboot(host, serial, slot, &kernel, &img, crate::ramboot::Expect::Android, say)?;
    crate::flash::log(serial, &format!("stock Android started from RAM ({}) - hythe", kernel.display()))
}

// ---- coming back: from stock Android to Linux ---------------------------------

/// Back to Linux from Android (or from fastboot or TWRP on the way): TWRP
/// from RAM, userdata made ext4 again, the system and the container's data
/// put back from the newest full backup - in parts, each checked on the
/// phone - misc cleared, and the port started from its slot.
/// `data_only`: the system is back and checked already (a way back that
/// stopped after it) - only the container's data and the rest go, in TWRP.
pub fn back(host: &str, serial: &str, data_only: bool, say: crate::ramboot::Say) -> Result<(), String> {
    let full = crate::backup::list(Some(serial)).into_iter().find(|b| b.manifest.kind == Kind::Full).ok_or("no whole-system backup of this phone")?;
    let slot = full.manifest.slot.trim_start_matches('_').chars().next().unwrap_or('a');
    let twrp = crate::full::twrp().ok_or("no TWRP image")?;
    let twrp_img = crate::ramboot::check_image(&twrp)?;
    say(format!("coming back from the backup of {}", full.manifest.created));

    // Into TWRP, from wherever the phone is: Linux (a system installed
    // afresh), the recovery already, the bootloader, or Android.
    if data_only && !crate::ramboot::in_recovery(serial) {
        return Err("only the data was to go back, but the phone is not in the recovery".into());
    }
    if crate::ramboot::in_recovery(serial) {
    } else if crate::phone::answers_fresh(host) {
        crate::ramboot::ram_boot(host, &twrp, crate::ramboot::Expect::Recovery, say)?;
    } else if !crate::ramboot::in_recovery(serial) {
        if !crate::ramboot::in_fastboot(serial) {
            say("into the bootloader".into());
            crate::ramboot::adb_to_bootloader(serial)?;
        }
        crate::ramboot::boot_in_fastboot(host, serial, slot, &twrp, &twrp_img, crate::ramboot::Expect::Recovery, say)?;
    }
    if !crate::full::fast_tools(serial) {
        return Err("this TWRP lacks pigz or nc - the way back needs them".into());
    }

    if data_only {
        crate::full::adb_shell(serial, &format!("mkdir -p /tmp/ud; mountpoint -q /tmp/ud || mount -t ext4 {BLK}/userdata /tmp/ud; test -f /tmp/ud/rootfs.img && echo ok"))?;
    } else {
        say("making userdata ext4 again (Android's data goes)".into());
        crate::flash::log(serial, "back to Linux: userdata made ext4 again - hythe")?;
        crate::full::adb_shell(serial, &format!("umount /tmp/ud 2>/dev/null; mke2fs -F -t ext4 -L userdata {BLK}/userdata >/dev/null && mkdir -p /tmp/ud && mount -t ext4 {BLK}/userdata /tmp/ud && echo ok"))?;

        // The image in 512 MB parts; empty parts are left as holes.
        let image = full.manifest.items.iter().find(|i| i.file == "rootfs.img.gz").ok_or("the backup has no rootfs image")?;
        let file = std::fs::File::open(full.dir.join(&image.file)).map_err(|e| e.to_string())?;
        let mut dec = flate2::read::MultiGzDecoder::new(std::io::BufReader::new(file));
        let (total, whole) = put_file(serial, "rootfs.img", &mut dec, "image", say)?;
        if whole != image.sha256 {
            return Err("the backup's image does not match its manifest - stopping in TWRP".into());
        }
        crate::full::adb_shell(serial, &format!("truncate -s {total} /tmp/ud/rootfs.img && chmod 644 /tmp/ud/rootfs.img"))?;
        say("checking the whole image on the phone".into());
        let there = crate::full::adb_shell(serial, "sha256sum /tmp/ud/rootfs.img | cut -d' ' -f1")?;
        if there.trim() != image.sha256 {
            return Err("the image on the phone differs from the backup - stopping in TWRP; run back again".into());
        }

    }

    say("putting back the Android container's data".into());
    let data = full.manifest.items.iter().find(|i| i.file == "android-data.tar.gz").ok_or("the backup has no container data")?;
    crate::full::adb_shell(serial, "rm -rf /tmp/ud/android-data")?;
    put_archive(serial, &full.dir.join(&data.file), "/tmp/ud", say)?;
    if let Some(rest) = full.manifest.items.iter().find(|i| i.file == crate::full::REST) {
        say(format!("putting back the rest of userdata ({})", rest.source.trim_start_matches("/userdata: ")));
        put_archive(serial, &full.dir.join(&rest.file), "/tmp/ud", say)?;
    }
    crate::full::adb_shell(serial, "ln -sf /halium-system/var/lib/lxc/android/android-rootfs.img /tmp/ud/android-rootfs.img; sync; umount /tmp/ud")?;

    // A failed stock boot may have left --prompt_and_wipe_data in misc.
    say("clearing misc".into());
    crate::full::adb_shell(serial, &format!("dd if=/dev/zero of={BLK}/misc bs=2048 count=1 2>/dev/null; sync"))?;
    crate::flash::log(serial, &format!("back to Linux: the system put back from {} - hythe", full.manifest.created))?;

    say("starting the port".into());
    crate::full::adb_shell(serial, "reboot").ok();
    let start = std::time::Instant::now();
    while !crate::phone::answers_fresh(host) {
        if start.elapsed() > std::time::Duration::from_secs(360) {
            return Err("Linux did not come back in 6 minutes - look at the phone's screen".into());
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    crate::ramboot::arm_brake_linux(host)?;
    crate::flash::log(serial, "back in Linux, parking brake armed - hythe")?;
    let _ = std::fs::remove_file(guest_path(serial));
    fingers_back(host, full.manifest.fingers, say);
    say("Linux is back: enter the PIN on the phone".into());
    Ok(())
}

/// The fingers as before the way back: the reader's daemon asked once it is
/// up (it starts with the Android container); fewer is said, not an error.
fn fingers_back(host: &str, before: Option<u32>, say: crate::ramboot::Say) {
    let Some(before) = before.filter(|n| *n > 0) else { return };
    let start = std::time::Instant::now();
    let now = loop {
        if let Some(n) = crate::phone::fingers(host) {
            // Its list is read a moment after it answers.
            std::thread::sleep(std::time::Duration::from_secs(2));
            break crate::phone::fingers(host).or(Some(n));
        }
        if start.elapsed() > std::time::Duration::from_secs(120) {
            break None;
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    };
    match now {
        Some(n) if n >= before => say(format!("fingerprints back: {n}")),
        Some(n) => say(format!("the fingerprints did not all come back ({n} of {before}): enrol them again on the phone")),
        None => say("the fingerprint reader did not answer: check the fingers on the phone".into()),
    }
}

/// The phone away from Linux (in Android, fastboot or TWRP): of the phones
/// this computer has whole-system backups of, the one on the USB now.
pub fn away_serial() -> Option<String> {
    let root = crate::backup::list(None);
    let mut serials: Vec<String> = root.into_iter().filter(|b| b.manifest.kind == Kind::Full).map(|b| b.manifest.serial.clone()).collect();
    serials.sort();
    serials.dedup();
    serials.into_iter().find(|s| crate::ramboot::usb_serial_present(s))
}

/// The port's kernel up with no system to start (Halium's initramfs shows
/// itself on the USB as 18d1:d001): the guest Android restarted plainly.
pub fn port_without_system() -> bool {
    std::fs::read_dir("/sys/bus/usb/devices").is_ok_and(|d| {
        d.flatten().any(|e| {
            let read = |f: &str| std::fs::read_to_string(e.path().join(f)).map(|s| s.trim().to_owned()).unwrap_or_default();
            read("idVendor") == "18d1" && read("idProduct") == "d001"
        })
    })
}

/// A phone this computer has backed up, on the USB in a way adb and fastboot
/// do not see (Android without USB debugging, or starting): its serial.
pub fn on_usb_quietly() -> Option<String> {
    let dirs = std::fs::read_dir(crate::backup::root()).ok()?;
    dirs.flatten().filter_map(|e| e.file_name().into_string().ok()).find(|s| crate::ramboot::usb_serial_present(s))
}

/// A file put onto userdata (mounted at /tmp/ud in TWRP) in 512 MB parts,
/// each checked on the phone and sent again if it differs; parts all zeros
/// left as holes. Returns its size and sha256.
pub(crate) fn put_file(serial: &str, name: &str, from: &mut dyn std::io::Read, label: &str, say: crate::ramboot::Say) -> Result<(u64, String), String> {
    use sha2::{Digest, Sha256};
    // A name is on userdata; a path (from /) is anywhere.
    let path = if name.starts_with('/') { name.to_owned() } else { format!("/tmp/ud/{name}") };
    let part = 512usize << 20;
    let mut buf = vec![0u8; part];
    let mut whole = Sha256::new();
    let (mut index, mut total, mut sent) = (0u64, 0u64, 0u64);
    crate::full::adb_shell(serial, &format!("rm -f '{path}' && touch '{path}'"))?;
    loop {
        let mut filled = 0;
        while filled < part {
            let n = from.read(&mut buf[filled..]).map_err(|e| format!("the backup's {name}: {e}"))?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled == 0 {
            break;
        }
        let chunk = &buf[..filled];
        whole.update(chunk);
        total += filled as u64;
        if chunk.iter().any(|b| *b != 0) {
            let want = format!("{:x}", Sha256::digest(chunk));
            let seek = index * 512;
            let mut ok = false;
            for _ in 0..3 {
                // dd without conv= (TWRP's toybox has it disabled): it cuts
                // the file at the seek, harmless as parts go in order.
                crate::full::adb_send(serial, &format!("dd of='{path}' bs=1048576 seek={seek} 2>/dev/null"), &mut &chunk[..])?;
                let there = crate::full::adb_shell(serial, &format!("dd if='{path}' bs=1048576 skip={seek} count={} 2>/dev/null | sha256sum | cut -d' ' -f1", filled.div_ceil(1 << 20)))?;
                if there.trim() == want {
                    ok = true;
                    break;
                }
            }
            if !ok {
                return Err(format!("part {index} of {name} did not arrive whole in three tries - the phone stays in TWRP; run back again"));
            }
            sent += filled as u64;
        }
        index += 1;
        if index % 8 == 0 {
            say(format!("  {label}: {} of it put back ({} sent)", crate::status::size_words(total / 1024), crate::status::size_words(sent / 1024)));
        }
    }
    crate::full::adb_shell(serial, &format!("truncate -s {total} '{path}'"))?;
    Ok((total, format!("{:x}", whole.finalize())))
}

/// A tar.gz put back under `root` in TWRP. Not all through tar there: its
/// toybox tar writes a big file from a stream short or empty and says all
/// went well (an 8 GB image; 31 MB logs in the container's data -
/// 2026-10-04). Small files go in one tar stream, big ones part by part,
/// checked; then every file's size is checked.
pub(crate) fn put_archive(serial: &str, archive: &Path, root: &str, say: crate::ramboot::Say) -> Result<(), String> {
    use std::io::Read;
    const BIG: u64 = 4 << 20;
    let open = || -> Result<tar::Archive<flate2::read::MultiGzDecoder<std::io::BufReader<std::fs::File>>>, String> {
        let f = std::fs::File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
        Ok(tar::Archive::new(flate2::read::MultiGzDecoder::new(std::io::BufReader::new(f))))
    };
    let mut small = tar::Builder::new(Vec::new());
    let (mut big, mut sizes) = (Vec::new(), String::new());
    let mut a = open()?;
    for entry in a.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.to_string_lossy().trim_start_matches("./").to_owned();
        if path.contains('\'') || path.contains('\n') || path.starts_with('/') || path.contains("..") {
            return Err(format!("a strange name in the backup: {path}"));
        }
        let header = entry.header().clone();
        let size = header.size().unwrap_or(0);
        let regular = header.entry_type() == tar::EntryType::Regular;
        if regular && size > BIG {
            big.push((path, size, header.mode().unwrap_or(0o644), header.uid().unwrap_or(0), header.gid().unwrap_or(0)));
            continue;
        }
        if regular {
            sizes.push_str(&format!("{size} {root}/{path}\n"));
        }
        let mut h = header.clone();
        if header.entry_type() == tar::EntryType::Symlink || header.entry_type() == tar::EntryType::Link {
            let target = entry.link_name().map_err(|e| e.to_string())?.unwrap_or_default().into_owned();
            small.append_link(&mut h, &path, &target).map_err(|e| format!("{path}: {e}"))?;
        } else {
            let mut body = Vec::new();
            entry.read_to_end(&mut body).map_err(|e| e.to_string())?;
            small.append_data(&mut h, &path, &body[..]).map_err(|e| format!("{path}: {e}"))?;
        }
    }
    let small = small.into_inner().map_err(|e| e.to_string())?;
    crate::full::adb_shell(serial, &format!("mkdir -p '{root}'"))?;
    // From a file, not a pipe: from a pipe TWRP's tar cut a 1.3 MB file
    // short and stopped there, quietly (2026-10-04).
    crate::full::push_file(serial, &small, "/tmp/hythe-small.tar")?;
    crate::full::adb_shell(serial, &format!("tar -C '{root}' -xpf /tmp/hythe-small.tar; rc=$?; rm -f /tmp/hythe-small.tar; exit $rc"))?;
    for (path, size, mode, uid, gid) in &big {
        say(format!("putting back {path} ({} MB)", size >> 20));
        let mut a = open()?;
        let mut done = false;
        for entry in a.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            if entry.path().map(|p| p.to_string_lossy().trim_start_matches("./") == path.as_str()).unwrap_or(false) {
                let target = format!("{root}/{path}");
                crate::full::adb_shell(serial, &format!("mkdir -p \"$(dirname '{target}')\""))?;
                put_file(serial, &target, &mut entry, path, say)?;
                crate::full::adb_shell(serial, &format!("chmod {mode:o} '{target}'; chown {uid}:{gid} '{target}'"))?;
                done = true;
                break;
            }
        }
        if !done {
            return Err(format!("{path} went missing from the backup"));
        }
    }
    crate::full::adb_send(serial, "cat > /tmp/hythe-sizes", &mut sizes.as_bytes())?;
    let bad = crate::full::adb_shell(serial, "n=0; while read s p; do [ \"$(stat -c %s \"$p\" 2>/dev/null)\" = \"$s\" ] || n=$((n+1)); done < /tmp/hythe-sizes; rm -f /tmp/hythe-sizes; echo $n")?;
    if bad.trim() != "0" {
        return Err(format!("{} files did not arrive whole - stopping in TWRP; run back again", bad.trim()));
    }
    Ok(())
}

/// The newest whole-system backup of this phone, if it is fresh: taken
/// within the hour and after the phone's last boot.
pub fn fresh_full(host: &str, serial: &str) -> Result<Option<Backup>, String> {
    let full = crate::backup::list(Some(serial)).into_iter().find(|b| b.manifest.kind == Kind::Full);
    let times: Vec<i64> = crate::phone::run(host, "date +%s; awk '/^btime/ {print $2}' /proc/stat\n")?.lines().filter_map(|l| l.trim().parse().ok()).collect();
    let (phone_now, booted) = (times.first().copied().unwrap_or(0), times.get(1).copied().unwrap_or(0));
    Ok(full.filter(|b| {
        let taken = local_secs(&b.manifest.created);
        taken > booted && phone_now - taken < FRESH_SECS
    }))
}
