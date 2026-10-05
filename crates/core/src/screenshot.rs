//! A screenshot: item saves its next frame when /tmp/item-shot appears (a
//! raw RGBA copy of its buffer, the rows bottom-up as GL keeps them); it is
//! fetched, turned the right way up, the hinge's strip cut out, and written
//! as a PNG.

use std::time::Duration;

/// The Duo's output, hinge included (physical px), and its two panels' x
/// ranges.
const SCREEN: (usize, usize) = (2784, 1800);
const PANELS: [(usize, usize); 2] = [(0, 1350), (1434, 2784)];

/// The next frame made `n` times smaller on the phone (duo-motion's
/// shrink, there once it has been followed), with its size; the whole one
/// if that is not there. For the drawn Duo: a fraction of the bytes over
/// Wi-Fi.
pub fn take_small(host: &str, n: usize) -> Result<(Vec<u8>, usize, usize), String> {
    let path = next_frame(host)?;
    let motion = crate::posture::MOTION_ON_PHONE;
    let (w, h) = (SCREEN.0 / n, SCREEN.1 / n);
    // An older duo-motion knows no shrink (it would follow for ever): a
    // few seconds at most, then the whole frame.
    let q = crate::logs::quote(&path);
    let small = crate::phone::run_bytes(host, &format!("timeout 5 {motion} shrink {} {} {n} < {q} && rm -f {q}", SCREEN.0, SCREEN.1))?;
    if small.len() == w * h * 4 {
        return Ok((small, w, h));
    }
    let rgba = fetch(host, &path)?;
    Ok((rgba, SCREEN.0, SCREEN.1))
}

/// Asks item for the next frame and fetches it. The ask is the owner's: item
/// runs as the owner and could not remove root's file in /tmp.
pub fn take(host: &str) -> Result<Vec<u8>, String> {
    let path = next_frame(host)?;
    fetch(host, &path)
}

/// item asked for its next frame; where it was written on the phone.
fn next_frame(host: &str) -> Result<String, String> {
    crate::phone::run(host, &crate::phone::as_owner("rm -f /tmp/item-shot-*.rgba; touch /tmp/item-shot"))?;
    let want = SCREEN.0 * SCREEN.1 * 4;
    // The frame is drawn and written within a few frames; while the screen
    // is dark nothing is drawn.
    let found = format!(
        "for i in $(seq 30); do f=$(ls /tmp/item-shot-*.rgba 2>/dev/null | head -1); \
         if [ -n \"$f\" ] && [ \"$(stat -c %s \"$f\")\" = {want} ]; then echo \"$f\"; exit 0; fi; sleep 0.1; done; \
         rm -f /tmp/item-shot; exit 1"
    );
    let path = crate::phone::run(host, &found)?.trim().to_owned();
    if path.is_empty() {
        return Err("no frame came: the screen is off or item is not drawing - wake the screen and try again".to_owned());
    }
    std::thread::sleep(Duration::from_millis(100));
    Ok(path)
}

/// The frame at `path` on the phone, whole (and gone from there).
fn fetch(host: &str, path: &str) -> Result<Vec<u8>, String> {
    let want = SCREEN.0 * SCREEN.1 * 4;
    let rgba = crate::phone::run_bytes(host, &format!("cat {0} && rm -f {0}", crate::logs::quote(path)))?;
    if rgba.len() != want {
        return Err(format!("the frame is {} bytes, not {want}", rgba.len()));
    }
    Ok(rgba)
}

/// The frame as a PNG: the right way up, the panels side by side without the
/// hinge's strip unless `hinge`.
pub fn png(rgba: &[u8], hinge: bool) -> Result<Vec<u8>, String> {
    let (w, h) = SCREEN;
    let spans: Vec<(usize, usize)> = if hinge { vec![(0, w)] } else { PANELS.to_vec() };
    let out_w: usize = spans.iter().map(|(a, b)| b - a).sum();
    let mut pixels = Vec::with_capacity(out_w * h * 4);
    for y in (0..h).rev() {
        let row = &rgba[y * w * 4..(y + 1) * w * 4];
        for (a, b) in &spans {
            pixels.extend_from_slice(&row[a * 4..b * 4]);
        }
    }
    let mut file = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut file, out_w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| format!("png: {e}"))?;
        writer.write_image_data(&pixels).map_err(|e| format!("png: {e}"))?;
    }
    Ok(file)
}

/// The two panels' pictures, each the right way up (RGBA, PANEL_W x
/// SCREEN.1), for whoever shows them apart.
pub fn panels(rgba: &[u8]) -> [Vec<u8>; 2] {
    let (w, h) = SCREEN;
    PANELS.map(|(a, b)| {
        let mut out = Vec::with_capacity((b - a) * h * 4);
        for y in (0..h).rev() {
            out.extend_from_slice(&rgba[(y * w + a) * 4..(y * w + b) * 4]);
        }
        out
    })
}

/// The two panels' pictures of a frame made `n` times smaller (take_small:
/// `w` x `h`), each the right way up; their width.
pub fn panels_small(rgba: &[u8], w: usize, h: usize) -> ([Vec<u8>; 2], usize) {
    let n = SCREEN.0 / w.max(1);
    let spans = PANELS.map(|(a, b)| (a / n, b / n));
    let pw = spans[0].1 - spans[0].0;
    (
        spans.map(|(a, _)| {
            let mut out = Vec::with_capacity(pw * h * 4);
            for y in (0..h).rev() {
                out.extend_from_slice(&rgba[(y * w + a) * 4..(y * w + a + pw) * 4]);
            }
            out
        }),
        pw,
    )
}

/// The whole frame's width (physical px): take_small gave it whole.
pub const SCREEN_W: usize = SCREEN.0;

/// A panel's picture's size (physical px).
pub const PANEL_SIZE: (usize, usize) = (PANELS[0].1 - PANELS[0].0, SCREEN.1);
