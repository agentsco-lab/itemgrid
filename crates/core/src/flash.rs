//! The RAM boots' and flashes' state, shared with the port's
//! tools/flash-safely.sh (schema v2, by serial): one counter and one history
//! for both, so neither can go around the other's limit. On a computer with
//! the port's tree it is that tree's out/flash-state.json; elsewhere
//! ~/.local/state/gridbay/flash-state.json. Read here only, for now.

use std::path::{Path, PathBuf};

/// RAM boots in a row not yet confirmed, before the gate closes.
pub const MAX_UNCONFIRMED: u64 = 2;

/// The port's tree: GRIDBAY_PORT, ~/.config/gridbay/port, or the usual place.
pub fn port_tree() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let from_env = std::env::var_os("GRIDBAY_PORT").map(PathBuf::from);
    let from_file = std::fs::read_to_string(home.join(".config/gridbay/port")).ok().map(|s| PathBuf::from(s.trim()));
    let tree = from_env.or(from_file).unwrap_or_else(|| home.join("Desktop/projects/surfaceduo/surfaceduo-droidian"));
    tree.join("tools/flash-safely.sh").exists().then_some(tree)
}

pub fn state_path() -> PathBuf {
    match port_tree() {
        Some(t) => t.join("out/flash-state.json"),
        None => Path::new(&std::env::var("HOME").unwrap_or_default()).join(".local/state/gridbay/flash-state.json"),
    }
}

/// This phone's part of the state.
#[derive(Debug, Clone, Default)]
pub struct Gate {
    pub unconfirmed: u64,
    pub confirmed_sha256: Option<String>,
    /// The last few events, newest last.
    pub history: Vec<(String, String)>,
}

impl Gate {
    pub fn open(&self) -> bool {
        self.unconfirmed < MAX_UNCONFIRMED
    }
}

pub fn gate(serial: &str) -> Gate {
    let Ok(text) = std::fs::read_to_string(state_path()) else { return Gate::default() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return Gate::default() };
    let d = &v["devices"][serial];
    Gate {
        unconfirmed: d["consecutive_unconfirmed"].as_u64().unwrap_or(0),
        confirmed_sha256: d["confirmed_sha256"].as_str().map(str::to_owned),
        history: d["history"]
            .as_array()
            .map(|h| h.iter().map(|e| (e["ts"].as_str().unwrap_or_default().to_owned(), e["event"].as_str().unwrap_or_default().to_owned())).collect())
            .unwrap_or_default(),
    }
}

/// What shows that the running system came from a slot's image: the slot
/// booted from, the kernel running, the kernel in that slot.
#[derive(Debug, Clone)]
pub struct Evidence {
    pub slot: char,
    pub running_kernel: String,
    pub slot_kernel: String,
    pub image: Option<String>,
    pub partition_sha256: String,
}

impl Evidence {
    /// Whether it holds: the kernel running is the one in the slot booted.
    pub fn holds(&self) -> bool {
        !self.running_kernel.is_empty() && self.running_kernel == self.slot_kernel
    }
}

/// Gathers the evidence on the phone at `host` (reading only).
pub fn evidence(host: &str) -> Result<Evidence, String> {
    let text = crate::phone::run(host, "for w in $(cat /proc/cmdline); do case \"$w\" in androidboot.slot_suffix=*) echo \"slot=${w##*_}\";; esac; done; echo \"kernel=$(uname -r)\"\n")?;
    let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).unwrap_or_default().trim().to_owned();
    let slot = get("slot").chars().next().ok_or("the slot booted from was not read")?;
    let slots = crate::slots::read(host)?;
    let s = slots.iter().find(|s| s.name == slot).ok_or("no such slot")?;
    Ok(Evidence { slot, running_kernel: get("kernel"), slot_kernel: s.kernel.clone(), image: s.image.clone(), partition_sha256: s.sha256.clone() })
}

/// Records that the running, flashed system booted: the unconfirmed RAM
/// boots counted back to 0, an event in the history. The image confirmed
/// for flashing (confirmed_sha256) is left as it is - that one only a RAM
/// boot gives. Refused unless the evidence holds.
pub fn confirm_flashed(serial: &str, ev: &Evidence) -> Result<(), String> {
    if !ev.holds() {
        return Err(format!("the kernel running ({}) is not the one in slot {} ({}): nothing confirmed", ev.running_kernel, ev.slot, ev.slot_kernel));
    }
    let path = state_path();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| "{\"version\": 2, \"devices\": {}}".into());
    let mut v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if v["version"] != 2 {
        return Err(format!("{}: not the state's version 2", path.display()));
    }
    let d = &mut v["devices"][serial];
    if d.is_null() {
        *d = serde_json::json!({"consecutive_unconfirmed": 0, "confirmed_sha256": null, "product": "surfaceduo", "baseline": null, "history": []});
    }
    d["consecutive_unconfirmed"] = 0.into();
    let event = format!(
        "CONFIRMED by a flashed boot of {} (slot _{}, running, {}) - gridbay",
        ev.partition_sha256,
        ev.slot,
        ev.image.clone().unwrap_or_else(|| format!("Linux {}", ev.running_kernel))
    );
    let ts = std::process::Command::new("date").arg("+%Y-%m-%d %H:%M:%S").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).unwrap_or_default();
    let history = d["history"].as_array_mut().ok_or("the state's history is not a list")?;
    history.push(serde_json::json!({"ts": ts, "event": event}));
    let keep = history.len().saturating_sub(50);
    history.drain(..keep);
    // Written whole or not at all.
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.new");
    std::fs::write(&tmp, serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}

/// The state, read for changing and written back whole (a temporary file
/// renamed), with this phone's part made if missing.
fn edit(serial: &str, change: impl FnOnce(&mut serde_json::Value) -> Result<(), String>) -> Result<(), String> {
    let path = state_path();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| "{\"version\": 2, \"devices\": {}}".into());
    let mut v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if v["version"] != 2 {
        return Err(format!("{}: not the state's version 2", path.display()));
    }
    let d = &mut v["devices"][serial];
    if d.is_null() {
        *d = serde_json::json!({"consecutive_unconfirmed": 0, "confirmed_sha256": null, "product": "surfaceduo", "baseline": null, "history": []});
    }
    change(d)?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.new");
    std::fs::write(&tmp, serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
}

fn stamp() -> String {
    std::process::Command::new("date").arg("+%Y-%m-%d %H:%M:%S").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).unwrap_or_default()
}

fn push_event(d: &mut serde_json::Value, event: &str) -> Result<(), String> {
    let history = d["history"].as_array_mut().ok_or("the state's history is not a list")?;
    history.push(serde_json::json!({"ts": stamp(), "event": event}));
    let extra = history.len().saturating_sub(50);
    history.drain(..extra);
    Ok(())
}

/// An event in this phone's history.
pub fn log(serial: &str, event: &str) -> Result<(), String> {
    edit(serial, |d| push_event(d, event))
}

/// This phone's health baseline, if one was taken.
pub fn baseline(serial: &str) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(state_path()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let b = v["devices"][serial]["baseline"].clone();
    (!b.is_null()).then_some(b)
}

/// A RAM boot about to be tried: counted and logged first (flash-safely.sh's
/// order); the attempt's number.
pub fn count_attempt(serial: &str, sha: &str) -> Result<u64, String> {
    let mut n = 0;
    edit(serial, |d| {
        n = d["consecutive_unconfirmed"].as_u64().unwrap_or(0) + 1;
        if n > MAX_UNCONFIRMED {
            return Err("the RAM boot gate is closed".into());
        }
        d["consecutive_unconfirmed"] = n.into();
        d["confirmed_sha256"] = serde_json::Value::Null;
        d["last_image_sha256"] = sha.into();
        d["product"] = "surfaceduo".into();
        push_event(d, &format!("ram-boot attempt {n}/{MAX_UNCONFIRMED} sha={sha} - gridbay"))
    })?;
    Ok(n)
}

/// A RAM boot that came up: the counter to 0, what fastboot read before it
/// the fresh baseline, and - for an image that boots Linux - the image
/// confirmed for flashing. A recovery never is: TWRP is RAM-booted only,
/// never flashed (SAFETY.md).
pub fn confirm_ram_boot(serial: &str, sha: &str, fb: &crate::ramboot::Fastboot, flashable: bool) -> Result<(), String> {
    edit(serial, |d| {
        d["consecutive_unconfirmed"] = 0.into();
        d["confirmed_sha256"] = if flashable { sha.into() } else { serde_json::Value::Null };
        d["baseline"] = serde_json::json!({
            "retry_a": fb.retry_a, "retry_b": fb.retry_b,
            "unbootable_a": fb.unbootable_a, "unbootable_b": fb.unbootable_b,
            "critical": fb.critical, "product": fb.product, "captured": stamp(),
        });
        push_event(d, &format!("CONFIRMED ram-boot of {sha}{} - gridbay", if flashable { "" } else { " (a recovery: not to be flashed)" }))
    })
}

/// The image confirmed for flashing taken back (a recovery's, confirmed by
/// an older gridbay).
pub fn unconfirm_flashing(serial: &str, why: &str) -> Result<(), String> {
    edit(serial, |d| {
        d["confirmed_sha256"] = serde_json::Value::Null;
        push_event(d, &format!("flash confirmation withdrawn: {why} - gridbay"))
    })
}
