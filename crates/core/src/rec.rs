//! What item/grid asks of a recovery on the phone: item-recovery's own
//! verbs, run by its adb daemon with no shell between (the recovery
//! crate's verbs.rs), or the shell line a TWRP takes (TWRP and the WOA
//! Project's headless images stay the fallback).

use crate::full::{adb_send, adb_shell};

pub(crate) struct Rec {
    serial: String,
    ours: bool,
}

impl Rec {
    pub(crate) fn on(serial: &str) -> Rec {
        Rec { serial: serial.to_owned(), ours: crate::full::ours(serial) }
    }

    pub(crate) fn ours(&self) -> bool {
        self.ours
    }

    /// `item-recovery VERB ARGS`: words split at spaces on the phone, any
    /// byte but a letter, a digit and `/._+=:,@-` written `%XX` (the
    /// recovery's adbd runs such a line straight, any other by its shell).
    pub(crate) fn line(args: &[&str]) -> Result<String, String> {
        if args.iter().any(|a| a.is_empty()) {
            return Err("an empty word for item-recovery".into());
        }
        let plain = |b: u8| b.is_ascii_alphanumeric() || b"/._+=:,@-".contains(&b);
        let esc = |a: &str| a.bytes().map(|b| if plain(b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
        Ok(format!("item-recovery {}", args.iter().map(|a| esc(a)).collect::<Vec<_>>().join(" ")))
    }

    /// The verb on item-recovery, the line on TWRP; what it printed.
    pub(crate) fn run(&self, verb: &[&str], twrp: &str) -> Result<String, String> {
        if self.ours {
            adb_shell(&self.serial, &Self::line(verb)?)
        } else {
            adb_shell(&self.serial, twrp)
        }
    }

    /// Data into a verb (exec-in), or into a TWRP's shell sink.
    pub(crate) fn send(&self, verb: &[&str], twrp_sink: &str, data: &mut dyn std::io::Read) -> Result<u64, String> {
        if self.ours {
            adb_send(&self.serial, &Self::line(verb)?, data)
        } else {
            adb_send(&self.serial, twrp_sink, data)
        }
    }

    pub(crate) fn size(&self, path: &str) -> Result<u64, String> {
        let said = self.run(&["size", path], &format!("blockdev --getsize64 {path} 2>/dev/null || stat -c %s {path}"))?;
        said.trim().parse().map_err(|_| format!("{path}'s size was not read ({})", said.trim()))
    }

    /// sha256 of the whole, or of `len` bytes from `at` (whole MBs on TWRP).
    pub(crate) fn hash(&self, path: &str, range: Option<(u64, u64)>) -> Result<String, String> {
        let said = match range {
            None => self.run(&["hash", path], &format!("sha256sum {path} | cut -d' ' -f1"))?,
            Some((at, len)) => self.run(&["hash", path, &at.to_string(), &len.to_string()], &format!("dd if={path} bs=1048576 skip={} count={} 2>/dev/null | sha256sum | cut -d' ' -f1", at >> 20, len.div_ceil(1 << 20)))?,
        };
        let h = said.trim().to_owned();
        if h.len() != 64 {
            return Err(format!("{path}'s sha256 was not read ({h})"));
        }
        Ok(h)
    }

    /// sha256 of the whole read from the flash itself (past the page
    /// cache, on item-recovery): the check after a write.
    pub(crate) fn hash_written(&self, path: &str) -> Result<String, String> {
        let said = self.run(&["hash", "--direct", path], &format!("sync; echo 3 > /proc/sys/vm/drop_caches; sha256sum {path} | cut -d' ' -f1"))?;
        let h = said.trim().to_owned();
        if h.len() != 64 {
            return Err(format!("{path}'s sha256 was not read ({h})"));
        }
        Ok(h)
    }

    /// Zeros over the first `len` bytes, or the whole; synced.
    pub(crate) fn zero(&self, path: &str, len: Option<u64>) -> Result<(), String> {
        match len {
            Some(n) => self.run(&["zero", path, &n.to_string()], &format!("dd if=/dev/zero of={path} bs={n} count=1 2>/dev/null; sync")),
            None => self.run(&["zero", path], &format!("dd if=/dev/zero of={path} bs=4194304 2>/dev/null; sync; true")),
        }
        .map(|_| ())
    }

    /// `dev` mounted on `dir` (made); read only and without the journal
    /// replayed when `ro`. A file goes through a loop device.
    pub(crate) fn mount(&self, dev: &str, dir: &str, ro: bool) -> Result<(), String> {
        if ro {
            self.run(&["mount", "-r", "-o", "noload", dev, dir], &format!("mkdir -p {dir} && (mountpoint -q {dir} || mount -t ext4 -o ro,noload {dev} {dir})"))
        } else {
            self.run(&["mount", dev, dir], &format!("mkdir -p {dir} && (mountpoint -q {dir} || mount {}{dev} {dir})", if dev.starts_with("/dev/") { "-t ext4 " } else { "-o loop,rw " }))
        }
        .map(|_| ())
    }

    pub(crate) fn umount(&self, dir: &str) -> Result<(), String> {
        self.run(&["umount", dir], &format!("sync; umount {dir} 2>/dev/null; true")).map(|_| ())
    }

    /// An empty ext4 labelled `label` made on `dev` (unmounted from `dir`
    /// first), then mounted there: item-recovery's own mkfs, a TWRP's
    /// mke2fs.
    pub(crate) fn make_ext4(&self, dev: &str, label: &str, dir: &str) -> Result<(), String> {
        self.umount(dir)?;
        self.run(&["mkfs", dev, label], &format!("mke2fs -F -t ext4 -L {label} {dev} >/dev/null"))?;
        self.mount(dev, dir, false)
    }

    /// `src` over the start of the partition `dev`, the rest of it zeroed.
    pub(crate) fn copy(&self, src: &str, dev: &str, len: u64) -> Result<(), String> {
        self.run(&["copy", src, dev], &format!("dd if={src} of={dev} bs=4194304 2>/dev/null; dd if=/dev/zero of={dev} bs=4096 seek={} 2>/dev/null; sync; true", len.div_ceil(4096))).map(|_| ())
    }

    /// Where a link (a partition by name) leads.
    pub(crate) fn real(&self, path: &str) -> Result<String, String> {
        Ok(self.run(&["real", path], &format!("readlink -f {path}"))?.trim().to_owned())
    }

    /// The phone restarted from the recovery (adb's own reboot service on
    /// item-recovery; a shell's reboot on TWRP). Not waited for.
    pub(crate) fn restart(&self) {
        if self.ours && crate::adbusb::present(&self.serial) {
            let _ = crate::adbusb::Conn::open(&self.serial).and_then(|mut c| c.reboot(""));
        } else {
            let _ = adb_shell(&self.serial, "reboot");
        }
    }

    pub(crate) fn mkdir(&self, dir: &str) -> Result<(), String> {
        self.run(&["mkdir", dir], &format!("mkdir -p '{dir}'")).map(|_| ())
    }

    pub(crate) fn rm(&self, path: &str) -> Result<(), String> {
        self.run(&["rm", path], &format!("rm -rf '{path}'")).map(|_| ())
    }

    /// Mode (octal), owner and group.
    pub(crate) fn meta(&self, path: &str, mode: u32, uid: u64, gid: u64) -> Result<(), String> {
        self.run(&["meta", path, &format!("{mode:o}"), &uid.to_string(), &gid.to_string()], &format!("chmod {mode:o} '{path}'; chown {uid}:{gid} '{path}'")).map(|_| ())
    }

    /// A small text file's contents.
    pub(crate) fn read_text(&self, path: &str) -> Result<String, String> {
        self.run(&["get", path], &format!("cat '{path}'"))
    }

    /// `data` as the whole of `path`, its mode and owner kept if it was
    /// there (made 0600 if not).
    pub(crate) fn write(&self, path: &str, data: &[u8]) -> Result<(), String> {
        if self.ours {
            return self.send(&["write", path], "", &mut &data[..]).map(|_| ());
        }
        let tmp = "/tmp/itemgrid-write";
        crate::full::push_file(&self.serial, data, tmp)?;
        adb_shell(&self.serial, &format!("cat {tmp} > '{path}'; rc=$?; rm -f {tmp}; exit $rc")).map(|_| ())
    }

    /// The bytes free under `dir` (root's).
    pub(crate) fn space(&self, dir: &str) -> Result<u64, String> {
        let said = self.run(&["space", dir], &format!("df -k '{dir}' | tail -1 | awk '{{print $4 * 1024}}'"))?;
        said.trim().parse().map_err(|_| format!("the space under {dir} was not read ({})", said.trim()))
    }

    /// The image `file`, mounted at `dir`, `more` bytes larger, its ext4
    /// grown into them while mounted (item-recovery's own; not on TWRP).
    pub(crate) fn grow(&self, file: &str, dir: &str, more: u64) -> Result<(), String> {
        if !self.ours {
            return Err(format!("{dir} has too little room, and TWRP is not asked to grow it"));
        }
        self.run(&["grow", file, dir, &more.to_string()], "").map(|_| ())
    }

    /// A tar unpacked under `root`, owners and modes kept.
    pub(crate) fn untar(&self, root: &str, tar: &[u8]) -> Result<(), String> {
        if self.ours {
            return self.send(&["untar", root], "", &mut &tar[..]).map(|_| ());
        }
        self.mkdir(root)?;
        // From a file, not a pipe: from a pipe TWRP's tar cut a 1.3 MB file
        // short and stopped there, quietly (2026-10-04).
        crate::full::push_file(&self.serial, tar, "/tmp/itemgrid-small.tar")?;
        adb_shell(&self.serial, &format!("tar -C '{root}' -xpf /tmp/itemgrid-small.tar; rc=$?; rm -f /tmp/itemgrid-small.tar; exit $rc")).map(|_| ())
    }

    /// "SIZE PATH" lines: how many of the files are not that size.
    pub(crate) fn check_sizes(&self, list: &str) -> Result<usize, String> {
        let file = "/tmp/itemgrid-sizes";
        self.write(file, list.as_bytes())?;
        let bad = self.run(&["sizes", file], &format!("n=0; while read s p; do [ \"$(stat -c %s \"$p\" 2>/dev/null)\" = \"$s\" ] || n=$((n+1)); done < {file}; echo $n"));
        self.rm(file)?;
        bad?.trim().parse().map_err(|_| "the sizes were not counted".into())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn words_escaped() {
        assert_eq!(super::Rec::line(&["put", "/tmp/ud/a b%c", "0"]).unwrap(), "item-recovery put /tmp/ud/a%20b%25c 0");
        assert_eq!(super::Rec::line(&["rm", "/tmp/ud/é"]).unwrap(), "item-recovery rm /tmp/ud/%C3%A9");
        assert_eq!(super::Rec::line(&["rm", "/tmp/a;b'c"]).unwrap(), "item-recovery rm /tmp/a%3Bb%27c");
    }
}
