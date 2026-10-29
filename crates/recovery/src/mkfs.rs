//! `mkfs DEV [LABEL]`: an empty ext4 made on DEV, in place of e2fsprogs'
//! mke2fs. What the release's mke2fs.conf makes for ext4, less what the
//! port's 4.14 kernel does not know: 4 KB blocks, 256-byte inodes, an
//! inode for every 16 KB, 5 % kept for root; extents, 64bit, flex_bg (16),
//! metadata_csum (crc32c), a journal, dir_nlink, extra_isize, sparse_super,
//! huge_file, ext_attr, dir_index. Not orphan_file, metadata_csum_seed,
//! resize_inode.
//!
//! Only the metadata is written: the superblock and its copies, the group
//! descriptors, the bitmaps, the first block of inodes, the root and
//! lost+found, the journal's superblock and its extent block. The rest of
//! the inode tables is marked unused (itable_unused), the kernel zeroing
//! it in the background as after mke2fs' lazy_itable_init.

use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};

const BS: u64 = 4096;
const BPG: u64 = 32768;
const INODE_SIZE: u64 = 256;
const INODE_RATIO: u64 = 16384;
const FLEX: u64 = 16;
const DESC: u64 = 64;
const FIRST_INO: u32 = 11;
const ROOT: u32 = 2;
const JOURNAL: u32 = 8;
const LOST: u32 = 11;
const LOST_BLOCKS: u64 = 4;

/// crc32c (Castagnoli) as ext4 runs it: no inversion before or after.
pub fn crc32c(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0x82F6_3B78 & (crc & 1).wrapping_neg());
        }
    }
    crc
}

fn put16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn put32be(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_be_bytes());
}

/// The filesystem's shape for a device of `bytes`.
#[derive(Debug, Clone)]
pub struct Shape {
    pub blocks: u64,
    pub groups: u64,
    pub ipg: u64,
    pub itable: u64,
    pub gdt: u64,
    pub journal: u64,
}

impl Shape {
    pub fn of(bytes: u64) -> Result<Shape, String> {
        let mut blocks = bytes / BS;
        // A last group too small for its own bitmaps' worth is dropped, as
        // mke2fs does.
        if !blocks.is_multiple_of(BPG) && blocks % BPG < 64 {
            blocks -= blocks % BPG;
        }
        if blocks < 4 * BPG {
            return Err(format!("{bytes} bytes is too small for this mkfs (it makes userdata)"));
        }
        let groups = blocks.div_ceil(BPG);
        // Inodes per group: by the ratio, a whole block of them, at most
        // what a bitmap block holds.
        let per_block = BS / INODE_SIZE;
        let ipg = ((blocks * BS / INODE_RATIO).div_ceil(groups)).div_ceil(per_block) * per_block;
        let ipg = ipg.clamp(per_block, BS * 8);
        let journal = match blocks {
            b if b < 32768 => 1024,
            b if b < 256 * 1024 => 4096,
            b if b < 512 * 1024 => 8192,
            b if b < 4096 * 1024 => 16384,
            b if b < 8192 * 1024 => 32768,
            b if b < 16384 * 1024 => 65536,
            b if b < 32768 * 1024 => 131072,
            _ => 262144,
        };
        Ok(Shape { blocks, groups, ipg, itable: ipg * INODE_SIZE / BS, gdt: (groups * DESC).div_ceil(BS), journal })
    }

    fn group_start(&self, g: u64) -> u64 {
        g * BPG
    }
    fn group_len(&self, g: u64) -> u64 {
        (self.blocks - g * BPG).min(BPG)
    }
    /// sparse_super: copies in groups 0, 1 and powers of 3, 5 and 7.
    fn has_super(g: u64) -> bool {
        if g <= 1 {
            return true;
        }
        [3u64, 5, 7].iter().any(|&p| {
            let mut n = p;
            while n < g {
                n *= p;
            }
            n == g
        })
    }
}

/// Where each group's metadata went.
struct Layout {
    bbitmap: Vec<u64>,
    ibitmap: Vec<u64>,
    itable: Vec<u64>,
    /// One bit a block: in use.
    used: Vec<u64>,
}

impl Layout {
    fn take(&mut self, b: u64) {
        self.used[(b / 64) as usize] |= 1 << (b % 64);
    }
    fn taken(&self, b: u64) -> bool {
        self.used[(b / 64) as usize] & (1 << (b % 64)) != 0
    }
    /// `n` free blocks in a row from `goal` on.
    fn run(&mut self, goal: u64, n: u64, end: u64) -> Result<u64, String> {
        let mut start = goal;
        while start + n <= end {
            match (start..start + n).find(|&b| self.taken(b)) {
                Some(b) => start = b + 1,
                None => {
                    (start..start + n).for_each(|b| self.take(b));
                    return Ok(start);
                }
            }
        }
        Err(format!("no {n} free blocks in a row"))
    }
}

fn lay_out(s: &Shape) -> Result<Layout, String> {
    let mut l = Layout { bbitmap: vec![0; s.groups as usize], ibitmap: vec![0; s.groups as usize], itable: vec![0; s.groups as usize], used: vec![0; s.blocks.div_ceil(64) as usize] };
    for g in 0..s.groups {
        if Shape::has_super(g) {
            (0..1 + s.gdt).for_each(|i| l.take(s.group_start(g) + i));
        }
    }
    // flex_bg: a flex group's bitmaps and tables together in its first
    // group(s), as mke2fs puts them.
    for f in 0..s.groups.div_ceil(FLEX) {
        let gs: Vec<u64> = (f * FLEX..((f + 1) * FLEX).min(s.groups)).collect();
        let goal = s.group_start(gs[0]);
        let n = gs.len() as u64;
        let b = l.run(goal, n, s.blocks)?;
        let i = l.run(b + n, n, s.blocks)?;
        let t = l.run(i + n, n * s.itable, s.blocks)?;
        for (k, &g) in gs.iter().enumerate() {
            let k = k as u64;
            l.bbitmap[g as usize] = b + k;
            l.ibitmap[g as usize] = i + k;
            l.itable[g as usize] = t + k * s.itable;
        }
    }
    Ok(l)
}

/// An inode's 256 bytes, its checksum set.
#[allow(clippy::too_many_arguments)]
fn inode(seed: u32, ino: u32, mode: u16, links: u16, size: u64, blocks512: u64, extents: Option<[u8; 60]>, now: u32) -> Vec<u8> {
    let mut i = vec![0u8; INODE_SIZE as usize];
    put16(&mut i, 0, mode);
    put32(&mut i, 4, size as u32);
    if mode != 0 {
        for at in [8, 12, 16] {
            put32(&mut i, at, now);
        }
        put32(&mut i, 144, now);
    }
    put16(&mut i, 26, links);
    put32(&mut i, 28, blocks512 as u32);
    put16(&mut i, 116, (blocks512 >> 32) as u16);
    if let Some(e) = extents {
        put32(&mut i, 32, 0x80000); // EXT4_EXTENTS_FL
        i[40..100].copy_from_slice(&e);
    }
    put32(&mut i, 108, (size >> 32) as u32);
    put16(&mut i, 128, 32); // i_extra_isize
    let mut c = crc32c(seed, &ino.to_le_bytes());
    c = crc32c(c, &0u32.to_le_bytes());
    c = crc32c(c, &i);
    put16(&mut i, 124, c as u16);
    put16(&mut i, 130, (c >> 16) as u16);
    i
}

/// An inline extent header with `entries` (depth 0: extents, 1: an index).
fn extents(depth: u16, entries: &[(u32, u16, u64)]) -> [u8; 60] {
    let mut e = [0u8; 60];
    put16(&mut e, 0, 0xF30A);
    put16(&mut e, 2, entries.len() as u16);
    put16(&mut e, 4, 4);
    put16(&mut e, 6, depth);
    for (k, &(lblk, len, start)) in entries.iter().enumerate() {
        let at = 12 + 12 * k;
        put32(&mut e, at, lblk);
        if depth == 0 {
            put16(&mut e, at + 4, len);
            put16(&mut e, at + 6, (start >> 32) as u16);
            put32(&mut e, at + 8, start as u32);
        } else {
            put32(&mut e, at + 4, start as u32);
            put16(&mut e, at + 8, (start >> 32) as u16);
        }
    }
    e
}

/// A directory block's entries ((inode, name, type)), the last stretched
/// to the checksum tail, which is set.
fn dir_block(seed: u32, ino: u32, entries: &[(u32, &str, u8)]) -> Vec<u8> {
    let mut b = vec![0u8; BS as usize];
    let mut at = 0usize;
    let room = BS as usize - 12;
    for (k, &(i, name, t)) in entries.iter().enumerate() {
        let len = if k + 1 == entries.len() { room - at } else { (8 + name.len()).div_ceil(4) * 4 };
        put32(&mut b, at, i);
        put16(&mut b, at + 4, len as u16);
        b[at + 6] = name.len() as u8;
        b[at + 7] = t;
        b[at + 8..at + 8 + name.len()].copy_from_slice(name.as_bytes());
        at += len;
    }
    if entries.is_empty() {
        put16(&mut b, 4, room as u16);
    }
    // The tail: an empty entry of 12 bytes, file type 0xDE, the checksum.
    put16(&mut b, room + 4, 12);
    b[room + 7] = 0xDE;
    let mut c = crc32c(seed, &ino.to_le_bytes());
    c = crc32c(c, &0u32.to_le_bytes());
    c = crc32c(c, &b[..room]);
    put32(&mut b, room + 8, c);
    b
}

fn random16() -> [u8; 16] {
    let mut u = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = std::io::Read::read_exact(&mut f, &mut u);
    }
    u[6] = (u[6] & 0x0F) | 0x40;
    u[8] = (u[8] & 0x3F) | 0x80;
    u
}

/// The filesystem made on `dev` (a device or an image file).
pub fn make(dev: &str, label: &str) -> Result<Shape, String> {
    let mut f = OpenOptions::new().read(true).write(true).open(dev).map_err(|e| format!("{dev}: {e}"))?;
    let bytes = f.seek(SeekFrom::End(0)).map_err(|e| format!("{dev}: {e}"))?;
    let s = Shape::of(bytes)?;
    let mut l = lay_out(&s)?;
    let uuid = random16();
    let seed = crc32c(!0, &uuid);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as u32;

    // The root's block and lost+found's just past group 0's metadata; the
    // journal and its extent block from the middle on, as mke2fs.
    let root_blk = l.run(0, 1, s.blocks)?;
    let lost_blk = l.run(root_blk, LOST_BLOCKS, s.blocks)?;
    let jgoal = s.group_start(s.groups / 2);
    let jleaf = l.run(jgoal, 1, s.blocks)?;
    let jstart = l.run(jleaf + 1, s.journal, s.blocks)?;

    let w = |f: &mut std::fs::File, block: u64, data: &[u8]| -> Result<(), String> {
        f.seek(SeekFrom::Start(block * BS)).map_err(|e| e.to_string())?;
        f.write_all(data).map_err(|e| format!("writing block {block}: {e}"))
    };

    // The inodes: 1-10 reserved, 2 the root, 8 the journal, 11 lost+found.
    let mut table = vec![0u8; (BS * s.itable.min(1)) as usize];
    let jext: Vec<(u32, u16, u64)> = (0..s.journal.div_ceil(32768)).map(|k| (k as u32 * 32768, (s.journal - k * 32768).min(32768) as u16, jstart + k * 32768)).collect();
    let jroot = if jext.len() <= 4 { extents(0, &jext) } else { extents(1, &[(0, 0, jleaf)]) };
    for ino in 1..=FIRST_INO {
        let i = match ino {
            ROOT => inode(seed, ino, 0o40755, 3, BS, 8, Some(extents(0, &[(0, 1, root_blk)])), now),
            JOURNAL => inode(seed, ino, 0o100600, 1, s.journal * BS, (s.journal + (jext.len() > 4) as u64) * 8, Some(jroot), now),
            LOST => inode(seed, ino, 0o40700, 2, LOST_BLOCKS * BS, LOST_BLOCKS * 8, Some(extents(0, &[(0, LOST_BLOCKS as u16, lost_blk)])), now),
            _ => inode(seed, ino, 0, 0, 0, 0, None, now),
        };
        let at = ((ino - 1) as u64 * INODE_SIZE) as usize;
        table[at..at + INODE_SIZE as usize].copy_from_slice(&i);
    }
    w(&mut f, l.itable[0], &table)?;

    // The directories.
    w(&mut f, root_blk, &dir_block(seed, ROOT, &[(ROOT, ".", 2), (ROOT, "..", 2), (LOST, "lost+found", 2)]))?;
    w(&mut f, lost_blk, &dir_block(seed, LOST, &[(LOST, ".", 2), (ROOT, "..", 2)]))?;
    for k in 1..LOST_BLOCKS {
        w(&mut f, lost_blk + k, &dir_block(seed, LOST, &[]))?;
    }

    // The journal: its extent block (if it needs one) and its superblock
    // (empty: s_start 0, nothing to replay).
    if jext.len() > 4 {
        let mut leaf = vec![0u8; BS as usize];
        let max = ((BS - 12) / 12) as u16;
        put16(&mut leaf, 0, 0xF30A);
        put16(&mut leaf, 2, jext.len() as u16);
        put16(&mut leaf, 4, max);
        for (k, &(lblk, len, start)) in jext.iter().enumerate() {
            let at = 12 + 12 * k;
            put32(&mut leaf, at, lblk);
            put16(&mut leaf, at + 4, len);
            put16(&mut leaf, at + 6, (start >> 32) as u16);
            put32(&mut leaf, at + 8, start as u32);
        }
        let tail = 12 + 12 * max as usize;
        let mut c = crc32c(seed, &JOURNAL.to_le_bytes());
        c = crc32c(c, &0u32.to_le_bytes());
        c = crc32c(c, &leaf[..tail]);
        put32(&mut leaf, tail, c);
        w(&mut f, jleaf, &leaf)?;
    } else {
        // Not needed: given back.
        l.used[(jleaf / 64) as usize] &= !(1 << (jleaf % 64));
    }
    let mut js = vec![0u8; BS as usize];
    put32be(&mut js, 0, 0xC03B_3998);
    put32be(&mut js, 4, 4); // superblock v2
    put32be(&mut js, 12, BS as u32);
    put32be(&mut js, 16, s.journal as u32);
    put32be(&mut js, 20, 1); // s_first
    put32be(&mut js, 24, 1); // s_sequence
    js[48..64].copy_from_slice(&uuid);
    put32be(&mut js, 64, 1); // nr_users
    js[256..272].copy_from_slice(&uuid);
    w(&mut f, jstart, &js)?;

    // The bitmaps and the descriptors.
    let mut gdt = vec![0u8; (s.gdt * BS) as usize];
    let (mut free_blocks, mut free_inodes) = (0u64, 0u64);
    for g in 0..s.groups {
        let (start, len) = (s.group_start(g), s.group_len(g));
        let mut bb = vec![0u8; BS as usize];
        let mut free = 0u64;
        for k in 0..BPG {
            if k >= len || l.taken(start + k) {
                bb[(k / 8) as usize] |= 1 << (k % 8);
            } else {
                free += 1;
            }
        }
        let mut ib = vec![0u8; BS as usize];
        let used_inodes = if g == 0 { FIRST_INO as u64 } else { 0 };
        for k in 0..BS * 8 {
            if k < used_inodes || k >= s.ipg {
                ib[(k / 8) as usize] |= 1 << (k % 8);
            }
        }
        w(&mut f, l.bbitmap[g as usize], &bb)?;
        w(&mut f, l.ibitmap[g as usize], &ib)?;
        let bcsum = crc32c(seed, &bb[..(BPG / 8) as usize]);
        let icsum = crc32c(seed, &ib[..(s.ipg / 8) as usize]);
        let d = &mut gdt[(g * DESC) as usize..((g + 1) * DESC) as usize];
        let (bbm, ibm, it) = (l.bbitmap[g as usize], l.ibitmap[g as usize], l.itable[g as usize]);
        let free_i = s.ipg - used_inodes;
        put32(d, 0, bbm as u32);
        put32(d, 4, ibm as u32);
        put32(d, 8, it as u32);
        put16(d, 12, free as u16);
        put16(d, 14, free_i as u16);
        put16(d, 16, if g == 0 { 2 } else { 0 });
        put16(d, 24, bcsum as u16);
        put16(d, 26, icsum as u16);
        put16(d, 28, free_i as u16); // itable_unused
        put32(d, 32, (bbm >> 32) as u32);
        put32(d, 36, (ibm >> 32) as u32);
        put32(d, 40, (it >> 32) as u32);
        put16(d, 44, (free >> 16) as u16);
        put16(d, 46, (free_i >> 16) as u16);
        put16(d, 50, (free_i >> 16) as u16);
        put16(d, 56, (bcsum >> 16) as u16);
        put16(d, 58, (icsum >> 16) as u16);
        let mut c = crc32c(seed, &(g as u32).to_le_bytes());
        c = crc32c(c, d);
        put16(d, 30, c as u16);
        free_blocks += free;
        free_inodes += free_i;
    }

    // The superblock, and its copies with their group's number.
    let mut sb = vec![0u8; 1024];
    let inodes = s.ipg * s.groups;
    put32(&mut sb, 0, inodes as u32);
    put32(&mut sb, 4, s.blocks as u32);
    put32(&mut sb, 8, (s.blocks / 20) as u32);
    put32(&mut sb, 12, free_blocks as u32);
    put32(&mut sb, 16, free_inodes as u32);
    put32(&mut sb, 24, 2); // 4 KB
    put32(&mut sb, 28, 2);
    put32(&mut sb, 32, BPG as u32);
    put32(&mut sb, 36, BPG as u32);
    put32(&mut sb, 40, s.ipg as u32);
    put32(&mut sb, 48, now);
    put16(&mut sb, 54, 0xFFFF); // no count-based checks
    put16(&mut sb, 56, 0xEF53);
    put16(&mut sb, 58, 1); // clean
    put16(&mut sb, 60, 1); // errors: continue
    put32(&mut sb, 64, now);
    put32(&mut sb, 76, 1); // dynamic revision
    put32(&mut sb, 84, FIRST_INO);
    put16(&mut sb, 88, INODE_SIZE as u16);
    put32(&mut sb, 92, 0x4 | 0x8 | 0x20); // has_journal, ext_attr, dir_index
    put32(&mut sb, 96, 0x2 | 0x40 | 0x80 | 0x200); // filetype, extent, 64bit, flex_bg
    put32(&mut sb, 100, 0x1 | 0x2 | 0x8 | 0x20 | 0x40 | 0x400); // sparse_super, large_file, huge_file, dir_nlink, extra_isize, metadata_csum
    sb[104..120].copy_from_slice(&uuid);
    let name = label.as_bytes();
    sb[120..120 + name.len().min(16)].copy_from_slice(&name[..name.len().min(16)]);
    put32(&mut sb, 224, JOURNAL);
    let hash = random16();
    sb[236..252].copy_from_slice(&hash);
    sb[252] = 1; // half_md4
    sb[253] = 1; // the journal's blocks kept here
    put16(&mut sb, 254, DESC as u16);
    put32(&mut sb, 256, 0x4 | 0x8); // user_xattr, acl
    put32(&mut sb, 264, now);
    sb[268..328].copy_from_slice(&jroot);
    put32(&mut sb, 328, ((s.journal * BS) >> 32) as u32);
    put32(&mut sb, 332, (s.journal * BS) as u32);
    put32(&mut sb, 336, (s.blocks >> 32) as u32);
    put32(&mut sb, 340, ((s.blocks / 20) >> 32) as u32);
    put32(&mut sb, 344, (free_blocks >> 32) as u32);
    put16(&mut sb, 348, 32);
    put16(&mut sb, 350, 32);
    put32(&mut sb, 352, if cfg!(target_arch = "aarch64") { 2 } else { 1 }); // the hash's char signedness
    sb[372] = 4; // log2 of the flex group
    sb[373] = 1; // crc32c
    for g in (0..s.groups).filter(|&g| Shape::has_super(g)) {
        let mut copy = sb.clone();
        put16(&mut copy, 90, g as u16);
        let c = crc32c(!0, &copy[..1020]);
        put32(&mut copy, 1020, c);
        let start = s.group_start(g);
        let mut first = vec![0u8; BS as usize];
        let at = if g == 0 { 1024 } else { 0 };
        first[at..at + 1024].copy_from_slice(&copy);
        w(&mut f, start, &first)?;
        w(&mut f, start + 1, &gdt)?;
    }
    f.sync_all().map_err(|e| format!("{dev}: {e}"))?;
    Ok(s)
}

/// `mkfs DEV [LABEL]`.
pub fn command(args: &[String]) -> Result<(), String> {
    let dev = args.first().ok_or("mkfs DEV [LABEL]")?;
    let s = make(dev, args.get(1).map_or("", String::as_str))?;
    println!("ext4 on {dev}: {} blocks of 4 KB, {} groups, {} inodes, a journal of {} MB", s.blocks, s.groups, s.ipg * s.groups, (s.journal * BS) >> 20);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32c_known() {
        assert_eq!(!crc32c(!0, b"123456789"), 0xE306_9283);
    }

    #[test]
    fn copies_where_mke2fs_puts_them() {
        let with: Vec<u64> = (0..800).filter(|&g| Shape::has_super(g)).collect();
        assert_eq!(with, [0, 1, 3, 5, 7, 9, 25, 27, 49, 81, 125, 243, 343, 625, 729]);
    }

    /// Made on an image as big as the Duo's userdata (sparse), then
    /// checked by e2fsck, if this computer has it.
    #[test]
    fn e2fsck_finds_nothing() {
        for bytes in [(27276283u64 - 12680) * 4096, 2 << 30, (5 << 30) + 4096 * 1000] {
            let p = std::env::temp_dir().join(format!("item-recovery-mkfs-{}-{bytes}", std::process::id()));
            let f = std::fs::File::create(&p).unwrap();
            f.set_len(bytes).unwrap();
            drop(f);
            make(p.to_str().unwrap(), "userdata").unwrap();
            let e2fsck = ["/usr/sbin/e2fsck", "/sbin/e2fsck"].into_iter().find(|p| std::path::Path::new(p).exists());
            let out = e2fsck.map(|e| std::process::Command::new(e).args(["-fn"]).arg(&p).output());
            let _ = std::fs::remove_file(&p);
            let Some(Ok(out)) = out else { return };
            assert!(out.status.success(), "{bytes}: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        }
    }
}
