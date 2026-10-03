//! cradle-core: what a connected Surface Duo is doing, and the safe ways to
//! act on it. The command line (crates/cli) and, later, the window are only
//! ways of showing what is here: every action and every safety rule lives in
//! this crate, so neither can be gone around.

/// What the phone is doing, as seen from the computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Linux up (Droidian and item): the USB network, ssh.
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
    /// What can be done from here, in a line.
    pub fn means(self) -> &'static str {
        match self {
            Mode::Linux => "Linux is up: status, update, logs, shell, screenshots, reboot",
            Mode::Fastboot => "fastboot: boot an image from RAM; flashing only after a good RAM boot",
            Mode::Recovery => "recovery: backups and restores",
            Mode::Android => "Android: not Linux; switching images comes with flashing",
            Mode::Gone => "not seen: check the cable, the battery, the keys",
        }
    }
}
