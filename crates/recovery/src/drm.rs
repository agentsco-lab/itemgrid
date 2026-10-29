//! The panels through the kernel's DRM, by hand (no libdrm): every
//! connected connector given a dumb buffer of its preferred mode, a CRTC
//! and the buffer as its framebuffer; drawn into through the mapping, and
//! set on its CRTC again after each drawing (the Duo's DSI panels are
//! command-mode: they show a frame only when one is committed, and
//! msm's DIRTYFB commits none). Only the DSI connectors: the DisplayPort
//! says it is connected with nothing on it. A connector wider than tall is
//! both panels side by side, the hinge's gap between.

use std::fs::File;
use std::os::fd::AsRawFd;

use crate::say;

const fn iowr(nr: u32, size: usize) -> libc::c_ulong {
    ((3u32 << 30) | ((size as u32) << 16) | (('d' as u32) << 8) | nr) as libc::c_ulong
}

#[repr(C)]
#[derive(Default)]
struct CardRes {
    fb_id_ptr: u64,
    crtc_id_ptr: u64,
    connector_id_ptr: u64,
    encoder_id_ptr: u64,
    count_fbs: u32,
    count_crtcs: u32,
    count_connectors: u32,
    count_encoders: u32,
    min_width: u32,
    max_width: u32,
    min_height: u32,
    max_height: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ModeInfo {
    clock: u32,
    hdisplay: u16,
    hsync_start: u16,
    hsync_end: u16,
    htotal: u16,
    hskew: u16,
    vdisplay: u16,
    vsync_start: u16,
    vsync_end: u16,
    vtotal: u16,
    vscan: u16,
    vrefresh: u32,
    flags: u32,
    kind: u32,
    name: [u8; 32],
}

#[repr(C)]
#[derive(Default)]
struct GetConnector {
    encoders_ptr: u64,
    modes_ptr: u64,
    props_ptr: u64,
    prop_values_ptr: u64,
    count_modes: u32,
    count_props: u32,
    count_encoders: u32,
    encoder_id: u32,
    connector_id: u32,
    connector_type: u32,
    connector_type_id: u32,
    connection: u32,
    mm_width: u32,
    mm_height: u32,
    subpixel: u32,
    pad: u32,
}

#[repr(C)]
#[derive(Default)]
struct GetEncoder {
    encoder_id: u32,
    encoder_type: u32,
    crtc_id: u32,
    possible_crtcs: u32,
    possible_clones: u32,
}

#[repr(C)]
#[derive(Default)]
struct CreateDumb {
    height: u32,
    width: u32,
    bpp: u32,
    flags: u32,
    handle: u32,
    pitch: u32,
    size: u64,
}

#[repr(C)]
#[derive(Default)]
struct MapDumb {
    handle: u32,
    pad: u32,
    offset: u64,
}

#[repr(C)]
#[derive(Default)]
struct FbCmd {
    fb_id: u32,
    width: u32,
    height: u32,
    pitch: u32,
    bpp: u32,
    depth: u32,
    handle: u32,
}

#[repr(C)]
struct Crtc {
    set_connectors_ptr: u64,
    count_connectors: u32,
    crtc_id: u32,
    fb_id: u32,
    x: u32,
    y: u32,
    gamma_size: u32,
    mode_valid: u32,
    mode: ModeInfo,
}

#[repr(C)]
#[derive(Default)]
struct DirtyFb {
    fb_id: u32,
    flags: u32,
    color: u32,
    num_clips: u32,
    clips_ptr: u64,
}

const GETRESOURCES: libc::c_ulong = iowr(0xA0, std::mem::size_of::<CardRes>());
const SETCRTC: libc::c_ulong = iowr(0xA2, std::mem::size_of::<Crtc>());
const GETENCODER: libc::c_ulong = iowr(0xA6, std::mem::size_of::<GetEncoder>());
const GETCONNECTOR: libc::c_ulong = iowr(0xA7, std::mem::size_of::<GetConnector>());
const ADDFB: libc::c_ulong = iowr(0xAE, std::mem::size_of::<FbCmd>());
const DIRTYFB: libc::c_ulong = iowr(0xB1, std::mem::size_of::<DirtyFb>());
const CREATE_DUMB: libc::c_ulong = iowr(0xB2, std::mem::size_of::<CreateDumb>());
const MAP_DUMB: libc::c_ulong = iowr(0xB3, std::mem::size_of::<MapDumb>());

fn ioctl<T>(fd: i32, req: libc::c_ulong, arg: &mut T) -> std::io::Result<()> {
    // SAFETY: `arg` is the request's own struct, laid out as the kernel's.
    let r = unsafe { libc::ioctl(fd, req as _, arg as *mut T) };
    if r < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// A connector shown: its buffers' pixels (0xXXRRGGBB), its size and row
/// length in pixels. Two buffers where the card gives them: one shown,
/// the other drawn into, then shown in its turn - a frame never seen half
/// drawn.
pub struct Output {
    connector: u32,
    crtc: u32,
    mode: ModeInfo,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    /// (framebuffer, mapping) a buffer.
    bufs: Vec<(u32, *mut u32)>,
    shown: usize,
    len: usize,
}

impl Output {
    /// The buffer to draw into: the one not shown (the only one if there
    /// is one).
    pub fn pixels(&mut self) -> &mut [u32] {
        let back = (self.shown + 1) % self.bufs.len();
        // SAFETY: the mapping lives as long as the device (never unmapped).
        unsafe { std::slice::from_raw_parts_mut(self.bufs[back].1, self.len) }
    }
}

pub struct Display {
    card: File,
    pub outputs: Vec<Output>,
}

// The mappings are only touched by the screen's thread.
unsafe impl Send for Display {}

impl Display {
    /// Every connected connector lit with a buffer of its own; None if
    /// there is no DRM device or nothing could be shown.
    pub fn open() -> Option<Display> {
        let card = std::fs::OpenOptions::new().read(true).write(true).open("/dev/dri/card0").map_err(|e| say!("screen: /dev/dri/card0: {e}")).ok()?;
        let fd = card.as_raw_fd();
        let mut res = CardRes::default();
        ioctl(fd, GETRESOURCES, &mut res).map_err(|e| say!("screen: resources: {e}")).ok()?;
        let mut crtcs = vec![0u32; res.count_crtcs as usize];
        let mut conns = vec![0u32; res.count_connectors as usize];
        let mut encs = vec![0u32; res.count_encoders as usize];
        let mut fbs = vec![0u32; res.count_fbs as usize];
        res.crtc_id_ptr = crtcs.as_mut_ptr() as u64;
        res.connector_id_ptr = conns.as_mut_ptr() as u64;
        res.encoder_id_ptr = encs.as_mut_ptr() as u64;
        res.fb_id_ptr = fbs.as_mut_ptr() as u64;
        ioctl(fd, GETRESOURCES, &mut res).ok()?;
        say!("screen: {} connectors, {} crtcs", conns.len(), crtcs.len());
        let mut used: Vec<u32> = Vec::new();
        let mut outputs = Vec::new();
        for &id in &conns {
            match light(fd, id, &crtcs, &mut used) {
                Ok(Some(o)) => outputs.push(o),
                Ok(None) => {}
                Err(e) => say!("screen: connector {id}: {e}"),
            }
        }
        if outputs.is_empty() {
            say!("screen: nothing to show on");
            return None;
        }
        Some(Display { card, outputs })
    }

    /// The buffers drawn into shown: each committed to its panel (the
    /// one drawn into becomes the one shown).
    pub fn flush(&mut self) {
        let fd = self.card.as_raw_fd();
        for o in &mut self.outputs {
            let back = (o.shown + 1) % o.bufs.len();
            let fb = o.bufs[back].0;
            let mut conn = o.connector;
            let mut set = Crtc { set_connectors_ptr: &mut conn as *mut u32 as u64, count_connectors: 1, crtc_id: o.crtc, fb_id: fb, x: 0, y: 0, gamma_size: 0, mode_valid: 1, mode: o.mode };
            if let Err(e) = ioctl(fd, SETCRTC, &mut set) {
                say!("screen: committing to crtc {}: {e}", o.crtc);
            }
            let mut d = DirtyFb { fb_id: fb, ..Default::default() };
            let _ = ioctl(fd, DIRTYFB, &mut d);
            o.shown = back;
        }
    }
}

fn light(fd: i32, id: u32, crtcs: &[u32], used: &mut Vec<u32>) -> Result<Option<Output>, String> {
    let mut c = GetConnector { connector_id: id, ..Default::default() };
    ioctl(fd, GETCONNECTOR, &mut c).map_err(|e| e.to_string())?;
    // DRM_MODE_CONNECTOR_DSI: the panels.
    if c.connection != 1 || c.count_modes == 0 || c.connector_type != 16 {
        return Ok(None);
    }
    // SAFETY: zeroed ModeInfo is a valid value of plain integers.
    let mut modes = vec![unsafe { std::mem::zeroed::<ModeInfo>() }; c.count_modes as usize];
    let mut encs = vec![0u32; c.count_encoders as usize];
    let mut props = vec![0u32; c.count_props as usize];
    let mut values = vec![0u64; c.count_props as usize];
    c.modes_ptr = modes.as_mut_ptr() as u64;
    c.encoders_ptr = encs.as_mut_ptr() as u64;
    c.props_ptr = props.as_mut_ptr() as u64;
    c.prop_values_ptr = values.as_mut_ptr() as u64;
    ioctl(fd, GETCONNECTOR, &mut c).map_err(|e| e.to_string())?;
    // The preferred mode (DRM_MODE_TYPE_PREFERRED), else the first.
    let mode = *modes.iter().find(|m| m.kind & 8 != 0).unwrap_or(&modes[0]);
    // A CRTC its encoder can drive, not taken by another connector.
    let mut crtc = 0;
    for &e in encs.iter().chain([c.encoder_id].iter()).filter(|&&e| e != 0) {
        let mut enc = GetEncoder { encoder_id: e, ..Default::default() };
        if ioctl(fd, GETENCODER, &mut enc).is_err() {
            continue;
        }
        if enc.crtc_id != 0 && !used.contains(&enc.crtc_id) {
            crtc = enc.crtc_id;
            break;
        }
        if let Some((_, &id)) = crtcs.iter().enumerate().find(|(i, id)| enc.possible_crtcs & (1 << i) != 0 && !used.contains(id)) {
            crtc = id;
            break;
        }
    }
    if crtc == 0 {
        return Err("no CRTC for it".into());
    }
    let (w, h) = (mode.hdisplay as u32, mode.vdisplay as u32);
    let buffer = || -> Result<(u32, *mut u32, u32), String> {
        let mut dumb = CreateDumb { width: w, height: h, bpp: 32, ..Default::default() };
        ioctl(fd, CREATE_DUMB, &mut dumb).map_err(|e| format!("a buffer of {w}x{h}: {e}"))?;
        let mut fb = FbCmd { width: w, height: h, pitch: dumb.pitch, bpp: 32, depth: 24, handle: dumb.handle, ..Default::default() };
        ioctl(fd, ADDFB, &mut fb).map_err(|e| format!("its framebuffer: {e}"))?;
        let mut map = MapDumb { handle: dumb.handle, ..Default::default() };
        ioctl(fd, MAP_DUMB, &mut map).map_err(|e| format!("mapping it: {e}"))?;
        // SAFETY: mapping the dumb buffer's offset on the card, its whole size.
        let ptr = unsafe { libc::mmap(std::ptr::null_mut(), dumb.size as usize, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, map.offset as libc::off_t) };
        if ptr == libc::MAP_FAILED {
            return Err(format!("mmap: {}", std::io::Error::last_os_error()));
        }
        Ok((fb.fb_id, ptr.cast(), dumb.pitch))
    };
    let (fb0, ptr0, pitch) = buffer()?;
    let mut bufs = vec![(fb0, ptr0)];
    // A second, to draw into while the first is shown (none: one will do).
    match buffer() {
        Ok((fb1, ptr1, _)) => bufs.push((fb1, ptr1)),
        Err(e) => say!("screen: one buffer only ({e})"),
    }
    let mut conn = id;
    let mut set = Crtc { set_connectors_ptr: &mut conn as *mut u32 as u64, count_connectors: 1, crtc_id: crtc, fb_id: fb0, x: 0, y: 0, gamma_size: 0, mode_valid: 1, mode };
    ioctl(fd, SETCRTC, &mut set).map_err(|e| format!("setting crtc {crtc}: {e}"))?;
    used.push(crtc);
    let name = String::from_utf8_lossy(&mode.name).trim_end_matches('\0').to_owned();
    say!("screen: connector {id} ({name}, {w}x{h}) on crtc {crtc}");
    let stride = (pitch / 4) as usize;
    Ok(Some(Output { connector: id, crtc, mode, width: w as usize, height: h as usize, stride, bufs, shown: 0, len: stride * h as usize }))
}

/// The panels' backlight (if they have one) at its brightest: an OLED
/// panel's brightness is its level here too.
pub fn brightest() {
    let Ok(dir) = std::fs::read_dir("/sys/class/backlight") else { return };
    for e in dir.flatten() {
        let max = std::fs::read_to_string(e.path().join("max_brightness")).unwrap_or_default();
        let max: u32 = max.trim().parse().unwrap_or(0);
        if max > 0 {
            let _ = std::fs::write(e.path().join("brightness"), (max * 6 / 10).max(1).to_string());
        }
    }
}

#[repr(C)]
#[derive(Default)]
struct ObjProps {
    props_ptr: u64,
    values_ptr: u64,
    count_props: u32,
    obj_id: u32,
    obj_type: u32,
}

#[repr(C)]
struct GetProperty {
    values_ptr: u64,
    enum_blob_ptr: u64,
    prop_id: u32,
    flags: u32,
    name: [u8; 32],
    count_values: u32,
    count_enum_blobs: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PropEnum {
    value: u64,
    name: [u8; 32],
}

#[repr(C)]
#[derive(Default)]
struct PlaneRes {
    planes_ptr: u64,
    count_planes: u32,
}

#[repr(C)]
#[derive(Default)]
struct ClientCap {
    capability: u64,
    value: u64,
}

const OBJ_GETPROPERTIES: libc::c_ulong = iowr(0xB9, std::mem::size_of::<ObjProps>());
const GETPROPERTY: libc::c_ulong = iowr(0xAA, std::mem::size_of::<GetProperty>());
const GETPLANERESOURCES: libc::c_ulong = iowr(0xB5, std::mem::size_of::<PlaneRes>());
const SET_CLIENT_CAP: libc::c_ulong = ((1u32 << 30) | ((std::mem::size_of::<ClientCap>() as u32) << 16) | (('d' as u32) << 8) | 0x0D) as libc::c_ulong;

/// Every CRTC's, connector's and plane's properties - names, kinds, values
/// (an enum's names, a range's bounds) - for looking at what the display
/// can do (`item-recovery drm`): its colour (CTM, GAMMA_LUT, the vendor's
/// own) above all. Reads only.
pub fn report() -> Result<String, String> {
    let card = std::fs::OpenOptions::new().read(true).write(true).open("/dev/dri/card0").map_err(|e| format!("/dev/dri/card0: {e}"))?;
    let fd = card.as_raw_fd();
    // Planes all listed (DRM_CLIENT_CAP_UNIVERSAL_PLANES), atomic's too.
    for cap in [2u64, 3] {
        let _ = ioctl(fd, SET_CLIENT_CAP, &mut ClientCap { capability: cap, value: 1 });
    }
    let mut res = CardRes::default();
    ioctl(fd, GETRESOURCES, &mut res).map_err(|e| format!("resources: {e}"))?;
    let mut crtcs = vec![0u32; res.count_crtcs as usize];
    let mut conns = vec![0u32; res.count_connectors as usize];
    let mut encs = vec![0u32; res.count_encoders as usize];
    let mut fbs = vec![0u32; res.count_fbs as usize];
    res.crtc_id_ptr = crtcs.as_mut_ptr() as u64;
    res.connector_id_ptr = conns.as_mut_ptr() as u64;
    res.encoder_id_ptr = encs.as_mut_ptr() as u64;
    res.fb_id_ptr = fbs.as_mut_ptr() as u64;
    ioctl(fd, GETRESOURCES, &mut res).map_err(|e| format!("resources: {e}"))?;
    let mut pr = PlaneRes::default();
    let _ = ioctl(fd, GETPLANERESOURCES, &mut pr);
    let mut planes = vec![0u32; pr.count_planes as usize];
    pr.planes_ptr = planes.as_mut_ptr() as u64;
    let _ = ioctl(fd, GETPLANERESOURCES, &mut pr);
    let name = |n: &[u8]| String::from_utf8_lossy(n).trim_end_matches('\0').to_owned();
    let mut out = String::new();
    for (kind, ty, ids) in [("crtc", 0xccccccccu32, &crtcs), ("connector", 0xc0c0c0c0, &conns), ("plane", 0xeeeeeeee, &planes)] {
        for &id in ids.iter() {
            let mut op = ObjProps { obj_id: id, obj_type: ty, ..Default::default() };
            if ioctl(fd, OBJ_GETPROPERTIES, &mut op).is_err() {
                continue;
            }
            let mut props = vec![0u32; op.count_props as usize];
            let mut values = vec![0u64; op.count_props as usize];
            op.props_ptr = props.as_mut_ptr() as u64;
            op.values_ptr = values.as_mut_ptr() as u64;
            ioctl(fd, OBJ_GETPROPERTIES, &mut op).map_err(|e| e.to_string())?;
            out.push_str(&format!("{kind} {id}\n"));
            for (p, v) in props.iter().zip(&values) {
                let mut gp = GetProperty { values_ptr: 0, enum_blob_ptr: 0, prop_id: *p, flags: 0, name: [0; 32], count_values: 0, count_enum_blobs: 0 };
                if ioctl(fd, GETPROPERTY, &mut gp).is_err() {
                    continue;
                }
                let mut vals = vec![0u64; gp.count_values as usize];
                let mut enums = vec![PropEnum { value: 0, name: [0; 32] }; gp.count_enum_blobs as usize];
                gp.values_ptr = vals.as_mut_ptr() as u64;
                gp.enum_blob_ptr = enums.as_mut_ptr() as u64;
                let _ = ioctl(fd, GETPROPERTY, &mut gp);
                // DRM_MODE_PROP_RANGE 2, ENUM 8, BLOB 16, BITMASK 32,
                // OBJECT 64, SIGNED_RANGE 128; IMMUTABLE 4.
                let f = gp.flags;
                let what = if f & 16 != 0 {
                    "blob".to_owned()
                } else if f & (8 | 32) != 0 {
                    format!("enum [{}]", enums.iter().map(|e| format!("{}={}", name(&e.name), e.value)).collect::<Vec<_>>().join(" "))
                } else if f & (2 | 128) != 0 && vals.len() >= 2 {
                    format!("range {}..{}", vals[0] as i64, vals[1] as i64)
                } else if f & 64 != 0 {
                    "object".to_owned()
                } else {
                    format!("flags {f:#x}")
                };
                out.push_str(&format!("  {:<28} = {:<12} {}{}\n", name(&gp.name), v, what, if f & 4 != 0 { " (immutable)" } else { "" }));
            }
        }
    }
    Ok(out)
}

#[repr(C)]
#[derive(Default)]
struct CreateBlob {
    data: u64,
    length: u32,
    blob_id: u32,
}

#[repr(C)]
#[derive(Default)]
struct DestroyBlob {
    blob_id: u32,
}

#[repr(C)]
#[derive(Default)]
struct ObjSetProperty {
    value: u64,
    prop_id: u32,
    obj_id: u32,
    obj_type: u32,
}

const CREATEPROPBLOB: libc::c_ulong = iowr(0xBD, std::mem::size_of::<CreateBlob>());
const DESTROYPROPBLOB: libc::c_ulong = iowr(0xBE, std::mem::size_of::<DestroyBlob>());
const OBJ_SETPROPERTY: libc::c_ulong = iowr(0xBA, std::mem::size_of::<ObjSetProperty>());
const OBJ_CRTC: u32 = 0xcccccccc;

impl Display {
    /// The CRTCs shown on, in the outputs' order.
    pub fn crtcs(&self) -> Vec<u32> {
        self.outputs.iter().map(|o| o.crtc).collect()
    }

    /// A blob property of `crtc` by its name (SDE_DSPP_PCC_V4...) set to
    /// `data`, or cleared (the feature off) with none.
    pub fn set_blob(&self, crtc: u32, name: &str, data: Option<&[u8]>) -> Result<(), String> {
        let fd = self.card.as_raw_fd();
        let prop = prop_id(fd, crtc, OBJ_CRTC, name)?;
        let mut blob = CreateBlob::default();
        if let Some(d) = data {
            blob = CreateBlob { data: d.as_ptr() as u64, length: d.len() as u32, blob_id: 0 };
            ioctl(fd, CREATEPROPBLOB, &mut blob).map_err(|e| format!("{name}'s blob: {e}"))?;
        }
        let mut set = ObjSetProperty { value: blob.blob_id as u64, prop_id: prop, obj_id: crtc, obj_type: OBJ_CRTC };
        let r = ioctl(fd, OBJ_SETPROPERTY, &mut set).map_err(|e| format!("{name} on crtc {crtc}: {e}"));
        // The property keeps its own reference to the blob.
        if blob.blob_id != 0 {
            let _ = ioctl(fd, DESTROYPROPBLOB, &mut DestroyBlob { blob_id: blob.blob_id });
        }
        r
    }
}

/// A property's id on an object, by its name.
fn prop_id(fd: i32, obj: u32, ty: u32, want: &str) -> Result<u32, String> {
    let mut op = ObjProps { obj_id: obj, obj_type: ty, ..Default::default() };
    ioctl(fd, OBJ_GETPROPERTIES, &mut op).map_err(|e| e.to_string())?;
    let mut props = vec![0u32; op.count_props as usize];
    let mut values = vec![0u64; op.count_props as usize];
    op.props_ptr = props.as_mut_ptr() as u64;
    op.values_ptr = values.as_mut_ptr() as u64;
    ioctl(fd, OBJ_GETPROPERTIES, &mut op).map_err(|e| e.to_string())?;
    for p in props {
        let mut gp = GetProperty { values_ptr: 0, enum_blob_ptr: 0, prop_id: p, flags: 0, name: [0; 32], count_values: 0, count_enum_blobs: 0 };
        if ioctl(fd, GETPROPERTY, &mut gp).is_ok() && String::from_utf8_lossy(&gp.name).trim_end_matches('\0') == want {
            return Ok(p);
        }
    }
    Err(format!("no {want} on object {obj}"))
}
