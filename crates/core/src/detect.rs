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
            Mode::Fastboot => "boot an image from RAM; flashing only after a good RAM boot (not yet in cradle)",
            Mode::Recovery => "backups and restores (not yet in cradle)",
            Mode::Android => "not Linux; switching images comes with flashing (not yet in cradle)",
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
    if let Some(host) = crate::phone::hosts().into_iter().find(|h| crate::phone::answers(h)) {
        return Seen { mode: Mode::Linux, via: host };
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
    Seen { mode: Mode::Gone, via: String::new() }
}

/// A tool's output lines, none if it is missing or fails.
fn lines(tool: &str, args: &[&str]) -> Vec<String> {
    let Ok(out) = Command::new(tool).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output() else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout).lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('*')).map(str::to_owned).collect()
}
