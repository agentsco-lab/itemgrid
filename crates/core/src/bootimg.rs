//! Android boot images, read and written by item/grid itself, in place of
//! AOSP's mkbootimg and unpack_bootimg (Python, vendored in the port): the
//! Duo's ABL takes header version 2, and item/grid builds item-recovery's
//! image from the port's and checks every image before it goes to the
//! phone.
//!
//! The layout (system/tools/mkbootimg, boot_img_hdr_v0..v2): one page of
//! header, then the kernel, the ramdisk, the second stage, the recovery
//! DTBO (v1+) and the DTB (v2), each padded to the page size. The header's
//! id is mkbootimg's: SHA-1 over each part followed by its size (a part
//! that is absent counts as its size alone), padded to 32 bytes - so an
//! image read and written again comes out byte for byte the same.

const MAGIC: &[u8; 8] = b"ANDROID!";
const NAME: usize = 16;
const ARGS: usize = 512;
const EXTRA_ARGS: usize = 1024;
/// The header's size by version: v0 ends at the extra command line, v1
/// adds the recovery DTBO and the header's own size, v2 the DTB.
const HEADER_V0: usize = 1632;
const HEADER_V1: usize = 1648;
const HEADER_V2: usize = 1660;

/// A boot image: its header's fields and its parts.
#[derive(Clone, Debug, PartialEq)]
pub struct BootImage {
    pub header_version: u32,
    pub page_size: u32,
    pub os_version: u32,
    pub kernel_addr: u32,
    pub ramdisk_addr: u32,
    pub second_addr: u32,
    pub tags_addr: u32,
    pub dtb_addr: u64,
    pub name: [u8; NAME],
    /// The whole command line: the header's two fields joined.
    pub cmdline: String,
    pub kernel: Vec<u8>,
    pub ramdisk: Vec<u8>,
    pub second: Vec<u8>,
    pub recovery_dtbo: Vec<u8>,
    pub dtb: Vec<u8>,
}

fn u32_at(b: &[u8], o: usize) -> Result<u32, String> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| "the header is cut short".to_owned())
}

fn u64_at(b: &[u8], o: usize) -> Result<u64, String> {
    b.get(o..o + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| "the header is cut short".to_owned())
}

fn text(b: &[u8]) -> String {
    let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

fn pages(n: usize, page: usize) -> usize {
    n.div_ceil(page) * page
}

impl BootImage {
    /// An image from its bytes; versions 0 to 2 (3 and on are laid out
    /// otherwise, and the Duo's ABL does not take them).
    pub fn parse(b: &[u8]) -> Result<BootImage, String> {
        if b.get(..8) != Some(MAGIC.as_slice()) {
            return Err("no ANDROID! boot magic".into());
        }
        let header_version = u32_at(b, 40)?;
        if header_version > 2 {
            return Err(format!("header version {header_version}: only 0 to 2 are read here"));
        }
        let page_size = u32_at(b, 36)?;
        if !(2048..=65536).contains(&page_size) || !page_size.is_power_of_two() {
            return Err(format!("page size {page_size}"));
        }
        let page = page_size as usize;
        let (ksize, rsize, ssize) = (u32_at(b, 8)? as usize, u32_at(b, 16)? as usize, u32_at(b, 24)? as usize);
        let (dtbo_size, dtb_size) = match header_version {
            0 => (0, 0),
            1 => (u32_at(b, 1632)? as usize, 0),
            _ => (u32_at(b, 1632)? as usize, u32_at(b, 1648)? as usize),
        };
        let mut at = page;
        let mut part = |size: usize| -> Result<Vec<u8>, String> {
            let p = b.get(at..at + size).ok_or("the image is shorter than its header says")?.to_vec();
            at += pages(size, page);
            Ok(p)
        };
        let kernel = part(ksize)?;
        let ramdisk = part(rsize)?;
        let second = part(ssize)?;
        let recovery_dtbo = part(dtbo_size)?;
        let dtb = part(dtb_size)?;
        let cmdline = text(&b[64..64 + ARGS]) + &text(&b[608..608 + EXTRA_ARGS]);
        Ok(BootImage {
            header_version,
            page_size,
            os_version: u32_at(b, 44)?,
            kernel_addr: u32_at(b, 12)?,
            ramdisk_addr: u32_at(b, 20)?,
            second_addr: u32_at(b, 28)?,
            tags_addr: u32_at(b, 32)?,
            dtb_addr: if header_version >= 2 { u64_at(b, 1652)? } else { 0 },
            name: b[48..48 + NAME].try_into().unwrap(),
            cmdline,
            kernel,
            ramdisk,
            second,
            recovery_dtbo,
            dtb,
        })
    }

    /// mkbootimg's id: SHA-1 over each part and its size.
    fn id(&self) -> [u8; 32] {
        use sha1::{Digest, Sha1};
        let mut h = Sha1::new();
        let mut put = |p: &[u8]| {
            h.update(p);
            h.update((p.len() as u32).to_le_bytes());
        };
        put(&self.kernel);
        put(&self.ramdisk);
        put(&self.second);
        if self.header_version > 0 {
            put(&self.recovery_dtbo);
        }
        if self.header_version > 1 {
            put(&self.dtb);
        }
        let mut id = [0u8; 32];
        id[..20].copy_from_slice(&h.finalize());
        id
    }

    /// The image's bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let page = self.page_size as usize;
        let cmd = self.cmdline.as_bytes();
        if cmd.len() > ARGS - 1 + EXTRA_ARGS - 1 {
            return Err(format!("the command line is {} bytes, more than the header holds", cmd.len()));
        }
        let (args, extra) = cmd.split_at(cmd.len().min(ARGS - 1));
        let header_len = match self.header_version {
            0 => HEADER_V0,
            1 => HEADER_V1,
            2 => HEADER_V2,
            v => return Err(format!("header version {v}: only 0 to 2 are written here")),
        };
        let mut h = vec![0u8; header_len];
        let put32 = |h: &mut Vec<u8>, o: usize, v: u32| h[o..o + 4].copy_from_slice(&v.to_le_bytes());
        h[..8].copy_from_slice(MAGIC);
        put32(&mut h, 8, self.kernel.len() as u32);
        put32(&mut h, 12, self.kernel_addr);
        put32(&mut h, 16, self.ramdisk.len() as u32);
        put32(&mut h, 20, self.ramdisk_addr);
        put32(&mut h, 24, self.second.len() as u32);
        put32(&mut h, 28, self.second_addr);
        put32(&mut h, 32, self.tags_addr);
        put32(&mut h, 36, self.page_size);
        put32(&mut h, 40, self.header_version);
        put32(&mut h, 44, self.os_version);
        h[48..48 + NAME].copy_from_slice(&self.name);
        h[64..64 + args.len()].copy_from_slice(args);
        h[576..608].copy_from_slice(&self.id());
        h[608..608 + extra.len()].copy_from_slice(extra);
        if self.header_version >= 1 {
            put32(&mut h, 1632, self.recovery_dtbo.len() as u32);
            // Where the recovery DTBO starts in the file, none: 0.
            let dtbo_at = if self.recovery_dtbo.is_empty() {
                0
            } else {
                (page + pages(self.kernel.len(), page) + pages(self.ramdisk.len(), page) + pages(self.second.len(), page)) as u64
            };
            h[1636..1644].copy_from_slice(&dtbo_at.to_le_bytes());
            put32(&mut h, 1644, header_len as u32);
        }
        if self.header_version >= 2 {
            put32(&mut h, 1648, self.dtb.len() as u32);
            h[1652..1660].copy_from_slice(&self.dtb_addr.to_le_bytes());
        }
        let mut out = h;
        out.resize(page, 0);
        for p in [&self.kernel, &self.ramdisk, &self.second, &self.recovery_dtbo, &self.dtb] {
            out.extend_from_slice(p);
            out.resize(pages(out.len(), page), 0);
        }
        Ok(out)
    }

    /// The os version as Android writes it: "11.0.0" (the patch level
    /// apart).
    pub fn os_version_text(&self) -> String {
        let os = self.os_version;
        format!("{}.{}.{}", os >> 25, (os >> 18) & 0x7f, (os >> 11) & 0x7f)
    }

    /// The patch level: "2022-09".
    pub fn patch_level(&self) -> String {
        let p = self.os_version & 0x7ff;
        format!("{}-{:02}", 2000 + (p >> 4), p & 0xf)
    }

    /// A word of the command line's, after `key=`.
    pub fn cmdline_value(&self, key: &str) -> Option<&str> {
        self.cmdline.split_whitespace().find_map(|w| w.strip_prefix(key).and_then(|v| v.strip_prefix('=')))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(version: u32) -> BootImage {
        BootImage {
            header_version: version,
            page_size: 4096,
            os_version: (11 << 25) | (2022 - 2000) << 4 | 9,
            kernel_addr: 0x8000,
            ramdisk_addr: 0x0100_0000,
            second_addr: 0,
            tags_addr: 0x100,
            dtb_addr: if version >= 2 { 0x0100_0000 } else { 0 },
            name: [0; NAME],
            cmdline: format!("androidboot.hardware=surfaceduo {}", "x".repeat(600)),
            kernel: vec![1; 5000],
            ramdisk: vec![2; 9000],
            second: Vec::new(),
            recovery_dtbo: Vec::new(),
            dtb: if version >= 2 { vec![3; 1234] } else { Vec::new() },
        }
    }

    #[test]
    fn written_and_read_again_the_same() {
        for v in 0..=2 {
            let img = sample(v);
            let bytes = img.to_bytes().unwrap();
            assert_eq!(bytes.len() % 4096, 0);
            let back = BootImage::parse(&bytes).unwrap();
            assert_eq!(back, img, "version {v}");
            assert_eq!(back.to_bytes().unwrap(), bytes);
        }
    }

    #[test]
    fn its_fields() {
        let img = sample(2);
        assert_eq!(img.os_version_text(), "11.0.0");
        assert_eq!(img.patch_level(), "2022-09");
        assert_eq!(img.cmdline_value("androidboot.hardware"), Some("surfaceduo"));
        assert!(BootImage::parse(b"not an image").is_err());
    }

    /// The port's boot image, if it is here: read and written again, byte
    /// for byte (mkbootimg's own layout and id).
    #[test]
    fn the_ports_image_round_trips() {
        let home = std::env::var("HOME").unwrap_or_default();
        let path = format!("{home}/Desktop/projects/surfaceduo/surfaceduo-droidian/out/boot-duo1-droidian-perf-127-lid.img");
        let Ok(b) = std::fs::read(&path) else { return };
        let img = BootImage::parse(&b).unwrap();
        assert_eq!(img.header_version, 2);
        let again = img.to_bytes().unwrap();
        assert!(again.len() <= b.len(), "longer than the original");
        assert!(again == b[..again.len()] && b[again.len()..].iter().all(|c| *c == 0), "the bytes differ");
    }
}
