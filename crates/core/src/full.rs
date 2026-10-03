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

const UD: &str = "/dev/block/platform/soc/1d84000.ufshc/by-name/userdata";

/// TWRP's image: CRADLE_TWRP, or the port's out/twrp/, or Cradle's own
/// ~/.local/share/cradle/twrp/.
pub fn twrp() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let mut places: Vec<PathBuf> = std::env::var_os("CRADLE_TWRP").map(PathBuf::from).into_iter().collect();
    if let Some(port) = crate::flash::port_tree() {
        places.push(port.join("out/twrp/surfaceduo1-twrp.img"));
    }
    places.push(home.join(".local/share/cradle/twrp/surfaceduo1-twrp.img"));
    places.into_iter().find(|p| p.exists())
}

fn adb_shell(serial: &str, cmd: &str) -> Result<String, String> {
    let out = Command::new("adb").args(["-s", serial, "shell", cmd]).stdin(Stdio::null()).output().map_err(|e| format!("adb: {e}"))?;
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
/// nc (a plain socket, through adb forward, rather than adb's terminal,
/// which carried ~4 MB/s).
fn fast_tools(serial: &str) -> bool {
    adb_shell(serial, "which pigz && which nc").is_ok_and(|o| o.lines().count() >= 2)
}

/// A command's output in TWRP streamed into `path`, the fast way: the
/// command's output on a socket (nc) in TWRP, adb forwarding it here.
fn adb_stream_fast(serial: &str, cmd: &str, path: &Path, gunzip: bool, say: &mut dyn FnMut(String)) -> Result<Streamed, String> {
    let port = 5599;
    let fwd = Command::new("adb").args(["-s", serial, "forward", &format!("tcp:{port}"), &format!("tcp:{port}")]).output().map_err(|e| format!("adb forward: {e}"))?;
    if !fwd.status.success() {
        return Err(format!("adb forward: {}", String::from_utf8_lossy(&fwd.stderr).trim()));
    }
    let mut server = Command::new("adb").args(["-s", serial, "shell", &format!("{cmd} | nc -l -p {port}")]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("adb: {e}"))?;
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
    let _ = Command::new("adb").args(["-s", serial, "forward", "--remove", &format!("tcp:{port}")]).output();
    result
}

/// A command's output in TWRP streamed into `path`, through adb's terminal
/// (slow: ~4 MB/s; the fallback when TWRP lacks pigz or nc).
fn adb_stream(serial: &str, cmd: &str, path: &Path, gunzip: bool, say: &mut dyn FnMut(String)) -> Result<Streamed, String> {
    let mut child = Command::new("adb").args(["-s", serial, "exec-out", cmd]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| format!("adb: {e}"))?;
    let out = child.stdout.take().expect("piped");
    let result = stream_into(out, path, gunzip, say);
    let status = child.wait().map_err(|e| e.to_string())?;
    if result.is_ok() && !status.success() {
        return Err(format!("the stream from TWRP failed ({status})"));
    }
    result
}

/// A stream into `path`, hashed; a gzip stream also decompressed on a
/// thread and what it gives hashed.
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
    let twrp = twrp().ok_or("no TWRP image: put surfaceduo1-twrp.img in ~/.local/share/cradle/twrp/")?;
    let f = crate::backup::facts(host)?;
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
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }

    say("into TWRP, from RAM".into());
    crate::ramboot::ram_boot(host, &twrp, crate::ramboot::Expect::Recovery, say)?;
    let serial = f.serial.clone();
    let result = (|| -> Result<Vec<Item>, String> {
        say("mounting userdata read-only".into());
        adb_shell(&serial, &format!("mkdir -p /tmp/ud && (mountpoint -q /tmp/ud || mount -t ext4 -o ro,noload {UD} /tmp/ud) && ls /tmp/ud"))?;
        let mut items = Vec::new();
        let fast = fast_tools(&serial);
        let stream = |cmd_fast: &str, cmd_slow: &str, path: &Path, say: &mut dyn FnMut(String)| {
            if fast { adb_stream_fast(&serial, cmd_fast, path, true, say) } else { adb_stream(&serial, cmd_slow, path, true, say) }
        };
        say(format!("taking rootfs.img ({})", if fast { "pigz and a socket" } else { "gzip through adb's terminal: slow" }));
        let (size, _gz_sha, inner) = stream("pigz -1 -c /tmp/ud/rootfs.img", "gzip -1 -c /tmp/ud/rootfs.img", &dir.join("rootfs.img.gz"), say)?;
        let (raw_size, raw_sha) = inner.expect("gunzipped");
        say("hashing rootfs.img on the phone".into());
        let there = adb_shell(&serial, "sha256sum /tmp/ud/rootfs.img | cut -d' ' -f1")?;
        if there.trim() != raw_sha {
            return Err("rootfs.img arrived different from what is on the phone - try again".into());
        }
        say(format!("rootfs.img: {} as gzip, {} whole, checked", crate::status::size_words(size / 1024), crate::status::size_words(raw_size / 1024)));
        items.push(Item { source: "/userdata/rootfs.img".into(), file: "rootfs.img.gz".into(), size, sha256: raw_sha });
        say("taking the Android container's data".into());
        let (size, sha, inner) = stream("tar -C /tmp/ud -cf - android-data | pigz -1 -c", "tar -C /tmp/ud -cf - android-data | gzip -1 -c", &dir.join("android-data.tar.gz"), say)?;
        if inner.is_none_or(|(n, _)| n < 10240) {
            return Err("the Android container's data did not arrive whole".into());
        }
        items.push(Item { source: "/userdata/android-data".into(), file: "android-data.tar.gz".into(), size, sha256: sha });
        let _ = adb_shell(&serial, "umount /tmp/ud");
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
    let backup = Backup { dir, manifest: Manifest { kind: Kind::Full, created, serial: f.serial.clone(), slot: f.slot, item: f.item, port: f.port, kernel: f.kernel, items, keep: false, off_computer: false } };
    backup.save()?;
    say(format!("whole system backed up: {}", crate::status::size_words(backup.size() / 1024)));
    crate::backup::prune(&f.serial, Kind::Full, say);
    Ok(backup)
}
