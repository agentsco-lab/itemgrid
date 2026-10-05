//! duo-motion: the Duo's motion for Cradle, on the phone.
//!
//! Cradle starts it over ssh (as root) while its window follows the phone,
//! and reads its lines; it ends when they can no longer be written (the
//! window closed, the link gone). Straight from sensorfw's data socket -
//! its own sessions, so the sensors run whatever the display does (the
//! port's sfduo-posture stops its own with the display) - and from the lid
//! switch's input device:
//!
//!   v 2          the protocol's version, first
//!   q W X Y Z    the phone's orientation, a quaternion: the right half's
//!                frame (x across its panel, y along it to its top, z out of
//!                its screen) to the world's (x to magnetic north, level; y
//!                west; z up), about 60 a second while it moves (ahrs.rs)
//!   n 0|1        whether the field is heeded just now (as it was: no
//!                computer or steel close by bending it)
//!   g X Y Z      which way is down in g, smoothed, ten a second
//!   h DEG        the hinge, as the sensor says it (whole degrees)
//!   l 0|1        the lid shut
//!   look RAD     the phone looked at: held up still, its top raised, its
//!                screen to the side, the lid open - the way its screen
//!                faces about the world's z, from north (counter-clockwise,
//!                as the quaternion turns): where the one looking at it is
//!                (again each 2 s while it lasts)
//!   .            alive, each second
//!
//! --awake: a kernel wakelock held while it runs (on the cable: the phone
//! does not sleep while Cradle follows it). Held for a few seconds at a
//! time and renewed each second: killed (the link cut, ssh's hangup), it
//! lets go by itself - a lock without a timeout outlives its process, and
//! those left kept the phone from sleeping until its suspend hung.

mod ahrs;
mod sensorfw;

use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::process::Command;
use std::time::{Duration, Instant};

use sensorfw::{f32_at, i32_at, u32_at, u64_at, Sensor};

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}
extern "C" {
    fn poll(fds: *mut PollFd, n: u64, timeout: i32) -> i32;
}
const POLLIN: i16 = 1;

/// The lid switch's input device (a switch device with SW_LID).
fn lid_device() -> Option<std::fs::File> {
    let text = std::fs::read_to_string("/proc/bus/input/devices").ok()?;
    for block in text.split("\n\n") {
        let sw = block.lines().find_map(|l| l.strip_prefix("B: SW=")).and_then(|m| u64::from_str_radix(m.trim().split(' ').next_back()?, 16).ok());
        if sw.is_some_and(|m| m & 1 == 1) {
            let ev = block.lines().find_map(|l| l.strip_prefix("H: Handlers="))?.split_whitespace().find(|h| h.starts_with("event"))?.to_owned();
            return std::fs::File::open(format!("/dev/input/{ev}")).ok();
        }
    }
    None
}

/// Looked at: still for 0.4 s, its top up 30 to 82 degrees, its screen to
/// the side (not to the floor), the lid open.
struct Looking {
    since: Option<Instant>,
    told: Instant,
}

impl Looking {
    fn new() -> Looking {
        Looking { since: None, told: Instant::now() - Duration::from_secs(10) }
    }

    /// The way to tell, when it is time to.
    fn check(&mut self, a: &ahrs::Ahrs, turning: f64, lid_shut: bool) -> Option<f64> {
        let [_, top, screen] = a.axes();
        let side = (screen[0] * screen[0] + screen[1] * screen[1]).sqrt();
        let held_up = !lid_shut && (0.5..0.99).contains(&top[2]) && side > 0.4 && screen[2] > -0.3 && turning < 0.4;
        if !held_up {
            self.since = None;
            return None;
        }
        let since = *self.since.get_or_insert_with(Instant::now);
        if since.elapsed() >= Duration::from_millis(400) && self.told.elapsed() >= Duration::from_secs(2) {
            self.told = Instant::now();
            return Some(screen[1].atan2(screen[0]));
        }
        None
    }
}

fn main() {
    let awake = std::env::args().any(|a| a == "--awake");
    let lock = format!("duo-motion-{}", std::process::id());
    let hold = || {
        if awake {
            let _ = std::fs::write("/sys/power/wake_lock", format!("{lock} 5000000000"));
        }
    };
    // Those an earlier one left (killed, before they timed out) let go.
    if let Ok(held) = std::fs::read_to_string("/sys/power/wake_lock") {
        for old in held.split_whitespace().filter(|l| l.starts_with("duo-motion-")) {
            let _ = std::fs::write("/sys/power/wake_unlock", old);
        }
    }
    hold();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut say = |line: String| -> bool { out.write_all(line.as_bytes()).and_then(|_| out.write_all(b"\n")).and_then(|_| out.flush()).is_ok() };
    let mut alive = say("v 2".into());

    let kinds = [sensorfw::ACCEL, sensorfw::GYRO, sensorfw::MAG, sensorfw::HINGE];
    let mut sensors: Vec<Option<Sensor>> = kinds.iter().map(|k| Sensor::open(*k)).collect();
    for (k, s) in kinds.iter().zip(&sensors) {
        if s.is_none() {
            eprintln!("duo-motion: no {}", k.name);
        }
    }
    let mut lid = lid_device();
    let mut lid_shut = Command::new("busctl")
        .args(["get-property", "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager", "LidClosed"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("true"));
    alive &= say(format!("l {}", lid_shut as u8));
    if let Some(reply) = sensorfw::gdbus("/SensorManager/hingesensor", "org.freedesktop.DBus.Properties.Get", &["local.HingeSensor", "hinge"]) {
        if let Some(deg) = reply.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).last().and_then(|s| s.parse::<u32>().ok()) {
            alive &= say(format!("h {deg}"));
        }
    }

    let began = Instant::now();
    let mut ahrs = ahrs::Ahrs::default();
    let mut looking = Looking::new();
    // The gyroscope (its offset still in: ahrs takes it off), held between
    // its readings (about 20 a second against the accelerometer's 100).
    let mut rate = [0.0f64; 3];
    let mut down = [0.0f64; 3];
    let mut field: Option<[f64; 3]> = None;
    let mut fresh_field = false;
    let mut steady_since: Option<Instant> = None;
    let mut last_t: Option<u64> = None;
    let mut sent_q = [0.0f64; 4];
    let mut sent_q_at = Instant::now() - Duration::from_secs(1);
    let mut sent_g = [9.0f64; 3];
    let mut sent_g_at = Instant::now() - Duration::from_secs(1);
    let mut sent_north: Option<bool> = None;
    let mut alive_at = Instant::now();
    let mut reopen_at: Option<Instant> = None;

    while alive {
        let mut fds: Vec<PollFd> = sensors.iter().flatten().map(|s| PollFd { fd: s.sock.as_raw_fd(), events: POLLIN, revents: 0 }).collect();
        let lid_at = fds.len();
        if let Some(f) = &lid {
            fds.push(PollFd { fd: f.as_raw_fd(), events: POLLIN, revents: 0 });
        }
        unsafe { poll(fds.as_mut_ptr(), fds.len() as u64, 200) };
        // The lid's device blocks: read only when it has something.
        let lid_ready = fds.get(lid_at).is_some_and(|f| f.revents & POLLIN != 0);

        // The gyroscope: milli-degrees a second.
        if let Some(g) = sensors[1].as_mut() {
            for r in g.take() {
                rate = [0, 1, 2].map(|i| (f32_at(&r, 8 + 4 * i) as f64 / 1000.0).to_radians());
            }
        }
        // The field: nT, calibrated by the sensor hub.
        if let Some(m) = sensors[2].as_mut() {
            for r in m.take() {
                let v = [0, 1, 2].map(|i| i32_at(&r, 8 + 4 * i) as f64);
                field = Some(match field {
                    Some(f) => [0, 1, 2].map(|i| f[i] + (v[i] - f[i]) * 0.3),
                    None => v,
                });
                fresh_field = true;
            }
        }
        // The accelerometer (milli-g) drives the steps, 100 a second.
        if let Some(a) = sensors[0].as_mut() {
            for r in a.take() {
                let t = u64_at(&r, 0);
                let raw = [0, 1, 2].map(|i| f32_at(&r, 8 + 4 * i) as f64 / 1000.0);
                // Down steady: the reading near the smoothed one, about 1 g.
                let off = (0..3).map(|i| (raw[i] - down[i]).abs()).fold(0.0, f64::max);
                let size = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]).sqrt();
                if off < 0.02 && (size - 1.0).abs() < 0.05 {
                    steady_since.get_or_insert_with(Instant::now);
                } else {
                    steady_since = None;
                }
                down = [0, 1, 2].map(|i| down[i] + (raw[i] - down[i]) * 0.15);
                if !ahrs.started {
                    // Started once the field is there (north at once), or
                    // without it after a second.
                    if field.is_some() || sensors[2].is_none() || began.elapsed() >= Duration::from_secs(1) {
                        ahrs.start(raw, field);
                    }
                    last_t = Some(t);
                    continue;
                }
                let dt = last_t.map_or(0.01, |p| t.saturating_sub(p) as f64 / 1e6).clamp(0.0, 0.1);
                last_t = Some(t);
                let spin = (rate[0] * rate[0] + rate[1] * rate[1] + rate[2] * rate[2]).sqrt();
                let still = spin < 0.06 && steady_since.is_some_and(|s| s.elapsed() >= Duration::from_millis(500));
                ahrs.step(rate, raw, field.filter(|_| std::mem::take(&mut fresh_field)), dt, still);
            }
        }
        if let Some(h) = sensors[3].as_mut() {
            for r in h.take() {
                alive &= say(format!("h {}", u32_at(&r, 8)));
            }
        }
        if let Some(f) = lid.as_mut().filter(|_| lid_ready) {
            let mut ev = [0u8; 24 * 16];
            if let Ok(n) = f.read(&mut ev) {
                for e in ev[..n].chunks_exact(24) {
                    let (kind, code, value) = (u16::from_le_bytes([e[16], e[17]]), u16::from_le_bytes([e[18], e[19]]), i32::from_le_bytes(e[20..24].try_into().unwrap()));
                    if kind == 5 && code == 0 {
                        lid_shut = value != 0;
                        alive &= say(format!("l {}", lid_shut as u8));
                    }
                }
            }
        }
        // Closed sockets let go of, and their sensors asked for again a
        // little later (sensorfw drops a session now and then).
        for (k, s) in kinds.iter().zip(sensors.iter_mut()) {
            if s.as_ref().is_some_and(|x| x.dead) {
                eprintln!("duo-motion: {} gone", k.name);
                *s = None;
                reopen_at = Some(Instant::now() + Duration::from_secs(3));
            }
        }
        if reopen_at.is_some_and(|t| Instant::now() >= t) {
            reopen_at = None;
            for (k, s) in kinds.iter().zip(sensors.iter_mut()) {
                if s.is_none() {
                    *s = Sensor::open(*k);
                }
            }
        }

        let now = Instant::now();
        if ahrs.started {
            let spin = (0..3).map(|i| (rate[i] - ahrs.bias[i]).powi(2)).sum::<f64>().sqrt();
            if let Some(b) = looking.check(&ahrs, spin, lid_shut) {
                alive &= say(format!("look {b:.4}"));
            }
            if now.duration_since(sent_q_at) >= Duration::from_millis(16) {
                let q = ahrs.q;
                if q.iter().zip(sent_q).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max) > 2e-4 {
                    alive &= say(format!("q {:.5} {:.5} {:.5} {:.5}", q[0], q[1], q[2], q[3]));
                    sent_q = q;
                    sent_q_at = now;
                }
            }
            if sent_north != Some(ahrs.north_trusted) {
                alive &= say(format!("n {}", ahrs.north_trusted as u8));
                sent_north = Some(ahrs.north_trusted);
            }
        }
        if now.duration_since(sent_g_at) >= Duration::from_millis(100) && down.iter().zip(sent_g).any(|(a, b)| (a - b).abs() > 0.01) {
            alive &= say(format!("g {:.3} {:.3} {:.3}", down[0], down[1], down[2]));
            sent_g = down;
            sent_g_at = now;
        }
        if now.duration_since(alive_at) >= Duration::from_secs(1) {
            alive &= say(".".into());
            alive_at = now;
            hold();
        }
    }
    for s in sensors.iter().flatten() {
        s.close();
    }
    if awake {
        let _ = std::fs::write("/sys/power/wake_unlock", &lock);
    }
}
