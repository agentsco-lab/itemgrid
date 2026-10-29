//! The panels' colour, as Microsoft calibrated each of them at the factory:
//! the sfpd partition (vfat) keeps a Qualcomm display calibration file a
//! panel (disp_user_calib_data_dsi_panel_lg_v2_amoled_cmd_{C3,R2}.xml), its
//! modes (sRGB, Vivid) each three steps: a table into linear light (IGC,
//! 256 points a channel, 12 bits), a colour matrix in it (PCC: the panel's
//! own gamut into the mode's, and its white) and a table back out of it
//! (GC, 1024 points, 10 bits). Nothing applied them under Linux: the panels
//! showed their raw colour, over-saturated and each its own white. Here
//! they go into the display's own colour hardware (each panel's DSPP:
//! SDE_DSPP_IGC_V3, SDE_DSPP_PCC_V4, SDE_DSPP_GC_V1), free of cost. All
//! three or none: the matrix and the gamma without the way into linear
//! light lift every grey (half white came out white).
//!
//! item/grid says which in /run/item-colour, a line a field:
//!   mode srgb|vivid|off       (off: the raw panels, as before)
//!   pcc on|off                (the matrix; on by default)
//!   gc on|off                 (the gamma; on by default)
//!   igc on|off                (the way into linear light; on by default)
//!   first r2|c3               (the calibration of the first panel shown,
//!                              the other the other; r2 by default: on the
//!                              owner's Duo R2's file is the left panel's,
//!                              by eye - c3 there left a tint between them)
//!   order rgb|gbr|brg         (the gamma table's channels as c0 c1 c2)
//!   only 0|1                  (one panel only, the other raw: to tell
//!                              which is which)
//! Absent, nothing is done.

pub const WANT: &str = "/run/item-colour";
const SFPD: &str = "/dev/block/by-name/sfpd";
const AT: &str = "/run/sfpd";

#[derive(Clone, Debug, PartialEq)]
pub struct Want {
    pub mode: String,
    pub pcc: bool,
    pub gc: bool,
    pub igc: bool,
    pub first: String,
    pub order: String,
    /// Only the panel shown first (0) or second (1); else both.
    pub only: Option<usize>,
}

impl Want {
    pub fn read(text: &str) -> Want {
        let mut w = Want { mode: "off".into(), pcc: true, gc: true, igc: true, first: "r2".into(), order: "rgb".into(), only: None };
        for line in text.lines() {
            let (k, v) = line.split_once(' ').unwrap_or((line, ""));
            let v = v.trim().to_lowercase();
            match k {
                "mode" => w.mode = v,
                "pcc" => w.pcc = v != "off",
                "gc" => w.gc = v != "off",
                "igc" => w.igc = v != "off",
                "first" => w.first = v,
                "order" => w.order = v,
                "only" => w.only = v.parse().ok(),
                _ => {}
            }
        }
        w
    }
}

/// A mode of one panel's calibration: the way into linear light (3
/// channels of 256 points, 12 bits), the matrix (3 rows of c, r, g, b, rg,
/// gb, rb, rgb, rr, gg, bb) and the gamma (3 channels of 1024 points, 10
/// bits), as the file has them.
#[derive(Clone, Debug)]
pub struct Calib {
    pub igc: [Vec<u32>; 3],
    pub pcc: [[f64; 11]; 3],
    pub gc: [Vec<u32>; 3],
}

/// The calibration of the panel `which` ("c3", "r2") in `mode` ("srgb",
/// "vivid"), from the sfpd partition (mounted read only, once).
pub fn load(which: &str, mode: &str) -> Result<Calib, String> {
    mount_sfpd()?;
    let dir = format!("{AT}/display");
    let file = std::fs::read_dir(&dir)
        .map_err(|e| format!("{dir}: {e}"))?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()).is_some_and(|n| n.starts_with("disp_user_calib_data") && n.ends_with(&format!("_{which}.xml"))))
        .ok_or_else(|| format!("no calibration for panel {which} in {dir}"))?;
    let xml = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    parse(&xml, mode)
}

fn mount_sfpd() -> Result<(), String> {
    if crate::verbs::mounted(AT) {
        return Ok(());
    }
    std::fs::create_dir_all(AT).map_err(|e| e.to_string())?;
    let c = |s: &str| std::ffi::CString::new(s).unwrap();
    let (src, dir, fs) = (c(SFPD), c(AT), c("vfat"));
    // SAFETY: valid C strings; read only.
    if unsafe { libc::mount(src.as_ptr(), dir.as_ptr(), fs.as_ptr(), libc::MS_RDONLY, std::ptr::null()) } != 0 {
        return Err(format!("mounting sfpd: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

/// A mode (by its name, any case) of a calibration file.
pub fn parse(xml: &str, mode: &str) -> Result<Calib, String> {
    let mut rest = xml;
    while let Some(i) = rest.find("<Mode ") {
        let block_end = rest[i..].find("</Mode>").map(|j| i + j).ok_or("a mode not closed")?;
        let block = &rest[i..block_end];
        rest = &rest[block_end..];
        let name = attr(block, "Name").unwrap_or_default();
        if !name.eq_ignore_ascii_case(mode) {
            continue;
        }
        let feature = |ty: &str| -> Option<Vec<u8>> {
            let at = block.find(&format!("FeatureType=\"{ty}\""))?;
            let start = block[at..].find('>')? + at + 1;
            let end = block[start..].find('<')? + start;
            hexes(block[start..end].trim())
        };
        let pcc_raw = feature("2").ok_or("no colour matrix (PCC) in the mode")?;
        if pcc_raw.len() < 8 + 33 * 8 {
            return Err("the colour matrix is short".into());
        }
        let mut pcc = [[0f64; 11]; 3];
        for (k, v) in pcc.iter_mut().flatten().enumerate() {
            *v = f64::from_le_bytes(pcc_raw[8 + 8 * k..16 + 8 * k].try_into().unwrap());
        }
        // The gamma: 12 bytes of header, then three channels of 1024.
        let gc_raw = feature("8").ok_or("no gamma (GC) in the mode")?;
        if gc_raw.len() < 12 + 3 * 1024 * 4 {
            return Err("the gamma is short".into());
        }
        let ch = |raw: &[u8], c: usize, n: usize| (0..n).map(|i| u32::from_le_bytes(raw[12 + 4 * (c * 1024 + i)..16 + 4 * (c * 1024 + i)].try_into().unwrap())).collect::<Vec<_>>();
        // Into linear light: the same 12 bytes and three slots of 1024, of
        // which the first 256 hold the table (its header says 256).
        let igc_raw = feature("7").ok_or("no way into linear light (IGC) in the mode")?;
        if igc_raw.len() < 12 + 3 * 1024 * 4 {
            return Err("the way into linear light is short".into());
        }
        let igc = [ch(&igc_raw, 0, 256), ch(&igc_raw, 1, 256), ch(&igc_raw, 2, 256)];
        return Ok(Calib { igc, pcc, gc: [ch(&gc_raw, 0, 1024), ch(&gc_raw, 1, 1024), ch(&gc_raw, 2, 1024)] });
    }
    Err(format!("no mode {mode}"))
}

fn attr(block: &str, name: &str) -> Option<String> {
    let at = block.find(&format!(" {name}=\""))? + name.len() + 3;
    let end = block[at..].find('"')? + at;
    Some(block[at..end].to_owned())
}

fn hexes(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// The matrix as struct drm_msm_pcc: flags (u64), then r, g, b each c, r,
/// g, b, rg, gb, rb, rgb, then r_rr r_gg r_bb g_rr ... b_bb - each a
/// signed fixed point with 15 bits of fraction (1.0 is 0x8000).
pub fn pcc_blob(c: &Calib) -> Vec<u8> {
    let fx = |v: f64| ((v * 32768.0).round() as i32) as u32;
    let mut out = Vec::with_capacity(8 + 4 * 33);
    out.extend_from_slice(&0u64.to_le_bytes());
    for row in &c.pcc {
        for v in &row[..8] {
            out.extend_from_slice(&fx(*v).to_le_bytes());
        }
    }
    for row in &c.pcc {
        for v in &row[8..11] {
            out.extend_from_slice(&fx(*v).to_le_bytes());
        }
    }
    // The struct padded to its u64's alignment: 144 bytes, the kernel
    // checks the length to the byte (140 was refused).
    out.resize(out.len().next_multiple_of(8), 0);
    out
}

/// Which of r, g, b `order` makes c0, c1, c2.
fn channels(order: &str) -> Vec<usize> {
    order
        .chars()
        .take(3)
        .map(|ch| match ch {
            'r' => 0,
            'g' => 1,
            _ => 2,
        })
        .collect()
}

/// The way into linear light as struct drm_msm_igc_lut: flags (u64), then
/// c0, c1, c2 of 256 points (12 bits), the channels in `order`, a strength
/// (u32, unused) and each channel's end point (c0_last..c2_last: full) -
/// 3096 bytes (without the ends, 3088, was refused).
pub fn igc_blob(c: &Calib, order: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + 3 * 256 * 4 + 16);
    out.extend_from_slice(&0u64.to_le_bytes());
    for ch in channels(order) {
        for v in &c.igc[ch] {
            out.extend_from_slice(&(v & 0xfff).to_le_bytes());
        }
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    for _ in 0..3 {
        out.extend_from_slice(&0xfffu32.to_le_bytes());
    }
    out
}

/// The gamma as struct drm_msm_pgc_lut: flags (u64), then c0, c1, c2 of
/// 512 words, two 10-bit points a word (the even in the low bits, the odd
/// from bit 16), the channels in `order` (which of r, g, b is c0, c1, c2).
pub fn gc_blob(c: &Calib, order: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + 3 * 512 * 4);
    out.extend_from_slice(&0u64.to_le_bytes());
    for ch in channels(order) {
        let v = &c.gc[ch];
        for i in 0..512 {
            let w = (v[2 * i] & 0x3ff) | ((v[2 * i + 1] & 0x3ff) << 16);
            out.extend_from_slice(&w.to_le_bytes());
        }
    }
    out
}

/// What `w` asks put on the panels shown (their CRTCs, in order): each its
/// own calibration in its mode, or the features off. Said to the kernel's
/// log.
pub fn apply(display: &crate::drm::Display, w: &Want) {
    let crtcs = display.crtcs();
    let other = if w.first == "r2" { "c3" } else { "r2" };
    for (i, &crtc) in crtcs.iter().enumerate() {
        let panel = if i == 0 { w.first.as_str() } else { other };
        let calib = if w.mode == "off" || w.only.is_some_and(|o| o != i) { None } else { load(panel, &w.mode).map_err(|e| crate::say!("colour: {e}")).ok() };
        let pcc = calib.as_ref().filter(|_| w.pcc).map(pcc_blob);
        let gc = calib.as_ref().filter(|_| w.gc).map(|c| gc_blob(c, &w.order));
        let igc = calib.as_ref().filter(|_| w.igc).map(|c| igc_blob(c, &w.order));
        let said = |r: Result<(), String>, on: bool| r.map_or_else(|e| e, |_| if on { "on".into() } else { "off".into() });
        let r0 = said(display.set_blob(crtc, "SDE_DSPP_IGC_V3", igc.as_deref()), igc.is_some());
        let r1 = said(display.set_blob(crtc, "SDE_DSPP_PCC_V4", pcc.as_deref()), pcc.is_some());
        let r2 = said(display.set_blob(crtc, "SDE_DSPP_GC_V1", gc.as_deref()), gc.is_some());
        crate::say!("colour: panel {i} (crtc {crtc}) as {panel} {}: igc {r0}, pcc {r1}, gc {r2}", w.mode);
    }
}

/// The colour asked, if it changed since `last`.
pub fn wanted(last: &mut Option<String>) -> Option<Want> {
    let text = std::fs::read_to_string(WANT).ok()?;
    if last.as_deref() == Some(text.as_str()) {
        return None;
    }
    *last = Some(text.clone());
    Some(Want::read(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The owner's Duo's panel C3, its sRGB mode, if its file is at hand
    /// (ITEM_CALIB=path): the matrix as read, the blobs' sizes as the
    /// kernel's structs.
    #[test]
    fn a_calibration_read() {
        use std::path::Path;
        let Some(path) = std::env::var_os("ITEM_CALIB") else { return };
        let xml = std::fs::read_to_string(Path::new(&path)).unwrap();
        let c = parse(&xml, "srgb").unwrap();
        assert!((c.pcc[0][1] - 0.7325).abs() < 0.001, "{:?}", c.pcc[0]);
        assert_eq!(c.gc[0][0], 0);
        assert_eq!(c.igc[0][255], 4095);
        assert_eq!(igc_blob(&c, "rgb").len(), 3096);
        assert_eq!(pcc_blob(&c).len(), 144);
        assert_eq!(gc_blob(&c, "rgb").len(), 8 + 3 * 512 * 4);
        assert!(parse(&xml, "vivid").is_ok());
        assert!(parse(&xml, "nothing").is_err());
    }
}
