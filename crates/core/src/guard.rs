//! The safety rules for what is sent to the phone. Every script goes through
//! `check` before it leaves the computer.

/// What is never sent, and why.
const REFUSED: &[(&str, &str)] = &[
    ("/sys/kernel/debug/gpio", "reading the GPIO debug file resets the Duo at once"),
    ("of=/dev/", "writing a block device is flashing: that goes through cradle flash, RAM boot first"),
    ("> /dev/sd", "writing a block device is flashing: that goes through cradle flash, RAM boot first"),
    (">/dev/sd", "writing a block device is flashing: that goes through cradle flash, RAM boot first"),
    ("mkfs", "making a filesystem is flashing: that goes through cradle flash"),
    ("blkdiscard", "discarding a block device erases it: that goes through cradle flash"),
];

/// Said when an interactive shell opens: what is typed there cannot be
/// checked.
pub const SHELL_WARNING: &str = "Interactive: what you type is not checked. Never read /sys/kernel/debug/gpio (the Duo resets at once); never write block devices here (cradle flash, RAM boot first); restart item by rebooting, not in place (#119).";

/// Whether a script may be sent to the phone; the reason if not.
pub fn check(script: &str) -> Result<(), String> {
    for (pattern, why) in REFUSED {
        if script.contains(pattern) {
            return Err(format!("refused: {why} ({pattern})"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn refuses_gpio_debug() {
        assert!(super::check("cat /sys/kernel/debug/gpio").is_err());
        assert!(super::check("uptime").is_ok());
    }

    #[test]
    fn refuses_writing_block_devices() {
        assert!(super::check("dd if=boot.img of=/dev/sde14").is_err());
        assert!(super::check("mkfs.ext4 /dev/loop3").is_err());
        assert!(super::check("ls /dev/block/by-name").is_ok());
    }
}
