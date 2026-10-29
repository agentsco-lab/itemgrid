//! An adb daemon of the recovery's own, over FunctionFS: what item/grid
//! asks of a recovery and nothing more.
//!
//! - the transport: CNXN without AUTH (the phone is in the hands of whoever
//!   has its cable and its unlocked bootloader), "recovery" as its state;
//!   OPEN, OKAY, WRTE, CLSE, one write outstanding per stream each way;
//! - `shell` (v2: stdout, stderr and the exit code apart; stdin; v1 too),
//!   `exec` (raw: `adb exec-in` streams into a command, `exec-out` out of
//!   it), `sync` (push, pull, stat, list), `reboot`.
//!
//! An endpoint's error ends the process; init starts it again, the gadget
//! bound anew (a cable pulled and put back).

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use crate::init::{FFS, GADGET};
use crate::say;

const A_CNXN: u32 = 0x4e58_4e43;
const A_OPEN: u32 = 0x4e45_504f;
const A_OKAY: u32 = 0x5941_4b4f;
const A_CLSE: u32 = 0x4553_4c43;
const A_WRTE: u32 = 0x4554_5257;
const VERSION: u32 = 0x0100_0001;
/// The largest payload offered (the host's own may be smaller).
const MAX_PAYLOAD: usize = 256 * 1024;
/// Bytes queued for a stream's service that the reader acks on arrival;
/// past them the service acks what it takes (the reader never waits: a
/// service waiting for an OKAY it routes would never get it).
const QUEUED_ACKED: usize = 64 << 20;
/// A read from the OUT endpoint: a whole payload and its header (f_fs
/// would keep what a larger transfer has beyond a read for the next one,
/// copying; at 64 KB a gigabyte took 69 s).
const READ: usize = MAX_PAYLOAD + 4096;
const BANNER: &str = "recovery::ro.product.name=surfaceduo;ro.product.model=Surface Duo;ro.product.device=duo;features=shell_v2,cmd";

/// When the write to the host now under way began (ms), 0: none.
static WRITING_SINCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(1).max(1)
}

/// shell v2's packet ids.
const STDIN: u8 = 0;
const STDOUT: u8 = 1;
const STDERR: u8 = 2;
const EXIT: u8 = 3;
const CLOSE_STDIN: u8 = 4;

struct Packet {
    cmd: u32,
    arg0: u32,
    arg1: u32,
    data: Vec<u8>,
}

fn packet(cmd: u32, arg0: u32, arg1: u32, data: Vec<u8>) -> Packet {
    Packet { cmd, arg0, arg1, data }
}

fn header(p: &Packet) -> [u8; 24] {
    let sum: u32 = p.data.iter().map(|&b| b as u32).fold(0u32, u32::wrapping_add);
    let mut h = [0u8; 24];
    for (i, v) in [p.cmd, p.arg0, p.arg1, p.data.len() as u32, sum, p.cmd ^ 0xffff_ffff].iter().enumerate() {
        h[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    h
}

fn le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// The function's descriptors: one interface (adb's class 0xff, 0x42, 1)
/// with a bulk OUT (ep1) and IN (ep2) endpoint, at full, high and super
/// speed.
fn descriptors() -> Vec<u8> {
    let intf = [9u8, 4, 0, 0, 2, 0xff, 0x42, 1, 1];
    let ep = |addr: u8, mps: u16| [7u8, 5, addr, 2, mps as u8, (mps >> 8) as u8, 0];
    let comp = [6u8, 0x30, 0, 0, 0, 0];
    let mut body = Vec::new();
    for mps in [64u16, 512] {
        body.extend_from_slice(&intf);
        body.extend_from_slice(&ep(0x01, mps));
        body.extend_from_slice(&ep(0x82, mps));
    }
    body.extend_from_slice(&intf);
    body.extend_from_slice(&ep(0x01, 1024));
    body.extend_from_slice(&comp);
    body.extend_from_slice(&ep(0x82, 1024));
    body.extend_from_slice(&comp);
    let mut out = Vec::new();
    // FUNCTIONFS_DESCRIPTORS_MAGIC_V2, the length, flags (fs, hs, ss), the
    // counts.
    for v in [3u32, (24 + body.len()) as u32, 1 | 2 | 4, 3, 3, 5] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&body);
    out
}

fn strings() -> Vec<u8> {
    let s = b"ADB Interface\0";
    let mut out = Vec::new();
    for v in [2u32, (16 + 2 + s.len()) as u32, 1, 1] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&0x0409u16.to_le_bytes());
    out.extend_from_slice(s);
    out
}

fn open_retry(path: &str, write: bool) -> File {
    for i in 0.. {
        let f = if write { std::fs::OpenOptions::new().write(true).open(path) } else { File::open(path) };
        match f {
            Ok(f) => return f,
            Err(e) => {
                if i % 50 == 0 {
                    say!("{path}: {e} (waiting)");
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    unreachable!()
}

pub fn run() -> ! {
    let mut ep0 = std::fs::OpenOptions::new().read(true).write(true).open(format!("{FFS}/ep0")).unwrap_or_else(|e| {
        say!("ep0: {e}");
        std::thread::sleep(Duration::from_secs(2));
        std::process::exit(1)
    });
    if let Err(e) = ep0.write_all(&descriptors()).and_then(|_| ep0.write_all(&strings())) {
        say!("descriptors: {e}");
        std::thread::sleep(Duration::from_secs(2));
        std::process::exit(1);
    }
    // Bound to the controller (anew if it was, after a restart).
    let _ = std::fs::write(format!("{GADGET}/UDC"), "\n");
    match crate::init::udc() {
        Some(udc) => match std::fs::write(format!("{GADGET}/UDC"), &udc) {
            Ok(()) => say!("adb on {udc} as {}", crate::init::serial()),
            Err(e) => say!("binding {udc}: {e}"),
        },
        None => say!("no USB device controller"),
    }
    // ep0's events read (and said), as f_fs wants them read.
    std::thread::spawn(move || {
        let mut ev = [0u8; 12];
        while ep0.read_exact(&mut ev).is_ok() {
            let kind = ev[8];
            if kind != 4 {
                say!("usb: {}", ["bind", "unbind", "enable", "disable", "setup", "suspend", "resume"].get(kind as usize).unwrap_or(&"?"));
            }
        }
    });
    let mut from_host = open_retry(&format!("{FFS}/ep1"), false);
    let mut to_host = open_retry(&format!("{FFS}/ep2"), true);
    let (out, out_rx) = channel::<Packet>();
    std::thread::spawn(move || {
        for p in out_rx {
            let h = header(&p);
            WRITING_SINCE.store(now_ms(), Ordering::Relaxed);
            let r = to_host.write_all(&h).and_then(|_| if p.data.is_empty() { Ok(()) } else { to_host.write_all(&p.data) });
            WRITING_SINCE.store(0, Ordering::Relaxed);
            if r.is_err() {
                say!("the host went (writing)");
                std::process::exit(1);
            }
        }
    });
    serve(&mut from_host, out)
}

/// A stream as the reader routes to it: the host's id, its input (taken
/// away when the host closes it), its OKAYs.
struct Stream {
    remote: u32,
    input: Sender<Input>,
    queued: Arc<AtomicUsize>,
    okay: Sender<()>,
}

fn serve(from_host: &mut File, out: Sender<Packet>) -> ! {
    let mut streams: HashMap<u32, Stream> = HashMap::new();
    let mut next = 1u32;
    let mut max = MAX_PAYLOAD;
    let mut pending: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; READ];
    loop {
        let n = match from_host.read(&mut buf) {
            Ok(n) => n,
            // A signal mid-read is not the host going (it was, once, mid
            // install: the daemon left and the host saw a protocol fault).
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                say!("the host went (reading: {e})");
                std::process::exit(1);
            }
        };
        pending.extend_from_slice(&buf[..n]);
        while pending.len() >= 24 {
            let len = le(&pending, 12) as usize;
            if le(&pending, 20) != le(&pending, 0) ^ 0xffff_ffff || len > 1 << 20 {
                say!("a broken message from the host: dropped what was read");
                pending.clear();
                break;
            }
            if pending.len() < 24 + len {
                break;
            }
            let (cmd, arg0, arg1) = (le(&pending, 0), le(&pending, 4), le(&pending, 8));
            let data: Vec<u8> = pending[24..24 + len].to_vec();
            pending.drain(..24 + len);
            match cmd {
                A_CNXN => {
                    // A host connecting while a write to the last one hangs
                    // (it went mid-stream and reads no more): started anew,
                    // the gadget bound again, the write gone.
                    let since = WRITING_SINCE.load(Ordering::Relaxed);
                    if since != 0 && now_ms().saturating_sub(since) > 1000 {
                        say!("a new host while a write to the last one hangs: starting again");
                        std::process::exit(1);
                    }
                    max = (arg1 as usize).clamp(4096, MAX_PAYLOAD);
                    streams.clear();
                    say!("the host connected (payloads of {max})");
                    let _ = std::fs::write(crate::screen::CONNECTED, "connected\n");
                    let _ = out.send(packet(A_CNXN, VERSION, MAX_PAYLOAD as u32, BANNER.as_bytes().to_vec()));
                }
                A_OPEN => {
                    let service = String::from_utf8_lossy(&data).trim_end_matches('\0').to_owned();
                    let local = next;
                    next = next.wrapping_add(1).max(1);
                    let (input, input_rx) = channel();
                    let (okay, okay_rx) = channel();
                    let queued = Arc::new(AtomicUsize::new(0));
                    streams.insert(local, Stream { remote: arg0, input, queued: queued.clone(), okay });
                    let _ = out.send(packet(A_OKAY, local, arg0, Vec::new()));
                    let ctx = Ctx { local, remote: arg0, out: out.clone(), max };
                    let input = Inbound { ctx: ctx.clone(), rx: input_rx, queued };
                    std::thread::spawn(move || open(ctx, &service, input, okay_rx));
                }
                A_WRTE => match streams.get(&arg1) {
                    Some(s) if s.remote == arg0 => {
                        let owed = s.queued.fetch_add(data.len(), Ordering::Relaxed) > QUEUED_ACKED;
                        let _ = s.input.send(Input { data, owed });
                        if !owed {
                            let _ = out.send(packet(A_OKAY, arg1, arg0, Vec::new()));
                        }
                    }
                    _ => {
                        let _ = out.send(packet(A_CLSE, 0, arg0, Vec::new()));
                    }
                },
                A_OKAY => {
                    if let Some(s) = streams.get(&arg1) {
                        let _ = s.okay.send(());
                    }
                }
                A_CLSE => {
                    streams.remove(&arg1);
                }
                _ => {}
            }
        }
    }
}

/// A payload from the host for a service, and whether the service owes
/// the host its OKAY (the queue was long when it came).
struct Input {
    data: Vec<u8>,
    owed: bool,
}

/// A service's side of the host's input.
struct Inbound {
    ctx: Ctx,
    rx: Receiver<Input>,
    queued: Arc<AtomicUsize>,
}

impl Inbound {
    /// The next payload (None: the host closed the stream), acked if owed.
    fn next(&self) -> Option<Vec<u8>> {
        let Input { data, owed } = self.rx.recv().ok()?;
        self.queued.fetch_sub(data.len(), Ordering::Relaxed);
        if owed {
            let _ = self.ctx.out.send(packet(A_OKAY, self.ctx.local, self.ctx.remote, Vec::new()));
        }
        Some(data)
    }
}

/// What a service writes with: its ids, the writer, the payload's limit.
#[derive(Clone)]
struct Ctx {
    local: u32,
    remote: u32,
    out: Sender<Packet>,
    max: usize,
}

impl Ctx {
    /// Data to the host, a payload at a time, each waited for (OKAY);
    /// whether the stream is still open.
    fn write(&self, okay: &Receiver<()>, data: &[u8]) -> bool {
        for chunk in data.chunks(self.max) {
            if self.out.send(packet(A_WRTE, self.local, self.remote, chunk.to_vec())).is_err() || okay.recv().is_err() {
                return false;
            }
        }
        true
    }

    fn close(&self) {
        let _ = self.out.send(packet(A_CLSE, self.local, self.remote, Vec::new()));
    }
}

fn open(ctx: Ctx, service: &str, input: Inbound, okay: Receiver<()>) {
    // What the host asks, in the kernel's log (a line, cut short): what a
    // job did there can be read afterwards.
    let said: String = service.chars().take(120).map(|c| if c == '\n' { ' ' } else { c }).collect();
    say!("asked: {said}");
    let (head, rest) = service.split_once(':').unwrap_or((service, ""));
    let mut words = head.split(',');
    let kind = words.next().unwrap_or_default();
    let v2 = words.any(|w| w == "v2");
    match kind {
        "shell" => shell(&ctx, rest, if v2 { Mode::V2 } else { Mode::V1 }, input, okay),
        "exec" => shell(&ctx, rest, Mode::Exec, input, okay),
        "sync" => sync(&ctx, input, okay),
        "reboot" => {
            say!("reboot {rest} asked");
            ctx.close();
            std::thread::sleep(Duration::from_millis(300));
            crate::init::reboot(Some(rest));
        }
        _ => {
            say!("a service not served: {service}");
            ctx.close();
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// stdout and stderr together, raw.
    V1,
    /// Framed: stdin, stdout, stderr, the exit code.
    V2,
    /// Raw both ways, stderr dropped (exec-in, exec-out).
    Exec,
}

fn shell(ctx: &Ctx, cmd: &str, mode: Mode, input: Inbound, okay: Receiver<()>) {
    // item/grid's verbs run straight, no shell between (verbs.rs).
    let words: Vec<&str> = cmd.split(' ').filter(|w| !w.is_empty()).collect();
    let direct = words.first() == Some(&"item-recovery") && words.get(1).is_some_and(|v| crate::verbs::VERBS.contains(v)) && cmd.bytes().all(|b| b == b' ' || crate::verbs::plain(b));
    let mut c = if direct {
        let mut c = Command::new(std::env::current_exe().unwrap_or_else(|_| "/sbin/item-recovery".into()));
        c.args(words[1..].iter().map(|w| crate::verbs::unescape(w)));
        c
    } else {
        // No shell here (one binary, no busybox): the line is answered so.
        let mut c = Command::new(std::env::current_exe().unwrap_or_else(|_| "/sbin/item-recovery".into()));
        c.arg("no-shell");
        c
    };
    // A verb by exec (in or out) has no way to tell its end: its stderr
    // and exit code are kept for `item-recovery last` (verbs.rs).
    let kept_err = if direct && mode == Mode::Exec {
        let _ = std::fs::remove_file(crate::verbs::LAST_RC);
        std::fs::File::create(crate::verbs::LAST_ERR).ok()
    } else {
        None
    };
    c.current_dir("/").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(match kept_err {
        Some(f) => Stdio::from(f),
        None if mode == Mode::Exec => Stdio::null(),
        None => Stdio::piped(),
    });
    let mut child = match c.spawn() {
        Ok(c) => c,
        Err(e) => {
            say!("sh: {e}");
            ctx.close();
            return;
        }
    };
    // The host's input into the command (framed in v2); its end closes
    // the command's stdin.
    let mut stdin = child.stdin.take();
    std::thread::spawn(move || {
        let mut framed: Vec<u8> = Vec::new();
        while let Some(data) = input.next() {
            if mode == Mode::V2 {
                framed.extend_from_slice(&data);
                while framed.len() >= 5 {
                    let len = le(&framed, 1) as usize;
                    if framed.len() < 5 + len {
                        break;
                    }
                    match framed[0] {
                        STDIN => {
                            if let Some(s) = stdin.as_mut() {
                                if s.write_all(&framed[5..5 + len]).is_err() {
                                    stdin = None;
                                }
                            }
                        }
                        CLOSE_STDIN => stdin = None,
                        _ => {}
                    }
                    framed.drain(..5 + len);
                }
            } else if let Some(s) = stdin.as_mut() {
                if s.write_all(&data).is_err() {
                    stdin = None;
                }
            }
        }
    });
    // The command's output, both streams onto one queue in turn.
    let (tx, rx) = channel::<(u8, Vec<u8>)>();
    let pump = |mut r: Box<dyn Read + Send>, id: u8, tx: Sender<(u8, Vec<u8>)>| {
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 256 * 1024];
            loop {
                let n = match r.read(&mut buf) {
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                };
                if n == 0 || tx.send((id, buf[..n].to_vec())).is_err() {
                    break;
                }
            }
        });
    };
    if let Some(o) = child.stdout.take() {
        pump(Box::new(o), STDOUT, tx.clone());
    }
    if let Some(e) = child.stderr.take() {
        pump(Box::new(e), STDERR, tx.clone());
    }
    drop(tx);
    let mut open = true;
    // What came while the last write waited for its OKAY, gathered into
    // one payload of a stream's (64 KB pieces one by one, each waiting for
    // its OKAY, gave 7 MB/s out).
    let mut held: Option<(u8, Vec<u8>)> = None;
    while let Some((id, mut data)) = held.take().or_else(|| rx.recv().ok()) {
        while data.len() < ctx.max {
            match rx.try_recv() {
                Ok((i, more)) if i == id => data.extend_from_slice(&more),
                Ok(other) => {
                    held = Some(other);
                    break;
                }
                Err(_) => break,
            }
        }
        let frame = if mode == Mode::V2 { framed(id, &data) } else { data };
        if !ctx.write(&okay, &frame) {
            open = false;
            let _ = child.kill();
            break;
        }
    }
    let status = child.wait();
    use std::os::unix::process::ExitStatusExt;
    let code = status.map(|s| s.code().unwrap_or_else(|| 128 + s.signal().unwrap_or(0))).unwrap_or(255);
    if direct && mode == Mode::Exec {
        let _ = std::fs::write(crate::verbs::LAST_RC, code.to_string());
    }
    if open && mode == Mode::V2 {
        ctx.write(&okay, &framed(EXIT, &[code as u8]));
    }
    ctx.close();
}

fn framed(id: u8, data: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(5 + data.len());
    f.push(id);
    f.extend_from_slice(&(data.len() as u32).to_le_bytes());
    f.extend_from_slice(data);
    f
}

/// The host's bytes as a sync request reads them, taken as they are
/// needed.
struct Incoming {
    input: Inbound,
    have: Vec<u8>,
}

impl Incoming {
    fn take(&mut self, n: usize) -> Option<Vec<u8>> {
        while self.have.len() < n {
            let d = self.input.next()?;
            self.have.extend_from_slice(&d);
        }
        Some(self.have.drain(..n).collect())
    }

    /// A request: its id and its length (or argument).
    fn request(&mut self) -> Option<([u8; 4], u32)> {
        let h = self.take(8)?;
        Some(([h[0], h[1], h[2], h[3]], le(&h, 4)))
    }
}

fn reply(id: &[u8; 4], words: &[u32], tail: &[u8]) -> Vec<u8> {
    let mut r = id.to_vec();
    for w in words {
        r.extend_from_slice(&w.to_le_bytes());
    }
    r.extend_from_slice(tail);
    r
}

fn fail(msg: &str) -> Vec<u8> {
    reply(b"FAIL", &[msg.len() as u32], msg.as_bytes())
}

fn stat_words(path: &str) -> [u32; 3] {
    use std::os::unix::fs::MetadataExt;
    match std::fs::symlink_metadata(path) {
        Ok(m) => [m.mode(), m.size() as u32, m.mtime() as u32],
        Err(_) => [0, 0, 0],
    }
}

fn sync(ctx: &Ctx, input: Inbound, okay: Receiver<()>) {
    let mut inc = Incoming { input, have: Vec::new() };
    while let Some((id, len)) = inc.request() {
        let ok = match &id {
            b"STAT" => {
                let Some(p) = inc.take(len as usize) else { break };
                let path = String::from_utf8_lossy(&p).into_owned();
                ctx.write(&okay, &reply(b"STAT", &stat_words(&path), &[]))
            }
            b"LIST" => {
                let Some(p) = inc.take(len as usize) else { break };
                let dir = String::from_utf8_lossy(&p).into_owned();
                let mut all = Vec::new();
                if let Ok(rd) = std::fs::read_dir(&dir) {
                    for e in rd.flatten() {
                        let name = e.file_name().to_string_lossy().into_owned();
                        let s = stat_words(&format!("{dir}/{name}"));
                        all.extend(reply(b"DENT", &[s[0], s[1], s[2], name.len() as u32], name.as_bytes()));
                    }
                }
                all.extend(reply(b"DONE", &[0, 0, 0, 0], &[]));
                ctx.write(&okay, &all)
            }
            b"SEND" => {
                let Some(p) = inc.take(len as usize) else { break };
                let spec = String::from_utf8_lossy(&p).into_owned();
                let (path, mode) = spec.rsplit_once(',').map(|(p, m)| (p.to_owned(), m.parse::<u32>().unwrap_or(0o644))).unwrap_or((spec.clone(), 0o644));
                let Some(r) = receive(&mut inc, &path, mode) else { break };
                ctx.write(&okay, &match r {
                    Ok(()) => reply(b"OKAY", &[0], &[]),
                    Err(e) => fail(&e),
                })
            }
            b"RECV" => {
                let Some(p) = inc.take(len as usize) else { break };
                let path = String::from_utf8_lossy(&p).into_owned();
                send_file(ctx, &okay, &path)
            }
            b"QUIT" => break,
            _ => {
                say!("sync: {} not served", String::from_utf8_lossy(&id));
                ctx.write(&okay, &fail("not served"));
                break;
            }
        };
        if !ok {
            break;
        }
    }
    ctx.close();
}

/// A pushed file's DATA until DONE, written at `path`: the result (None:
/// the stream ended on the way).
fn receive(inc: &mut Incoming, path: &str, mode: u32) -> Option<Result<(), String>> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut file = File::create(path).map_err(|e| format!("{path}: {e}"));
    loop {
        let (id, len) = inc.request()?;
        match &id {
            b"DATA" => {
                let d = inc.take(len as usize)?;
                if let Ok(f) = file.as_mut() {
                    if let Err(e) = f.write_all(&d) {
                        file = Err(format!("{path}: {e}"));
                    }
                }
            }
            b"DONE" => {
                return Some(file.and_then(|f| {
                    f.sync_all().map_err(|e| format!("{path}: {e}"))?;
                    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & 0o7777));
                    Ok(())
                }));
            }
            _ => return Some(Err("a push without DONE".into())),
        }
    }
}

fn send_file(ctx: &Ctx, okay: &Receiver<()>, path: &str) -> bool {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(e) => return ctx.write(okay, &fail(&format!("{path}: {e}"))),
    };
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match f.read(&mut buf) {
            Ok(0) => return ctx.write(okay, &reply(b"DONE", &[0], &[])),
            Ok(n) => {
                if !ctx.write(okay, &reply(b"DATA", &[n as u32], &buf[..n])) {
                    return false;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return ctx.write(okay, &fail(&format!("{path}: {e}"))),
        }
    }
}
