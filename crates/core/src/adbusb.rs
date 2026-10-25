//! adb spoken by item/grid itself over USB (nusb), to item-recovery - in
//! place of platform-tools' adb and its server for item/grid's own
//! recovery. Its daemon wants no AUTH (crates/recovery/src/adb.rs), so the
//! transport is CNXN, then OPEN, OKAY, WRTE and CLSE, one write outstanding
//! per stream each way. Stock Android's adbd (RSA keys, the user's tap) and
//! TWRP stay with platform-tools.
//!
//! item-recovery is known by its USB product string, without a connection;
//! adb's server, if it holds the interface (item/grid asks it about other
//! phones), is stopped so the interface can be had.

use std::time::{Duration, Instant};

use nusb::transfer::{Bulk, In, Out};
use nusb::MaybeFuture;

const A_CNXN: u32 = 0x4e58_4e43;
const A_OPEN: u32 = 0x4e45_504f;
const A_OKAY: u32 = 0x5941_4b4f;
const A_CLSE: u32 = 0x4553_4c43;
const A_WRTE: u32 = 0x4554_5257;
const A_AUTH: u32 = 0x4854_5541;
const VERSION: u32 = 0x0100_0001;
const MAX_PAYLOAD: usize = 256 * 1024;
/// adb's interface: vendor class, subclass 0x42, protocol 1.
const CLASS: (u8, u8, u8) = (0xff, 0x42, 0x01);
/// item-recovery's gadget names itself so (crates/recovery/src/init.rs).
const PRODUCT: &str = "item recovery";

/// A base for a connection's stream ids, new each connection in this
/// process (with the time, apart across processes).
pub(crate) fn next_base() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static BASE: AtomicU32 = AtomicU32::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    BASE.fetch_add(0x1000, Ordering::Relaxed).wrapping_add(t)
}

/// Whether item/grid's own adb is to be left aside (platform-tools' used).
pub fn platform() -> bool {
    std::env::var("ITEMGRID_ADB").is_ok_and(|v| v == "platform")
}

fn is_ours(d: &nusb::DeviceInfo) -> bool {
    d.product_string().is_some_and(|p| p.contains(PRODUCT)) && d.interfaces().any(|i| (i.class(), i.subclass(), i.protocol()) == CLASS)
}

/// The serials of the phones in item-recovery on USB.
pub fn devices() -> Vec<String> {
    if platform() {
        return Vec::new();
    }
    let Ok(list) = nusb::list_devices().wait() else { return Vec::new() };
    list.filter(is_ours).map(|d| d.serial_number().unwrap_or("").to_owned()).collect()
}

/// Whether `serial` is in item-recovery on USB.
pub fn present(serial: &str) -> bool {
    devices().iter().any(|s| s == serial)
}

struct Message {
    cmd: u32,
    arg0: u32,
    arg1: u32,
    data: Vec<u8>,
}

/// Why a connection was not had: an error, or a daemon that did not
/// answer (its device, to be reset).
enum Stuck {
    Other(String),
    Silent(nusb::Device),
}

enum Opened {
    Err(String),
    Silent(nusb::Device),
}

impl From<String> for Opened {
    fn from(e: String) -> Opened {
        Opened::Err(e)
    }
}

impl From<&str> for Opened {
    fn from(e: &str) -> Opened {
        Opened::Err(e.to_owned())
    }
}

/// A connection to a phone's item-recovery.
pub struct Conn {
    _iface: nusb::Interface,
    ep_in: nusb::Endpoint<Bulk, In>,
    ep_out: nusb::Endpoint<Bulk, Out>,
    max_packet: usize,
    /// The device's largest payload.
    max: usize,
    /// Bytes read and not yet a whole message.
    pending: Vec<u8>,
    next_id: u32,
}

fn le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

impl Conn {
    /// Connected to `serial`'s item-recovery. A daemon that does not answer
    /// (left mid-stream by a host that went: its write to USB waits for
    /// ever) is reset - the USB device reset, f_fs ends the write, the
    /// daemon exits and init starts it anew - and asked once more.
    pub fn open(serial: &str) -> Result<Conn, String> {
        match Conn::open_once(serial, Duration::from_secs(3)) {
            Ok(c) => Ok(c),
            Err(Stuck::Other(e)) => Err(e),
            Err(Stuck::Silent(device)) => {
                let _ = device.reset().wait();
                drop(device);
                let until = Instant::now() + Duration::from_secs(20);
                loop {
                    std::thread::sleep(Duration::from_millis(500));
                    if present(serial) {
                        match Conn::open_once(serial, Duration::from_secs(10)) {
                            Ok(c) => return Ok(c),
                            Err(Stuck::Other(e)) if Instant::now() < until && e.contains("busy") => continue,
                            Err(Stuck::Other(e)) => return Err(e),
                            Err(Stuck::Silent(_)) => return Err("item-recovery's adb does not answer, even after a USB reset".into()),
                        }
                    }
                    if Instant::now() > until {
                        return Err("item-recovery did not come back after a USB reset".into());
                    }
                }
            }
        }
    }

    fn open_once(serial: &str, answer_within: Duration) -> Result<Conn, Stuck> {
        let o = |e: String| Stuck::Other(e);
        let r = Conn::open_inner(serial, answer_within);
        match r {
            Ok(c) => Ok(c),
            Err(Opened::Silent(d)) => Err(Stuck::Silent(d)),
            Err(Opened::Err(e)) => Err(o(e)),
        }
    }

    fn open_inner(serial: &str, answer_within: Duration) -> Result<Conn, Opened> {
        let find = || -> Result<nusb::DeviceInfo, String> {
            nusb::list_devices().wait().map_err(|e| format!("USB: {e}"))?.find(|d| is_ours(d) && d.serial_number() == Some(serial)).ok_or_else(|| format!("{serial}: not in item-recovery on USB"))
        };
        let info = find()?;
        let number = info.interfaces().find(|i| (i.class(), i.subclass(), i.protocol()) == CLASS).map(|i| i.interface_number()).ok_or("no adb interface")?;
        let device = info.open().wait().map_err(|e| format!("USB: {e}"))?;
        let iface = match device.detach_and_claim_interface(number).wait() {
            Ok(i) => i,
            Err(_) => {
                // adb's server holds it: stopped, and the interface asked
                // for again.
                let _ = std::process::Command::new(crate::programs::adb()).arg("kill-server").output();
                std::thread::sleep(Duration::from_millis(500));
                device.detach_and_claim_interface(number).wait().map_err(|e| format!("USB: {e} (adb's server still holding it?)"))?
            }
        };
        let config = device.active_configuration().map_err(|e| format!("USB: {e}"))?;
        let (mut a_in, mut a_out, mut max_packet) = (None, None, 512);
        for alt in config.interface_alt_settings().filter(|a| a.interface_number() == number) {
            for ep in alt.endpoints() {
                if ep.transfer_type() != nusb::descriptors::TransferType::Bulk {
                    continue;
                }
                max_packet = ep.max_packet_size();
                match ep.direction() {
                    nusb::transfer::Direction::In => a_in = Some(ep.address()),
                    nusb::transfer::Direction::Out => a_out = Some(ep.address()),
                }
            }
        }
        let (Some(a_in), Some(a_out)) = (a_in, a_out) else { return Err("the adb interface has no bulk endpoints".into()) };
        let ep_in = iface.endpoint::<Bulk, In>(a_in).map_err(|e| format!("USB: {e}"))?;
        let ep_out = iface.endpoint::<Bulk, Out>(a_out).map_err(|e| format!("USB: {e}"))?;
        // Stream ids not used by a connection before (a closed one's late
        // messages must not land in a new stream of the same number).
        let first_id = (std::process::id().wrapping_mul(7919) ^ (Instant::now().elapsed().subsec_nanos() as u32)).wrapping_add(crate::adbusb::next_base()) | 1;
        let mut c = Conn { _iface: iface, ep_in, ep_out, max_packet, max: MAX_PAYLOAD, pending: Vec::new(), next_id: first_id };
        c.send(A_CNXN, VERSION, MAX_PAYLOAD as u32, b"host::\0")?;
        let until = Instant::now() + answer_within;
        loop {
            let m = match c.recv(until.saturating_duration_since(Instant::now())) {
                Ok(m) => m,
                Err(e) if e.contains("in time") || e.contains("USB:") => {
                    drop(c);
                    return Err(Opened::Silent(device));
                }
                Err(e) => return Err(Opened::Err(e)),
            };
            match m.cmd {
                A_CNXN => {
                    if !String::from_utf8_lossy(&m.data).starts_with("recovery::") {
                        return Err("the phone's adb is not item-recovery's".into());
                    }
                    c.max = (m.arg1 as usize).clamp(4096, MAX_PAYLOAD);
                    return Ok(c);
                }
                A_AUTH => return Err("the phone's adb asks for a key: not item-recovery".into()),
                _ => {}
            }
        }
    }

    fn send(&mut self, cmd: u32, arg0: u32, arg1: u32, data: &[u8]) -> Result<(), String> {
        let sum: u32 = data.iter().map(|b| *b as u32).fold(0u32, u32::wrapping_add);
        let mut h = Vec::with_capacity(24);
        for v in [cmd, arg0, arg1, data.len() as u32, sum, cmd ^ 0xffff_ffff] {
            h.extend_from_slice(&v.to_le_bytes());
        }
        self.write(&h)?;
        if !data.is_empty() {
            self.write(data)?;
        }
        Ok(())
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        let c = self.ep_out.transfer_blocking(bytes.to_vec().into(), Duration::from_secs(30));
        c.status.map_err(|e| format!("USB: {e}"))?;
        if c.actual_len != bytes.len() {
            return Err(format!("USB: {} of {} bytes sent", c.actual_len, bytes.len()));
        }
        // A transfer a whole number of packets long ends with an empty one:
        // the recovery reads more than a payload at a time and would wait
        // for the rest otherwise (a 256 KB payload hung it, 2026-10-26).
        if !bytes.is_empty() && bytes.len() % self.max_packet == 0 {
            let z = self.ep_out.transfer_blocking(Vec::<u8>::new().into(), Duration::from_secs(5));
            z.status.map_err(|e| format!("USB: {e}"))?;
        }
        Ok(())
    }

    /// The next whole message, within `timeout`: its header read alone (24
    /// bytes), then exactly its payload - as adb's own host reads them, so
    /// no read waits for more than the daemon sends (it sends no empty
    /// packet after a payload of whole packets).
    fn recv(&mut self, timeout: Duration) -> Result<Message, String> {
        let until = Instant::now() + timeout;
        let h = self.read_exact_len(24, until)?;
        if le(&h, 20) != le(&h, 0) ^ 0xffff_ffff {
            return Err("a broken message from the phone".into());
        }
        let len = le(&h, 12) as usize;
        if len > 1 << 20 {
            return Err("a broken message from the phone".into());
        }
        let data = if len > 0 { self.read_exact_len(len, until)? } else { Vec::new() };
        Ok(Message { cmd: le(&h, 0), arg0: le(&h, 4), arg1: le(&h, 8), data })
    }

    /// Exactly `n` bytes from the IN endpoint (what was read beyond, kept).
    fn read_exact_len(&mut self, n: usize, until: Instant) -> Result<Vec<u8>, String> {
        while self.pending.len() < n {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("the phone's adb did not answer in time".into());
            }
            // (nusb takes whole packets: the length rounded up - the
            // daemon's transfer is exactly what is wanted, so it ends there,
            // short or whole.)
            let want = (n - self.pending.len()).div_ceil(self.max_packet) * self.max_packet;
            let c = self.ep_in.transfer_blocking(self.ep_in.allocate(want), left);
            match c.status {
                Ok(()) => self.pending.extend_from_slice(&c.buffer[..c.actual_len]),
                Err(nusb::transfer::TransferError::Cancelled) => return Err("the phone's adb did not answer in time".into()),
                Err(e) => return Err(format!("USB: {e}")),
            }
        }
        Ok(self.pending.drain(..n).collect())
    }

    /// A stream opened to `service`: its local and remote ids.
    fn open_stream(&mut self, service: &str) -> Result<(u32, u32), String> {
        let local = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let mut s = service.as_bytes().to_vec();
        s.push(0);
        self.send(A_OPEN, local, 0, &s)?;
        loop {
            let m = self.recv(Duration::from_secs(15))?;
            match m.cmd {
                A_OKAY if m.arg1 == local => return Ok((local, m.arg0)),
                A_CLSE if m.arg1 == local => return Err(format!("{service}: refused")),
                _ => {}
            }
        }
    }

    /// A stream's whole output, until the phone closes it (each payload
    /// acknowledged); `sink` takes each.
    fn read_stream(&mut self, local: u32, remote: u32, timeout: Duration, sink: &mut dyn FnMut(&[u8])) -> Result<(), String> {
        let until = Instant::now() + timeout;
        loop {
            let m = self.recv(until.saturating_duration_since(Instant::now()))?;
            if m.arg1 != local {
                continue;
            }
            match m.cmd {
                A_WRTE => {
                    sink(&m.data);
                    self.send(A_OKAY, local, remote, &[])?;
                }
                A_CLSE => return Ok(()),
                _ => {}
            }
        }
    }

    /// Data into a stream, a payload at a time, each acknowledged.
    fn write_stream(&mut self, local: u32, remote: u32, data: &[u8]) -> Result<(), String> {
        for chunk in data.chunks(self.max) {
            self.send(A_WRTE, local, remote, chunk)?;
            loop {
                let m = self.recv(Duration::from_secs(60))?;
                if m.arg1 != local {
                    continue;
                }
                match m.cmd {
                    A_OKAY => break,
                    A_CLSE => return Err("the phone closed the stream while it was being written".into()),
                    // A write from the phone meanwhile (a command's output):
                    // acknowledged, left.
                    A_WRTE => self.send(A_OKAY, local, remote, &[])?,
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// A command in the recovery's shell (v2): its stdout, stderr and exit.
    pub fn shell(&mut self, cmd: &str, timeout: Duration) -> Result<(Vec<u8>, Vec<u8>, i32), String> {
        let (local, remote) = self.open_stream(&format!("shell,v2,raw:{cmd}"))?;
        let (mut raw, mut out, mut err, mut code) = (Vec::new(), Vec::new(), Vec::new(), -1);
        self.read_stream(local, remote, timeout, &mut |d| raw.extend_from_slice(d))?;
        // shell v2's packets: an id, a length, the bytes.
        let mut at = 0;
        while at + 5 <= raw.len() {
            let (id, len) = (raw[at], le(&raw, at + 1) as usize);
            let body = raw.get(at + 5..at + 5 + len).unwrap_or(&[]);
            match id {
                1 => out.extend_from_slice(body),
                2 => err.extend_from_slice(body),
                3 => code = body.first().copied().map(i32::from).unwrap_or(-1),
                _ => {}
            }
            at += 5 + len;
        }
        Ok((out, err, code))
    }

    /// Bytes from `data` streamed into a command (exec, raw); the bytes
    /// sent.
    pub fn exec_in(&mut self, cmd: &str, data: &mut dyn std::io::Read) -> Result<u64, String> {
        let (local, remote) = self.open_stream(&format!("exec:{cmd}"))?;
        let mut buf = vec![0u8; self.max];
        let mut n = 0u64;
        loop {
            let got = data.read(&mut buf).map_err(|e| format!("reading: {e}"))?;
            if got == 0 {
                break;
            }
            self.write_stream(local, remote, &buf[..got])?;
            n += got as u64;
        }
        self.send(A_CLSE, local, remote, &[])?;
        Ok(n)
    }

    /// A command's output streamed out (exec, raw) into `sink`.
    pub fn exec_out(&mut self, cmd: &str, timeout: Duration, sink: &mut dyn FnMut(&[u8])) -> Result<(), String> {
        let (local, remote) = self.open_stream(&format!("exec:{cmd}"))?;
        self.read_stream(local, remote, timeout, sink)
    }

    /// Bytes written as a file at `to` (sync: SEND, DATA, DONE).
    pub fn push(&mut self, data: &[u8], to: &str, mode: u32) -> Result<(), String> {
        let (local, remote) = self.open_stream("sync:")?;
        let mut req = Vec::new();
        let target = format!("{to},{mode}");
        req.extend_from_slice(b"SEND");
        req.extend_from_slice(&(target.len() as u32).to_le_bytes());
        req.extend_from_slice(target.as_bytes());
        for piece in data.chunks(64 * 1024) {
            req.extend_from_slice(b"DATA");
            req.extend_from_slice(&(piece.len() as u32).to_le_bytes());
            req.extend_from_slice(piece);
        }
        req.extend_from_slice(b"DONE");
        req.extend_from_slice(&(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as u32).to_le_bytes());
        self.write_stream(local, remote, &req)?;
        // The answer: OKAY or FAIL with why.
        let mut answer = Vec::new();
        let until = Instant::now() + Duration::from_secs(30);
        while answer.len() < 8 {
            let m = self.recv(until.saturating_duration_since(Instant::now()))?;
            if m.arg1 != local {
                continue;
            }
            match m.cmd {
                A_WRTE => {
                    answer.extend_from_slice(&m.data);
                    self.send(A_OKAY, local, remote, &[])?;
                }
                A_CLSE => break,
                _ => {}
            }
        }
        let _ = self.send(A_CLSE, local, remote, &[]);
        match answer.get(..4) {
            Some(b"OKAY") => Ok(()),
            Some(b"FAIL") => Err(format!("push {to}: {}", String::from_utf8_lossy(answer.get(8..).unwrap_or(&[])))),
            _ => Err(format!("push {to}: no answer")),
        }
    }

    /// The phone restarted (`to`: bootloader, recovery, or "" for itself).
    pub fn reboot(&mut self, to: &str) -> Result<(), String> {
        let (local, remote) = self.open_stream(&format!("reboot:{to}"))?;
        let _ = self.read_stream(local, remote, Duration::from_secs(5), &mut |_| {});
        Ok(())
    }
}

/// adb's command line for the commands item/grid gives a recovery
/// (`devices`, `-s SERIAL shell CMD`, `-s SERIAL reboot [bootloader]`),
/// when item-recovery is on USB; its output in adb's words. None: not ours
/// to answer (platform-tools' then) - and while item-recovery is there,
/// adb's server is never started (it would take the interface).
pub fn run(args: &[&str], limit: Duration) -> Option<Result<String, String>> {
    if platform() {
        return None;
    }
    match args {
        ["devices"] => {
            let ours = devices();
            (!ours.is_empty()).then(|| Ok(format!("List of devices attached\n{}\n", ours.iter().map(|s| format!("{s}\trecovery")).collect::<Vec<_>>().join("\n"))))
        }
        ["-s", serial, rest @ ..] if present(serial) => Some((|| {
            let mut c = Conn::open(serial)?;
            match rest {
                ["shell", cmd] => {
                    let (out, err, _) = c.shell(cmd, limit)?;
                    Ok(format!("{}{}", String::from_utf8_lossy(&out), String::from_utf8_lossy(&err)))
                }
                ["reboot"] => c.reboot("").map(|_| String::new()),
                ["reboot", to] => c.reboot(to).map(|_| String::new()),
                other => Err(format!("adb {}: not a command item/grid speaks to its recovery", other.join(" "))),
            }
        })()),
        _ => None,
    }
}
