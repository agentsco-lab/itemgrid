//! The kernel's log, the one place a recovery without a screen can tell
//! what it does (`dmesg`, or the next boot's pstore).

use std::io::Write;

pub fn say(what: std::fmt::Arguments) {
    let line = format!("<6>item-recovery: {what}\n");
    if let Ok(mut k) = std::fs::OpenOptions::new().write(true).open("/dev/kmsg") {
        let _ = k.write_all(line.as_bytes());
    }
}

#[macro_export]
macro_rules! say {
    ($($t:tt)*) => { $crate::log::say(format_args!($($t)*)) };
}
