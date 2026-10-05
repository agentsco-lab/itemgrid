//! sensorfw's sensors, read straight from its data socket: a session asked
//! for over D-Bus (gdbus, once), then frames of readings on /run/sensord.sock
//! - a count, then as many readings.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::time::Duration;

const SERVICE: &str = "com.nokia.SensorService";
const SOCKET: &str = "/run/sensord.sock";

/// A call to sensorfw, given up after 3 s (stuck, it answered nothing, and
/// gdbus waited 25 s a call).
pub fn gdbus(path: &str, method: &str, args: &[&str]) -> Option<String> {
    let out = Command::new("gdbus").args(["call", "--system", "--timeout", "3", "--dest", SERVICE, "--object-path", path, "--method", method]).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The sensors duo-motion reads: sensorfw's name, its interface, the bytes
/// a reading takes, the interval asked for (ms).
#[derive(Clone, Copy, Debug)]
pub struct Kind {
    pub name: &'static str,
    pub iface: &'static str,
    pub size: usize,
    pub interval_ms: Option<u32>,
}

/// A timestamp (u64 us), x, y, z (f32), padding: milli-g.
pub const ACCEL: Kind = Kind { name: "accelerometersensor", iface: "local.AccelerometerSensor", size: 24, interval_ms: Some(10) };
/// The same layout: milli-degrees a second.
pub const GYRO: Kind = Kind { name: "gyroscopesensor", iface: "local.GyroscopeSensor", size: 24, interval_ms: Some(10) };
/// A timestamp, x, y, z calibrated (i32, nT), the raw three, a level,
/// padding.
pub const MAG: Kind = Kind { name: "magnetometersensor", iface: "local.MagnetometerSensor", size: 40, interval_ms: Some(20) };
/// A timestamp, the degrees (u32), padding.
pub const HINGE: Kind = Kind { name: "hingesensor", iface: "local.HingeSensor", size: 16, interval_ms: None };

/// A session and its socket.
pub struct Sensor {
    pub kind: Kind,
    id: i32,
    pub sock: UnixStream,
    buf: Vec<u8>,
    /// Its socket closed (sensorfw let the session go): to be dropped and
    /// asked for again - polled, a closed socket is always readable.
    pub dead: bool,
}

impl Sensor {
    pub fn open(kind: Kind) -> Option<Sensor> {
        gdbus("/SensorManager", "local.SensorManager.loadPlugin", &[kind.name])?;
        let reply = gdbus("/SensorManager", "local.SensorManager.requestSensor", &[kind.name, "0"])?;
        let id: i32 = reply.split(|c: char| !c.is_ascii_digit() && c != '-').find(|s| !s.is_empty())?.parse().ok()?;
        let session = Self::connect(kind, id);
        // A session got and not started let go: left, sensorfw kept writing
        // to them, and with a session asked for again every few seconds it
        // stuck (two of its threads blocked writing to its own pipes).
        if session.is_none() {
            release(kind, id);
        }
        session
    }

    fn connect(kind: Kind, id: i32) -> Option<Sensor> {
        // The greeting: a byte from sensorfw, the session back.
        let mut sock = UnixStream::connect(SOCKET).ok()?;
        sock.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
        let mut one = [0u8; 1];
        sock.read_exact(&mut one).ok()?;
        sock.write_all(&id.to_le_bytes()).ok()?;
        sock.set_nonblocking(true).ok()?;
        let path = format!("/SensorManager/{}", kind.name);
        if let Some(ms) = kind.interval_ms {
            let _ = gdbus(&path, &format!("{}.setInterval", kind.iface), &[&id.to_string(), &ms.to_string()]);
        }
        gdbus(&path, &format!("{}.start", kind.iface), &[&id.to_string()])?;
        Some(Sensor { kind, id, sock, buf: Vec::new(), dead: false })
    }

    /// The readings come since, each its bytes.
    pub fn take(&mut self) -> Vec<Vec<u8>> {
        let mut chunk = [0u8; 65536];
        loop {
            match self.sock.read(&mut chunk) {
                Ok(0) => {
                    self.dead = true;
                    break;
                }
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => break,
                Err(_) => {
                    self.dead = true;
                    break;
                }
            }
        }
        let size = self.kind.size;
        let mut out = Vec::new();
        while self.buf.len() >= 4 {
            let count = u32::from_le_bytes(self.buf[..4].try_into().unwrap()) as usize;
            if count > 1000 {
                self.buf.clear();
                break;
            }
            let end = 4 + count * size;
            if self.buf.len() < end {
                break;
            }
            for i in 0..count {
                out.push(self.buf[4 + i * size..4 + (i + 1) * size].to_vec());
            }
            self.buf.drain(..end);
        }
        out
    }

    pub fn close(&self) {
        release(self.kind, self.id);
    }
}

fn release(kind: Kind, id: i32) {
    let path = format!("/SensorManager/{}", kind.name);
    let _ = gdbus(&path, &format!("{}.stop", kind.iface), &[&id.to_string()]);
    let _ = gdbus("/SensorManager", "local.SensorManager.releaseSensor", &[kind.name, &id.to_string(), "0"]);
}

pub fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
pub fn i32_at(b: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
pub fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
pub fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
