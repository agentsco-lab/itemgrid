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
    r#"for s in a b; do d=/tmp/itemgrid-super-$s; mkdir -p $d
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
    let img = crate::bootimg::BootImage::parse(&std::fs::read(path).ok()?).ok()?;
    let k = img.kernel.as_slice();
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

/// Where stock kernels are looked for: item/grid's own stock folder, its
/// backups, and the port's out/backups/.
fn kernel_places() -> Vec<PathBuf> {
    let mut dirs = vec![crate::paths::data().join("stock"), crate::backup::root()];
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
    let mut builds = super_builds(host)?;
    // Android's own slot first: the one the phone starts (Linux runs from
    // it), then the newer build - after an update both slots hold one, and
    // the old one is only Android's spare.
    let cmdline = crate::phone::run(host, "cat /proc/cmdline\n")?;
    let running = cmdline.split_whitespace().find_map(|w| w.strip_prefix("androidboot.slot_suffix=_")).and_then(|s| s.chars().next());
    builds.sort_by_key(|b| (Some(b.slot) != running, std::cmp::Reverse(b.vendor_utc)));
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
        None => stops.push("no stock kernel from the build in super on this computer (~/.local/share/itemgrid/stock/, or the backup taken before the port went on)".into()),
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
    if crate::full::recovery().is_none() {
        stops.push("no TWRP image: it is needed to erase and to come back".into());
    }
    Ok(Plan { builds, kernel, kernels_seen, battery: st.battery, full, full_fresh, device_data, losses, stops })
}

/// A manifest's local "YYYY-MM-DD HH:MM:SS" as UTC seconds.
fn local_secs(stamp: &str) -> i64 {
    crate::clock::local_secs(stamp)
}

/// UTC seconds in words.
pub fn when(utc: i64) -> String {
    crate::clock::when_utc(utc)
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
    crate::paths::config().join("android").join(format!("{serial}.json"))
}

fn remember_guest(serial: &str, kernel: &Path, slot: char) -> Result<(), String> {
    let path = guest_path(serial);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let body = serde_json::json!({"kernel": kernel, "slot": slot.to_string()});
    std::fs::write(&path, body.to_string()).map_err(|e| format!("{}: {e}", path.display()))
}

/// The guest's kernel and slot, if item/grid started Android on this phone.
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
    let mut block = Sha256::digest(b"itemgrid way back").to_vec();
    while (data.len() as u64) < TEST_BYTES {
        block = Sha256::digest(&block).to_vec();
        data.extend_from_slice(&block);
    }
    let want = format!("{:x}", Sha256::digest(&data));
    let rec = crate::rec::Rec::on(serial);
    let trial = "/tmp/itemgrid-way-back";
    rec.run(&["rm", trial], &format!("rm -f {trial}"))?;
    rec.send(&["put", trial], &format!("cat > {trial}"), &mut data.as_slice())?;
    let there = rec.hash(trial, None);
    rec.run(&["rm", trial], &format!("rm -f {trial}"))?;
    if there? != want {
        return Err("the way back failed its trial (what arrived differs) - nothing is erased".into());
    }
    Ok(())
}

/// metadata and userdata zeroed whole, from TWRP - a zeroed start is not
/// enough: ext4's backup superblocks could bring the port's filesystem back
/// under Android's first boot, and Android would poison misc
/// (STOCK-ANDROID.md). Then a few places read back: all zeros.
fn erase(serial: &str, say: crate::ramboot::Say) -> Result<(), String> {
    let rec = crate::rec::Rec::on(serial);
    let _ = rec.umount("/tmp/ud");
    for name in ["metadata", "userdata"] {
        let link = format!("{BLK}/{name}");
        let dev = rec.run(&["real", &link], &format!("readlink -f {link}"))?.trim().to_owned();
        if !dev.starts_with("/dev/block/sd") {
            return Err(format!("{name} is not where it should be ({dev}): stopping"));
        }
        say(format!("zeroing {name} ({dev}) whole{}", if name == "userdata" { " - about 7 minutes" } else { "" }));
        rec.zero(&dev, None)?;
        // Read back at the start, the end and between.
        let mb = rec.size(&dev)? >> 20;
        for k in 0..14u64 {
            let at = if k == 13 { mb.saturating_sub(1) } else { mb * k / 13 };
            if rec.hash(&dev, Some((at << 20, 1 << 20)))? != ZERO_MB {
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
    let twrp = crate::full::recovery().ok_or("no recovery image (tools/build-recovery.sh, or a release with item-recovery.img)")?;
    let _ = crate::ramboot::check_image(&twrp)?;

    // A whole-system backup is used when a fresh one is there, and not
    // taken otherwise (18 minutes nobody needed, the owner 2026-10-23): the
    // way back to Linux is then a fresh install from a release image (the
    // device data and the quick backup carry what is the owner's).
    let backed = match plan.full.as_ref().filter(|_| plan.full_fresh) {
        Some(b) => {
            verify_full(b, say)?;
            format!("the system backed up in {}", b.dir.display())
        }
        None => {
            say("no whole-system backup: the way back to Linux is a fresh install".into());
            "no whole-system backup".to_owned()
        }
    };
    crate::flash::log(&serial, &format!("return to Android begun: {backed} - itemgrid"))?;

    // Into TWRP, where the way back is tried and userdata erased.
    say("into the recovery".into());
    crate::ramboot::ram_boot(host, &twrp, crate::ramboot::Expect::Recovery, say)?;
    crate::screen::begin(&serial, &["test", "erase", "android"]);
    say("trying the way back (512 MB onto the phone, checked)".into());
    crate::screen::step(0, "testing", "512 mb|nothing kept");
    if let Err(e) = try_the_way_back(&serial) {
        crate::screen::error(&e);
        let _ = crate::ramboot::leave_recovery(host, &serial, say);
        return Err(e);
    }
    crate::flash::log(&serial, &format!("ERASING metadata and userdata for stock Android ({}) - itemgrid", build.fingerprint))?;
    crate::screen::step(1, "erasing linux", "about 7 min");
    if let Err(e) = erase(&serial, say) {
        crate::screen::error(&e);
        return Err(e);
    }
    crate::flash::log(&serial, "metadata and userdata zeroed and read back - itemgrid")?;
    crate::screen::done("android next", "set it up|on the phone");

    // The stock kernel from RAM, on its slot.
    say("into the bootloader".into());
    crate::ramboot::adb_to_bootloader(&serial)?;
    remember_guest(&serial, &kernel.path, build.slot)?;
    crate::ramboot::boot_in_fastboot(host, &serial, build.slot, &kernel.path, &stock, crate::ramboot::Expect::Android, say)?;
    crate::flash::log(&serial, &format!("stock Android started from RAM ({}) - itemgrid", kernel.path.display()))?;
    say("Android is setting itself up on the phone. For item/grid later: Settings - About phone - tap Build number seven times - Developer options - USB debugging. Don't restart it plainly: the port's kernel is still on the slot and finds no system. For the bootloader: Volume Down + Power from off (or item/grid, with USB debugging on).".into());
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
    let (kernel, slot) = guest(serial).ok_or("Android was never started by item/grid on this phone: return to it first")?;
    let img = crate::ramboot::check_image(&kernel)?;
    if !crate::ramboot::in_fastboot(serial) {
        say("into the bootloader".into());
        crate::ramboot::adb_to_bootloader(serial)?;
    }
    crate::ramboot::boot_in_fastboot(host, serial, slot, &kernel, &img, crate::ramboot::Expect::Android, say)?;
    crate::flash::log(serial, &format!("stock Android started from RAM ({}) - itemgrid", kernel.display()))
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
    let twrp = crate::full::recovery().ok_or("no recovery image (tools/build-recovery.sh, or a release with item-recovery.img)")?;
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
    let rec = crate::rec::Rec::on(serial);
    if !rec.ours() && !crate::full::fast_tools(serial) {
        return Err("this TWRP lacks pigz or nc - the way back needs them".into());
    }
    // Its steps on the phone's screen (item/grid's own recovery).
    crate::screen::begin(serial, &["clean", "linux", "check", "data", "restart"]);
    let r = (|| -> Result<(), String> {
        let userdata = format!("{BLK}/userdata");

        if data_only {
            rec.mount(&userdata, "/tmp/ud", false)?;
            rec.size("/tmp/ud/rootfs.img").map_err(|_| "no rootfs.img on userdata: only the data cannot go back".to_owned())?;
        } else {
            say("making userdata ext4 again (Android's data goes)".into());
            crate::screen::step(0, "cleaning", "for linux");
            crate::flash::log(serial, "back to Linux: userdata made ext4 again - itemgrid")?;
            rec.make_ext4(&userdata, "userdata", "/tmp/ud")?;

            // The image in 4 MB pieces (put_file); empty ones left as holes.
            crate::screen::step(1, "writing item", "from the backup");
            let image = full.manifest.items.iter().find(|i| i.file == "rootfs.img.gz").ok_or("the backup has no rootfs image")?;
            let file = std::fs::File::open(full.dir.join(&image.file)).map_err(|e| e.to_string())?;
            let mut dec = flate2::read::MultiGzDecoder::new(std::io::BufReader::new(file));
            let (total, whole) = put_file(serial, "rootfs.img", &mut dec, "image", false, say)?;
            if whole.is_some_and(|w| w != image.sha256) {
                return Err("the backup's image does not match its manifest - stopping in TWRP".into());
            }
            rec.run(&["meta", "/tmp/ud/rootfs.img", "644", "0", "0"], &format!("truncate -s {total} /tmp/ud/rootfs.img && chmod 644 /tmp/ud/rootfs.img"))?;
            say("checking the whole image on the phone".into());
            crate::screen::step(2, "checking", "every byte");
            if rec.hash_written("/tmp/ud/rootfs.img")? != image.sha256 {
                return Err("the image on the phone differs from the backup - stopping in TWRP; run back again".into());
            }

        }

        say("putting back the Android container's data".into());
        crate::screen::step(3, "android data", "");
        let data = full.manifest.items.iter().find(|i| i.file == "android-data.tar.gz").ok_or("the backup has no container data")?;
        rec.run(&["rm", "/tmp/ud/android-data"], "rm -rf /tmp/ud/android-data")?;
        put_archive(serial, &full.dir.join(&data.file), "/tmp/ud", say)?;
        if let Some(rest) = full.manifest.items.iter().find(|i| i.file == crate::full::REST) {
            say(format!("putting back the rest of userdata ({})", rest.source.trim_start_matches("/userdata: ")));
            put_archive(serial, &full.dir.join(&rest.file), "/tmp/ud", say)?;
        }
        rec.run(&["link", "/halium-system/var/lib/lxc/android/android-rootfs.img", "/tmp/ud/android-rootfs.img"], "ln -sf /halium-system/var/lib/lxc/android/android-rootfs.img /tmp/ud/android-rootfs.img")?;
        rec.umount("/tmp/ud")?;

        // A failed stock boot may have left --prompt_and_wipe_data in misc.
        say("clearing misc".into());
        rec.zero(&format!("{BLK}/misc"), Some(2048))?;
        crate::flash::log(serial, &format!("back to Linux: the system put back from {} - itemgrid", full.manifest.created))?;
        Ok(())
    })();
    match &r {
        Ok(()) => crate::screen::done("linux next", "enter your pin"),
        Err(e) => crate::screen::error(e),
    }
    r?;

    say("starting the port".into());
    rec.restart();
    let start = std::time::Instant::now();
    while !crate::phone::answers_fresh(host) {
        if start.elapsed() > std::time::Duration::from_secs(360) {
            return Err("Linux did not come back in 6 minutes - look at the phone's screen".into());
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    crate::ramboot::arm_brake_linux(host)?;
    crate::flash::log(serial, "back in Linux, parking brake armed - itemgrid")?;
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
    crate::usb::present(crate::usb::INITRAMFS)
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
/// `whole`: the file's sha256 worked out here as it is read (the release
/// image's, against its manifest); without it none (a backup's image is
/// checked on the phone, from its flash, after).
pub(crate) fn put_file(serial: &str, name: &str, from: &mut dyn std::io::Read, label: &str, whole: bool, say: crate::ramboot::Say) -> Result<(u64, Option<String>), String> {
    // A name is on userdata; a path (from /) is anywhere.
    let path = if name.starts_with('/') { name.to_owned() } else { format!("/tmp/ud/{name}") };
    let rec = crate::rec::Rec::on(serial);
    if rec.ours() {
        return put_pieces(&rec, &path, from, label, whole, say);
    }
    put_file_twrp(serial, &path, name, from, label, say).map(|(n, sha)| (n, Some(sha)))
}

/// item-recovery's way: the file in 4 MB pieces, those all zeros not
/// sent (left as holes), the rest gzipped on every core here and sent as
/// `unsparse` records in batches of 64 pieces (a batch whose sending
/// failed sent again, three times at most), each piece checked against
/// its sha256 there. The 512 MB parts before, nearly never all zeros
/// (ext4 inside the image spreads its tables), sent 26 of 86 GB.
fn put_pieces(rec: &crate::rec::Rec, path: &str, from: &mut dyn std::io::Read, label: &str, whole: bool, say: crate::ramboot::Say) -> Result<(u64, Option<String>), String> {
    use sha2::{Digest, Sha256};
    const PIECE: usize = 4 << 20;
    const BATCH: usize = 64;
    rec.rm(path)?;
    let mut hash = whole.then(Sha256::new);
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let (mut total, mut sent, mut next_note) = (0u64, 0u64, 4u64 << 30);
    let send = |payload: &[u8], say: &mut dyn FnMut(String)| -> Result<(), String> {
        let mut last = String::new();
        for _ in 0..3 {
            match rec.send(&["unsparse", path], "", &mut &payload[..]) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    say(format!("  {label}: {e} - sending it again"));
                    last = e;
                    std::thread::sleep(std::time::Duration::from_secs(3));
                }
            }
        }
        Err(format!("{label} did not arrive whole in three tries ({last}) - the phone stays in the recovery; run it again"))
    };
    loop {
        let mut pieces: Vec<(u64, Vec<u8>)> = Vec::new();
        let mut end = false;
        for _ in 0..BATCH {
            let mut buf = vec![0u8; PIECE];
            let mut filled = 0;
            while filled < PIECE {
                let n = from.read(&mut buf[filled..]).map_err(|e| format!("{label}: {e}"))?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            buf.truncate(filled);
            if let Some(h) = hash.as_mut() {
                h.update(&buf);
            }
            let at = total;
            total += filled as u64;
            if buf.iter().any(|&b| b != 0) {
                pieces.push((at, buf));
            }
            if filled < PIECE {
                end = true;
                break;
            }
        }
        if !pieces.is_empty() {
            send(&piece_records(&pieces, cores), &mut *say)?;
            sent += pieces.iter().map(|(_, r)| r.len() as u64).sum::<u64>();
        }
        crate::screen::progress(&format!("writing {label}"), total);
        if total >= next_note || end {
            say(format!("  {label}: {} of it put back ({} sent)", crate::status::size_words(total / 1024), crate::status::size_words(sent / 1024)));
            next_note = total + (4 << 30);
        }
        if end {
            break;
        }
    }
    let mut e = vec![b'E'];
    e.extend_from_slice(&total.to_le_bytes());
    e.extend_from_slice(&[0u8; 32]);
    send(&e, &mut *say)?;
    Ok((total, hash.map(|h| format!("{:x}", h.finalize()))))
}

/// `unsparse` records for `pieces` (offset, bytes): gzipped on every core,
/// in order.
pub(crate) fn piece_records(pieces: &[(u64, Vec<u8>)], cores: usize) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let share = pieces.len().div_ceil(cores);
    let records: Vec<Vec<u8>> = std::thread::scope(|s| {
        let jobs: Vec<_> = pieces
            .chunks(share)
            .map(|chunk| {
                s.spawn(move || {
                    chunk
                        .iter()
                        .map(|(at, raw)| {
                            let mut e = flate2::write::GzEncoder::new(Vec::with_capacity(raw.len() / 4), flate2::Compression::fast());
                            let _ = e.write_all(raw);
                            let gz = e.finish().unwrap_or_default();
                            let mut r = Vec::with_capacity(49 + gz.len());
                            r.push(b'D');
                            r.extend_from_slice(&at.to_le_bytes());
                            r.extend_from_slice(&(raw.len() as u32).to_le_bytes());
                            r.extend_from_slice(&(gz.len() as u32).to_le_bytes());
                            r.extend_from_slice(&Sha256::digest(raw));
                            r.extend_from_slice(&gz);
                            r
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        jobs.into_iter().flat_map(|j| j.join().unwrap_or_default()).collect()
    });
    records.concat()
}

/// A TWRP's way: 512 MB parts by dd, each read back and hashed there.
fn put_file_twrp(serial: &str, path: &str, name: &str, from: &mut dyn std::io::Read, label: &str, say: crate::ramboot::Say) -> Result<(u64, String), String> {
    use sha2::{Digest, Sha256};
    let path = path.to_owned();
    let part = 512usize << 20;
    let mut buf = vec![0u8; part];
    let mut whole = Sha256::new();
    let (mut index, mut total, mut sent) = (0u64, 0u64, 0u64);
    let rec = crate::rec::Rec::on(serial);
    rec.run(&["rm", &path], &format!("rm -f '{path}' && touch '{path}'"))?;
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
        crate::screen::progress(&format!("writing {label}"), total);
        if chunk.iter().any(|b| *b != 0) {
            let want = format!("{:x}", Sha256::digest(chunk));
            let seek = index * 512;
            let mut ok = false;
            for _ in 0..3 {
                // dd without conv= (TWRP's toybox has it disabled): it cuts
                // the file at the seek, harmless as parts go in order.
                // A part whose sending failed (the cable, the phone's adb
                // restarting) is sent again, as one that arrived different.
                let sent = rec.send(&["put", &path, &(seek << 20).to_string()], &format!("dd of='{path}' bs=1048576 seek={seek} 2>/dev/null"), &mut &chunk[..]);
                if let Err(e) = sent {
                    say(format!("  part {index}: {e} - sending it again"));
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    continue;
                }
                if rec.hash(&path, Some((seek << 20, filled as u64))).is_ok_and(|h| h == want) {
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
    rec.run(&["truncate", "-s", &total.to_string(), &path], &format!("truncate -s {total} '{path}'"))?;
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
    let rec = crate::rec::Rec::on(serial);
    rec.untar(root, &small)?;
    for (path, size, mode, uid, gid) in &big {
        say(format!("putting back {path} ({} MB)", size >> 20));
        let mut a = open()?;
        let mut done = false;
        for entry in a.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            if entry.path().map(|p| p.to_string_lossy().trim_start_matches("./") == path.as_str()).unwrap_or(false) {
                let target = format!("{root}/{path}");
                let parent = target.rsplit_once('/').map_or(root, |(d, _)| d);
                rec.mkdir(parent)?;
                put_file(serial, &target, &mut entry, path, false, say)?;
                rec.meta(&target, *mode, *uid, *gid)?;
                done = true;
                break;
            }
        }
        if !done {
            return Err(format!("{path} went missing from the backup"));
        }
    }
    let bad = rec.check_sizes(&sizes)?;
    if bad != 0 {
        return Err(format!("{bad} files did not arrive whole - stopping in the recovery; run back again"));
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

// ---- clean: the phone as it came, for good ------------------------------------

/// What a clean return to Android would do (reading only): the build in
/// super, what the port changed of its slot put back from this computer -
/// the stock kernel that belongs to the build, and the stock vbmeta beside
/// it (Microsoft's package's, or the backup taken before the port went on;
/// dtbo the port never changes: left) - and what stops it.
#[derive(Debug, Clone)]
pub struct CleanPlan {
    pub build: Option<SuperBuild>,
    pub chain: Vec<crate::bootchain::Part>,
    pub battery: Option<u32>,
    pub stops: Vec<String>,
}

/// The clean return's plan, from the running port.
pub fn clean_plan(host: &str) -> Result<CleanPlan, String> {
    let p = plan(host)?;
    let mut stops: Vec<String> = Vec::new();
    // What the old way needed and this one does not: the device data's
    // backup (radio calibration, IMEI - left on the phone, not touched).
    stops.extend(p.stops.iter().filter(|s| !s.contains("device data")).cloned());
    let mut chain = Vec::new();
    if let Some((k, build)) = &p.kernel {
        chain.push(crate::bootchain::Part::of("boot", &k.path)?);
        // Its vbmeta, by the names a package's and a backup's have: a stock
        // one only (the port's has verification off).
        let dir = k.path.parent().map(Path::to_path_buf).unwrap_or_default();
        let slot = build.slot;
        let vbmeta = [format!("vbmeta_{slot}-stock-backup.img"), "vbmeta.img".to_owned()].into_iter().map(|n| dir.join(n)).find(|f| crate::bootchain::vbmeta_flags(f) == Some(0));
        match vbmeta {
            Some(f) => chain.push(crate::bootchain::Part::of("vbmeta", &f)?),
            None => stops.push(format!("no stock vbmeta for slot {slot} beside the stock kernel ({})", dir.display())),
        }
        // Linux runs from Android's slot: the slot written is the one the
        // phone starts (no slot switched).
        let cmdline = crate::phone::run(host, "cat /proc/cmdline\n")?;
        let running = cmdline.split_whitespace().find_map(|w| w.strip_prefix("androidboot.slot_suffix=_")).and_then(|s| s.chars().next());
        if running != Some(slot) {
            stops.push(format!("Linux runs from slot {}, Android's build is on slot {slot}: not switched here", running.map_or("?".to_owned(), |c| c.to_string())));
        }
    }
    Ok(CleanPlan { build: p.kernel.map(|(_, b)| b), chain, battery: p.battery, stops })
}

/// Back to the phone's stock Android for good, as it came: TWRP from RAM,
/// metadata and userdata zeroed (Linux and all it held go), the stock boot
/// and vbmeta written to Android's slot - the one the phone starts (the
/// other slot holds no Android: not touched) - misc cleared, and a restart.
/// About ten minutes. `confirm` must be confirm_word's word.
pub fn go_clean(host: &str, plan: &CleanPlan, confirm: &str, say: crate::ramboot::Say) -> Result<(), String> {
    crate::link::need_cable(host)?;
    let _ = crate::phone::keep_awake(host, true);
    if !plan.stops.is_empty() {
        return Err(format!("stopped before anything: {}", plan.stops.join("; ")));
    }
    let serial = crate::backup::serial(host)?;
    if confirm.trim() != confirm_word(&serial) {
        return Err(format!("the confirmation does not match ({} expected) - nothing is changed", confirm_word(&serial)));
    }
    let build = plan.build.clone().ok_or("no Android build in super")?;
    // The stock kernel checked as any image the phone is given.
    let boot = plan.chain.iter().find(|p| p.name == "boot").ok_or("no stock boot")?;
    crate::ramboot::check_image(&boot.file)?;
    let twrp = crate::full::recovery().ok_or("no recovery image (tools/build-recovery.sh, or a release with item-recovery.img)")?;
    let _ = crate::ramboot::check_image(&twrp)?;
    crate::flash::log(&serial, &format!("clean return to Android begun ({}) - itemgrid", build.fingerprint))?;

    say("into the recovery".into());
    crate::ramboot::ram_boot(host, &twrp, crate::ramboot::Expect::Recovery, say)?;
    // Its steps on the phone's screen (item/grid's own recovery).
    crate::screen::begin(&serial, &["erase", "android", "restart"]);
    let r = (|| -> Result<(), String> {
        crate::flash::log(&serial, &format!("ERASING metadata and userdata for stock Android ({}) - itemgrid", build.fingerprint))?;
        crate::screen::step(0, "erasing linux", "about 7 min");
        erase(&serial, say)?;
        say(format!("the stock boot on slot {}", build.slot));
        crate::screen::step(1, "android boot", &format!("slot {}", build.slot));
        crate::bootchain::write(&serial, build.slot, &plan.chain, say)?;
        say("clearing misc".into());
        crate::rec::Rec::on(&serial).zero(&format!("{BLK}/misc"), Some(2048))
    })();
    if let Err(e) = r {
        crate::screen::error(&e);
        return Err(e);
    }
    let _ = std::fs::remove_file(guest_path(&serial));
    crate::flash::log(&serial, "the phone is stock Android again, for good - itemgrid")?;
    crate::screen::done("android next", "its welcome|in a few min");
    say("starting Android: it sets itself up (a few minutes), its welcome screens open".into());
    std::thread::sleep(std::time::Duration::from_secs(2));
    crate::rec::Rec::on(&serial).restart();
    Ok(())
}

/// The clean return finished from the recovery, when it stopped after
/// userdata was erased (the stock boot not yet written): the boot and
/// vbmeta that last brought this phone back to Android onto the slot the
/// phone runs, read back; misc cleared; a restart into Android. The
/// phone `serial` must be in the recovery. Returns the plan's words when
/// `go` is false.
pub fn finish_clean(serial: &str, go: bool, say: crate::ramboot::Say) -> Result<String, String> {
    if !crate::ramboot::in_recovery(serial) {
        return Err("the phone is not in the recovery".into());
    }
    let rec = crate::rec::Rec::on(serial);
    let cmdline = rec.read_text("/proc/cmdline")?;
    let slot = cmdline.split_whitespace().find_map(|w| w.strip_prefix("androidboot.slot_suffix=_")).and_then(|s| s.chars().next()).ok_or("the slot the phone runs was not read")?;
    let ud = rec.real(&format!("{BLK}/userdata"))?;
    if rec.hash(&ud, Some((0, 1 << 20)))? != ZERO_MB {
        return Err("userdata is not erased: this finishes only a return that erased it".into());
    }
    // The boot and vbmeta that last brought this phone back to Android
    // (its log: written before "stock Android again"), not any stock build
    // here - a boot of another build than super's loops (2026-10-25).
    let (boot, vbmeta) = last_stock_pair(serial).ok_or("no boot and vbmeta in this phone's log that brought it back to Android")?;
    let chain = vec![crate::bootchain::Part::of("boot", &boot)?, crate::bootchain::Part::of("vbmeta", &vbmeta)?];
    if crate::bootchain::vbmeta_flags(&vbmeta) != Some(0) {
        return Err(format!("{}: not a stock vbmeta (verification off)", vbmeta.display()));
    }
    crate::ramboot::check_image(&chain[0].file)?;
    let plan = format!(
        "Finish the return to Android\n\nPhone:   {serial}, in the recovery, userdata erased, runs slot {}\nWrites:  {}\nThen:    misc cleared, a restart into Android",
        slot.to_ascii_uppercase(),
        chain.iter().map(|p| format!("{}_{slot} from {} ({})", p.name, p.file.display(), &p.sha256[..16])).collect::<Vec<_>>().join(", ")
    );
    if !go {
        return Ok(plan);
    }
    crate::screen::begin(serial, &["erase", "android", "restart"]);
    crate::screen::step(1, "android boot", &format!("slot {slot}"));
    let r = crate::bootchain::write(serial, slot, &chain, say).and_then(|_| rec.zero(&format!("{BLK}/misc"), Some(2048)));
    if let Err(e) = r {
        crate::screen::error(&e);
        return Err(e);
    }
    let _ = std::fs::remove_file(guest_path(serial));
    crate::flash::log(serial, "the return to Android finished from the recovery - itemgrid")?;
    crate::screen::done("android next", "its welcome|in a few min");
    say("starting Android: it sets itself up (a few minutes)".into());
    std::thread::sleep(std::time::Duration::from_secs(2));
    rec.restart();
    Ok(plan)
}

/// From the phone's log: the boot_a and vbmeta_a files written last before
/// "the phone is stock Android again" (that return worked), still here.
fn last_stock_pair(serial: &str) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let events = crate::flash::history(serial);
    let done = events.iter().rposition(|e| e.contains("stock Android again"))?;
    let file = |part: &str| {
        events[..done].iter().rev().find_map(|e| e.strip_prefix(&format!("wrote {part}_a from ")).and_then(|r| r.split(" (").next()).map(std::path::PathBuf::from))
    };
    let (b, v) = (file("boot")?, file("vbmeta")?);
    (b.exists() && v.exists()).then_some((b, v))
}

#[cfg(test)]
mod pieces_tests {
    /// Records made here taken by item-recovery's `unsparse` (the built
    /// program, if it is there): the same file.
    #[test]
    fn pieces_there() {
        use std::io::Write;
        let bin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/item-recovery");
        if !bin.exists() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("itemgrid-pieces-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut want = vec![0u8; 13 << 20];
        want[5..9].copy_from_slice(b"item");
        want[(8 << 20) + 1] = 9;
        let pieces: Vec<(u64, Vec<u8>)> = vec![(0, want[..4 << 20].to_vec()), (8 << 20, want[8 << 20..12 << 20].to_vec())];
        let mut stream = super::piece_records(&pieces, 3);
        stream.push(b'E');
        stream.extend_from_slice(&(want.len() as u64).to_le_bytes());
        stream.extend_from_slice(&[0u8; 32]);
        let out = dir.join("f");
        let mut child = std::process::Command::new(&bin).arg("unsparse").arg(&out).stdin(std::process::Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(&stream).unwrap();
        assert!(child.wait().unwrap().success());
        assert!(std::fs::read(&out).unwrap() == want);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
