//! The screen saver: idle for a while (nothing pressed, the pointer still),
//! the window fills the second monitor, the eye drifting slowly over the
//! table; anything done since takes it back. GNOME has no screen savers of
//! others' to run: the idle time is its own (Mutter's IdleMonitor, on X11
//! and Wayland alike), the rest the window's.

use gtk::prelude::*;
use gtk::{gdk, gio};

/// Idle this long, the saver comes - off for now (the window as the second
/// monitor's wallpaper instead: place::desktop).
pub const AFTER_MS: u64 = u64::MAX;

/// How long nothing has been pressed or moved (ms), as GNOME counts it -
/// not waited for on the main thread (a D-Bus call in its own time: the
/// window draws on meanwhile).
pub async fn idle_ms_async() -> Option<u64> {
    let bus = gio::bus_get_future(gio::BusType::Session).await.ok()?;
    let reply = bus
        .call_future(
            Some("org.gnome.Mutter.IdleMonitor"),
            "/org/gnome/Mutter/IdleMonitor/Core",
            "org.gnome.Mutter.IdleMonitor",
            "GetIdletime",
            None,
            Some(gtk::glib::VariantTy::new("(t)").ok()?),
            gio::DBusCallFlags::NONE,
            500,
        )
        .await
        .ok()?;
    reply.get::<(u64,)>().map(|(ms,)| ms)
}

/// Nothing done at this computer this long (ms): the phone let go -
/// followed no more, not looked for over Wi-Fi - until something is.
pub fn rest_after_ms() -> u64 {
    // ITEMGRID_REST_S=seconds: sooner (to try it).
    std::env::var("ITEMGRID_REST_S").ok().and_then(|v| v.parse::<u64>().ok()).map_or(10 * 60 * 1000, |s| s * 1000)
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
