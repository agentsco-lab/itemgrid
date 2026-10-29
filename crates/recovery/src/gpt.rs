//! `gpt [--write] DEV`: Windows taken off the Duo's table - the WOA
//! Project's `esp` and `win` dropped, userdata grown back over their room
//! to the disk's end, as the phone left the factory. What their uninstall
//! FFU writes (OEMEP_Uninstall.ffu: the table alone, no partition's
//! contents), but worked out from the disk itself, so a 256 GB Duo gets
//! its whole disk too and a Duo 2's rawdump and vm-data stay where they
//! are. Without --write the plan alone is shown. userdata must be made
//! anew afterwards (its old start is gone).
//!
//! The backup table is written first, the primary last: cut off midway,
//! the primary still holds the old table whole.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

/// The partitions Windows adds (their Partitioning guides).
const WINDOWS: [&str; 2] = ["esp", "win"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub raw: [u8; 128],
}

impl Entry {
    fn used(&self) -> bool {
        self.raw[..16] != [0; 16]
    }
    pub fn first(&self) -> u64 {
        u64::from_le_bytes(self.raw[32..40].try_into().unwrap())
    }
    pub fn last(&self) -> u64 {
        u64::from_le_bytes(self.raw[40..48].try_into().unwrap())
    }
    fn set_range(&mut self, first: u64, last: u64) {
        self.raw[32..40].copy_from_slice(&first.to_le_bytes());
        self.raw[40..48].copy_from_slice(&last.to_le_bytes());
    }
    pub fn name(&self) -> String {
        let units: Vec<u16> = self.raw[56..128].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
        String::from_utf16_lossy(&units)
    }
}

/// A disk's table: its sector size, the primary header and the entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub sector: u64,
    pub header: Vec<u8>,
    pub entries: Vec<Entry>,
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

impl Table {
    fn header_size(&self) -> usize {
        u32_at(&self.header, 12) as usize
    }
    pub fn alternate(&self) -> u64 {
        u64_at(&self.header, 32)
    }
    pub fn last_usable(&self) -> u64 {
        u64_at(&self.header, 48)
    }
    fn entry_size(&self) -> usize {
        u32_at(&self.header, 84) as usize
    }
    /// Sectors the entries take.
    fn entry_sectors(&self) -> u64 {
        ((self.entries.len() * self.entry_size()) as u64).div_ceil(self.sector)
    }
    fn entry_bytes(&self) -> Vec<u8> {
        let size = self.entry_size();
        let mut b = Vec::with_capacity(self.entries.len() * size);
        for e in &self.entries {
            b.extend_from_slice(&e.raw);
            b.resize(b.len() + size - 128, 0);
        }
        b
    }

    /// The header with its checksums, as written at `at` with its other
    /// copy at `other` and the entries at `entries`.
    fn header_for(&self, at: u64, other: u64, entries: u64) -> Vec<u8> {
        let mut h = self.header.clone();
        h[24..32].copy_from_slice(&at.to_le_bytes());
        h[32..40].copy_from_slice(&other.to_le_bytes());
        h[72..80].copy_from_slice(&entries.to_le_bytes());
        h[88..92].copy_from_slice(&crc32(&self.entry_bytes()).to_le_bytes());
        h[16..20].copy_from_slice(&[0; 4]);
        let crc = crc32(&h[..self.header_size()]);
        h[16..20].copy_from_slice(&crc.to_le_bytes());
        h
    }

    /// Read from a disk (or a disk's image), its checksums checked.
    pub fn read(disk: &mut (impl Read + Seek)) -> Result<Table, String> {
        for sector in [4096u64, 512] {
            let mut h = vec![0u8; sector as usize];
            disk.seek(SeekFrom::Start(sector)).map_err(|e| e.to_string())?;
            if disk.read_exact(&mut h).is_err() || &h[..8] != b"EFI PART" {
                continue;
            }
            let hsize = u32_at(&h, 12) as usize;
            if !(92..=sector as usize).contains(&hsize) {
                return Err(format!("the table's header says {hsize} bytes"));
            }
            let mut z = h[..hsize].to_vec();
            z[16..20].copy_from_slice(&[0; 4]);
            if crc32(&z) != u32_at(&h, 16) {
                return Err("the table's header fails its checksum".into());
            }
            let (count, size) = (u32_at(&h, 80) as usize, u32_at(&h, 84) as usize);
            if size < 128 || count == 0 || count > 1024 {
                return Err(format!("the table says {count} entries of {size} bytes"));
            }
            let mut raw = vec![0u8; count * size];
            disk.seek(SeekFrom::Start(u64_at(&h, 72) * sector)).map_err(|e| e.to_string())?;
            disk.read_exact(&mut raw).map_err(|e| format!("the entries: {e}"))?;
            if crc32(&raw) != u32_at(&h, 88) {
                return Err("the table's entries fail their checksum".into());
            }
            let entries = raw.chunks(size).map(|c| Entry { raw: c[..128].try_into().unwrap() }).collect();
            return Ok(Table { sector, header: h, entries });
        }
        Err("no GPT on it".into())
    }

    /// The partitions in use, in the table's order.
    pub fn used(&self) -> impl Iterator<Item = (usize, &Entry)> {
        self.entries.iter().enumerate().filter(|(_, e)| e.used())
    }

    /// The table with Windows' partitions gone and userdata over their
    /// room, up to the disk's end; None when there is no Windows on it.
    pub fn without_windows(&self) -> Result<Option<Table>, String> {
        if !self.used().any(|(_, e)| WINDOWS.contains(&e.name().as_str())) {
            return Ok(None);
        }
        let mut kept: Vec<Entry> = self.used().map(|(_, e)| e.clone()).filter(|e| !WINDOWS.contains(&e.name().as_str())).collect();
        kept.sort_by_key(Entry::first);
        let ud = kept.iter().position(|e| e.name() == "userdata").ok_or("no userdata on it")?;
        if ud + 1 != kept.len() {
            return Err(format!("{} lies after userdata: not the layout WOA's guides make", kept[ud + 1].name()));
        }
        let start = if ud == 0 { u64_at(&self.header, 40) } else { kept[ud - 1].last() + 1 };
        // Only Windows' partitions between the one before and userdata.
        for (_, e) in self.used() {
            if WINDOWS.contains(&e.name().as_str()) && (e.first() < start || e.last() >= kept[ud].first()) {
                return Err(format!("{} is not between {} and userdata", e.name(), kept.get(ud.wrapping_sub(1)).map_or("the table's start".into(), Entry::name)));
            }
        }
        kept[ud].set_range(start, self.last_usable());
        let mut t = self.clone();
        for (i, slot) in t.entries.iter_mut().enumerate() {
            *slot = kept.get(i).cloned().unwrap_or(Entry { raw: [0; 128] });
        }
        Ok(Some(t))
    }

    /// Both copies written: the backup's entries and header, then the
    /// primary's, each synced.
    pub fn write(&self, disk: &mut File) -> Result<(), String> {
        let alt = self.alternate();
        let backup_entries = alt - self.entry_sectors();
        let entries = self.entry_bytes();
        let at = |lba: u64| SeekFrom::Start(lba * self.sector);
        let mut put = |lba: u64, b: &[u8]| -> Result<(), String> {
            disk.seek(at(lba)).map_err(|e| e.to_string())?;
            disk.write_all(b).map_err(|e| format!("writing at sector {lba}: {e}"))?;
            disk.sync_data().map_err(|e| e.to_string())
        };
        put(backup_entries, &entries)?;
        put(alt, &self.header_for(alt, 1, backup_entries))?;
        put(2, &entries)?;
        put(1, &self.header_for(1, alt, 2))
    }

    pub fn lines(&self) -> Vec<String> {
        let size = |b: u64| if b >= 1_000_000_000 { format!("{:.1} GB", b as f64 / 1e9) } else { format!("{:.1} MB", b as f64 / 1e6) };
        self.used().map(|(i, e)| format!("{:>3} {:<10} {:>10} - {:<10} {:>10}", i + 1, e.name(), e.first(), e.last(), size((e.last() + 1 - e.first()) * self.sector))).collect()
    }
}

/// CRC-32 (IEEE), as GPT has it.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}

/// `gpt [--write] DEV`: 0 done (or planned), 1 no Windows there, 2 failed.
pub fn command(args: &[String]) -> i32 {
    let write = args.iter().any(|a| a == "--write");
    let Some(dev) = args.iter().find(|a| !a.starts_with('-')) else {
        eprintln!("gpt [--write] DEV");
        return 2;
    };
    let run = || -> Result<bool, String> {
        let mut disk = std::fs::OpenOptions::new().read(true).write(write).open(dev).map_err(|e| format!("{dev}: {e}"))?;
        let now = Table::read(&mut disk)?;
        // What this module writes, made from what it read, must be what is
        // there: else its writing is not to be trusted on this disk.
        let hsize = now.header_size();
        if now.header_for(1, now.alternate(), 2)[..hsize] != now.header[..hsize] {
            return Err("the header made again differs from the one read: not writing on this disk".into());
        }
        println!("now:");
        now.lines().iter().for_each(|l| println!("{l}"));
        let Some(plan) = now.without_windows()? else {
            println!("no Windows on {dev}: nothing to do");
            return Ok(false);
        };
        println!("without Windows:");
        plan.lines().iter().for_each(|l| println!("{l}"));
        if !write {
            return Ok(true);
        }
        plan.write(&mut disk)?;
        let back = Table::read(&mut disk)?;
        if back.entries != plan.entries {
            return Err("read back, the table differs from what was written".into());
        }
        reread(&disk);
        drop(disk);
        println!("written, {} partitions by name again; userdata is to be made anew", crate::init::relink());
        Ok(true)
    };
    match run() {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(e) => {
            eprintln!("gpt: {e}");
            2
        }
    }
}

/// The kernel told to read the table again (BLKRRPART), for a block
/// device; nothing for an image.
fn reread(disk: &File) {
    use std::os::unix::io::AsRawFd;
    const BLKRRPART: libc::c_ulong = 0x125f;
    // SAFETY: an ioctl without an argument on an open descriptor.
    if unsafe { libc::ioctl(disk.as_raw_fd(), BLKRRPART as _) } != 0 {
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::ENOTTY) {
            println!("the kernel did not read the table again ({e}): a restart will");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATA: [u8; 16] = [0xe6, 0xe7, 0x81, 0x1b, 0x0d, 0xf5, 0x9b, 0x41, 0xa7, 0x39, 0x2a, 0xee, 0xf8, 0xda, 0x33, 0x35];

    fn entry(name: &str, first: u64, last: u64) -> Entry {
        let mut raw = [0u8; 128];
        raw[..16].copy_from_slice(&DATA);
        raw[16] = name.len() as u8;
        let mut e = Entry { raw };
        e.set_range(first, last);
        for (i, u) in name.encode_utf16().enumerate() {
            e.raw[56 + 2 * i..58 + 2 * i].copy_from_slice(&u.to_le_bytes());
        }
        e
    }

    /// A table as OEMEP_Uninstall.ffu (a 128 GB Duo) has its header.
    fn table(parts: &[(&str, u64, u64)]) -> Table {
        let mut header = vec![0u8; 4096];
        header[..8].copy_from_slice(b"EFI PART");
        header[8..12].copy_from_slice(&0x10000u32.to_le_bytes());
        header[12..16].copy_from_slice(&92u32.to_le_bytes());
        header[40..48].copy_from_slice(&6u64.to_le_bytes());
        header[48..56].copy_from_slice(&27276282u64.to_le_bytes());
        header[80..84].copy_from_slice(&32u32.to_le_bytes());
        header[84..88].copy_from_slice(&128u32.to_le_bytes());
        let mut entries: Vec<Entry> = parts.iter().map(|&(n, f, l)| entry(n, f, l)).collect();
        entries.resize(32, Entry { raw: [0; 128] });
        let mut t = Table { sector: 4096, header, entries };
        t.header = t.header_for(1, 27276287, 2);
        t
    }

    const HEAD: [(&str, u64, u64); 5] = [("ssd", 6, 7), ("persist", 8, 8199), ("metadata", 8200, 12295), ("frp", 12296, 12423), ("misc", 12424, 12679)];

    #[test]
    fn crc32_known() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn windows_taken_off_as_the_ffu_has_it() {
        let mut woa = HEAD.to_vec();
        woa.extend([("esp", 12680, 164550), ("win", 164551, 16942079), ("userdata", 16942080, 26855423)]);
        let mut stock = HEAD.to_vec();
        stock.push(("userdata", 12680, 27276282));
        let plan = table(&woa).without_windows().unwrap().unwrap();
        assert_eq!(plan.entries, table(&stock).entries);
        assert_eq!(table(&stock).without_windows().unwrap(), None);
    }

    #[test]
    fn written_and_read_back() {
        let mut woa = HEAD.to_vec();
        woa.extend([("esp", 12680, 164550), ("win", 164551, 16942079), ("userdata", 16942080, 26855423)]);
        let path = std::env::temp_dir().join(format!("item-recovery-gpt-{}", std::process::id()));
        let mut f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
        f.set_len(27276288 * 4096).unwrap();
        table(&woa).write(&mut f).unwrap();
        let read = Table::read(&mut f).unwrap();
        assert_eq!(read, table(&woa));
        let plan = read.without_windows().unwrap().unwrap();
        plan.write(&mut f).unwrap();
        assert_eq!(Table::read(&mut f).unwrap().entries, plan.entries);
        drop(f);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn not_woa_left_alone() {
        let mut odd = HEAD.to_vec();
        odd.extend([("userdata", 12680, 20000000), ("win", 20000001, 27276282)]);
        assert!(table(&odd).without_windows().is_err());
    }
}
