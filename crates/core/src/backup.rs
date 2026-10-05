//! Backups of the phone on this computer - reading only; nothing here
//! writes to the phone.
//!
//! - **Device data** (`device`): what no image can give back - the radio's
//!   calibration and IMEI (modemst1/2, fsg, fsc), persist, devinfo, secdata.
//!   Taken once, kept for good, never removed by Hythe; worth a copy off
//!   this computer too.
//! - **Boot chain** (`boot`): boot, dtbo and vbmeta of both slots, and misc.
//!   The last 10 kept, and always the first.
//! - **Quick** (`quick`): /home and /etc as a tar.gz, and the list of the
//!   packages. The last 5 kept.
//!
//! Each partition is read over ssh while its sha256 is worked out, then
//! hashed again on the phone; the backup stands only if the two agree. Old
//! backups go only after a new one has stood. Everything lives in
//! ~/hythe-backups/<serial>/<when>-<kind>/, each with its manifest.json;
//! the directories are the owner's only (they hold keys and /etc/shadow).

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEVICE_PARTS: &[&str] = &["persist", "modemst1", "modemst2", "fsg", "fsc", "devinfo", "secdata"];
pub const BOOT_PARTS: &[&str] = &["boot_a", "boot_b", "dtbo_a", "dtbo_b", "vbmeta_a", "vbmeta_b", "misc"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Device,
    Boot,
    Quick,
    /// The whole system: the rootfs image and the Android container's data,
    /// taken from TWRP (full.rs).
    Full,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Device => "device",
            Kind::Boot => "boot",
            Kind::Quick => "quick",
            Kind::Full => "full",
        }
    }

    pub fn words(self) -> &'static str {
        match self {
            Kind::Device => "Device data",
            Kind::Boot => "Boot chain",
            Kind::Quick => "Home and settings",
            Kind::Full => "Whole system",
        }
    }

    /// How many are kept (none: all).
    fn keep(self) -> Option<usize> {
        match self {
            Kind::Device => None,
            Kind::Boot => Some(10),
            Kind::Quick => Some(5),
            Kind::Full => Some(2),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    /// The partition or what was archived.
    pub source: String,
    pub file: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub kind: Kind,
    /// Local time, YYYY-MM-DD HH:MM:SS.
    pub created: String,
    pub serial: String,
    pub slot: String,
    pub item: String,
    pub port: String,
    pub kernel: String,
    pub items: Vec<Item>,
    /// Never removed by Hythe.
    #[serde(default)]
    pub keep: bool,
    /// The owner says a copy is off this computer (device data).
    #[serde(default)]
    pub off_computer: bool,
    /// Fingers the reader knew when it was taken (full backups): checked
    /// after a way back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingers: Option<u32>,
}

/// A backup on disk: where, and what.
#[derive(Debug, Clone)]
pub struct Backup {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl Backup {
    pub fn size(&self) -> u64 {
        self.manifest.items.iter().map(|i| i.size).sum()
    }

    pub(crate) fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(&self.manifest).map_err(|e| e.to_string())?;
        std::fs::write(self.dir.join("manifest.json"), text).map_err(|e| e.to_string())
    }

    /// Marked to be kept for good, or not.
    pub fn set_keep(&mut self, keep: bool) -> Result<(), String> {
        self.manifest.keep = keep;
        self.save()
    }

    /// The owner's word that a copy is off this computer.
    pub fn set_off_computer(&mut self, off: bool) -> Result<(), String> {
        self.manifest.off_computer = off;
        self.save()
    }
}

pub fn root() -> PathBuf {
    Path::new(&std::env::var("HOME").unwrap_or_default()).join("hythe-backups")
}

/// The backups on this computer, newest first; of one phone if `serial`.
pub fn list(serial: Option<&str>) -> Vec<Backup> {
    let mut out = Vec::new();
    let Ok(phones) = std::fs::read_dir(root()) else { return out };
    for phone in phones.flatten() {
        if serial.is_some_and(|s| phone.file_name().to_string_lossy() != s) {
            continue;
        }
        let Ok(dirs) = std::fs::read_dir(phone.path()) else { continue };
        for dir in dirs.flatten() {
            let Ok(text) = std::fs::read_to_string(dir.path().join("manifest.json")) else { continue };
            if let Ok(manifest) = serde_json::from_str::<Manifest>(&text) {
                out.push(Backup { dir: dir.path(), manifest });
            }
        }
    }
    out.sort_by(|a, b| b.manifest.created.cmp(&a.manifest.created));
    out
}

/// What the phone is, for the manifest.
pub(crate) struct Facts {
    pub(crate) serial: String,
    pub(crate) slot: String,
    pub(crate) item: String,
    pub(crate) port: String,
    pub(crate) kernel: String,
}

const FACTS: &str = r#"
for w in $(cat /proc/cmdline); do case "$w" in androidboot.serialno=*) echo "serial=${w#*=}";; androidboot.slot_suffix=*) echo "slot=${w#*=}";; esac; done
echo "item=$(dpkg-query -W -f='${Version}' item-shell 2>/dev/null)"
echo "port=$(dpkg-query -W -f='${Version}' adaptation-droidian-surfaceduo 2>/dev/null)"
echo "kernel=$(uname -r)"
"#;

pub(crate) fn facts(host: &str) -> Result<Facts, String> {
    let text = crate::phone::run(host, FACTS)?;
    let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).unwrap_or_default().to_owned();
    let f = Facts { serial: get("serial"), slot: get("slot"), item: get("item"), port: get("port"), kernel: get("kernel") };
    if f.serial.is_empty() || f.serial.contains('/') {
        return Err("the phone's serial number was not read".into());
    }
    Ok(f)
}

pub(crate) fn now() -> String {
    let out = std::process::Command::new("date").arg("+%Y-%m-%d %H:%M:%S").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).unwrap_or_default();
    out
}

/// Free space where the backups go (bytes).
pub(crate) fn free_here() -> u64 {
    let dir = root();
    let _ = std::fs::create_dir_all(&dir);
    std::process::Command::new("df")
        .args(["-Pk", &dir.display().to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().nth(1).and_then(|l| l.split_whitespace().nth(3).and_then(|v| v.parse::<u64>().ok())))
        .map(|k| k * 1024)
        .unwrap_or(0)
}

/// The size of each partition, read on the phone.
fn sizes(host: &str, parts: &[&str]) -> Result<Vec<u64>, String> {
    let script: String = parts.iter().map(|p| format!("blockdev --getsize64 /dev/disk/by-partlabel/{p}\n")).collect();
    let text = crate::phone::run(host, &script)?;
    let sizes: Vec<u64> = text.lines().filter_map(|l| l.trim().parse().ok()).collect();
    if sizes.len() != parts.len() {
        return Err("the partitions' sizes were not read".into());
    }
    Ok(sizes)
}

/// Takes a backup of `kind`; `say` hears each step. Returns it, once it
/// stands; older ones beyond the kind's count are then removed.
pub fn take(host: &str, kind: Kind, say: &mut dyn FnMut(String)) -> Result<Backup, String> {
    let f = facts(host)?;
    // How much room it needs, roughly.
    let need: u64 = match kind {
        Kind::Device => sizes(host, DEVICE_PARTS)?.iter().sum(),
        Kind::Boot => sizes(host, BOOT_PARTS)?.iter().sum(),
        Kind::Quick => crate::phone::run(host, "du -sxk /home /etc | awk '{s+=$1} END {print s*1024}'")?.trim().parse().unwrap_or(0),
        Kind::Full => return Err("the whole system is taken with full::take, from TWRP".into()),
    };
    let free = free_here();
    if free < need + need / 10 + (512 << 20) {
        return Err(format!("not enough room here: {} needed, {} free", crate::status::size_words(need / 1024), crate::status::size_words(free / 1024)));
    }
    let created = now();
    let stamp = created.replace([' ', ':'], "").replace('-', "");
    let phone_dir = root().join(&f.serial);
    let dir = phone_dir.join(format!("{stamp}-{}", kind.name()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for d in [root(), phone_dir.clone(), dir.clone()] {
        let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
    }
    let result = take_into(host, kind, &dir, say);
    let items = match result {
        Ok(items) => items,
        Err(e) => {
            // A backup that did not stand is not left to be taken for one.
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
    };
    let backup = Backup { dir, manifest: Manifest { kind, created, serial: f.serial.clone(), slot: f.slot, item: f.item, port: f.port, kernel: f.kernel, items, keep: false, off_computer: false, fingers: None } };
    backup.save()?;
    say(format!("{} backed up: {}", kind.words(), crate::status::size_words(backup.size() / 1024)));
    prune(&f.serial, kind, say);
    Ok(backup)
}

fn take_into(host: &str, kind: Kind, dir: &Path, say: &mut dyn FnMut(String)) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    match kind {
        Kind::Device | Kind::Boot => {
            let parts = if kind == Kind::Device { DEVICE_PARTS } else { BOOT_PARTS };
            for part in parts {
                say(format!("reading {part}"));
                let dev = format!("/dev/disk/by-partlabel/{part}");
                let file = format!("{part}.img");
                let (size, sha) = crate::phone::download(host, &format!("exec cat {dev}\n"), &dir.join(&file))?;
                // Read again on the phone: the two must agree.
                let there = crate::phone::run(host, &format!("sha256sum {dev} | cut -d' ' -f1\n"))?;
                if there.trim() != sha {
                    return Err(format!("{part}: what arrived differs from what is on the phone - try again"));
                }
                items.push(Item { source: part.to_string(), file, size, sha256: sha });
            }
        }
        Kind::Full => return Err("the whole system is taken with full::take, from TWRP".into()),
        Kind::Quick => {
            say("listing the packages".into());
            let list = crate::phone::run(host, "dpkg-query -W -f='${Package}\\t${Version}\\n'; echo '#manual'; apt-mark showmanual 2>/dev/null\n")?;
            std::fs::write(dir.join("packages.txt"), &list).map_err(|e| e.to_string())?;
            items.push(Item { source: "the packages".into(), file: "packages.txt".into(), size: list.len() as u64, sha256: sha_of(list.as_bytes()) });
            say("archiving /home and /etc".into());
            let (size, sha) = crate::phone::download(host, "exec tar -C / --one-file-system -czf - home etc 2>/dev/null\n", &dir.join("home-etc.tar.gz"))?;
            // A tar read on the fly has nothing to compare with on the
            // phone: it is checked whole instead.
            say("checking the archive".into());
            let ok = std::process::Command::new("gzip").args(["-t", &dir.join("home-etc.tar.gz").display().to_string()]).status().is_ok_and(|s| s.success());
            if !ok || size < 1024 {
                return Err("the archive did not arrive whole - try again".into());
            }
            items.push(Item { source: "/home /etc".into(), file: "home-etc.tar.gz".into(), size, sha256: sha });
        }
    }
    Ok(items)
}

fn sha_of(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// The kind's backups beyond its count removed - never one kept for good,
/// never the first boot chain (where it all began).
pub(crate) fn prune(serial: &str, kind: Kind, say: &mut dyn FnMut(String)) {
    let Some(count) = kind.keep() else { return };
    let mut all: Vec<Backup> = list(Some(serial)).into_iter().filter(|b| b.manifest.kind == kind).collect();
    // Oldest last (list gives newest first); the first is spared.
    let first = all.last().map(|b| b.dir.clone());
    all.retain(|b| !b.manifest.keep && (kind != Kind::Boot || Some(&b.dir) != first.as_ref()));
    for old in all.into_iter().skip(count) {
        if std::fs::remove_dir_all(&old.dir).is_ok() {
            say(format!("removed an old backup from {}", old.manifest.created));
        }
    }
}

/// Whether this phone's device data is backed up here.
pub fn has_device_data(serial: &str) -> bool {
    list(Some(serial)).iter().any(|b| b.manifest.kind == Kind::Device)
}

/// The phone's serial number, read on it.
pub fn serial(host: &str) -> Result<String, String> {
    facts(host).map(|f| f.serial)
}
