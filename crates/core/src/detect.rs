//! What the phone is doing, as seen from the computer: Linux answers ssh,
//! the bootloader answers fastboot, recovery and Android answer adb.

use std::process::{Command, Stdio};

/// What the phone is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Linux up (Droidian and item): ssh.
    Linux,
    /// The bootloader's fastboot.
    Fastboot,
    /// TWRP or another recovery, over adb.
    Recovery,
    /// Android (a test image), over adb.
    Android,
    /// Nothing seen.
    Gone,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Linux => "Linux",
            Mode::Fastboot => "fastboot",
            Mode::Recovery => "recovery",
            Mode::Android => "Android",
            Mode::Gone => "not seen",
        }
    }

    /// What can be done from here, in a line.
    pub fn means(self) -> &'static str {
        match self {
            Mode::Linux => "status, update, logs, shell, screenshots, reboot",
            Mode::Fastboot => "boot an image from RAM; flashing only after a good RAM boot (not yet in hythe)",
            Mode::Recovery => "backups and restores (not yet in hythe)",
            Mode::Android => "not Linux; switching images comes with flashing (not yet in hythe)",
            Mode::Gone => "check the cable, the battery; hold power and volume down for fastboot",
        }
    }
}

/// The mode and where it was seen: the ssh host, or the serial number.
#[derive(Debug, Clone)]
pub struct Seen {
    pub mode: Mode,
    pub via: String,
}

/// Finds out what the phone is doing.
pub fn detect() -> Seen {
    let cable = crate::link::CABLE;
    let ignored = crate::link::cable_ignored();
    if !ignored && crate::phone::answers(cable) {
        learn_now_and_then();
        return Seen { mode: Mode::Linux, via: cable.to_owned() };
    }
    // Asleep: the link pings, ssh does not answer until it is woken.
    if !ignored && crate::phone::pings(cable) && crate::phone::wake(cable) {
        return Seen { mode: Mode::Linux, via: cable.to_owned() };
    }
    if let Some(line) = lines("fastboot", &["devices"]).into_iter().next() {
        let serial = line.split_whitespace().next().unwrap_or_default().to_owned();
        return Seen { mode: Mode::Fastboot, via: serial };
    }
    for line in lines("adb", &["devices"]).into_iter().skip_while(|l| l.starts_with("List of devices")) {
        let mut parts = line.split_whitespace();
        let (Some(serial), Some(state)) = (parts.next(), parts.next()) else { continue };
        let mode = match state {
            "recovery" | "sideload" => Mode::Recovery,
            "device" => Mode::Android,
            _ => continue,
        };
        return Seen { mode, via: serial.to_owned() };
    }
    // Off the cable: on the network, where the cable showed it (link.rs) -
    // unless HYTHE_NO_WIFI=1 (a battery test: a look over Wi-Fi wakes it).
    if std::env::var_os("HYTHE_NO_WIFI").is_some() {
        return Seen { mode: Mode::Gone, via: String::new() };
    }
    if let Some(host) = crate::link::wifi_hosts().into_iter().find(|h| crate::phone::answers(h)) {
        return Seen { mode: Mode::Linux, via: host };
    }
    Seen { mode: Mode::Gone, via: String::new() }
}

/// The phone on the cable noted for Wi-Fi (link.rs): at the first look, then
/// every ten minutes.
fn learn_now_and_then() {
    use std::sync::Mutex;
    static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap();
    if last.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(600)) {
        return;
    }
    *last = Some(std::time::Instant::now());
    if let Err(e) = crate::link::learn() {
        eprintln!("hythe: noting the phone for Wi-Fi: {e}");
    }
}

/// A tool's output lines, none if it is missing, fails or takes over 5 s
/// (adb's server, stuck after the cable came out, held a look for 40 s).
fn lines(tool: &str, args: &[&str]) -> Vec<String> {
    // adb's first call starts its server, which keeps the output's pipe
    // open: the server started on its own first, its output nowhere.
    if tool == "adb" {
        let _ = Command::new("timeout").args(["5", "adb", "start-server"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let Ok(out) = Command::new("timeout").arg("5").arg(tool).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output() else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout).lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('*')).map(str::to_owned).collect()
}
