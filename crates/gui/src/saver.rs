//! The screen saver: idle for a while (nothing pressed, the pointer still),
//! the window fills the second monitor, the eye drifting slowly over the
//! table; anything done since takes it back. GNOME has no screen savers of
//! others' to run: the idle time is its own (Mutter's IdleMonitor, on X11
//! and Wayland alike), the rest the window's.

use gtk::prelude::*;
use gtk::{gdk, gio};

/// Idle this long, the saver comes.
pub const AFTER_MS: u64 = 5 * 60 * 1000;

/// How long nothing has been pressed or moved (ms), as GNOME counts it.
pub fn idle_ms() -> Option<u64> {
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()?;
    let reply = bus
        .call_sync(
            Some("org.gnome.Mutter.IdleMonitor"),
            "/org/gnome/Mutter/IdleMonitor/Core",
            "org.gnome.Mutter.IdleMonitor",
            "GetIdletime",
            None,
            Some(gtk::glib::VariantTy::new("(t)").ok()?),
            gio::DBusCallFlags::NONE,
            500,
            gio::Cancellable::NONE,
        )
        .ok()?;
    reply.get::<(u64,)>().map(|(ms,)| ms)
}

/// The second monitor: not the primary one (xrandr's word for it on X11);
/// with one monitor, that one.
pub fn second_monitor(display: &gdk::Display) -> Option<gdk::Monitor> {
    let monitors: Vec<gdk::Monitor> = display.monitors().iter::<gdk::Monitor>().flatten().collect();
    let primary = std::process::Command::new("xrandr")
        .arg("--query")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().find(|l| l.contains(" connected primary")).and_then(|l| l.split_whitespace().next().map(str::to_owned)));
    monitors.iter().find(|m| primary.is_some() && m.connector().map(|c| c.to_string()) != primary).or(monitors.last()).cloned()
}
