//! How the computer reaches the phone's Linux: the USB cable, or the home
//! network (tracker #156).
//!
//! While the phone is on the cable, item/grid notes - per serial - its Wi-Fi
//! address, its name on the network and its ssh host key. Off the cable it
//! looks for the phone at that address and by that name, and talks to it
//! only if the host key is the one it noted: no other device on the network
//! is taken for the phone. The key changes with each new system image; the
//! cable teaches item/grid the new one.
//!
//! Over Wi-Fi: status, logs, Update item, the quick backup, screenshots,
//! restarts. Cable only: everything that takes the phone out of Linux (the
//! bootloader and TWRP have no Wi-Fi), and the whole-system backup.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// The phone on the USB link.
pub const CABLE: &str = "172.16.42.1";

/// How the phone was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Cable,
    Wifi,
}

impl Via {
    pub fn of(host: &str) -> Via {
        if host == CABLE {
            Via::Cable
        } else {
            Via::Wifi
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Via::Cable => "cable",
            Via::Wifi => "Wi-Fi",
        }
    }
}

/// A phone as seen on the cable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Known {
    pub serial: String,
    /// Its Wi-Fi address then.
    pub address: String,
    /// Its name on the network (hostname.local).
    pub name: String,
    /// Its ssh host key (ed25519, "type base64").
    pub key: String,
    /// When it was last seen on the cable, local time.
    pub seen: String,
}

fn dir() -> PathBuf {
    std::path::Path::new(&std::env::var("HOME").unwrap_or_default()).join(".local/share/itemgrid")
}

fn known_path() -> PathBuf {
    dir().join("links.json")
}

/// The known_hosts file item/grid checks Wi-Fi connections against: one alias
/// per phone (itemgrid-SERIAL), whatever its address.
pub fn known_hosts_path() -> PathBuf {
    dir().join("known_hosts")
}

pub fn known() -> Vec<Known> {
    std::fs::read_to_string(known_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save(all: &[Known]) -> Result<(), String> {
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(all).map_err(|e| e.to_string())?;
    std::fs::write(known_path(), text).map_err(|e| e.to_string())?;
    write_known_hosts(all)
}

/// The known_hosts file as the known phones say (written anew if not: one
/// written under an old name - hythe-SERIAL - matched no alias, and Wi-Fi
/// found nothing).
fn write_known_hosts(all: &[Known]) -> Result<(), String> {
    let lines: String = all.iter().map(|k| format!("itemgrid-{} {}\n", k.serial, k.key)).collect();
    if std::fs::read_to_string(known_hosts_path()).ok().as_deref() == Some(lines.as_str()) {
        return Ok(());
    }
    std::fs::write(known_hosts_path(), lines).map_err(|e| e.to_string())
}

const LEARN: &str = r#"
for w in $(cat /proc/cmdline); do case "$w" in androidboot.serialno=*) echo "serial=${w#*=}";; esac; done
echo "address=$(ip -4 -o addr show wlan0 2>/dev/null | awk '{print $4}' | cut -d/ -f1 | head -1)"
echo "name=$(hostname).local"
echo "key=$(cut -d' ' -f1,2 /etc/ssh/ssh_host_ed25519_key.pub 2>/dev/null)"
"#;

/// What the phone on the cable is called on the network, noted; whether it
/// changed. Run now and then while on the cable.
pub fn learn() -> Result<bool, String> {
    let text = crate::phone::run(CABLE, LEARN)?;
    let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).unwrap_or_default().trim().to_owned();
    let (serial, key) = (get("serial"), get("key"));
    if serial.is_empty() || !key.starts_with("ssh-ed25519 ") {
        return Err("the phone's serial or host key was not read".into());
    }
    let mut all = known();
    let was = all.iter().find(|k| k.serial == serial).cloned();
    let mut now = Known { serial: serial.clone(), address: get("address"), name: get("name"), key, seen: String::new() };
    // No Wi-Fi now: the address it had stays the one to try.
    if now.address.is_empty() {
        now.address = was.as_ref().map(|w| w.address.clone()).unwrap_or_default();
    }
    let changed = was.as_ref().is_none_or(|w| (&w.address, &w.name, &w.key) != (&now.address, &now.name, &now.key));
    now.seen = crate::backup::now();
    all.retain(|k| k.serial != serial);
    all.push(now);
    save(&all)?;
    Ok(changed)
}

/// Wi-Fi addresses resolved to the phone they are for (the ssh alias).
static FOR: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// The serial a Wi-Fi host was found for: its host key is checked as that
/// phone's.
pub fn serial_of(host: &str) -> Option<String> {
    FOR.lock().unwrap().as_ref()?.get(host).cloned()
}

/// Where to look for the known phones on the network, newest first: the
/// address each had, then its name resolved now.
pub fn wifi_hosts() -> Vec<String> {
    let mut all = known();
    if let Err(e) = write_known_hosts(&all) {
        eprintln!("itemgrid: the phones' host keys: {e}");
    }
    all.sort_by(|a, b| b.seen.cmp(&a.seen));
    let mut out: Vec<String> = Vec::new();
    let mut map = HashMap::new();
    for k in &all {
        let mut tries = Vec::new();
        if !k.address.is_empty() {
            tries.push(k.address.clone());
        }
        if let Some(ip) = resolve(&k.name) {
            tries.push(ip);
        }
        for t in tries {
            if !out.contains(&t) {
                map.insert(t.clone(), k.serial.clone());
                out.push(t);
            }
        }
    }
    *FOR.lock().unwrap() = Some(map);
    out
}

/// Whether `host`'s ssh port takes a connection, at once (0.3 s at most).
pub fn port_open(host: &str) -> bool {
    format!("{host}:22").parse::<std::net::SocketAddr>().is_ok_and(|a| std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_millis(300)).is_ok())
}

/// The addresses the known phones had (newest first), noted for their
/// host keys - no names resolved (wifi_hosts: up to 2 s each, the phone
/// asleep).
pub fn wifi_addresses() -> Vec<String> {
    let mut all = known();
    all.sort_by(|a, b| b.seen.cmp(&a.seen));
    let mut guard = FOR.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    let mut out = Vec::new();
    for k in all.iter().filter(|k| !k.address.is_empty()) {
        if !out.contains(&k.address) {
            map.insert(k.address.clone(), k.serial.clone());
            out.push(k.address.clone());
        }
    }
    out
}

/// Whether a known phone's ssh answers on the network at the address it
/// had, at once (a connection opened and closed, 0.3 s at most each): to
/// notice it back from sleep without a whole look.
pub fn wifi_answers_quickly() -> bool {
    known().iter().filter(|k| !k.address.is_empty()).any(|k| port_open(&k.address))
}

/// A .local name to an address (nss-mdns through getent), quickly.
fn resolve(name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let out = std::process::Command::new("timeout").args(["2", "getent", "ahostsv4", name]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).split_whitespace().next().map(str::to_owned)
}

/// Whether the phone's USB network is up on this computer: an interface
/// holding an address in 172.16.42.0/24. Without it 172.16.42.1 is no
/// phone - some other network can answer there (one did, 8 ms away), and
/// the cable's ssh checks no host key.
pub fn usb_up() -> bool {
    std::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.contains(" 172.16.42.") && !l.contains(" 172.16.42.1/")))
        .unwrap_or(false)
}

/// `ITEMGRID_NO_CABLE=1`: the cable taken as not there (to try Wi-Fi with
/// the phone still plugged in).
pub fn cable_ignored() -> bool {
    std::env::var_os("ITEMGRID_NO_CABLE").is_some()
}

/// Whether the phone is on the cable: its Linux there, or adb or fastboot.
pub fn on_cable() -> bool {
    if cable_ignored() {
        return false;
    }
    crate::phone::answers(CABLE) || known().iter().any(|k| crate::ramboot::usb_serial_present(&k.serial))
}

/// Refused unless the phone is on the cable: what leaves Linux goes where
/// only the cable reaches.
pub fn need_cable(host: &str) -> Result<(), String> {
    if (Via::of(host) == Via::Cable && !cable_ignored()) || on_cable() {
        return Ok(());
    }
    Err("plug in the cable: this takes the phone out of Linux, where Wi-Fi does not reach".into())
}
