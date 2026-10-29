//! What item/grid asks of the recovery, as commands of this program run
//! straight by the adb daemon (no shell between): `item-recovery VERB
//! ARGS...`, words split at spaces, `%XX` in a word the byte XX (any byte
//! but a letter, a digit and `/._+=:,@-`: item/grid's rec.rs writes them
//! so; a line with another is a shell's, adb.rs gives it to sh). Each says what failed on stderr and
//! exits 1; what it answers goes to stdout. In place of busybox's lines
//! (dd, sha256sum | cut, blockdev, tar, pigz, mount, ...).

use std::fs::{File, OpenOptions};
use std::io::{BufRead, Read, Seek, SeekFrom, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

/// A verb run by exec-in or exec-out: its exit code and stderr, written by
/// adb.rs as it ends, for `last`.
pub const LAST_RC: &str = "/tmp/item-recovery-last.rc";
pub const LAST_ERR: &str = "/tmp/item-recovery-last.err";

/// The verbs, for adb.rs: a command line starting with one of these after
/// `item-recovery ` is run here.
pub const VERBS: &[&str] = &["drm", "unzstd", "log", "stat", "unsparse", "sparse", "mkfs", "last", "real", "truncate", "write", "replace", "copy", "list", "size", "hash", "zero", "put", "get", "gz", "tar", "untar", "mount", "umount", "link", "rm", "mkdir", "meta", "sizes", "sync", "gpt"];

pub fn run(verb: &str, args: &[String]) -> i32 {
    let a = |i: usize| args.get(i).map(String::as_str).ok_or_else(|| format!("{verb}: an argument is missing"));
    let num = |i: usize| -> Result<u64, String> { a(i)?.parse().map_err(|_| format!("{verb}: {} is not a number", args[i])) };
    let r: Result<(), String> = match verb {
        // The last exec's end: its code ("" while it runs), then its stderr.
        "last" => {
            println!("{}", std::fs::read_to_string(LAST_RC).unwrap_or_default().trim());
            print!("{}", std::fs::read_to_string(LAST_ERR).unwrap_or_default());
            Ok(())
        }
        "real" => a(0).and_then(|p| std::fs::canonicalize(p).map(|r| println!("{}", r.display())).map_err(|e| format!("{p}: {e}"))),
        "truncate" => match truncate(args) {
            0 => Ok(()),
            _ => Err("truncate failed".into()),
        },
        "size" => a(0).and_then(|p| size(p).map(|n| println!("{n}"))),
        // The kernel's log as it is now (dmesg's), its last N lines.
        "log" => (if args.is_empty() { Ok(200) } else { num(0) }).and_then(|n| kernel_log(n as usize)),
        // What a path is: its kind, mode, owner, size; a link's target.
        "stat" => args.iter().try_for_each(|p| stat(p)),
        // `hash --direct PATH`: read from the flash, past the page cache
        // (O_DIRECT): what was written, not what is remembered of it.
        "hash" if args.first().is_some_and(|a| a == "--direct") => a(1).and_then(|p| hash_direct(p).map(|h| println!("{h}"))),
        "hash" => a(0).and_then(|p| {
            let range = if args.len() >= 3 { Some((num(1)?, num(2)?)) } else { None };
            hash(p, range).map(|h| println!("{h}"))
        }),
        "zero" => a(0).and_then(|p| zero(p, if args.len() >= 2 { Some(num(1)?) } else { None })),
        "put" => a(0).and_then(|p| put(p, if args.len() >= 2 { num(1)? } else { 0 })),
        "write" => a(0).and_then(write),
        // stdin as PATH, whole at once for its readers (PATH.new, renamed):
        // the screen's status file.
        "replace" => a(0).and_then(|p| {
            let new = format!("{p}.new");
            let mut data = Vec::new();
            std::io::stdin().read_to_end(&mut data).map_err(|e| e.to_string())?;
            std::fs::write(&new, &data).and_then(|_| std::fs::rename(&new, p)).map_err(|e| format!("{p}: {e}"))
        }),
        "copy" => a(0).and_then(|src| a(1).and_then(|dst| copy(src, dst))),
        // The names in a directory, one a line, sorted.
        "list" => a(0).and_then(|d| {
            let mut names: Vec<String> = std::fs::read_dir(d).map_err(|e| format!("{d}: {e}"))?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            names.sort();
            names.iter().for_each(|n| println!("{n}"));
            Ok(())
        }),
        "get" => a(0).and_then(|p| get(p, if args.len() >= 3 { Some((num(1)?, num(2)?)) } else { None })),
        "gz" => a(0).and_then(gz_file),
        "sparse" => a(0).and_then(|p| sparse(p, &mut std::io::stdout().lock())),
        "unsparse" => a(0).and_then(|p| unsparse(p, &mut std::io::stdin().lock())),
        // What the display can do, its colour above all (reads only).
        "drm" => crate::drm::report().map(|r| print!("{r}")),
        "unzstd" => a(0).and_then(|p| unzstd(p, &mut std::io::stdin().lock()).map(|n| println!("{n}"))),
        "tar" => a(0).and_then(|dir| tar_gz(dir, &args[1..], &mut std::io::stdout().lock())),
        "untar" => a(0).and_then(|root| untar(root, args.get(1).map(String::as_str))),
        "mount" => mount_verb(args),
        "umount" => a(0).and_then(umount),
        "link" => a(0).and_then(|target| a(1).and_then(|path| link(target, path))),
        "rm" => args.iter().try_for_each(|p| rm(p)),
        "mkdir" => args.iter().try_for_each(|p| std::fs::create_dir_all(p).map_err(|e| format!("mkdir {p}: {e}"))),
        "meta" => (|| meta(a(0)?, u32::from_str_radix(a(1)?, 8).map_err(|_| "meta: the mode is octal")?, num(2)? as u32, num(3)? as u32))(),
        "sizes" => sizes(args.first().map(String::as_str)).map(|n| println!("{n}")),
        "sync" => {
            // SAFETY: a system call without arguments.
            unsafe { libc::sync() };
            Ok(())
        }
        "gpt" => return crate::gpt::command(args),
        "mkfs" => crate::mkfs::command(args),
        _ => Err(format!("{verb}: not a verb of item-recovery")),
    };
    match r {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// The bytes a verb's line holds as they are: a line of only these (and
/// spaces) after `item-recovery VERB` is run straight.
pub fn plain(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"/._+=:,@-%".contains(&b)
}

/// A word as rec.rs wrote it: `%XX` the byte XX.
pub fn unescape(word: &str) -> String {
    let b = word.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && word.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&word[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `truncate -s SIZE FILE...` (bytes, or with K, M, G): each file made
/// that long, created if it is not there.
fn truncate(args: &[String]) -> i32 {
    let mut size: Option<u64> = None;
    let mut files = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "-s" {
            size = it.next().and_then(|v| parse_size(v));
        } else if let Some(v) = a.strip_prefix("-s") {
            size = parse_size(v);
        } else {
            files.push(a);
        }
    }
    let Some(size) = size else {
        eprintln!("truncate -s SIZE FILE...");
        return 1;
    };
    let mut rc = 0;
    for f in files {
        let r = std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(f).and_then(|h| h.set_len(size));
        if let Err(e) = r {
            eprintln!("truncate: {f}: {e}");
            rc = 1;
        }
    }
    rc
}

fn parse_size(v: &str) -> Option<u64> {
    let (n, k) = match v.chars().last()? {
        'K' | 'k' => (&v[..v.len() - 1], 1 << 10),
        'M' | 'm' => (&v[..v.len() - 1], 1 << 20),
        'G' | 'g' => (&v[..v.len() - 1], 1 << 30),
        _ => (v, 1),
    };
    n.parse::<u64>().ok().map(|n| n * k)
}

/// The kernel's ring buffer read from /dev/kmsg without waiting: each
/// record's time and text, the last `n`.
fn kernel_log(n: usize) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open("/dev/kmsg").map_err(|e| format!("/dev/kmsg: {e}"))?;
    let mut lines = std::collections::VecDeque::new();
    let mut buf = vec![0u8; 8192];
    loop {
        // One record a read; EAGAIN at the end; EPIPE where the ring ran
        // over what was not read yet (read on).
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(k) => {
                let rec = String::from_utf8_lossy(&buf[..k]);
                let (head, text) = rec.split_once(';').unwrap_or(("", &rec));
                let usec: u64 = head.split(',').nth(2).and_then(|t| t.parse().ok()).unwrap_or(0);
                lines.push_back(format!("[{:>5}.{:06}] {}", usec / 1_000_000, usec % 1_000_000, text.lines().next().unwrap_or("")));
                if lines.len() > n {
                    lines.pop_front();
                }
            }
            Err(e) if e.raw_os_error() == Some(libc::EPIPE) || e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => return Err(format!("/dev/kmsg: {e}")),
        }
    }
    lines.iter().for_each(|l| println!("{l}"));
    Ok(())
}

/// A path's kind, mode, owner and group, size (a device's: where its end
/// is), modified time; a link's target. One line.
fn stat(path: &str) -> Result<(), String> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let m = std::fs::symlink_metadata(path).map_err(|e| format!("{path}: {e}"))?;
    let t = m.file_type();
    let kind = if t.is_symlink() {
        "link"
    } else if t.is_dir() {
        "dir"
    } else if t.is_block_device() {
        "block"
    } else if t.is_char_device() {
        "char"
    } else if t.is_fifo() {
        "fifo"
    } else if t.is_socket() {
        "socket"
    } else {
        "file"
    };
    let size = if t.is_block_device() { size(path).unwrap_or(0) } else { m.len() };
    let target = if t.is_symlink() { std::fs::read_link(path).map(|l| format!(" -> {}", l.display())).unwrap_or_default() } else { String::new() };
    println!("{kind} {:o} {}:{} {size} {} {path}{target}", m.mode() & 0o7777, m.uid(), m.gid(), m.mtime());
    Ok(())
}

fn open(path: &str) -> Result<File, String> {
    File::open(path).map_err(|e| format!("{path}: {e}"))
}

/// Bytes in a file or a block device (a device's size is where its end is).
pub fn size(path: &str) -> Result<u64, String> {
    open(path)?.seek(SeekFrom::End(0)).map_err(|e| format!("{path}: {e}"))
}

/// sha256 of the whole, or of `len` bytes from `at` (fewer at the end).
pub fn hash(path: &str, range: Option<(u64, u64)>) -> Result<String, String> {
    let mut f = open(path)?;
    let mut h = Sha256::new();
    let mut left = match range {
        Some((at, len)) => {
            f.seek(SeekFrom::Start(at)).map_err(|e| format!("{path}: {e}"))?;
            len
        }
        None => u64::MAX,
    };
    let mut buf = vec![0u8; 4 << 20];
    while left > 0 {
        let want = buf.len().min(left.min(usize::MAX as u64) as usize);
        let n = f.read(&mut buf[..want]).map_err(|e| format!("{path}: {e}"))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        left -= n as u64;
    }
    Ok(format!("{:x}", h.finalize()))
}

/// sha256 of the whole, read with O_DIRECT in 4 MB aligned reads.
fn hash_direct(path: &str) -> Result<String, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = OpenOptions::new().read(true).custom_flags(libc::O_DIRECT).open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut h = Sha256::new();
    // A 4 KB-aligned buffer, as O_DIRECT wants.
    let mut raw = vec![0u8; (4 << 20) + 4096];
    let off = raw.as_ptr().align_offset(4096);
    let buf = &mut raw[off..off + (4 << 20)];
    loop {
        let n = f.read(buf).map_err(|e| format!("{path}: {e}"))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// Zeros over the first `len` bytes, or over the whole. A whole block
/// device is discarded first (the flash told the blocks are free, no
/// writing: seconds, not minutes), then read at 64 places; written with
/// zeros only if any reads otherwise.
fn zero(path: &str, len: Option<u64>) -> Result<(), String> {
    if len.is_none() && discarded(path)? {
        return Ok(());
    }
    let mut f = OpenOptions::new().write(true).open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut left = match len {
        Some(n) => n,
        None => size(path)?,
    };
    let buf = vec![0u8; 4 << 20];
    while left > 0 {
        let n = buf.len().min(left as usize);
        f.write_all(&buf[..n]).map_err(|e| format!("{path}: {e}"))?;
        left -= n as u64;
    }
    f.sync_all().map_err(|e| format!("{path}: {e}"))
}

/// A block device discarded whole and read back as zeros at 64 places
/// (O_DIRECT); false for a file, or where it did not read as zeros.
pub(crate) fn discarded(path: &str) -> Result<bool, String> {
    use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
    use std::os::unix::io::AsRawFd;
    if !std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?.file_type().is_block_device() {
        return Ok(false);
    }
    let len = size(path)?;
    let f = OpenOptions::new().read(true).write(true).custom_flags(libc::O_DIRECT).open(path).map_err(|e| format!("{path}: {e}"))?;
    const BLKDISCARD: libc::c_ulong = 0x1277;
    let range = [0u64, len];
    // SAFETY: the range the ioctl reads, on an open block device.
    if unsafe { libc::ioctl(f.as_raw_fd(), BLKDISCARD as _, range.as_ptr()) } != 0 {
        return Ok(false);
    }
    let mut raw = vec![0u8; 8192];
    let off = raw.as_ptr().align_offset(4096);
    let buf = &mut raw[off..off + 4096];
    for k in 0..64u64 {
        let at = (len / 4096 * k / 63).min(len / 4096 - 1) * 4096;
        std::os::unix::fs::FileExt::read_exact_at(&f, buf, at).map_err(|e| format!("{path} at {at}: {e}"))?;
        if buf.iter().any(|&b| b != 0) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// stdin written into `path` from byte `at` on (a file is made if it is
/// not there, never cut), synced.
fn put(path: &str, at: u64) -> Result<(), String> {
    let mut f = OpenOptions::new().write(true).create(!path.starts_with("/dev/")).truncate(false).open(path).map_err(|e| format!("{path}: {e}"))?;
    f.seek(SeekFrom::Start(at)).map_err(|e| format!("{path}: {e}"))?;
    std::io::copy(&mut std::io::stdin().lock(), &mut f).map_err(|e| format!("{path}: {e}"))?;
    f.sync_all().map_err(|e| format!("{path}: {e}"))
}

/// stdin as the whole of `path`: what was there goes, its mode and owner
/// stay (a new file is made 0600).
fn write(path: &str) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path).map_err(|e| format!("{path}: {e}"))?;
    std::io::copy(&mut std::io::stdin().lock(), &mut f).map_err(|e| format!("{path}: {e}"))?;
    f.sync_all().map_err(|e| format!("{path}: {e}"))
}

/// `src` written over the start of `dst` (a partition), the rest of `dst`
/// zeroed to its end: nothing of what was there left behind; synced.
fn copy(src: &str, dst: &str) -> Result<(), String> {
    let room = size(dst)?;
    let mut from = open(src)?;
    let mut to = OpenOptions::new().write(true).open(dst).map_err(|e| format!("{dst}: {e}"))?;
    let n = std::io::copy(&mut from, &mut to).map_err(|e| format!("{src} into {dst}: {e}"))?;
    if n > room {
        return Err(format!("{src} ({n} bytes) is larger than {dst} ({room})"));
    }
    let buf = vec![0u8; 4 << 20];
    let mut left = room - n;
    while left > 0 {
        let k = buf.len().min(left as usize);
        to.write_all(&buf[..k]).map_err(|e| format!("{dst}: {e}"))?;
        left -= k as u64;
    }
    to.sync_all().map_err(|e| format!("{dst}: {e}"))
}

/// The whole, or `len` bytes from `at`, to stdout.
fn get(path: &str, range: Option<(u64, u64)>) -> Result<(), String> {
    let mut f = open(path)?;
    let out = &mut std::io::stdout().lock();
    match range {
        Some((at, len)) => {
            f.seek(SeekFrom::Start(at)).map_err(|e| format!("{path}: {e}"))?;
            std::io::copy(&mut f.take(len), out)
        }
        None => std::io::copy(&mut f, out),
    }
    .map(|_| ())
    .map_err(|e| format!("{path}: {e}"))
}

/// A gzip stream of `input` into `out`, made on every core as pigz does:
/// 4 MB pieces compressed apart, each its own gzip member, written in
/// order (a gzip reader takes the members as one stream).
fn gz(input: &mut dyn Read, out: &mut dyn Write) -> Result<(), String> {
    use std::sync::mpsc;
    const PIECE: usize = 4 << 20;
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    // In flight, in order: each piece's answer comes on its own channel.
    let mut pending: std::collections::VecDeque<mpsc::Receiver<Vec<u8>>> = Default::default();
    loop {
        let mut piece = vec![0u8; PIECE];
        let mut n = 0;
        while n < PIECE {
            let got = input.read(&mut piece[n..]).map_err(|e| e.to_string())?;
            if got == 0 {
                break;
            }
            n += got;
        }
        piece.truncate(n);
        if n > 0 {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let mut e = flate2::write::GzEncoder::new(Vec::with_capacity(piece.len() / 2), flate2::Compression::fast());
                let _ = e.write_all(&piece);
                let _ = tx.send(e.finish().unwrap_or_default());
            });
            pending.push_back(rx);
        }
        // Full: the oldest waited for; at the end, all of them.
        while pending.len() >= cores || (n < PIECE && !pending.is_empty()) {
            let done = pending.pop_front().unwrap().recv().map_err(|_| "a piece was lost")?;
            out.write_all(&done).map_err(|e| e.to_string())?;
        }
        if n < PIECE {
            return out.flush().map_err(|e| e.to_string());
        }
    }
}

/// A file's data without its holes, to `out`: for each 4 MB piece of data
/// that is not all zeros, `D`, its offset (u64), its length (u32), its
/// gzip's length (u32), its sha256 (32 bytes), then the gzip (a member of
/// its own, made on every core); at the end `E`, the file's size (u64) and
/// the whole file's sha256 (32 bytes, hashed here on a thread as it is
/// read: the computer need not hash the zeros). Little-endian. The holes
/// (SEEK_HOLE) and the zero pieces are not sent: a reader puts zeros there.
pub fn sparse(path: &str, out: &mut dyn Write) -> Result<(), String> {
    use std::os::unix::fs::FileExt;
    use std::os::unix::io::AsRawFd;
    use std::sync::mpsc;
    const PIECE: u64 = 4 << 20;
    let f = open(path)?;
    let size = f.metadata().map_err(|e| format!("{path}: {e}"))?.len();
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let fd = f.as_raw_fd();
    // SAFETY: advice on an open descriptor: read ahead further.
    unsafe { libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_SEQUENTIAL) };
    // The whole hashed in order on its own thread: a piece's bytes, or a
    // run of zeros.
    let (whole_tx, whole_rx) = mpsc::sync_channel::<Result<std::sync::Arc<Vec<u8>>, u64>>(8);
    let whole = std::thread::spawn(move || {
        let zeros = vec![0u8; PIECE as usize];
        let mut h = Sha256::new();
        for part in whole_rx {
            match part {
                Ok(piece) => h.update(&piece[..]),
                Err(mut n) => {
                    while n > 0 {
                        let k = n.min(PIECE) as usize;
                        h.update(&zeros[..k]);
                        n -= k as u64;
                    }
                }
            }
        }
        <[u8; 32]>::from(h.finalize())
    });
    let mut hashed = 0u64;
    let seek = |at: u64, whence: libc::c_int| -> Option<u64> {
        // SAFETY: lseek on an open descriptor; ENXIO past the last data.
        let r = unsafe { libc::lseek(fd, at as libc::off_t, whence) };
        (r >= 0).then_some(r as u64)
    };
    // In flight, in order: a piece's offset, its length, and where its
    // gzip and sha256 come.
    type Pending = std::collections::VecDeque<(u64, u32, mpsc::Receiver<(Vec<u8>, [u8; 32])>)>;
    let mut pending = Pending::new();
    let mut drain = |pending: &mut Pending, keep: usize| -> Result<(), String> {
        while pending.len() > keep {
            let (at, len, rx) = pending.pop_front().unwrap();
            let (gz, sha) = rx.recv().map_err(|_| "a piece was lost")?;
            let mut head = Vec::with_capacity(57);
            head.push(b'D');
            head.extend_from_slice(&at.to_le_bytes());
            head.extend_from_slice(&len.to_le_bytes());
            head.extend_from_slice(&(gz.len() as u32).to_le_bytes());
            head.extend_from_slice(&sha);
            out.write_all(&head).and_then(|_| out.write_all(&gz)).map_err(|e| e.to_string())?;
        }
        Ok(())
    };
    let mut pos = 0u64;
    while pos < size {
        let Some(data) = seek(pos, libc::SEEK_DATA) else { break };
        let hole = seek(data, libc::SEEK_HOLE).unwrap_or(size).min(size);
        // Whole pieces, aligned, so each lies in one piece of the reader's.
        let mut at = data - data % PIECE;
        if at > hashed {
            whole_tx.send(Err(at - hashed)).map_err(|_| "the hashing stopped")?;
        }
        while at < hole {
            let len = PIECE.min(size - at);
            let mut piece = vec![0u8; len as usize];
            f.read_exact_at(&mut piece, at).map_err(|e| format!("{path} at {at}: {e}"))?;
            hashed = at + len;
            if !piece.iter().any(|&b| b != 0) {
                whole_tx.send(Err(len)).map_err(|_| "the hashing stopped")?;
            } else {
                let piece = std::sync::Arc::new(piece);
                whole_tx.send(Ok(piece.clone())).map_err(|_| "the hashing stopped")?;
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let sha: [u8; 32] = Sha256::digest(&piece[..]).into();
                    let mut e = flate2::write::GzEncoder::new(Vec::with_capacity(piece.len() / 4), flate2::Compression::fast());
                    let _ = e.write_all(&piece[..]);
                    let _ = tx.send((e.finish().unwrap_or_default(), sha));
                });
                pending.push_back((at, len as u32, rx));
                drain(&mut pending, cores)?;
            }
            at += len;
        }
        pos = hole.max(at);
    }
    drain(&mut pending, 0)?;
    if size > hashed {
        whole_tx.send(Err(size - hashed)).map_err(|_| "the hashing stopped")?;
    }
    drop(whole_tx);
    let sha = whole.join().map_err(|_| "the hashing stopped")?;
    let mut end = vec![b'E'];
    end.extend_from_slice(&size.to_le_bytes());
    end.extend_from_slice(&sha);
    out.write_all(&end).and_then(|_| out.flush()).map_err(|e| e.to_string())
}

/// `sparse`'s records from `input` put into `path`: `D` pieces gzipped,
/// `R` pieces as they are (the same header: offset, length, the payload's
/// length, sha256), each unpacked, checked against its sha256 and written
/// at its place; what no piece covers stays a hole. `E`, a size and 32
/// bytes (the whole's sha256, or zeros): the file made that long. Made if it is not there, never cut first: the
/// computer sends a big file in batches, each adding to the last. Synced.
pub fn unsparse(path: &str, input: &mut dyn Read) -> Result<(), String> {
    use std::os::unix::fs::FileExt;
    let f = OpenOptions::new().write(true).create(true).truncate(false).open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut raw = Vec::with_capacity(4 << 20);
    loop {
        let mut kind = [0u8; 1];
        match input.read_exact(&mut kind) {
            Ok(()) => {}
            // A batch without `E`: it ends where its input does.
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        }
        match kind[0] {
            b'D' | b'R' => {
                let mut h = [0u8; 48];
                input.read_exact(&mut h).map_err(|e| format!("a piece's header: {e}"))?;
                let at = u64::from_le_bytes(h[0..8].try_into().unwrap());
                let len = u32::from_le_bytes(h[8..12].try_into().unwrap()) as usize;
                let plen = u32::from_le_bytes(h[12..16].try_into().unwrap()) as usize;
                let mut payload = vec![0u8; plen];
                input.read_exact(&mut payload).map_err(|e| format!("the piece at {at}: {e}"))?;
                let data = if kind[0] == b'D' {
                    raw.clear();
                    flate2::read::GzDecoder::new(&payload[..]).read_to_end(&mut raw).map_err(|e| format!("the piece at {at} does not unpack: {e}"))?;
                    &raw[..]
                } else {
                    &payload[..]
                };
                if data.len() != len || <[u8; 32]>::from(Sha256::digest(data)) != h[16..48] {
                    return Err(format!("the piece at {at} arrived different"));
                }
                f.write_all_at(data, at).map_err(|e| format!("{path} at {at}: {e}"))?;
            }
            b'E' => {
                // The size, and the whole's sha256 (`sparse` gives it; not
                // checked here - hash --direct does, from the flash).
                let mut e = [0u8; 40];
                input.read_exact(&mut e).map_err(|e| format!("the size: {e}"))?;
                f.set_len(u64::from_le_bytes(e[..8].try_into().unwrap())).map_err(|e| format!("{path}: {e}"))?;
            }
            k => return Err(format!("not a sparse record ({k:#x})")),
        }
    }
    f.sync_all().map_err(|e| format!("{path}: {e}"))
}

/// A zstd stream (a release's rootfs.img.zst, as it is: a third of its
/// gzip over the cable) unpacked into `path`, made anew: its 4 MB pieces
/// that are all zeros left as holes, the file cut to the stream's length.
/// Synced. Its size back. (Its sha256 is checked after, from the flash.)
pub fn unzstd(path: &str, input: &mut dyn Read) -> Result<u64, String> {
    use std::os::unix::fs::FileExt;
    const PIECE: usize = 4 << 20;
    let f = OpenOptions::new().write(true).create(true).truncate(true).open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut d = zstd::stream::read::Decoder::new(input).map_err(|e| format!("zstd: {e}"))?;
    // The release images are made with --long=27.
    d.window_log_max(31).map_err(|e| format!("zstd: {e}"))?;
    let mut piece = vec![0u8; PIECE];
    let mut at = 0u64;
    loop {
        let mut filled = 0;
        while filled < PIECE {
            match d.read(&mut piece[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(format!("the stream at {}: {e}", at + filled as u64)),
            }
        }
        if filled == 0 {
            break;
        }
        if piece[..filled].iter().any(|&b| b != 0) {
            f.write_all_at(&piece[..filled], at).map_err(|e| format!("{path} at {at}: {e}"))?;
        }
        at += filled as u64;
        if filled < PIECE {
            break;
        }
    }
    f.set_len(at).map_err(|e| format!("{path}: {e}"))?;
    f.sync_all().map_err(|e| format!("{path}: {e}"))?;
    Ok(at)
}

fn gz_file(path: &str) -> Result<(), String> {
    gz(&mut open(path)?, &mut std::io::stdout().lock())
}

/// `names` under `dir` as a tar (owners, modes, links kept), gzipped,
/// into `out`. Nothing but regular files, directories and symlinks; a name
/// that is not there is skipped.
fn tar_gz(dir: &str, names: &[String], out: &mut dyn Write) -> Result<(), String> {
    let (mut reader, writer) = std::io::pipe().map_err(|e| e.to_string())?;
    let (dir, names) = (dir.to_owned(), names.to_vec());
    let maker = std::thread::spawn(move || -> Result<(), String> {
        let mut t = tar::Builder::new(writer);
        t.follow_symlinks(false);
        t.mode(tar::HeaderMode::Complete);
        for n in &names {
            add(&mut t, Path::new(&dir), Path::new(n))?;
        }
        t.finish().map_err(|e| e.to_string())
    });
    let r = gz(&mut reader, out);
    drop(reader);
    maker.join().map_err(|_| "the tar's maker failed")??;
    r
}

/// `name` (under `dir`) into the tar, a directory with all it holds. A
/// socket is left out, as busybox's tar does: nothing to keep in one (its
/// program makes it again).
fn add<W: Write>(t: &mut tar::Builder<W>, dir: &Path, name: &Path) -> Result<(), String> {
    use std::os::unix::fs::FileTypeExt;
    let p = dir.join(name);
    let Ok(m) = p.symlink_metadata() else { return Ok(()) };
    let kind = m.file_type();
    if kind.is_socket() {
        return Ok(());
    }
    if kind.is_fifo() || kind.is_char_device() || kind.is_block_device() {
        // Its header made here: the tar crate's own for these takes the
        // full path for the name, and refuses it.
        use std::os::unix::fs::MetadataExt;
        let mut h = tar::Header::new_gnu();
        h.set_metadata_in_mode(&m, tar::HeaderMode::Complete);
        h.set_entry_type(if kind.is_fifo() {
            tar::EntryType::Fifo
        } else if kind.is_char_device() {
            tar::EntryType::Char
        } else {
            tar::EntryType::Block
        });
        let rdev = m.rdev();
        h.set_device_major(libc::major(rdev)).and_then(|_| h.set_device_minor(libc::minor(rdev))).map_err(|e| format!("{}: {e}", p.display()))?;
        h.set_size(0);
        return t.append_data(&mut h, name, std::io::empty()).map_err(|e| format!("{}: {e}", p.display()));
    }
    t.append_path_with_name(&p, name).map_err(|e| format!("{}: {e}", p.display()))?;
    if kind.is_dir() {
        let mut names: Vec<_> = std::fs::read_dir(&p).map_err(|e| format!("{}: {e}", p.display()))?.flatten().map(|e| e.file_name()).collect();
        names.sort();
        for n in names {
            add(t, dir, &name.join(n))?;
        }
    }
    Ok(())
}

/// A tar (a file, or stdin with no file) unpacked under `root`, owners,
/// modes and times kept.
fn untar(root: &str, file: Option<&str>) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|e| format!("{root}: {e}"))?;
    let input: Box<dyn Read> = match file {
        Some(f) => Box::new(open(f)?),
        None => Box::new(std::io::stdin()),
    };
    let mut t = tar::Archive::new(input);
    t.set_preserve_permissions(true);
    t.set_preserve_ownerships(true);
    t.set_preserve_mtime(true);
    t.set_overwrite(true);
    for entry in t.entries().map_err(|e| format!("unpacking into {root}: {e}"))? {
        let mut entry = entry.map_err(|e| format!("unpacking into {root}: {e}"))?;
        let kind = entry.header().entry_type();
        if matches!(kind, tar::EntryType::Fifo | tar::EntryType::Char | tar::EntryType::Block) {
            // The tar crate makes these plain files: made here, as they were.
            special(root, &entry)?;
            continue;
        }
        entry.unpack_in(root).map_err(|e| format!("unpacking into {root}: {e}"))?;
    }
    Ok(())
}

/// A FIFO or a device node from a tar entry, under `root`.
fn special<R: Read>(root: &str, entry: &tar::Entry<R>) -> Result<(), String> {
    let h = entry.header();
    let name = entry.path().map_err(|e| e.to_string())?;
    if name.is_absolute() || name.components().any(|c| c == std::path::Component::ParentDir) {
        return Err(format!("a strange name in the tar: {}", name.display()));
    }
    let path = Path::new(root).join(&name);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let _ = std::fs::remove_file(&path);
    let kind = match h.entry_type() {
        tar::EntryType::Fifo => libc::S_IFIFO,
        tar::EntryType::Char => libc::S_IFCHR,
        _ => libc::S_IFBLK,
    };
    let dev = libc::makedev(h.device_major().ok().flatten().unwrap_or(0), h.device_minor().ok().flatten().unwrap_or(0));
    let mode = h.mode().unwrap_or(0o600) & 0o7777;
    let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).map_err(|_| "a NUL in a name")?;
    // SAFETY: a valid path; the node's kind, mode and device from the tar.
    if unsafe { libc::mknod(c.as_ptr(), kind | mode, dev) } != 0 {
        return Err(format!("{}: {}", path.display(), std::io::Error::last_os_error()));
    }
    let (uid, gid) = (h.uid().unwrap_or(0) as u32, h.gid().unwrap_or(0) as u32);
    let _ = std::os::unix::fs::lchown(&path, Some(uid), Some(gid));
    // The mode as it was (mknod's is cut by the umask).
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).map_err(|e| format!("{}: {e}", path.display()))
}

/// `mount [-r] [-t TYPE] [-o OPTS] SRC DIR`: DIR made; a regular file is
/// mounted through a loop device; nothing done if DIR is mounted already.
fn mount_verb(args: &[String]) -> Result<(), String> {
    let (mut ro, mut fs, mut opts, mut rest) = (false, "ext4".to_owned(), String::new(), Vec::new());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-r" => ro = true,
            "-t" => fs = it.next().cloned().unwrap_or_default(),
            "-o" => opts = it.next().cloned().unwrap_or_default(),
            _ => rest.push(a.clone()),
        }
    }
    let [src, dir] = rest.as_slice() else { return Err("mount [-r] [-t TYPE] [-o OPTS] SRC DIR".into()) };
    std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
    if mounted(dir) {
        return Ok(());
    }
    // A loop device stays bound while it is open: held until the mount
    // holds it (autoclear lets it go at its last close otherwise).
    let (dev, _held) = if std::fs::metadata(src).map_err(|e| format!("{src}: {e}"))?.is_file() {
        let (d, f) = loop_for(src, ro)?;
        (d, Some(f))
    } else {
        (src.clone(), None)
    };
    let c = |s: &str| std::ffi::CString::new(s).map_err(|_| "a NUL in a name".to_string());
    let (s, t, f, d) = (c(&dev)?, c(dir)?, c(&fs)?, c(&opts)?);
    let flags = if ro { libc::MS_RDONLY } else { 0 };
    // SAFETY: valid C strings for the call's duration.
    if unsafe { libc::mount(s.as_ptr(), t.as_ptr(), f.as_ptr(), flags, d.as_ptr().cast()) } != 0 {
        return Err(format!("mount {src} on {dir}: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

/// Whether something is mounted on `dir` (as /proc/mounts has it).
pub fn mounted(dir: &str) -> bool {
    let real = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.into());
    std::fs::read_to_string("/proc/mounts").unwrap_or_default().lines().any(|l| l.split_whitespace().nth(1).is_some_and(|m| Path::new(m) == real))
}

/// A free loop device bound to `file`, and the device open (closed with
/// nothing on it, autoclear unbinds it).
fn loop_for(file: &str, ro: bool) -> Result<(String, File), String> {
    const LOOP_CTL_GET_FREE: libc::c_ulong = 0x4C82;
    const LOOP_SET_FD: libc::c_ulong = 0x4C00;
    use std::os::unix::io::AsRawFd;
    let ctl = OpenOptions::new().read(true).write(true).open("/dev/loop-control").map_err(|e| format!("/dev/loop-control: {e}"))?;
    // SAFETY: an ioctl without an argument on an open descriptor.
    let n = unsafe { libc::ioctl(ctl.as_raw_fd(), LOOP_CTL_GET_FREE as _) };
    if n < 0 {
        return Err(format!("no free loop device: {}", std::io::Error::last_os_error()));
    }
    let dev = format!("/dev/loop{n}");
    if !Path::new(&dev).exists() {
        // init's devtmpfs makes them; made here if not.
        let c = std::ffi::CString::new(dev.clone()).unwrap();
        // SAFETY: a valid path; block device 7:n.
        unsafe { libc::mknod(c.as_ptr(), libc::S_IFBLK | 0o600, libc::makedev(7, n as u32)) };
    }
    let backing = OpenOptions::new().read(true).write(!ro).open(file).map_err(|e| format!("{file}: {e}"))?;
    let lo = OpenOptions::new().read(true).write(!ro).open(&dev).map_err(|e| format!("{dev}: {e}"))?;
    // SAFETY: both descriptors open; the loop device keeps its own
    // reference to the file after the call.
    if unsafe { libc::ioctl(lo.as_raw_fd(), LOOP_SET_FD as _, backing.as_raw_fd()) } != 0 {
        return Err(format!("{dev} for {file}: {}", std::io::Error::last_os_error()));
    }
    // Let go of at the umount (LO_FLAGS_AUTOCLEAR), read only if asked:
    // struct loop_info64, its lo_flags at byte 52.
    const LOOP_SET_STATUS64: libc::c_ulong = 0x4C04;
    let mut info = [0u8; 232];
    info[52..56].copy_from_slice(&(4u32 | ro as u32).to_le_bytes());
    // SAFETY: a loop_info64-sized buffer the kernel reads.
    if unsafe { libc::ioctl(lo.as_raw_fd(), LOOP_SET_STATUS64 as _, info.as_ptr()) } != 0 {
        return Err(format!("{dev}'s flags: {}", std::io::Error::last_os_error()));
    }
    Ok((dev, lo))
}

/// Unmounted (and its loop device let go, the kernel's autoclear); not
/// mounted is no failure.
fn umount(dir: &str) -> Result<(), String> {
    if !mounted(dir) {
        return Ok(());
    }
    // SAFETY: a system call; sync first so the data is down.
    unsafe { libc::sync() };
    let t = std::ffi::CString::new(dir).map_err(|_| "a NUL in a name")?;
    // SAFETY: a valid C string.
    if unsafe { libc::umount(t.as_ptr()) } != 0 {
        return Err(format!("umount {dir}: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

fn link(target: &str, path: &str) -> Result<(), String> {
    let _ = std::fs::remove_file(path);
    std::os::unix::fs::symlink(target, path).map_err(|e| format!("{path}: {e}"))
}

/// A file, a link or a whole tree; not there is no failure.
fn rm(path: &str) -> Result<(), String> {
    let r = match std::fs::symlink_metadata(path) {
        Err(_) => return Ok(()),
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
    };
    r.map_err(|e| format!("rm {path}: {e}"))
}

/// Mode, owner and group (`meta PATH MODE UID GID`, the mode in octal),
/// the link itself for a symlink.
fn meta(path: &str, mode: u32, uid: u32, gid: u32) -> Result<(), String> {
    let p = Path::new(path);
    std::os::unix::fs::lchown(p, Some(uid), Some(gid)).map_err(|e| format!("{path}: {e}"))?;
    if !p.is_symlink() {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(())
}

/// "SIZE PATH" lines in a file (or on stdin): how many files are not that
/// size.
fn sizes(file: Option<&str>) -> Result<usize, String> {
    let input: Box<dyn BufRead> = match file {
        Some(f) => Box::new(std::io::BufReader::new(open(f)?)),
        None => Box::new(std::io::stdin().lock()),
    };
    let mut bad = 0;
    for line in input.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let Some((n, p)) = line.split_once(' ') else { continue };
        if std::fs::symlink_metadata(p).map(|m| m.len().to_string()).ok().as_deref() != Some(n) {
            bad += 1;
        }
    }
    Ok(bad)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> String {
        let d = std::env::temp_dir().join(format!("item-recovery-verbs-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join(name).display().to_string()
    }

    #[test]
    fn words_unescaped() {
        assert_eq!(unescape("/tmp/ud/a%20b%25c"), "/tmp/ud/a b%c");
        assert_eq!(unescape("50%"), "50%");
        assert_eq!(unescape("%2"), "%2");
        assert_eq!(unescape("/tmp/ud/%C3%A9"), "/tmp/ud/é");
    }

    #[test]
    fn size_hash_and_a_range() {
        let p = tmp("a");
        std::fs::write(&p, b"0123456789").unwrap();
        assert_eq!(size(&p).unwrap(), 10);
        assert_eq!(hash(&p, None).unwrap(), format!("{:x}", Sha256::digest(b"0123456789")));
        assert_eq!(hash(&p, Some((2, 3))).unwrap(), format!("{:x}", Sha256::digest(b"234")));
        assert_eq!(hash(&p, Some((8, 100))).unwrap(), format!("{:x}", Sha256::digest(b"89")));
        zero(&p, Some(4)).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"\0\0\0\0456789");
        zero(&p, None).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), [0u8; 10]);
    }

    /// `sparse` there, `unsparse` back: the same file, its holes holes.
    #[test]
    fn sparse_and_back() {
        let (src, dst) = (tmp("sp-src"), tmp("sp-dst"));
        let f = File::create(&src).unwrap();
        f.set_len(20 << 20).unwrap();
        std::os::unix::fs::FileExt::write_all_at(&f, &[7u8; 5000], (9 << 20) - 10).unwrap();
        std::os::unix::fs::FileExt::write_all_at(&f, &[9u8; 3], (20 << 20) - 3).unwrap();
        drop(f);
        let mut stream = Vec::new();
        sparse(&src, &mut stream).unwrap();
        let _ = std::fs::remove_file(&dst);
        // A piece that arrives different is refused.
        let mut bad = stream.clone();
        bad[20] ^= 1;
        assert!(unsparse(&dst, &mut &bad[..]).is_err());
        let _ = std::fs::remove_file(&dst);
        unsparse(&dst, &mut &stream[..]).unwrap();
        assert!(std::fs::read(&src).unwrap() == std::fs::read(&dst).unwrap());
    }

    #[test]
    fn unzstd_with_holes() {
        let mut data = vec![0u8; (9 << 20) + 123];
        data[10..20].copy_from_slice(b"0123456789");
        let n = data.len();
        data[n - 1] = 7;
        let z = zstd::encode_all(&data[..], 3).unwrap();
        let p = tmp("unz");
        assert_eq!(unzstd(&p, &mut &z[..]).unwrap(), data.len() as u64);
        assert!(std::fs::read(&p).unwrap() == data);
        // A stream cut short is refused.
        assert!(unzstd(&p, &mut &z[..z.len() / 2]).is_err());
    }

    #[test]
    fn hash_past_the_cache() {
        let p = std::env::current_dir().unwrap().join(format!("target-hash-direct-{}", std::process::id()));
        let data: Vec<u8> = (0..(5 << 20) + 123).map(|i| (i % 7) as u8).collect();
        std::fs::write(&p, &data).unwrap();
        let p = p.display().to_string();
        let got = hash_direct(&p);
        let _ = std::fs::remove_file(&p);
        // A filesystem without O_DIRECT (tmpfs) says so: nothing to see.
        if let Ok(h) = got {
            assert_eq!(h, format!("{:x}", Sha256::digest(&data)));
        }
        assert!(!discarded("/dev/null").unwrap());
    }

    #[test]
    fn copy_over_and_the_rest_zeroed() {
        let (src, dst) = (tmp("c-src"), tmp("c-dst"));
        std::fs::write(&src, b"new").unwrap();
        std::fs::write(&dst, b"old old old").unwrap();
        copy(&src, &dst).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"new\0\0\0\0\0\0\0\0");
        std::fs::write(&src, b"far too long for it").unwrap();
        assert!(copy(&src, &dst).is_err());
    }

    #[test]
    fn gz_in_members_reads_as_one() {
        let data: Vec<u8> = (0..(9 << 20)).map(|i| (i * 7 % 251) as u8).collect();
        let p = tmp("g");
        std::fs::write(&p, &data).unwrap();
        let mut out = Vec::new();
        gz(&mut open(&p).unwrap(), &mut out).unwrap();
        let mut back = Vec::new();
        flate2::read::MultiGzDecoder::new(&out[..]).read_to_end(&mut back).unwrap();
        assert_eq!(back.len(), data.len());
        assert!(back == data);
    }

    #[test]
    fn tar_there_and_back() {
        let src = tmp("src");
        std::fs::create_dir_all(format!("{src}/d/e")).unwrap();
        std::fs::write(format!("{src}/d/e/f"), b"x").unwrap();
        std::os::unix::fs::symlink("e/f", format!("{src}/d/l")).unwrap();
        let _socket = std::os::unix::net::UnixListener::bind(format!("{src}/d/s")).unwrap();
        let fifo = std::ffi::CString::new(format!("{src}/d/p")).unwrap();
        // SAFETY: a valid path.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o640) }, 0);
        let mut out = Vec::new();
        tar_gz(&src, &["d".into(), "missing".into()], &mut out).unwrap();
        let dst = tmp("dst");
        let mut tar = Vec::new();
        flate2::read::MultiGzDecoder::new(&out[..]).read_to_end(&mut tar).unwrap();
        let file = tmp("t.tar");
        std::fs::write(&file, &tar).unwrap();
        untar(&dst, Some(&file)).unwrap();
        assert_eq!(std::fs::read(format!("{dst}/d/e/f")).unwrap(), b"x");
        assert_eq!(std::fs::read_link(format!("{dst}/d/l")).unwrap(), Path::new("e/f"));
        assert!(!Path::new(&format!("{dst}/d/s")).exists());
        use std::os::unix::fs::FileTypeExt;
        let p = std::fs::symlink_metadata(format!("{dst}/d/p")).unwrap();
        assert!(p.file_type().is_fifo());
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(p.permissions().mode() & 0o777, 0o640);
    }
}
