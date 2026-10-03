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
    r#"for s in a b; do d=/tmp/cradle-super-$s; mkdir -p $d
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

/// Where stock kernels are looked for: Cradle's own stock folder, its
/// backups, and the port's out/backups/.
fn kernel_places() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let mut dirs = vec![home.join(".local/share/cradle/stock"), crate::backup::root()];
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
    let losses: Vec<(String, u64)> = listing
        .lines()
        .filter_map(|l| l.split_once(' '))
        .filter(|(_, name)| !matches!(*name, "rootfs.img" | "android-data" | "lost+found"))
        .map(|(s, name)| (name.to_owned(), s.parse().unwrap_or(0)))
        .collect();
    let mut stops = Vec::new();
    if builds.is_empty() {
        stops.push("no Android build found in super - nothing to return to".into());
    }
    match &kernel {
        None => stops.push("no stock kernel from the build in super on this computer (~/.local/share/cradle/stock/, or the backup taken before the port went on)".into()),
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
