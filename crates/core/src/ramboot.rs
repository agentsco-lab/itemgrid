//! A boot image tried from RAM (fastboot boot) - the port's docs/SAFETY.md
//! and tools/flash-safely.sh, as code. Nothing is flashed here; the slots
//! are not touched. The writes are the parking brake into misc - the safe
//! partition (LUN 0), backed up with the boot chain - and the state shared
//! with flash-safely.sh.
//!
//! In order, each a stop if it fails:
//!  1. the image checked on this computer: Android boot header v2, a kernel
//!     with ARM64's magic, an embedded DTB with FDT's, an os version, and
//!     androidboot.hardware=surfaceduo;
//!  2. on Linux: the battery (>= 20 %), the slot booted from (the one
//!     fastboot must find), the RAM boot gate (< 2 unconfirmed);
//!  3. the parking brake armed and read back; the phone into the bootloader
//!     (the reboot syscall's "bootloader": misc alone does not stop a
//!     deliberate reboot);
//!  4. in fastboot: the same phone (serial), unlocked, the same slot, the
//!     battery, the health against its baseline (retry counts not lower,
//!     slot a not unbootable, slot b not newly so, critical unlock not
//!     flipped to false);
//!  5. misc erased and the brake flashed again (the cadence in SAFETY.md);
//!  6. the attempt counted and logged, then fastboot boot - its output
//!     searched for the known pre-brick answers, a stop with no retry;
//!  7. Linux awaited: back, the boot confirmed (counter to 0, the image
//!     confirmed for flashing, a fresh baseline) and the brake armed again
//!     (a fastboot boot spends it). Not back: the attempt stays counted -
//!     never boot the same image again to see.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MIN_BATTERY: u32 = 20;
/// The parking brake: misc's first 2 KB, "bootonce-bootloader" and zeros.
const BRAKE: &[u8] = b"bootonce-bootloader";
const BRAKE_LEN: usize = 2048;
/// What the bootloader answers on the way to a brick (flash-safely.sh).
const PRE_BRICK: &[&str] = &["Failed to load/authenticate boot image", "Device Error", "not allowed for Critical Partitions", "Load Error"];

/// What the image is, once checked.
#[derive(Debug, Clone)]
pub struct Image {
    pub sha256: String,
    pub size: u64,
    pub hardware: String,
    pub os_version: String,
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Checks a boot image on this computer (step 1).
pub fn check_image(path: &Path) -> Result<Image, String> {
    use sha2::{Digest, Sha256};
    let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let fail = |m: &str| Err(format!("{}: {m}", path.display()));
    if b.len() < 4 << 20 {
        return fail("too small for a boot image");
    }
    if b.len() > 100 << 20 {
        return fail("larger than the boot partition");
    }
    if &b[..8] != b"ANDROID!" {
        return fail("no ANDROID! boot magic");
    }
    if u32_at(&b, 40) != 2 {
        return fail(&format!("header version {}, the Duo's ABL wants 2", u32_at(&b, 40)));
    }
    let (ksize, rsize, ssize, page) = (u32_at(&b, 8) as usize, u32_at(&b, 16) as usize, u32_at(&b, 24) as usize, u32_at(&b, 36) as usize);
    let os = u32_at(&b, 44);
    if os == 0 {
        return fail("no os version in the header: strict ABLs refuse it");
    }
    let os_version = format!("{}.{}.{}", os >> 25, (os >> 18) & 0x7f, (os >> 11) & 0x7f);
    let text = |range: std::ops::Range<usize>| String::from_utf8_lossy(&b[range]).trim_end_matches('\0').to_owned();
    let cmdline = text(64..576) + &text(608..1632);
    let hardware = cmdline.split_whitespace().find_map(|w| w.strip_prefix("androidboot.hardware=")).unwrap_or_default().to_owned();
    if hardware != "surfaceduo" {
        return fail(&format!("androidboot.hardware='{hardware}', not the Duo 1"));
    }
    if ksize == 0 || page == 0 {
        return fail("no kernel");
    }
    let align = |n: usize| n.div_ceil(page) * page;
    let kernel = b.get(page..page + ksize).ok_or("the kernel runs past the image's end")?;
    // The Image's magic, after gzip if it is packed.
    let mut head = vec![0u8; 64];
    let head_ok = if kernel[..2] == [0x1f, 0x8b] {
        flate2::read::GzDecoder::new(kernel).read_exact(&mut head).is_ok()
    } else {
        head.copy_from_slice(&kernel[..64]);
        true
    };
    if !head_ok || &head[56..60] != b"ARM\x64" {
        return fail("the kernel has no ARM64 Image magic");
    }
    let dtbo_size = u32_at(&b, 1632) as usize;
    let dtb_size = u32_at(&b, 1648) as usize;
    if dtb_size == 0 {
        return fail("no DTB embedded: a v2 image without one does not boot on the Duo");
    }
    let dtb_at = page + align(ksize) + align(rsize) + align(ssize) + align(dtbo_size);
    if b.get(dtb_at..dtb_at + 4) != Some(&[0xd0, 0x0d, 0xfe, 0xed][..]) {
        return fail("the embedded DTB has no FDT magic");
    }
    Ok(Image { sha256: format!("{:x}", Sha256::digest(&b)), size: b.len() as u64, hardware, os_version })
}

/// What fastboot says the phone is.
#[derive(Debug, Clone, Default)]
pub struct Fastboot {
    pub serial: String,
    pub product: String,
    pub unlocked: String,
    pub slot: String,
    pub battery: Option<u32>,
    pub retry_a: String,
    pub retry_b: String,
    pub unbootable_a: String,
    pub unbootable_b: String,
    pub critical: String,
}

fn fastboot(args: &[&str], limit: Duration) -> Result<String, String> {
    let mut child = Command::new("fastboot").args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("fastboot: {e}"))?;
    let start = Instant::now();
    loop {
        if let Some(_status) = child.try_wait().map_err(|e| e.to_string())? {
            let out = child.wait_with_output().map_err(|e| e.to_string())?;
            return Ok(format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            return Err(format!("fastboot {} took longer than {} s", args.join(" "), limit.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// `fastboot getvar all`, read (step 4).
pub fn probe() -> Result<Fastboot, String> {
    let all = fastboot(&["getvar", "all"], Duration::from_secs(20))?;
    let var = |name: &str| {
        all.lines()
            .find_map(|l| l.trim().strip_prefix("(bootloader) ").and_then(|r| r.strip_prefix(name)).and_then(|r| r.strip_prefix(':')))
            .map(|v| v.trim().to_owned())
            .unwrap_or_default()
    };
    let mut critical = all.lines().find(|l| l.to_lowercase().contains("critical unlocked")).map(str::to_owned).unwrap_or_default();
    if critical.is_empty() {
        critical = fastboot(&["oem", "device-info"], Duration::from_secs(15)).unwrap_or_default().lines().find(|l| l.to_lowercase().contains("critical unlocked")).map(str::to_owned).unwrap_or_default();
    }
    let critical = if critical.contains("rue") { "true" } else if critical.contains("alse") { "false" } else { "unknown" }.to_owned();
    let f = Fastboot {
        serial: var("serialno"),
        product: var("product"),
        unlocked: var("unlocked"),
        slot: var("current-slot"),
        battery: var("battery-level").parse().ok(),
        retry_a: var("slot-retry-count:a"),
        retry_b: var("slot-retry-count:b"),
        unbootable_a: var("slot-unbootable:a"),
        unbootable_b: var("slot-unbootable:b"),
        critical,
    };
    if f.serial.is_empty() || f.product.is_empty() {
        return Err("fastboot did not give the serial and product".into());
    }
    Ok(f)
}

/// The phone's health against its baseline (flash-safely.sh's check_health):
/// what got worse, if anything.
pub fn worse(now: &Fastboot, base: &serde_json::Value) -> Vec<String> {
    let mut problems = Vec::new();
    let num = |s: &str| s.parse::<i64>().ok();
    for (slot, cur, key) in [("a", &now.retry_a, "retry_a"), ("b", &now.retry_b, "retry_b")] {
        if let (Some(c), Some(b)) = (num(cur), base[key].as_str().and_then(num)) {
            if c < b {
                problems.push(format!("slot-retry-count:{slot} dropped {b} -> {c}"));
            }
        }
    }
    if now.unbootable_a == "yes" {
        problems.push("slot a marked unbootable".into());
    }
    if now.unbootable_b == "yes" && base["unbootable_b"].as_str() != Some("yes") {
        problems.push("slot b newly marked unbootable".into());
    }
    if base["critical"].as_str() == Some("true") && now.critical == "false" {
        problems.push("critical_unlocked flipped true -> false (the brick's signature)".into());
    }
    problems
}

/// The brake's 2 KB.
fn brake_bytes() -> Vec<u8> {
    let mut b = vec![0u8; BRAKE_LEN];
    b[..BRAKE.len()].copy_from_slice(BRAKE);
    b
}

/// The parking brake armed from Linux and read back.
pub fn arm_brake_linux(host: &str) -> Result<(), String> {
    let script = format!(
        "printf '{}' | dd of=/dev/disk/by-partlabel/misc bs={BRAKE_LEN} count=1 conv=sync,notrunc,fsync status=none && head -c {BRAKE_LEN} /dev/disk/by-partlabel/misc | sha256sum | cut -d' ' -f1\n",
        String::from_utf8_lossy(BRAKE)
    );
    let back = crate::phone::run_vetted(host, &script)?;
    let want = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(brake_bytes()))
    };
    if back.trim() != want {
        return Err("the parking brake read back differs from what was written".into());
    }
    Ok(())
}

/// The phone into the bootloader from Linux: the reboot syscall with
/// "bootloader", as Android asks it (SAFETY.md).
fn to_bootloader(host: &str) {
    let script = "python3 -c 'import ctypes; l=ctypes.CDLL(\"libc.so.6\"); l.sync(); l.syscall(142, 0xfee1dead, 672274793, 0xA1B2C3D4, b\"bootloader\")'\n";
    crate::phone::close_shared(host);
    let _ = crate::phone::run_vetted(host, script);
}

/// Where a RAM boot is, for whoever shows it.
pub type Say<'a> = &'a mut dyn FnMut(String);

/// What the image boots into, so the boot is known to have come up: Linux
/// (ssh), or a recovery (TWRP; adb, in state "recovery").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    Linux,
    Recovery,
}

impl Expect {
    /// A recovery by its name (TWRP's images say so), Linux otherwise.
    pub fn of(image: &Path) -> Expect {
        let name = image.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        if name.contains("twrp") || name.contains("recovery") { Expect::Recovery } else { Expect::Linux }
    }
}

fn adb(args: &[&str], limit: Duration) -> Result<String, String> {
    let mut child = Command::new("adb").args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("adb: {e}"))?;
    let start = Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            let out = child.wait_with_output().map_err(|e| e.to_string())?;
            return Ok(format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            return Err(format!("adb {} took longer than {} s", args.join(" "), limit.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Whether this phone is in recovery over adb.
fn in_recovery(serial: &str) -> bool {
    adb(&["devices"], Duration::from_secs(10)).is_ok_and(|o| o.lines().any(|l| {
        let mut w = l.split_whitespace();
        w.next() == Some(serial) && w.next() == Some("recovery")
    }))
}

/// The parking brake armed from the recovery (adb) and read back.
fn arm_brake_recovery(serial: &str) -> Result<(), String> {
    // Where the recovery keeps it: TWRP for the Duo (built on cepheus's) has
    // the UFS controller's by-name, not bootdevice; the first that is there.
    let script = format!(
        "for m in /dev/block/platform/soc/1d84000.ufshc/by-name/misc /dev/block/by-name/misc /dev/block/bootdevice/by-name/misc; do [ -e \"$m\" ] && break; m=; done; \
         [ -n \"$m\" ] || {{ echo no-misc; exit 1; }}; \
         printf '{}' | dd of=\"$m\" bs={BRAKE_LEN} count=1 conv=sync,notrunc,fsync 2>/dev/null; head -c {BRAKE_LEN} \"$m\" | sha256sum | cut -d' ' -f1",
        String::from_utf8_lossy(BRAKE)
    );
    let back = adb(&["-s", serial, "shell", &script], Duration::from_secs(30))?;
    let want = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(brake_bytes()))
    };
    if back.trim() != want {
        return Err(format!("the parking brake read back in the recovery differs: {}", back.trim()));
    }
    Ok(())
}

/// Out of the recovery, back into Linux: adb reboot, and Linux awaited. If
/// the brake stops it in fastboot, the bootloader is told to go on with the
/// same slot (fastboot continue).
pub fn leave_recovery(host: &str, serial: &str, say: Say) -> Result<(), String> {
    if !in_recovery(serial) {
        return Err("the phone is not in the recovery".into());
    }
    say("leaving the recovery".into());
    adb(&["-s", serial, "reboot"], Duration::from_secs(20))?;
    let start = Instant::now();
    let mut continued = false;
    loop {
        if crate::phone::answers_fresh(host) {
            break;
        }
        if !continued && fastboot(&["devices"], Duration::from_secs(10)).is_ok_and(|o| o.contains(serial)) {
            say("the parking brake held it in fastboot: going on with the same slot".into());
            let _ = fastboot(&["continue"], Duration::from_secs(20));
            continued = true;
        }
        if start.elapsed() > Duration::from_secs(300) {
            return Err("Linux did not come back in 5 minutes - look at the phone's screen".into());
        }
        std::thread::sleep(Duration::from_secs(3));
    }
    say("Linux is back: arming the parking brake".into());
    arm_brake_linux(host)?;
    crate::flash::log(serial, "left the recovery, parking brake armed from Linux - cradle")?;
    say("done: enter the PIN on the phone".into());
    Ok(())
}

/// Everything that can be checked before the phone leaves Linux (steps 1-2):
/// the image and what the phone says. `Err` is a stop.
pub fn preflight(host: &str, image: &Path) -> Result<(Image, String, char), String> {
    let img = check_image(image)?;
    let serial = crate::backup::serial(host)?;
    let gate = crate::flash::gate(&serial);
    if !gate.open() {
        return Err(format!("the RAM boot gate is closed ({} of {} unconfirmed): see cradle slots", gate.unconfirmed, crate::flash::MAX_UNCONFIRMED));
    }
    let st = crate::status::read(host)?;
    match st.battery {
        Some(b) if b >= MIN_BATTERY => {}
        Some(b) => return Err(format!("the battery is at {b} %: charge it to {MIN_BATTERY} % first")),
        None => return Err("the battery was not read".into()),
    }
    let ev = crate::flash::evidence(host)?;
    Ok((img, serial, ev.slot))
}

/// The whole RAM boot (steps 1-7). Only with the owner's go.
pub fn ram_boot(host: &str, image: &Path, expect: Expect, say: Say) -> Result<(), String> {
    say("checking the image and the phone".into());
    let (img, serial, slot) = preflight(host, image)?;
    say(format!("image fine: header v2, ARM64 kernel, DTB, Android {}, sha {}", img.os_version, &img.sha256[..16]));

    say("arming the parking brake".into());
    arm_brake_linux(host)?;
    crate::flash::log(&serial, "parking brake armed from Linux - cradle")?;

    say("into the bootloader".into());
    to_bootloader(host);
    let start = Instant::now();
    loop {
        if fastboot(&["devices"], Duration::from_secs(10)).is_ok_and(|o| o.contains("fastboot")) {
            break;
        }
        if start.elapsed() > Duration::from_secs(120) {
            return Err("the phone did not reach fastboot in 2 minutes - look at its screen; nothing was booted".into());
        }
        std::thread::sleep(Duration::from_secs(2));
    }

    say("in fastboot: checking it is the same phone, and its health".into());
    let fb = probe()?;
    if fb.serial != serial {
        return Err(format!("fastboot shows another phone ({}): stopping", fb.serial));
    }
    if fb.product != "surfaceduo" {
        return Err(format!("fastboot shows a '{}', not a Duo 1: stopping", fb.product));
    }
    if fb.unlocked != "yes" {
        return Err(format!("the bootloader is not unlocked ({}): stopping", fb.unlocked));
    }
    if fb.slot != slot.to_string() {
        return Err(format!("fastboot is on slot {}, Linux booted from {slot}: the slot drifted - stopping, nothing booted", fb.slot));
    }
    if fb.battery.is_some_and(|b| b < MIN_BATTERY) {
        return Err(format!("the battery is at {} %: stopping", fb.battery.unwrap_or(0)));
    }
    let base = crate::flash::baseline(&serial);
    if let Some(base) = &base {
        let problems = worse(&fb, base);
        if !problems.is_empty() {
            crate::flash::log(&serial, &format!("HEALTH-STOP: {} - cradle", problems.join("; ")))?;
            return Err(format!("the phone's health is worse than its baseline: {} - do not boot or flash anything; see docs/SAFETY.md", problems.join("; ")));
        }
    }

    say("re-arming the parking brake in fastboot".into());
    let brake = std::env::temp_dir().join("cradle-misc-brake.img");
    std::fs::write(&brake, brake_bytes()).map_err(|e| e.to_string())?;
    for args in [vec!["erase", "misc"], vec!["flash", "misc", brake.to_str().unwrap_or_default()]] {
        let out = fastboot(&args, Duration::from_secs(30))?;
        if let Some(sig) = PRE_BRICK.iter().find(|s| out.contains(*s)) {
            crate::flash::log(&serial, &format!("BRICK-SIGNATURE during: fastboot {} ({sig}) - cradle", args.join(" ")))?;
            return Err(format!("the bootloader answered '{sig}' to fastboot {}: STOP - do not retry; see docs/SAFETY.md", args.join(" ")));
        }
        if !out.contains("OKAY") {
            return Err(format!("fastboot {} did not answer OKAY: {}", args.join(" "), out.trim()));
        }
    }

    let n = crate::flash::count_attempt(&serial, &img.sha256)?;
    say(format!("booting the image from RAM (attempt {n} of {})", crate::flash::MAX_UNCONFIRMED));
    let out = fastboot(&["boot", image.to_str().unwrap_or_default()], Duration::from_secs(120))?;
    if let Some(sig) = PRE_BRICK.iter().find(|s| out.contains(*s)) {
        crate::flash::log(&serial, &format!("BRICK-SIGNATURE during: fastboot boot ({sig}) - cradle"))?;
        return Err(format!("the bootloader answered '{sig}': STOP - do not retry this image; see docs/SAFETY.md"));
    }
    if !out.contains("OKAY") {
        return Err(format!("fastboot boot did not answer OKAY - the attempt stays counted; do not retry: {}", out.trim()));
    }

    let (what, up): (&str, Box<dyn Fn() -> bool>) = match expect {
        Expect::Linux => ("Linux", Box::new(|| crate::phone::answers_fresh(host))),
        Expect::Recovery => ("the recovery", Box::new(|| in_recovery(&serial))),
    };
    say(format!("waiting for {what}"));
    let start = Instant::now();
    while !up() {
        if start.elapsed() > Duration::from_secs(240) {
            return Err(format!("{what} did not come up in 4 minutes: the attempt stays counted ({n} of {}). Do not boot this image again to see.", crate::flash::MAX_UNCONFIRMED));
        }
        std::thread::sleep(Duration::from_secs(3));
    }
    crate::flash::confirm_ram_boot(&serial, &img.sha256, &fb, expect == Expect::Linux)?;
    say(format!("{what} is up: the RAM boot is confirmed"));
    say("re-arming the parking brake".into());
    match expect {
        Expect::Linux => {
            arm_brake_linux(host)?;
            crate::flash::log(&serial, "parking brake armed from Linux after the RAM boot - cradle")?;
            say("done: enter the PIN on the phone".into());
        }
        Expect::Recovery => {
            arm_brake_recovery(&serial)?;
            crate::flash::log(&serial, "parking brake armed from the recovery after the RAM boot - cradle")?;
            say("done: the phone is in the recovery (no touch there); cradle recovery-exit brings it back".into());
        }
    }
    Ok(())
}
