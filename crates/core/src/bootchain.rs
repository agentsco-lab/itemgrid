//! A slot's boot partitions written from TWRP (RAM-booted, adb as root) -
//! by the port's SAFETY.md: never `fastboot flash` (on the Duo it can answer
//! Device Error and write nothing, and boot, dtbo and vbmeta live on a LUN
//! no software reaches if the bootloader stops taking images), only `dd`
//! from a booted system; each image checked here, sent and checked on the
//! phone, written, and read back from the partition and compared.

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::full::{adb_send, adb_shell};

/// A partition's image to write: its name in by-name without the slot
/// (boot, dtbo, vbmeta), the file, its size and sha256.
#[derive(Debug, Clone)]
pub struct Part {
    pub name: &'static str,
    pub file: PathBuf,
    pub size: u64,
    pub sha256: String,
}

impl Part {
    /// The image at `file`, read and hashed.
    pub fn of(name: &'static str, file: &Path) -> Result<Part, String> {
        let mut f = std::fs::File::open(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut size = 0u64;
        loop {
            let n = f.read(&mut buf).map_err(|e| format!("{}: {e}", file.display()))?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
            size += n as u64;
        }
        if size == 0 {
            return Err(format!("{} is empty", file.display()));
        }
        Ok(Part { name, file: file.to_owned(), size, sha256: format!("{:x}", h.finalize()) })
    }
}

/// `parts` written to `slot` ('a' or 'b') from TWRP on the phone `serial`:
/// each sent to /tmp and checked there, the partition found by name and
/// checked to be a UFS one large enough, written with dd, and its first
/// `size` bytes read back against the image. Stops at the first that
/// differs. `log`: each written partition noted (flash.rs's history).
pub fn write(serial: &str, slot: char, parts: &[Part], say: crate::ramboot::Say) -> Result<(), String> {
    let blk = crate::android::BLK;
    for p in parts {
        let dev = adb_shell(serial, &format!("readlink -f {blk}/{}_{slot}", p.name))?.trim().to_owned();
        if !dev.starts_with("/dev/block/sd") {
            return Err(format!("{}_{slot} is not where it should be ({dev}): stopping", p.name));
        }
        let room: u64 = adb_shell(serial, &format!("blockdev --getsize64 {dev}"))?.trim().parse().map_err(|_| format!("{}_{slot}'s size was not read", p.name))?;
        if room < p.size {
            return Err(format!("{} ({} bytes) does not fit {}_{slot} ({room} bytes): stopping", p.file.display(), p.size, p.name));
        }
        say(format!("writing {}_{slot}", p.name));
        let tmp = format!("/tmp/itemgrid-{}.img", p.name);
        let mut f = std::fs::File::open(&p.file).map_err(|e| format!("{}: {e}", p.file.display()))?;
        adb_send(serial, &format!("cat > {tmp}"), &mut f)?;
        let there = adb_shell(serial, &format!("sha256sum {tmp} | cut -d' ' -f1"))?;
        if there.trim() != p.sha256 {
            let _ = adb_shell(serial, &format!("rm -f {tmp}"));
            return Err(format!("{} arrived different on the phone - nothing written to {}_{slot}", p.name, p.name));
        }
        // Written whole (the rest of a larger partition zeroed: nothing of
        // what was there left behind the image), then read back.
        adb_shell(serial, &format!("dd if={tmp} of={dev} bs=4194304 2>/dev/null; dd if=/dev/zero of={dev} bs=4096 seek={} 2>/dev/null; sync; rm -f {tmp}; true", p.size.div_ceil(4096)))?;
        let back = adb_shell(serial, &format!("head -c {} {dev} | sha256sum | cut -d' ' -f1", p.size))?;
        if back.trim() != p.sha256 {
            return Err(format!("{}_{slot} reads back different from the image - stopping (the phone still starts TWRP from the computer)", p.name));
        }
        crate::flash::log(serial, &format!("wrote {}_{slot} from {} ({}) - itemgrid", p.name, p.file.display(), &p.sha256[..16]))?;
    }
    Ok(())
}

/// `part` written to `slot` from the running Linux at `host` (an image that
/// booted Linux on this phone from RAM first: SAFETY.md's rule one) - sent,
/// written with dd, the partition's first `size` bytes read back and
/// compared.
pub fn write_from_linux(host: &str, serial: &str, slot: char, part: &Part, say: crate::ramboot::Say) -> Result<(), String> {
    let partition = format!("{}_{slot}", part.name);
    say(format!("writing {partition}"));
    let remote = format!("/var/tmp/itemgrid-install/{partition}.img");
    crate::phone::upload(host, &part.file, &remote)?;
    let dev = format!("/dev/disk/by-partlabel/{partition}");
    let script = format!("dd if='{remote}' of={dev} bs=4M conv=fsync status=none && sync && head -c {} {dev} | sha256sum | cut -d' ' -f1; rm -f '{remote}'\n", part.size);
    let back = crate::phone::run_vetted(host, &script)?;
    if back.trim() != part.sha256 {
        crate::flash::log(serial, &format!("INSTALL-STOP: {partition} read back differs after writing - itemgrid"))?;
        return Err(format!("{partition} reads back different after writing - STOP: do not restart the phone; the port runs from RAM now"));
    }
    crate::flash::log(serial, &format!("wrote {partition} from {} ({}) - itemgrid", part.file.display(), &part.sha256[..16]))
}
