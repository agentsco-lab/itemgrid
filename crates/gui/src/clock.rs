//! The frame's time for the animations: GTK's frame clock gives each frame
//! one time, smoothed to the display's refresh (gdk_frame_clock_get_frame_time
//! - "the time that should be used for animations"); the animations took
//! the system clock instead, each read a little later than the one before
//! inside a frame and off the frames' beat, so what was drawn for a frame
//! was not quite drawn for its moment. Set at the top of the tick (main.rs)
//! and read everywhere an animation asks what time it is; outside a frame
//! (a click between two) the last frame's time, which is near enough.

use std::cell::Cell;
use std::time::{Duration, Instant};

thread_local! {
    static FRAME: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// This frame's time (GDK's frame_time: µs on the monotonic clock), as
/// an Instant.
pub fn frame(frame_time_us: i64) {
    let now = Instant::now();
    let off = gtk::glib::monotonic_time() - frame_time_us;
    let at = if off >= 0 { now.checked_sub(Duration::from_micros(off as u64)) } else { now.checked_add(Duration::from_micros((-off) as u64)) };
    FRAME.with(|f| f.set(Some(at.unwrap_or(now))));
}

/// The time the animations take as now.
pub fn now() -> Instant {
    FRAME.with(|f| f.get()).unwrap_or_else(Instant::now)
}

/// How long since `then` (never negative).
pub fn since(then: Instant) -> Duration {
    now().saturating_duration_since(then)
}

/// Seconds since `then` (never negative).
pub fn secs_since(then: Instant) -> f32 {
    since(then).as_secs_f32()
}
