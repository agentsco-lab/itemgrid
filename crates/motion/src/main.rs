//! duo-motion: the Duo's motion for Cradle, on the phone.
//!
//! Cradle starts it over ssh (as root) while its window follows the phone,
//! and reads its lines; it ends when they can no longer be written (the
//! window closed, the link gone). Straight from sensorfw's data socket -
//! its own sessions, so the sensors run whatever the display does (the
//! port's sfduo-posture stops its own with the display) - and from the lid
//! switch's input device:
//!
//!   q W X Y Z    the phone's orientation (the right half's frame: x across
//!                its panel, y along it to its top, z out of its screen; the
//!                world's z up, its yaw from where it was at the start), a
//!                quaternion, about 60 a second while it moves
//!   g X Y Z      which way is down in g, smoothed, ten a second
//!   h DEG        the hinge, as the sensor says it (whole degrees)
//!   l 0|1        the lid shut
//!   c RAD        the compass: where magnetic north is in that world, about
//!                its z (tilt-compensated), smoothed, twice a second
//!   look RAD     the phone looked at: held up still, its top raised, its
//!                screen to the side - the way its screen faces about the
//!                world's z, which is where the one looking at it is
//!                (again each 2 s while it lasts)
//!   .            alive, each second
//!
//! The orientation is fused here (Mahony's filter): the gyroscope (about 20
//! readings a second, held between them) turns it, the accelerometer (100
//! a second) pulls it back toward which way is down when the phone is not
//! being swung. The gravity sfduo-posture tells came three or four times a
//! second and knew no turn about the vertical.
//!
//! --awake: a kernel wakelock held while it runs (on the cable: the phone
//! does not sleep while Cradle follows it).

use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::time::{Duration, Instant};

const SERVICE: &str = "com.nokia.SensorService";
const SOCKET: &str = "/run/sensord.sock";

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

fn gdbus(path: &str, method: &str, args: &[&str]) -> Option<String> {
    let out = Command::new("gdbus").args(["call", "--system", "--dest", SERVICE, "--object-path", path, "--method", method]).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A sensor's session in sensorfw, with its data socket.
struct Sensor {
    name: &'static str,
    iface: &'static str,
    id: i32,
    sock: UnixStream,
    buf: Vec<u8>,
    /// Bytes a reading takes on the socket.
    size: usize,
    /// Its socket closed (sensorfw let the session go): dropped - polled, a
    /// closed socket is always readable and the loop spun a core.
    dead: bool,
}

impl Sensor {
    fn open(name: &'static str, iface: &'static str, size: usize, interval_ms: Option<u32>) -> Option<Sensor> {
        gdbus("/SensorManager", "local.SensorManager.loadPlugin", &[name])?;
        let reply = gdbus("/SensorManager", "local.SensorManager.requestSensor", &[name, "0"])?;
        let id: i32 = reply.trim_matches(|c: char| !c.is_ascii_digit() && c != '-').split(|c: char| !c.is_ascii_digit() && c != '-').next()?.parse().ok()?;
        // The socket's greeting: a byte from sensorfw, the session back.
        let mut sock = UnixStream::connect(SOCKET).ok()?;
        sock.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
        let mut one = [0u8; 1];
        sock.read_exact(&mut one).ok()?;
        sock.write_all(&id.to_le_bytes()).ok()?;
        sock.set_nonblocking(true).ok()?;
        let path = format!("/SensorManager/{name}");
        if let Some(ms) = interval_ms {
            let _ = gdbus(&path, &format!("{iface}.setInterval"), &[&id.to_string(), &ms.to_string()]);
        }
        gdbus(&path, &format!("{iface}.start"), &[&id.to_string()])?;
        Some(Sensor { name, iface, id, sock, buf: Vec::new(), size, dead: false })
    }

    /// The readings that came, each its bytes (the frames' counts taken
    /// off).
    fn take(&mut self) -> Vec<Vec<u8>> {
        let mut chunk = [0u8; 65536];
        loop {
            match self.sock.read(&mut chunk) {
                Ok(0) => {
                    self.dead = true;
                    break;
                }
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::Interrupted => break,
                Err(_) => {
                    self.dead = true;
                    break;
                }
            }
        }
        let mut out = Vec::new();
        loop {
            if self.buf.len() < 4 {
                break;
            }
            let count = u32::from_le_bytes(self.buf[..4].try_into().unwrap()) as usize;
            if count > 1000 {
                self.buf.clear();
                break;
            }
            let end = 4 + count * self.size;
            if self.buf.len() < end {
                break;
            }
            for i in 0..count {
                out.push(self.buf[4 + i * self.size..4 + (i + 1) * self.size].to_vec());
            }
            self.buf.drain(..end);
        }
        out
    }

    fn close(&self) {
        let path = format!("/SensorManager/{}", self.name);
        let _ = gdbus(&path, &format!("{}.stop", self.iface), &[&self.id.to_string()]);
        let _ = gdbus("/SensorManager", "local.SensorManager.releaseSensor", &[self.name, &self.id.to_string(), "0"]);
    }
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

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

/// Mahony's filter: the orientation (w, x, y, z), the gyroscope's error
/// integrated.
struct Fusion {
    q: [f64; 4],
    integral: [f64; 3],
    started: bool,
}

impl Fusion {
    fn new() -> Fusion {
        Fusion { q: [1.0, 0.0, 0.0, 0.0], integral: [0.0; 3], started: false }
    }

    /// Started level with which way is down (yaw 0).
    fn start(&mut self, a: [f64; 3]) {
        let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-9);
        let (ax, ay, az) = (a[0] / n, a[1] / n, a[2] / n);
        let roll = ay.atan2(az);
        let pitch = (-ax).atan2((ay * ay + az * az).sqrt());
        let (cr, sr, cp, sp) = ((roll / 2.0).cos(), (roll / 2.0).sin(), (pitch / 2.0).cos(), (pitch / 2.0).sin());
        self.q = [cr * cp, sr * cp, cr * sp, -sr * sp];
        self.started = true;
    }

    /// A step of `dt` s turning at `w` (rad/s), down being `a` (any unit).
    fn step(&mut self, w: [f64; 3], a: [f64; 3], dt: f64) {
        let [q0, q1, q2, q3] = self.q;
        let mut w = w;
        let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        // Pulled toward down only when nothing else accelerates it much.
        if n > 1e-6 {
            let (ax, ay, az) = (a[0] / n, a[1] / n, a[2] / n);
            let vx = 2.0 * (q1 * q3 - q0 * q2);
            let vy = 2.0 * (q0 * q1 + q2 * q3);
            let vz = q0 * q0 - q1 * q1 - q2 * q2 + q3 * q3;
            let e = [ay * vz - az * vy, az * vx - ax * vz, ax * vy - ay * vx];
            let trust = (1.0 - (n - 1.0).abs() * 4.0).clamp(0.0, 1.0);
            let (kp, ki) = (2.0 * trust, 0.05 * trust);
            for i in 0..3 {
                self.integral[i] += ki * e[i] * dt;
                w[i] += kp * e[i] + self.integral[i];
            }
        }
        let h = 0.5 * dt;
        let dq = [
            -q1 * w[0] - q2 * w[1] - q3 * w[2],
            q0 * w[0] + q2 * w[2] - q3 * w[1],
            q0 * w[1] - q1 * w[2] + q3 * w[0],
            q0 * w[2] + q1 * w[1] - q2 * w[0],
        ];
        let mut q = [q0 + dq[0] * h, q1 + dq[1] * h, q2 + dq[2] * h, q3 + dq[3] * h];
        let l = q.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-12);
        for v in &mut q {
            *v /= l;
        }
        self.q = q;
    }
}

fn main() {
    let awake = std::env::args().any(|a| a == "--awake");
    let lock = format!("duo-motion-{}", std::process::id());
    if awake {
        let _ = std::fs::write("/sys/power/wake_lock", &lock);
    }
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut say = |line: String| -> bool { out.write_all(line.as_bytes()).and_then(|_| out.write_all(b"\n")).and_then(|_| out.flush()).is_ok() };

    // Readings: a timestamp (u64 us), x, y, z (f32), padding; the hinge's a
    // timestamp and a u32.
    let mut accel = Sensor::open("accelerometersensor", "local.AccelerometerSensor", 24, Some(10));
    let mut gyro = Sensor::open("gyroscopesensor", "local.GyroscopeSensor", 24, Some(10));
    let mut hinge = Sensor::open("hingesensor", "local.HingeSensor", 16, None);
    // The magnetometer: a timestamp, x, y, z calibrated (nT), the raw three,
    // a level, padding.
    let mut mag = Sensor::open("magnetometersensor", "local.MagnetometerSensor", 40, Some(50));
    let mut lid = lid_device();
    if accel.is_none() {
        eprintln!("duo-motion: no accelerometer");
    }
    // Where things are at the start.
    if let Ok(o) = Command::new("busctl").args(["get-property", "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager", "LidClosed"]).output() {
        let shut = String::from_utf8_lossy(&o.stdout).contains("true");
        say(format!("l {}", shut as u8));
    }
    if let Some(reply) = gdbus("/SensorManager/hingesensor", "org.freedesktop.DBus.Properties.Get", &["local.HingeSensor", "hinge"]) {
        if let Some(deg) = reply.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).last().and_then(|s| s.parse::<u32>().ok()) {
            say(format!("h {deg}"));
        }
    }

    let mut fusion = Fusion::new();
    let mut rate = [0.0f64; 3];
    let mut down = [0.0f64; 3];
    let mut last_t: Option<u64> = None;
    let mut sent_q = [0.0f64; 4];
    let mut sent_q_at = Instant::now() - Duration::from_secs(1);
    let mut sent_g = [9.0f64; 3];
    let mut sent_g_at = Instant::now() - Duration::from_secs(1);
    let mut alive_at = Instant::now();
    let mut field = [0.0f64; 3];
    let mut north = (0.0f64, 0.0f64);
    let mut north_at = Instant::now();
    let mut still_since: Option<Instant> = None;
    let mut looked_at = Instant::now() - Duration::from_secs(10);
    let mut reopen_at: Option<Instant> = None;
    // The gyroscope's offset, learnt while still; since when down has held.
    let mut bias = [0.0f64; 3];
    let mut steady_since: Option<Instant> = None;
    let mut alive = true;
    while alive {
        let mut fds: Vec<PollFd> = Vec::new();
        for s in [&accel, &gyro, &hinge, &mag].into_iter().flatten() {
            fds.push(PollFd { fd: s.sock.as_raw_fd(), events: POLLIN, revents: 0 });
        }
        let lid_at = fds.len();
        if let Some(f) = &lid {
            fds.push(PollFd { fd: f.as_raw_fd(), events: POLLIN, revents: 0 });
        }
        unsafe { poll(fds.as_mut_ptr(), fds.len() as u64, 200) };
        // The lid's device blocks: read only when it has something.
        let lid_ready = fds.get(lid_at).is_some_and(|f| f.revents & POLLIN != 0);

        if let Some(g) = gyro.as_mut() {
            for r in g.take() {
                // milli-degrees a second
                let raw = [0, 1, 2].map(|i| (f32_at(&r, 8 + 4 * i) as f64 / 1000.0).to_radians());
                // Still (nothing turning to speak of, down steady for half a
                // second): what the gyroscope says then is its own offset -
                // learnt, and taken off (left in, the phone lying on the
                // table turned slowly, up to 10 degrees a minute).
                let spin = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]).sqrt();
                let still = spin < 0.06 && steady_since.is_some_and(|t: Instant| t.elapsed() >= Duration::from_millis(500));
                if still {
                    bias = [0, 1, 2].map(|i| bias[i] + (raw[i] - bias[i]) * 0.05);
                }
                rate = [0, 1, 2].map(|i| raw[i] - bias[i]);
                // Lying still, what is left is noise: no turn at all.
                if still && (rate[0] * rate[0] + rate[1] * rate[1] + rate[2] * rate[2]).sqrt() < 0.006 {
                    rate = [0.0; 3];
                }
            }
        }
        if let Some(a) = accel.as_mut() {
            for r in a.take() {
                let t = u64_at(&r, 0);
                // milli-g
                let raw = [0, 1, 2].map(|i| f32_at(&r, 8 + 4 * i) as f64 / 1000.0);
                // Down steady: the reading near the smoothed one, about 1 g.
                let off = (0..3).map(|i| (raw[i] - down[i]).abs()).fold(0.0, f64::max);
                let g = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]).sqrt();
                if off < 0.02 && (g - 1.0).abs() < 0.05 {
                    steady_since.get_or_insert_with(Instant::now);
                } else {
                    steady_since = None;
                }
                down = [0, 1, 2].map(|i| down[i] + (raw[i] - down[i]) * 0.15);
                if !fusion.started {
                    fusion.start(raw);
                    last_t = Some(t);
                    continue;
                }
                let dt = last_t.map_or(0.01, |p| (t.saturating_sub(p)) as f64 / 1e6).clamp(0.0, 0.1);
                last_t = Some(t);
                fusion.step(rate, raw, dt);
            }
        }
        if let Some(m) = mag.as_mut() {
            for r in m.take() {
                let v = [0, 1, 2].map(|i| i32::from_le_bytes(r[8 + 4 * i..12 + 4 * i].try_into().unwrap()) as f64);
                field = [0, 1, 2].map(|i| field[i] + (v[i] - field[i]) * 0.3);
            }
        }
        // The phone's axes in the world: its x, y (to its top), z (out of its
        // screen) - the rotation's columns.
        let axes = |q: [f64; 4]| {
            let [w, x, y, z] = q;
            [
                [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z), 2.0 * (x * z - w * y)],
                [2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)],
                [2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 1.0 - 2.0 * (x * x + y * y)],
            ]
        };
        if fusion.started {
            let [ax, ay, az] = axes(fusion.q);
            // The compass: the field in the world, its bearing about z.
            if field.iter().any(|v| *v != 0.0) {
                let fw = [0, 1, 2].map(|i| ax[i] * field[0] + ay[i] * field[1] + az[i] * field[2]);
                let b = fw[1].atan2(fw[0]);
                north = (north.0 + (b.cos() - north.0) * 0.1, north.1 + (b.sin() - north.1) * 0.1);
                if north_at.elapsed() >= Duration::from_millis(500) {
                    alive &= say(format!("c {:.4}", north.1.atan2(north.0)));
                    north_at = Instant::now();
                }
            }
            // Looked at: still (turning under 0.4 rad/s) for 0.4 s, its top
            // up 30 to 82 degrees, its screen facing to the side.
            let turning = (rate[0] * rate[0] + rate[1] * rate[1] + rate[2] * rate[2]).sqrt();
            let side = (az[0] * az[0] + az[1] * az[1]).sqrt();
            let held_up = (0.5..0.99).contains(&ay[2]) && side > 0.4 && az[2] > -0.3;
            if turning < 0.4 && held_up {
                let since = *still_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_millis(400) && looked_at.elapsed() >= Duration::from_secs(2) {
                    alive &= say(format!("look {:.4}", az[1].atan2(az[0])));
                    looked_at = Instant::now();
                }
            } else {
                still_since = None;
            }
        }
        if let Some(h) = hinge.as_mut() {
            for r in h.take() {
                let deg = u32::from_le_bytes(r[8..12].try_into().unwrap());
                alive &= say(format!("h {deg}"));
            }
        }
        if let Some(f) = lid.as_mut().filter(|_| lid_ready) {
            let mut ev = [0u8; 24 * 16];
            if let Ok(n) = f.read(&mut ev) {
                for e in ev[..n].chunks_exact(24) {
                    let (kind, code, value) = (u16::from_le_bytes([e[16], e[17]]), u16::from_le_bytes([e[18], e[19]]), i32::from_le_bytes(e[20..24].try_into().unwrap()));
                    if kind == 5 && code == 0 {
                        alive &= say(format!("l {}", (value != 0) as u8));
                    }
                }
            }
        }
        // Closed sockets let go of, and their sensors asked for again a
        // little later (sensorfw drops a session now and then).
        for s in [&mut accel, &mut gyro, &mut hinge, &mut mag] {
            if s.as_ref().is_some_and(|x| x.dead) {
                eprintln!("duo-motion: {} gone", s.as_ref().unwrap().name);
                *s = None;
                reopen_at = Some(Instant::now() + Duration::from_secs(3));
            }
        }
        if reopen_at.is_some_and(|t| Instant::now() >= t) {
            reopen_at = None;
            if accel.is_none() {
                accel = Sensor::open("accelerometersensor", "local.AccelerometerSensor", 24, Some(10));
            }
            if gyro.is_none() {
                gyro = Sensor::open("gyroscopesensor", "local.GyroscopeSensor", 24, Some(10));
            }
            if hinge.is_none() {
                hinge = Sensor::open("hingesensor", "local.HingeSensor", 16, None);
            }
            if mag.is_none() {
                mag = Sensor::open("magnetometersensor", "local.MagnetometerSensor", 40, Some(50));
            }
        }
        let now = Instant::now();
        if fusion.started && now.duration_since(sent_q_at) >= Duration::from_millis(16) {
            let q = fusion.q;
            let moved = q.iter().zip(sent_q).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max) > 2e-4;
            if moved {
                alive &= say(format!("q {:.5} {:.5} {:.5} {:.5}", q[0], q[1], q[2], q[3]));
                sent_q = q;
                sent_q_at = now;
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
        }
    }
    for s in [&accel, &gyro, &hinge, &mag].into_iter().flatten() {
        s.close();
    }
    if awake {
        let _ = std::fs::write("/sys/power/wake_unlock", &lock);
    }
}
