//! The whole system backed up - the rootfs image and the Android container's
//! data on userdata - taken from TWRP, so it is consistent: the system is
//! not running. The phone goes into TWRP by a RAM boot (ramboot.rs, all its
//! checks), userdata is mounted read-only (noload: nothing replayed, nothing
//! written), each image is gzipped on the phone and streamed over adb, its
//! sha256 taken there and compared with the stream's here, decompressed; then
//! the phone goes back into Linux (recovery-exit). Kept: the last two.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::backup::{Backup, Item, Kind, Manifest};

/// The rest of userdata, in a full backup.
pub const REST: &str = "userdata-rest.tar.gz";
pub(crate) const UD: &str = "/dev/block/platform/soc/1d84000.ufshc/by-name/userdata";

/// The recovery item/grid boots from RAM to put a system on the phone:
/// its own (item-recovery.img: tools/build-recovery.sh, or a release's),
/// else TWRP for the Duo (surfaceduo1-twrp.img), as before. Each looked
/// for in ITEMGRID_RECOVERY / ITEMGRID_TWRP, this computer's item/grid
/// data, the port's tree, and the release images' folders (the newest).
pub fn recovery() -> Option<PathBuf> {
    let releases = |file: &str| -> Vec<PathBuf> {
        let mut rel: Vec<PathBuf> = crate::install::places().iter().filter_map(|d| std::fs::read_dir(d).ok()).flatten().flatten().map(|e| e.path().join(file)).collect();
        rel.sort();
        rel.reverse();
        rel
    };
    let mut places: Vec<PathBuf> = std::env::var_os("ITEMGRID_RECOVERY").map(PathBuf::from).into_iter().collect();
    places.push(crate::paths::data().join("recovery/item-recovery.img"));
    places.extend(releases("item-recovery.img"));
    places.extend(std::env::var_os("ITEMGRID_TWRP").map(PathBuf::from));
    if let Some(port) = crate::flash::port_tree() {
        places.push(port.join("out/twrp/surfaceduo1-twrp.img"));
    }
    places.push(crate::paths::data().join("twrp/surfaceduo1-twrp.img"));
    places.extend(releases("surfaceduo1-twrp.img"));
    places.into_iter().find(|p| p.exists())
}

/// Whether the recovery on the phone `serial` is item/grid's own (it
/// streams with exec-in; TWRP through nc and a forwarded port).
pub(crate) fn ours(serial: &str) -> bool {
    crate::adbusb::present(serial) || (crate::adbusb::platform() && adb_shell(serial, "item-recovery last").is_ok())
}

pub(crate) fn adb_shell(serial: &str, cmd: &str) -> Result<String, String> {
    // item-recovery: item/grid's own adb (adbusb.rs), the exit code its
    // shell's (v2).
    if crate::adbusb::present(serial) {
        let (out, err, code) = crate::adbusb::Conn::open(serial)?.shell(cmd, std::time::Duration::from_secs(3600))?;
        let text = String::from_utf8_lossy(&out).into_owned();
        if code != 0 {
            return Err(format!("in the recovery, '{cmd}': {}{}", text.trim(), String::from_utf8_lossy(&err).trim()));
        }
        return Ok(text);
    }
    let out = Command::new(crate::programs::adb()).args(["-s", serial, "shell", cmd]).stdin(Stdio::null()).output().map_err(|e| format!("adb: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        return Err(format!("in TWRP, '{cmd}': {}{}", text.trim(), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(text)
}

/// What one stream gave: its bytes' sha256, its size, and - for a gzip
/// stream - the size and sha256 of what it decompresses to.
type Streamed = (u64, String, Option<(u64, String)>);

/// Whether this TWRP has what the fast way needs: pigz (all eight cores) and
/// nc (a plain socket through adb forward, rather than adb's terminal).
/// Measured on the phone, a release build: the image's 16 GB of data at
/// ~40 MB/s as gzip, its empty rest at ~200 MB/s read; 18 minutes in all,
/// the phone's own sha256 of the image six of them. (A debug build of item/grid
/// was the bottleneck once: its decompressing and hashing ran at ~4 MB/s.)
pub(crate) fn fast_tools(serial: &str) -> bool {
    adb_shell(serial, "which pigz && which nc").is_ok_and(|o| o.lines().count() >= 2)
}

/// A command's output in TWRP streamed into `path`, the fast way: the
/// command's output on a socket (nc) in TWRP, adb forwarding it here.
fn adb_stream_fast(serial: &str, cmd: &str, path: &Path, gunzip: bool, say: &mut dyn FnMut(String)) -> Result<Streamed, String> {
    let port = 5599;
    let fwd = Command::new(crate::programs::adb()).args(["-s", serial, "forward", &format!("tcp:{port}"), &format!("tcp:{port}")]).output().map_err(|e| format!("adb forward: {e}"))?;
    if !fwd.status.success() {
        return Err(format!("adb forward: {}", String::from_utf8_lossy(&fwd.stderr).trim()));
    }
    let mut server = Command::new(crate::programs::adb()).args(["-s", serial, "shell", &format!("{cmd} | nc -l -p {port}")]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("adb: {e}"))?;
    // The listener needs a moment.
    let mut conn = None;
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        if let Ok(c) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            conn = Some(c);
            break;
        }
    }
    let result = match conn {
        Some(c) => stream_into(c, path, gunzip, say),
        None => Err("the stream's socket in TWRP did not open".into()),
    };
    let _ = server.kill();
    let _ = server.wait();
    let _ = Command::new(crate::programs::adb()).args(["-s", serial, "forward", "--remove", &format!("tcp:{port}")]).output();
    result
}

/// A command's output in TWRP streamed into `path`, through adb's terminal
/// (the fallback when TWRP lacks pigz or nc; single-threaded gzip).
/// How a stream is taken in: gzip as it comes (and gunzipped to check, if
/// so), or item-recovery's `sparse` records (sparse_into).
#[derive(Clone, Copy, PartialEq)]
enum Taken {
    Gzip { gunzip: bool },
    Sparse,
}

fn take_in(reader: impl Read, path: &Path, how: Taken, say: &mut dyn FnMut(String)) -> Result<Streamed, String> {
    match how {
        Taken::Gzip { gunzip } => stream_into(reader, path, gunzip, say),
        Taken::Sparse => sparse_into(reader, path, say),
    }
}

fn adb_stream(serial: &str, cmd: &str, path: &Path, how: Taken, say: &mut dyn FnMut(String)) -> Result<Streamed, String> {
    if crate::adbusb::present(serial) {
        // item-recovery: its exec stream read on a thread into a pipe.
        let (mut reader, mut writer) = std::io::pipe().map_err(|e| e.to_string())?;
        let (serial_t, cmd_t) = (serial.to_owned(), cmd.to_owned());
        let feeder = std::thread::spawn(move || -> Result<(), String> {
            let (serial, cmd) = (serial_t, cmd_t);
            use std::io::Write;
            let mut c = crate::adbusb::Conn::open(&serial)?;
            let mut failed = None;
            c.exec_out(&cmd, std::time::Duration::from_secs(4 * 3600), &mut |d| {
                if failed.is_none() {
                    if let Err(e) = writer.write_all(d) {
                        failed = Some(e.to_string());
                    }
                }
            })?;
            failed.map_or(Ok(()), Err)
        });
        let result = take_in(&mut reader, path, how, say);
        drop(reader);
        let fed = feeder.join().map_err(|_| "the stream's thread panicked".to_owned())?;
        let r = result.and_then(|r| fed.map(|_| r))?;
        // A verb's stream says how it ended apart from it.
        if cmd.starts_with("item-recovery ") {
            last(serial, cmd)?;
        }
        return Ok(r);
    }
    let mut child = Command::new(crate::programs::adb()).args(["-s", serial, "exec-out", cmd]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| format!("adb: {e}"))?;
    let out = child.stdout.take().expect("piped");
    let result = take_in(out, path, how, say);
    let status = child.wait().map_err(|e| e.to_string())?;
    if result.is_ok() && !status.success() {
        return Err(format!("the stream from TWRP failed ({status})"));
    }
    result
}

/// A stream into `path`, hashed; a gzip stream also decompressed on a
/// thread and what it gives hashed.
/// item-recovery's `sparse` stream (its verbs.rs) into `path` as one gzip
/// of the whole file: each piece's gzip written as it came, the holes as
/// gzip members of zeros made here. Each piece unpacked on a thread and
/// checked against its sha256; the whole file's sha256 is the phone's,
/// hashed there as it read (this computer's CPU hashes 86 GB of zeros in
/// five minutes, the phone's alongside its reading). What it gives is what
/// stream_into gives for the same file sent as gzip: the gzip's size and
/// sha256, the file's size and sha256.
fn sparse_into(mut input: impl Read, path: &Path, say: &mut dyn FnMut(String)) -> Result<Streamed, String> {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    enum Part {
        Zeros(u64),
        Piece(Vec<u8>, u32, [u8; 32]),
    }
    let (tx, rx) = std::sync::mpsc::sync_channel::<Part>(64);
    let checker = std::thread::spawn(move || -> Result<u64, String> {
        let mut size = 0u64;
        for part in rx {
            match part {
                Part::Zeros(n) => size += n,
                Part::Piece(gz, len, want) => {
                    let mut raw = Vec::with_capacity(len as usize);
                    flate2::read::GzDecoder::new(&gz[..]).read_to_end(&mut raw).map_err(|e| format!("a piece does not unpack: {e}"))?;
                    if raw.len() != len as usize || <[u8; 32]>::from(Sha256::digest(&raw)) != want {
                        return Err(format!("a piece at {size} arrived different from what is on the phone - try again"));
                    }
                    size += len as u64;
                }
            }
        }
        Ok(size)
    });
    // The gzip file as it is written: hashed and counted; zeros as gzip
    // members (one for 4 MB of them, made once, said again).
    struct Out<'a> {
        file: std::fs::File,
        path: &'a Path,
        hash: Sha256,
        size: u64,
        pos: u64,
        four: Vec<u8>,
    }
    fn member(n: usize) -> Vec<u8> {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        let _ = e.write_all(&vec![0u8; n]);
        e.finish().unwrap_or_default()
    }
    impl Out<'_> {
        fn put(&mut self, b: &[u8]) -> Result<(), String> {
            self.hash.update(b);
            self.size += b.len() as u64;
            self.file.write_all(b).map_err(|e| format!("{}: {e}", self.path.display()))
        }
        /// Zeros from where it is up to `to`.
        fn zeros_to(&mut self, to: u64, tx: &std::sync::mpsc::SyncSender<Part>) -> Result<(), String> {
            if to < self.pos {
                return Err("the stream went back".into());
            }
            let n = to - self.pos;
            for _ in 0..n / (4 << 20) {
                let four = std::mem::take(&mut self.four);
                self.put(&four)?;
                self.four = four;
            }
            if !n.is_multiple_of(4 << 20) {
                self.put(&member((n % (4 << 20)) as usize))?;
            }
            if n > 0 {
                tx.send(Part::Zeros(n)).map_err(|_| "the checker stopped")?;
            }
            self.pos = to;
            Ok(())
        }
    }
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Out { file, path, hash: Sha256::new(), size: 0, pos: 0, four: member(4 << 20) };
    let mut next_note = 4u64 << 30;
    let total = loop {
        let mut kind = [0u8; 1];
        input.read_exact(&mut kind).map_err(|e| format!("the stream ended early: {e}"))?;
        match kind[0] {
            b'D' => {
                let mut h = [0u8; 48];
                input.read_exact(&mut h).map_err(|e| format!("the stream ended early: {e}"))?;
                let at = u64::from_le_bytes(h[0..8].try_into().unwrap());
                let len = u32::from_le_bytes(h[8..12].try_into().unwrap());
                let glen = u32::from_le_bytes(h[12..16].try_into().unwrap()) as usize;
                let sha: [u8; 32] = h[16..48].try_into().unwrap();
                let mut gz = vec![0u8; glen];
                input.read_exact(&mut gz).map_err(|e| format!("the stream ended early: {e}"))?;
                out.zeros_to(at, &tx)?;
                out.put(&gz)?;
                tx.send(Part::Piece(gz, len, sha)).map_err(|_| "the checker stopped")?;
                out.pos += len as u64;
                if out.pos >= next_note {
                    say(format!("  {} read, {} as gzip", crate::status::size_words(out.pos / 1024), crate::status::size_words(out.size / 1024)));
                    next_note = out.pos + (4 << 30);
                }
            }
            b'E' => {
                let mut e = [0u8; 40];
                input.read_exact(&mut e).map_err(|e| format!("the stream ended early: {e}"))?;
                let sha: String = e[8..].iter().map(|b| format!("{b:02x}")).collect();
                break (u64::from_le_bytes(e[..8].try_into().unwrap()), sha);
            }
            k => return Err(format!("the stream is not item-recovery's sparse one ({k:#x})")),
        }
    };
    let (total, whole) = total;
    out.zeros_to(total, &tx)?;
    drop(tx);
    let put_together = checker.join().map_err(|_| "the checker stopped".to_owned())??;
    out.file.sync_all().map_err(|e| e.to_string())?;
    if put_together != total {
        return Err(format!("{put_together} bytes put together of {total}"));
    }
    Ok((out.size, format!("{:x}", out.hash.finalize()), Some((total, whole))))
}

fn stream_into(out: impl Read, path: &Path, gunzip: bool, say: &mut dyn FnMut(String)) -> Result<Streamed, String> {
    use sha2::{Digest, Sha256};
    // The stream to the file, hashed; a copy to a decoder thread, which
    // hashes what it decompresses to.
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(64);
    let decoder = gunzip.then(|| {
        std::thread::spawn(move || -> Result<(u64, String), String> {
            struct Chan(std::sync::mpsc::Receiver<Vec<u8>>, Vec<u8>, usize);
            impl Read for Chan {
                fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                    if self.2 >= self.1.len() {
                        match self.0.recv() {
                            Ok(v) => {
                                self.1 = v;
                                self.2 = 0;
                            }
                            Err(_) => return Ok(0),
                        }
                    }
                    let n = buf.len().min(self.1.len() - self.2);
                    buf[..n].copy_from_slice(&self.1[self.2..self.2 + n]);
                    self.2 += n;
                    Ok(n)
                }
            }
            let mut dec = flate2::read::MultiGzDecoder::new(Chan(rx, Vec::new(), 0));
            let mut hash = Sha256::new();
            let mut buf = vec![0u8; 1 << 20];
            let mut size = 0u64;
            loop {
                let n = dec.read(&mut buf).map_err(|e| format!("the stream does not decompress: {e}"))?;
                if n == 0 {
                    break;
                }
                hash.update(&buf[..n]);
                size += n as u64;
            }
            Ok((size, format!("{:x}", hash.finalize())))
        })
    });
    let mut file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buf = vec![0u8; 1 << 20];
    let mut reader = out;
    let mut next_note = 512u64 << 20;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("adb: {e}"))?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| format!("{}: {e}", path.display()))?;
        if gunzip {
            let _ = tx.send(buf[..n].to_vec());
        }
        size += n as u64;
        if size >= next_note {
            say(format!("  {} received", crate::status::size_words(size / 1024)));
            next_note += 512 << 20;
        }
    }
    drop(tx);
    file.sync_all().map_err(|e| e.to_string())?;
    let inner = match decoder {
        Some(d) => Some(d.join().map_err(|_| "the decoder stopped".to_owned())??),
        None => None,
    };
    Ok((size, format!("{:x}", hash.finalize()), inner))
}

/// The whole system backed up. The phone is in TWRP from about a minute in,
/// and back in Linux at the end; `say` hears each step.
pub fn take(host: &str, say: crate::ramboot::Say) -> Result<Backup, String> {
    crate::link::need_cable(host)?;
    let _ = crate::phone::keep_awake(host, true);
    let twrp = recovery().ok_or("no recovery image: tools/build-recovery.sh, or a release with item-recovery.img")?;
    let f = crate::backup::facts(host)?;
    // The fingers known now, to see them back after a way back.
    let fingers = crate::phone::fingers(host);
    // Room here: the used part of userdata, gzipped, with room to spare.
    let used_kib: u64 = crate::phone::run(host, "df -Pk /userdata | awk 'NR==2 {print $3}'\n")?.trim().parse().unwrap_or(0);
    let rootfs_used_kib: u64 = crate::phone::run(host, "df -Pk / | awk 'NR==2 {print $3}'\n")?.trim().parse().unwrap_or(0);
    let need = rootfs_used_kib * 1024 + (20 << 30);
    if crate::backup::free_here() < need {
        return Err(format!("not enough room here: about {} needed", crate::status::size_words(need / 1024)));
    }
    let _ = used_kib;
    let created = crate::backup::now();
    let stamp = created.replace([' ', ':', '-'], "");
    let dir = crate::backup::root().join(&f.serial).join(format!("{stamp}-full"));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }

    say("into TWRP, from RAM".into());
    crate::ramboot::ram_boot(host, &twrp, crate::ramboot::Expect::Recovery, say)?;
    let serial = f.serial.clone();
    let result = (|| -> Result<Vec<Item>, String> {
        say("mounting userdata read-only".into());
        let rec = crate::rec::Rec::on(&serial);
        rec.mount(UD, "/tmp/ud", true)?;
        let mut items = Vec::new();
        let fast = !rec.ours() && fast_tools(&serial);
        // item-recovery's verb (gzip on every core, by exec-out), or a
        // TWRP's pigz on a socket, or its gzip through adb's terminal.
        let stream = |verb: &[&str], cmd_fast: &str, cmd_slow: &str, path: &Path, say: &mut dyn FnMut(String)| {
            if rec.ours() {
                adb_stream(&serial, &crate::rec::Rec::line(verb)?, path, Taken::Gzip { gunzip: true }, say)
            } else if fast {
                adb_stream_fast(&serial, cmd_fast, path, true, say)
            } else {
                adb_stream(&serial, cmd_slow, path, Taken::Gzip { gunzip: true }, say)
            }
        };
        say(format!("taking rootfs.img ({})", if rec.ours() { "its data alone, gzip on every core" } else if fast { "pigz and a socket" } else { "gzip through adb's terminal: slow" }));
        let (size, raw_size, raw_sha) = if rec.ours() {
            // Its holes not read, each piece checked against the phone's
            // sha256 of it as it came (sparse_into): no second pass.
            let (size, _gz_sha, inner) = adb_stream(&serial, &crate::rec::Rec::line(&["sparse", "/tmp/ud/rootfs.img"])?, &dir.join("rootfs.img.gz"), Taken::Sparse, say)?;
            let (raw_size, raw_sha) = inner.expect("put together");
            (size, raw_size, raw_sha)
        } else {
            let (size, _gz_sha, inner) = stream(&["gz", "/tmp/ud/rootfs.img"], "pigz -1 -c /tmp/ud/rootfs.img", "gzip -1 -c /tmp/ud/rootfs.img", &dir.join("rootfs.img.gz"), say)?;
            let (raw_size, raw_sha) = inner.expect("gunzipped");
            say("hashing rootfs.img on the phone".into());
            if rec.hash("/tmp/ud/rootfs.img", None)? != raw_sha {
                return Err("rootfs.img arrived different from what is on the phone - try again".into());
            }
            (size, raw_size, raw_sha)
        };
        say(format!("rootfs.img: {} as gzip, {} whole, checked", crate::status::size_words(size / 1024), crate::status::size_words(raw_size / 1024)));
        items.push(Item { source: "/userdata/rootfs.img".into(), file: "rootfs.img.gz".into(), size, sha256: raw_sha });
        say("taking the Android container's data".into());
        let (size, sha, inner) = stream(&["tar", "/tmp/ud", "android-data"], "tar -C /tmp/ud -cf - android-data | pigz -1 -c", "tar -C /tmp/ud -cf - android-data | gzip -1 -c", &dir.join("android-data.tar.gz"), say)?;
        if inner.is_none_or(|(n, _)| n < 10240) {
            return Err("the Android container's data did not arrive whole".into());
        }
        items.push(Item { source: "/userdata/android-data".into(), file: "android-data.tar.gz".into(), size, sha256: sha });
        // The rest of userdata (a test image, logs, the symlink): nothing left
        // behind if userdata is erased - the return to Android does.
        let rest = rec.run(&["list", "/tmp/ud"], "cd /tmp/ud && ls -A")?;
        let names: Vec<&str> = rest.lines().map(str::trim).filter(|n| !n.is_empty() && !["rootfs.img", "android-data", "lost+found"].contains(n)).collect();
        if !names.is_empty() {
            say(format!("taking the rest of userdata: {}", names.join(", ")));
            let list = names.iter().map(|n| format!("'{n}'")).collect::<Vec<_>>().join(" ");
            let mut verb = vec!["tar", "/tmp/ud"];
            verb.extend(&names);
            let (size, sha, _) = stream(&verb, &format!("tar -C /tmp/ud -cf - {list} | pigz -1 -c"), &format!("tar -C /tmp/ud -cf - {list} | gzip -1 -c"), &dir.join(REST), say)?;
            items.push(Item { source: format!("/userdata: {}", names.join(" ")), file: REST.into(), size, sha256: sha });
        }
        let _ = rec.umount("/tmp/ud");
        Ok(items)
    })();
    // Back into Linux whatever happened in TWRP.
    let back = crate::ramboot::leave_recovery(host, &serial, say);
    let items = match result {
        Ok(items) => items,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(match back {
                Ok(()) => e,
                Err(b) => format!("{e}; and coming back: {b}"),
            });
        }
    };
    back?;
    let backup = Backup { dir, manifest: Manifest { kind: Kind::Full, created, serial: f.serial.clone(), slot: f.slot, item: f.item, port: f.port, kernel: f.kernel, items, keep: false, off_computer: false, fingers } };
    backup.save()?;
    say(format!("whole system backed up: {}", crate::status::size_words(backup.size() / 1024)));
    crate::backup::prune(&f.serial, Kind::Full, say);
    Ok(backup)
}

/// Data sent into TWRP: `sink` runs there reading it - nc starts it for the
/// connection (piping nc's output into it lost everything: TWRP's toybox nc
/// ends at once when its own input does) - and adb forwards the socket, so
/// nothing passes adb's terminal. Returns the bytes sent, once the sink has
/// finished and closed the connection.
pub(crate) fn adb_send(serial: &str, sink: &str, data: &mut dyn Read) -> Result<u64, String> {
    use std::io::Write;
    // A small send by adb push and the sink run on the file: through the
    // socket, a few kilobytes went missing (an ssh key, a 21 KB file -
    // 2026-10-04), while megabytes and gigabytes never did.
    const SMALL: usize = 4 << 20;
    // item-recovery's own verb: straight in by exec-in, its end asked of
    // item-recovery itself (`last`).
    if sink.starts_with("item-recovery ") {
        let n = if crate::adbusb::present(serial) {
            crate::adbusb::Conn::open(serial)?.exec_in(sink, data)?
        } else {
            let mut child = Command::new(crate::programs::adb()).args(["-s", serial, "exec-in", sink]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| format!("adb: {e}"))?;
            let mut stdin = child.stdin.take().ok_or("no stdin")?;
            let n = std::io::copy(data, &mut stdin).map_err(|e| format!("sending: {e}"));
            drop(stdin);
            let out = child.wait_with_output().map_err(|e| format!("adb: {e}"))?;
            if !out.status.success() {
                return Err(format!("adb exec-in: {}", String::from_utf8_lossy(&out.stderr).trim()));
            }
            n?
        };
        return last(serial, sink).map(|_| n);
    }
    let mut head = Vec::new();
    data.take(SMALL as u64 + 1).read_to_end(&mut head).map_err(|e| format!("reading: {e}"))?;
    if head.len() <= SMALL && crate::adbusb::present(serial) {
        crate::adbusb::Conn::open(serial)?.push(&head, "/tmp/itemgrid-send", 0o600)?;
        adb_shell(serial, &format!("cat /tmp/itemgrid-send | {sink}; rc=$?; rm -f /tmp/itemgrid-send; exit $rc"))?;
        return Ok(head.len() as u64);
    }
    if head.len() <= SMALL {
        let tmp = std::env::temp_dir().join(format!("itemgrid-send-{}", std::process::id()));
        std::fs::write(&tmp, &head).map_err(|e| e.to_string())?;
        let out = Command::new(crate::programs::adb()).args(["-s", serial, "push"]).arg(&tmp).arg("/tmp/itemgrid-send").stdin(Stdio::null()).output().map_err(|e| format!("adb: {e}"));
        let _ = std::fs::remove_file(&tmp);
        let out = out?;
        if !out.status.success() {
            return Err(format!("adb push: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        adb_shell(serial, &format!("cat /tmp/itemgrid-send | {sink}; rc=$?; rm -f /tmp/itemgrid-send; exit $rc"))?;
        return Ok(head.len() as u64);
    }
    let mut data = std::io::Cursor::new(head).chain(data);
    let data: &mut dyn Read = &mut data;
    // item/grid's own recovery: straight into the sink (adb exec-in); its
    // exit is not told that way, so the sink says it at the end.
    if ours(serial) {
        let _ = adb_shell(serial, "rm -f /tmp/itemgrid-sink-rc");
        if crate::adbusb::present(serial) {
            let n = crate::adbusb::Conn::open(serial)?.exec_in(&format!("{sink}; echo $? > /tmp/itemgrid-sink-rc"), data)?;
            return wait_sink_rc(serial, n);
        }
        let mut child = Command::new(crate::programs::adb())
            .args(["-s", serial, "exec-in", &format!("{sink}; echo $? > /tmp/itemgrid-sink-rc")])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("adb: {e}"))?;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        let n = std::io::copy(data, &mut stdin).map_err(|e| format!("sending: {e}"));
        drop(stdin);
        let out = child.wait_with_output().map_err(|e| format!("adb: {e}"))?;
        let n = n?;
        if !out.status.success() {
            return Err(format!("adb exec-in: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        return wait_sink_rc(serial, n);
    }
    let port = 5598u16;
    let fwd = Command::new(crate::programs::adb()).args(["-s", serial, "forward", &format!("tcp:{port}"), &format!("tcp:{port}")]).output().map_err(|e| format!("adb forward: {e}"))?;
    if !fwd.status.success() {
        return Err(format!("adb forward: {}", String::from_utf8_lossy(&fwd.stderr).trim()));
    }
    let quoted = sink.replace('\'', "'\\''");
    let mut server = Command::new(crate::programs::adb())
        .args(["-s", serial, "shell", &format!("nc -l -p {port} sh -c '{quoted}'")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("adb: {e}"))?;
    let result = (|| -> Result<u64, String> {
        // adb's end accepts at once, listener or not: wait for the listener.
        let listening = format!("grep -qi ':{port:04X} 00000000:0000 0A' /proc/net/tcp || grep -qi ':{port:04X} 00000000000000000000000000000000:0000 0A' /proc/net/tcp6");
        let start = std::time::Instant::now();
        while adb_shell(serial, &listening).is_err() {
            if start.elapsed() > std::time::Duration::from_secs(15) {
                return Err("the receiving socket in TWRP did not open".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let mut c = std::net::TcpStream::connect(("127.0.0.1", port)).map_err(|e| format!("connecting: {e}"))?;
        let n = std::io::copy(data, &mut c).map_err(|e| format!("sending: {e}"))?;
        c.flush().map_err(|e| e.to_string())?;
        c.shutdown(std::net::Shutdown::Write).map_err(|e| e.to_string())?;
        // The sink done: the connection closes from its end.
        c.set_read_timeout(Some(std::time::Duration::from_secs(600))).map_err(|e| e.to_string())?;
        let mut rest = Vec::new();
        c.read_to_end(&mut rest).map_err(|e| format!("waiting for the receiving end: {e}"))?;
        Ok(n)
    })();
    let _ = server.kill();
    let _ = server.wait();
    let _ = Command::new(crate::programs::adb()).args(["-s", serial, "forward", "--remove", &format!("tcp:{port}")]).output();
    result
}

/// The stream closes before the sink is done: its exit waited for (it
/// writes it at its end); `n` the bytes sent, if it said 0.
/// How item-recovery's verb run by exec (in or out) ended: waited for,
/// its stderr the error if it failed.
pub(crate) fn last(serial: &str, cmd: &str) -> Result<(), String> {
    let start = std::time::Instant::now();
    loop {
        let said = adb_shell(serial, "item-recovery last")?;
        let (code, err) = said.split_once('\n').unwrap_or((said.as_str(), ""));
        match code.trim() {
            "" => {}
            "0" => return Ok(()),
            c => return Err(format!("in the recovery, '{cmd}' ended {c}: {}", err.trim())),
        }
        if start.elapsed() > std::time::Duration::from_secs(600) {
            return Err(format!("in the recovery, '{cmd}' did not end in 10 minutes"));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

fn wait_sink_rc(serial: &str, n: u64) -> Result<u64, String> {
    let start = std::time::Instant::now();
    loop {
        // (true: no file yet is no error.)
        let rc = adb_shell(serial, "cat /tmp/itemgrid-sink-rc 2>/dev/null; true")?;
        if !rc.trim().is_empty() {
            let _ = adb_shell(serial, "rm -f /tmp/itemgrid-sink-rc");
            if rc.trim() != "0" {
                return Err(format!("the receiving end said {}", rc.trim()));
            }
            return Ok(n);
        }
        if start.elapsed() > std::time::Duration::from_secs(600) {
            return Err("the receiving end did not finish in 10 minutes".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// Bytes put at `to` in TWRP as a file, by adb push.
pub(crate) fn push_file(serial: &str, data: &[u8], to: &str) -> Result<(), String> {
    if crate::adbusb::present(serial) {
        return crate::adbusb::Conn::open(serial)?.push(data, to, 0o644);
    }
    let tmp = std::env::temp_dir().join(format!("itemgrid-push-{}-{}", std::process::id(), data.len()));
    std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
    let out = Command::new(crate::programs::adb()).args(["-s", serial, "push"]).arg(&tmp).arg(to).stdin(Stdio::null()).output().map_err(|e| format!("adb: {e}"));
    let _ = std::fs::remove_file(&tmp);
    let out = out?;
    if !out.status.success() {
        return Err(format!("adb push: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

#[cfg(test)]
mod sparse_tests {
    use std::io::{Read, Seek, SeekFrom, Write};

    /// A sparse file through item-recovery's `sparse` (the built program,
    /// if it is there) and sparse_into: the gzip unpacks to the file, the
    /// sizes and sha256s are the file's.
    #[test]
    fn a_sparse_file_there_and_back() {
        use sha2::{Digest, Sha256};
        let bin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/item-recovery");
        if !bin.exists() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("itemgrid-sparse-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.img");
        let mut f = std::fs::File::create(&src).unwrap();
        f.set_len(100 << 20).unwrap();
        // Data at the start, in the middle across a 4 MB line, a zero
        // piece written (not a hole), data at the very end.
        for (at, byte, len) in [(0u64, 1u8, 5000usize), ((40 << 20) - 100, 2, 300), (60 << 20, 0, 4 << 20), ((100 << 20) - 10, 3, 10)] {
            f.seek(SeekFrom::Start(at)).unwrap();
            f.write_all(&vec![byte; len]).unwrap();
        }
        drop(f);
        let out = std::process::Command::new(&bin).arg("sparse").arg(&src).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let gz = dir.join("out.gz");
        let (gz_size, _, inner) = super::sparse_into(&out.stdout[..], &gz, &mut |_| {}).unwrap();
        let want = std::fs::read(&src).unwrap();
        let (n, sha) = inner.unwrap();
        assert_eq!(n, want.len() as u64);
        assert_eq!(sha, format!("{:x}", Sha256::digest(&want)));
        let mut back = Vec::new();
        flate2::read::MultiGzDecoder::new(std::fs::File::open(&gz).unwrap()).read_to_end(&mut back).unwrap();
        assert!(back == want);
        // The holes and the zero piece cost next to nothing.
        assert!(gz_size < 1 << 20, "{gz_size}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
