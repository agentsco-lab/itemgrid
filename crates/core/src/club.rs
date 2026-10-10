//! The Duo owners' club - the device registry on agentsco.uk (AgentsCo's
//! AC-191). Each Duo gets a number there, in order (00001, 00002, ...),
//! written onto the phone (/etc/item/device-id) and shown by item/grid.
//!
//! - Quiet: no account and no token - the number is claimed by the
//!   serial's hash when item/grid first sees the phone (`claim`). A token
//!   kept in the keyring from before is used only with a club that has no
//!   quiet way yet.
//! - The serial number never leaves this computer: what is sent is
//!   sha256("cradle-device:" + serial), and the server keeps only its own
//!   keyed hash of that.
//! - The numbers known here are kept in ~/.local/share/itemgrid/devices.json
//!   (club and serial -> number, label), so the phone has its number offline
//!   too.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// Kept from when it was Cradle: the token is in the keyring under it.
const SERVICE: &str = "cradle";
const ACCOUNT: &str = "agentsco-registry";

/// The registry's site: ITEMGRID_REGISTRY, ~/.config/itemgrid/registry, or
/// agentsco.uk.
pub fn server() -> String {
    std::env::var("ITEMGRID_REGISTRY")
        .ok()
        .or_else(|| std::fs::read_to_string(crate::paths::config().join("registry")).ok().map(|s| s.trim().to_owned()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://agentsco.uk".into())
        .trim_end_matches('/')
        .to_owned()
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, &format!("{ACCOUNT}@{}", server())).map_err(|e| e.to_string())
}

/// The token kept in the keyring, if any.
pub fn token() -> Option<String> {
    entry().ok()?.get_password().ok()
}

/// A token kept in the keyring (from the site's Settings -> Device registry).
pub fn set_token(token: &str) -> Result<(), String> {
    let token = token.trim();
    if !token.starts_with("creg_") {
        return Err("that is not a registry token (they start with creg_)".into());
    }
    entry()?.set_password(token).map_err(|e| format!("the keyring: {e}"))
}

pub fn forget_token() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// What the serial becomes before it leaves this computer.
pub fn serial_hash(serial: &str) -> String {
    use sha2::{Digest, Sha256};
    // "cradle-device:" kept from when it was Cradle: the club knows the
    // phones by it.
    format!("{:x}", Sha256::digest(format!("cradle-device:{serial}").as_bytes()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub number: String,
    pub model: String,
    pub label: Option<String>,
    #[serde(default)]
    pub new: bool,
}

fn local_path() -> PathBuf {
    crate::paths::data().join("devices.json")
}

fn local() -> HashMap<String, Device> {
    std::fs::read_to_string(local_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// A serial's key in the list: with the club it is from, so a number from a
/// test server never passes for the real one.
fn key(serial: &str) -> String {
    format!("{} {serial}", server())
}

/// The number this computer knows for a serial, from this club.
pub fn known(serial: &str) -> Option<Device> {
    local().get(&key(serial)).cloned()
}

fn remember(serial: &str, device: &Device) {
    let mut all = local();
    all.insert(key(serial), device.clone());
    let path = local_path();
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(text) = serde_json::to_string_pretty(&all) {
        let tmp = path.with_extension("json.new");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// The phone's number in the club, asked for quietly - no account, no
/// token: the serial's hash sent, the number the club keeps for it given
/// back (the next free one the first time - 00001, 00002, ... - and the same
/// one for good after). Known here already, nothing is sent.
///
/// The club's side (POST /api/registry/claim, docs/REGISTRY.md): one number
/// per hash, never given again; new hashes limited per address.
pub fn claim(serial: &str) -> Result<Device, String> {
    if serial.trim().is_empty() {
        return Err("no serial".into());
    }
    if let Some(d) = known(serial) {
        return Ok(d);
    }
    let body = serde_json::json!({"serial_hash": serial_hash(serial), "model": "surfaceduo"});
    let (status, text) = post(&format!("{}/api/registry/claim", server()), &body, None)?;
    // A club without the quiet way yet: the old one, with a token kept from
    // before (the owner's).
    let (status, text) = match (status, token()) {
        (404 | 405, Some(t)) => post(&format!("{}/api/registry/devices", server()), &body, Some(&t))?,
        other => (other.0, text),
    };
    match status {
        200 | 201 => {}
        429 => return Err("the club: too many new phones from here just now".into()),
        409 => return Err("the club: this Duo's number is held elsewhere".into()),
        s => return Err(format!("the club answered {s}: {}", text.trim())),
    }
    let device: Device = serde_json::from_str(&text).map_err(|e| format!("the club's answer: {e}"))?;
    remember(serial, &device);
    Ok(device)
}

fn post(url: &str, body: &serde_json::Value, token: Option<&str>) -> Result<(u16, String), String> {
    let mut req = ureq::post(url);
    if let Some(t) = token {
        req = req.header("Authorization", &format!("Bearer {t}"));
    }
    let mut resp = req.config().http_status_as_error(false).timeout_global(Some(std::time::Duration::from_secs(15))).build().send_json(body).map_err(|e| format!("the club: {e}"))?;
    Ok((resp.status().as_u16(), resp.body_mut().read_to_string().unwrap_or_default()))
}

/// The number written onto the phone at `host` (/etc/item/device-id), if it
/// is not there yet.
pub fn put_on_phone(host: &str, device: &Device) -> Result<(), String> {
    if on_phone(host).as_deref() == Some(device.number.as_str()) {
        return Ok(());
    }
    crate::phone::run(host, &format!("mkdir -p /etc/item && printf '%s\\n' {} > /etc/item/device-id\n", crate::logs::quote(&device.number))).map(|_| ())
}

/// The phone at `host`: its number claimed and written onto it.
pub fn register(host: &str) -> Result<Device, String> {
    let serial = crate::backup::serial(host)?;
    let device = claim(&serial)?;
    put_on_phone(host, &device)?;
    Ok(device)
}

/// The number written on the phone, if it has one.
pub fn on_phone(host: &str) -> Option<String> {
    crate::phone::run(host, "cat /etc/item/device-id 2>/dev/null\n").ok().map(|s| s.trim().to_owned()).filter(|s| s.len() == 5 && s.chars().all(|c| c.is_ascii_digit()))
}
