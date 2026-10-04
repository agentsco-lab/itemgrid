//! A slot's boot chain (boot, dtbo, vbmeta) put back from a Cradle backup -
//! flashing, so by the port's SAFETY.md:
//!
//! - one slot at a time: the other stays as it is, the way out;
//! - only from Cradle's boot-chain backups, each file checked against its
//!   manifest's sha256;
//! - a partition that already holds the backup's bytes is not written;
//! - a boot that differs is first booted from RAM (the RAM boot, with all
//!   its checks); dtbo and vbmeta, which cannot be tried alone, only with
//!   the boot from the same backup - a set that booted together;
//! - the chain as it is now backed up first, so a restore can be undone;
//! - written with dd from the running Linux (fastboot flash boot on the Duo
//!   can answer Device Error and write nothing), each partition read back
//!   and compared;
//! - the battery at 30 % or more; then a reboot into the slot, and Linux
//!   awaited - booted from it.

use std::path::{Path, PathBuf};

use crate::backup::{Backup, Kind};

pub const PARTS: [&str; 3] = ["boot", "dtbo", "vbmeta"];
const MIN_BATTERY: u32 = 30;

/// One partition of the slot: the backup's file and whether the phone holds
/// something else.
#[derive(Debug, Clone)]
pub struct Part {
    pub partition: String,
    pub file: PathBuf,
    pub sha256: String,
    /// The phone holds something else.
    pub differs: bool,
    /// To be written: it differs, or a rewrite of the same bytes was asked
    /// (to try the writing itself on a spare slot).
    pub write: bool,
}

/// What a restore would do.
#[derive(Debug, Clone)]
pub struct Plan {
    pub slot: char,
    pub backup: Backup,
    pub parts: Vec<Part>,
    pub active_slot: char,
}

impl Plan {
    pub fn writes(&self) -> Vec<&Part> {
        self.parts.iter().filter(|p| p.write).collect()
    }
}

/// The plan, checked - nothing changed on the phone (reading only).
pub fn plan(host: &str, backup: &Backup, slot: char, rewrite: bool) -> Result<Plan, String> {
    use sha2::{Digest, Sha256};
    if backup.manifest.kind != Kind::Boot {
        return Err("only a boot-chain backup restores a slot".into());
    }
    if slot != 'a' && slot != 'b' {
        return Err(format!("no slot '{slot}'"));
    }
    let serial = crate::backup::serial(host)?;
    if backup.manifest.serial != serial {
        return Err(format!("the backup is of another phone ({})", backup.manifest.serial));
    }
    let mut parts = Vec::new();
    for name in PARTS {
        let partition = format!("{name}_{slot}");
        let item = backup.manifest.items.iter().find(|i| i.source == partition).ok_or(format!("the backup has no {partition}"))?;
        let file = backup.dir.join(&item.file);
        let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        if format!("{:x}", Sha256::digest(&bytes)) != item.sha256 {
            return Err(format!("{} does not match its manifest: the backup is damaged", file.display()));
        }
        let there = crate::phone::run(host, &format!("sha256sum /dev/disk/by-partlabel/{partition} | cut -d' ' -f1\n"))?;
        let differs = there.trim() != item.sha256;
        parts.push(Part { partition, file, sha256: item.sha256.clone(), differs, write: differs || rewrite });
    }
    let active_slot = crate::flash::evidence(host)?.slot;
    Ok(Plan { slot, backup: backup.clone(), parts, active_slot })
}

/// The restore, by the plan. Only with the owner's go.
pub fn restore(host: &str, plan: &Plan, say: crate::ramboot::Say) -> Result<(), String> {
    let _ = crate::phone::keep_awake(host, true);
    let writes: Vec<Part> = plan.writes().into_iter().cloned().collect();
    if writes.is_empty() {
        say(format!("slot {} already holds this backup: nothing to write", plan.slot.to_ascii_uppercase()));
        return Ok(());
    }
    let st = crate::status::read(host)?;
    match st.battery {
        Some(b) if b >= MIN_BATTERY => {}
        Some(b) => return Err(format!("the battery is at {b} %: charge it to {MIN_BATTERY} % first")),
        None => return Err("the battery was not read".into()),
    }
    let serial = crate::backup::serial(host)?;

    // A boot that changes: tried from RAM first (the same bytes rewritten
    // change nothing to try).
    if let Some(boot) = writes.iter().find(|p| p.partition.starts_with("boot_") && p.differs) {
        say(format!("{} differs: booting it from RAM first", boot.partition));
        crate::ramboot::ram_boot(host, &boot.file, crate::ramboot::Expect::Linux, say)?;
    }

    say("backing up the boot chain as it is now".into());
    let before = crate::backup::take(host, Kind::Boot, &mut |l| say(l))?;
    crate::flash::log(&serial, &format!("restore of slot {} from {}: the chain before backed up in {} - cradle", plan.slot, plan.backup.manifest.created, before.dir.display()))?;

    for p in &writes {
        say(format!("writing {}", p.partition));
        let remote = format!("/var/tmp/cradle-restore/{}.img", p.partition);
        crate::phone::upload(host, &p.file, &remote)?;
        let dev = format!("/dev/disk/by-partlabel/{}", p.partition);
        let script = format!(
            "dd if='{remote}' of={dev} bs=4M conv=fsync status=none && sync && sha256sum {dev} | cut -d' ' -f1; rm -f '{remote}'\n"
        );
        let back = crate::phone::run_vetted(host, &script)?;
        if back.trim() != p.sha256 {
            crate::flash::log(&serial, &format!("RESTORE-STOP: {} read back differs after writing - cradle", p.partition))?;
            return Err(format!("{} read back differs from the backup after writing - STOP: do not reboot; the chain before is in {}", p.partition, before.dir.display()));
        }
        crate::flash::log(&serial, &format!("restored {} from {} ({}) - cradle", p.partition, plan.backup.manifest.created, &p.sha256[..16]))?;
    }

    // Booted from the restored slot, it proves itself.
    if plan.slot == plan.active_slot {
        say(format!("rebooting into slot {}", plan.slot.to_ascii_uppercase()));
        crate::ramboot::arm_brake_linux(host)?;
        crate::phone::reboot(host, &mut |b| say(b.words().to_owned()))?;
        let ev = crate::flash::evidence(host)?;
        if ev.slot != plan.slot {
            return Err(format!("Linux came back from slot {}, not {}", ev.slot, plan.slot));
        }
        say(format!("Linux is back from slot {}", plan.slot.to_ascii_uppercase()));
    } else {
        say(format!("slot {} is not the one in use: written and checked, not booted (it stays the spare)", plan.slot.to_ascii_uppercase()));
    }
    Ok(())
}

/// The newest boot-chain backup of this phone, or the one at `dir`.
pub fn pick(serial: &str, dir: Option<&Path>) -> Option<Backup> {
    let all: Vec<Backup> = crate::backup::list(Some(serial)).into_iter().filter(|b| b.manifest.kind == Kind::Boot).collect();
    match dir {
        Some(d) => all.into_iter().find(|b| b.dir == d || b.dir.file_name() == d.file_name()),
        None => all.into_iter().next(),
    }
}
