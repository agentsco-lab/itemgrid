//! What the recovery shows on the phone, drawn as item/grid draws its
//! table: a dark grid of squares, a letter a square, lowercase; the left
//! panel says what goes on, the right one the steps and how far.
//!
//! item/grid tells it in /run/item-screen, a line a field:
//!   title writing the system
//!   detail 3.4 of 5.4 gb
//!   steps userdata|system|check|ssh key|restart
//!   step 1
//!   progress 0.62
//!   state work|done|error
//!   id 00001             (the phone's item id, item/grid's to know)
//!   line #ffffff         (the grid as item/grid's window draws it:
//!   strength 1.0          its lines' colour by night, how strong,
//!   width 2.0             how wide)
//! Before it does: ready for item/grid.

use fontdue::{Font, FontSettings};

pub const STATUS: &str = "/run/item-screen";
/// The font in the initramfs (item/grid's own: Open Sans Light, OFL).
pub const FONT: &str = "/usr/share/item-recovery/font.ttf";

#[derive(Clone, Default, PartialEq)]
pub struct Status {
    pub title: String,
    pub detail: String,
    pub steps: Vec<String>,
    pub step: usize,
    pub progress: Option<f32>,
    pub state: String,
    pub serial: String,
    pub id: String,
    /// The grid's lines: colour, strength (1 plain), width (px on the
    /// computer's table).
    pub line: Option<u32>,
    pub strength: f32,
    pub width: f32,
}

impl Status {
    pub fn waiting(serial: &str) -> Status {
        Status { title: "ready".into(), detail: "for item/grid".into(), state: "wait".into(), serial: serial.into(), ..Default::default() }
    }

    /// The status as item/grid wrote it (`serial` kept).
    pub fn read(text: &str, serial: &str) -> Status {
        let mut s = Status { serial: serial.into(), state: "work".into(), ..Default::default() };
        for line in text.lines() {
            let (k, v) = line.split_once(' ').unwrap_or((line, ""));
            let v = v.trim().to_lowercase();
            match k {
                "title" => s.title = v,
                "detail" => s.detail = v,
                "steps" => s.steps = v.split('|').map(|x| x.trim().to_owned()).filter(|x| !x.is_empty()).collect(),
                "step" => s.step = v.parse().unwrap_or(0),
                "progress" => s.progress = v.parse::<f32>().ok().map(|p| p.clamp(0.0, 1.0)),
                "state" => s.state = v,
                "id" => s.id = v,
                "line" => s.line = u32::from_str_radix(v.trim_start_matches('#'), 16).ok(),
                "strength" => s.strength = v.parse().unwrap_or(0.0),
                "width" => s.width = v.parse().unwrap_or(0.0),
                _ => {}
            }
        }
        s
    }
}

/// Colours, 0xRRGGBB: the table at night, its lines, the letters (dim,
/// lit), the accents.
const PAPER: u32 = 0x242526;
const DIM: u32 = 0x77797c;
const LIT: u32 = 0xe4e6e8;
const GREEN: u32 = 0x57e389;
const RED: u32 = 0xe8695e;
const FILL: u32 = 0x3a3c3f;

/// A panel's squares across (the Duo's 1350 px: 75 px squares).
const ACROSS: usize = 18;

pub struct Painter {
    font: Font,
    cache: std::collections::HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
}

impl Painter {
    pub fn new(font_bytes: &[u8]) -> Option<Painter> {
        let font = Font::from_bytes(font_bytes, FontSettings::default()).ok()?;
        Some(Painter { font, cache: Default::default() })
    }

    /// The panel `which` (0 left, 1 right) of `w` x `h` pixels, 0xXXRRGGBB
    /// a pixel, rows of `stride` pixels.
    pub fn panel(&mut self, which: usize, s: &Status, px: &mut [u32], w: usize, h: usize, stride: usize) {
        let sq = (w / ACROSS).max(8);
        let rows = h / sq;
        let ox = (w - sq * ACROSS) / 2;
        let oy = (h - sq * rows) / 2;
        // The table and its grid: the lines as item/grid's window has
        // them (their colour on the dark table, how strong, how wide), else
        // a faint white.
        let strength = if s.strength > 0.0 { s.strength } else { 1.0 };
        let line = blend(PAPER, s.line.unwrap_or(0xffffff), ((0.16 * strength).clamp(0.04, 0.5) * 255.0) as u32);
        let thick = if s.width > 0.0 { (s.width * 0.8).round().clamp(1.0, 4.0) as usize } else { 1 };
        for y in 0..h {
            let row = &mut px[y * stride..y * stride + w];
            let on_line = y >= oy && (y - oy) % sq < thick;
            for (x, p) in row.iter_mut().enumerate() {
                *p = if on_line || (x >= ox && (x - ox) % sq < thick) { line } else { PAPER };
            }
        }
        let mut g = Grid { px, stride, sq, ox, oy, across: ACROSS, rows };
        let accent = match s.state.as_str() {
            "done" => GREEN,
            "error" => RED,
            _ => LIT,
        };
        if which == 0 {
            self.words(&mut g, 2, 3, "item", LIT);
            self.words(&mut g, 2, 4, "recovery", DIM);
            let mut r = 8;
            for line in wrap(&s.title, ACROSS - 4) {
                self.words(&mut g, 2, r, &line, accent);
                r += 1;
            }
            for line in wrap(&s.detail, ACROSS - 4) {
                self.words(&mut g, 2, r, &line, DIM);
                r += 1;
            }
            let foot = if s.state == "done" { "restarting now" } else { "keep the cable in" };
            self.words(&mut g, 1, rows.saturating_sub(3), foot, if s.state == "error" { RED } else { DIM });
        } else {
            let mut r = 3;
            for (i, step) in s.steps.iter().enumerate() {
                let (mark, colour) = if i < s.step || s.state == "done" {
                    ("•", DIM)
                } else if i == s.step {
                    ("›", accent)
                } else {
                    (" ", DIM)
                };
                self.words(&mut g, 2, r, mark, colour);
                self.words(&mut g, 4, r, step, colour);
                r += 1;
                // Under the step at work: its progress, a square a tenth,
                // and the percent.
                if i == s.step && s.state != "done" {
                    if let Some(p) = s.progress {
                        let n = 10;
                        let lit = ((p * n as f32).round() as usize).min(n);
                        for c in 0..n {
                            g.fill(4 + c, r, if c < lit { accent } else { FILL });
                        }
                        self.words(&mut g, 4 + n, r, &format!("{}%", (p * 100.0).round() as u32), DIM);
                        r += 1;
                    }
                }
            }
            // Whose phone: its item id (once item/grid has said it), the
            // model, its serial.
            if !s.id.is_empty() {
                self.words(&mut g, 2, rows.saturating_sub(5), &format!("item id {}", s.id), LIT);
            }
            self.words(&mut g, 2, rows.saturating_sub(4), "duo 1", DIM);
        }
        Self::vignette(px, w, h, stride);
    }

    /// The panel's edges darkened as item darkens them over its pictures
    /// (vignette.frag, its default: strength 0.55, reach 0.5, soft 0.6).
    fn vignette(px: &mut [u32], w: usize, h: usize, stride: usize) {
        // Thresholds in (0, 1), a 4x4 Bayer matrix.
        const BAYER: [f32; 16] = [0.5 / 16.0, 8.5 / 16.0, 2.5 / 16.0, 10.5 / 16.0, 12.5 / 16.0, 4.5 / 16.0, 14.5 / 16.0, 6.5 / 16.0, 3.5 / 16.0, 11.5 / 16.0, 1.5 / 16.0, 9.5 / 16.0, 15.5 / 16.0, 7.5 / 16.0, 13.5 / 16.0, 5.5 / 16.0];
        const STRENGTH: f32 = 0.55;
        let start = 1.3 + (0.12 - 1.3) * 0.5;
        let width = 0.1 + (1.15 - 0.1) * 0.6;
        let smooth = |a: f32, b: f32, x: f32| {
            let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        for y in 0..h {
            let qy = ((y as f32 + 0.5) / h as f32 - 0.5) * 2.0 * 0.92;
            for x in 0..w {
                let qx = ((x as f32 + 0.5) / w as f32 - 0.5) * 2.0;
                let av = STRENGTH * smooth(start, start + width, (qx * qx + qy * qy).sqrt());
                if av < 0.004 {
                    continue;
                }
                // Dithered (a 4x4 ordered threshold in place of rounding):
                // the paper darkened in 8 bits a channel falls into wide
                // steps otherwise. Black stays black.
                let k = 1.0 - av;
                let t = BAYER[(y & 3) * 4 + (x & 3)];
                let p = &mut px[y * stride + x];
                let c = *p;
                let ch = |v: u32| ((v & 0xff) as f32 * k + t).floor().clamp(0.0, 255.0) as u32;
                *p = (ch(c >> 16) << 16) | (ch(c >> 8) << 8) | ch(c);
            }
        }
    }

    /// `text` from square (`col`, `row`), a letter a square.
    fn words(&mut self, g: &mut Grid, col: usize, row: usize, text: &str, colour: u32) {
        for (i, ch) in text.chars().enumerate() {
            let c = col + i;
            if c >= g.across || row >= g.rows {
                break;
            }
            if ch != ' ' {
                self.letter(g, c, row, ch, colour);
            }
        }
    }

    fn letter(&mut self, g: &mut Grid, col: usize, row: usize, ch: char, colour: u32) {
        let size = (g.sq as f32 * 0.52).round() as u32;
        let font = &self.font;
        let (m, bitmap) = self.cache.entry((ch, size)).or_insert_with(|| font.rasterize(ch, size as f32));
        // Centred in the square, on a baseline a little under its middle.
        let x0 = g.ox + col * g.sq + (g.sq.saturating_sub(m.width)) / 2;
        let base = g.oy + row * g.sq + g.sq / 2 + (size as usize) * 36 / 100;
        let y0 = base as i64 - m.height as i64 - m.ymin as i64;
        for y in 0..m.height {
            let py = y0 + y as i64;
            if py < 0 {
                continue;
            }
            let row_px = py as usize * g.stride;
            for x in 0..m.width {
                let a = bitmap[y * m.width + x] as u32;
                if a == 0 {
                    continue;
                }
                let at = row_px + x0 + x;
                if let Some(p) = g.px.get_mut(at) {
                    *p = blend(*p, colour, a);
                }
            }
        }
    }
}

struct Grid<'a> {
    px: &'a mut [u32],
    stride: usize,
    sq: usize,
    ox: usize,
    oy: usize,
    across: usize,
    rows: usize,
}

impl Grid<'_> {
    /// A square filled, inside its lines.
    fn fill(&mut self, col: usize, row: usize, colour: u32) {
        let (x0, y0) = (self.ox + col * self.sq + 1, self.oy + row * self.sq + 1);
        let inset = self.sq / 6;
        for y in y0 + inset..y0 + self.sq - 1 - inset {
            for x in x0 + inset..x0 + self.sq - 1 - inset {
                if let Some(p) = self.px.get_mut(y * self.stride + x) {
                    *p = colour;
                }
            }
        }
    }
}

fn blend(under: u32, over: u32, a: u32) -> u32 {
    let ch = |c: u32, s: u32| ((c >> s & 0xff) * (255 - a) + (over >> s & 0xff) * a) / 255;
    (ch(under, 16) << 16) | (ch(under, 8) << 8) | ch(under, 0)
}

/// Words wrapped to `wide` letters.
/// Lines of `text` at most `wide` letters: "|" ends a line where item/grid
/// wants it (a phrase kept whole), the rest broken between words.
fn wrap(text: &str, wide: usize) -> Vec<String> {
    text.split('|').flat_map(|part| wrap_words(part, wide)).collect()
}

fn wrap_words(text: &str, wide: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for w in text.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + w.chars().count() > wide {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Both panels as portable pixmaps in `dir`, for looking at the design on
/// a computer (`item-recovery preview DIR FONT`): the waiting, a write at
/// 62 %, the end, an error.
pub fn preview(dir: &str, font: &str) -> i32 {
    let Ok(bytes) = std::fs::read(font) else {
        eprintln!("{font}: not read");
        return 1;
    };
    let Some(mut p) = Painter::new(&bytes) else { return 1 };
    let steps = "clean|item 0.3.0|check|link|restart";
    // The grid as item/grid's window keeps it here, if it does.
    let grid = std::env::var_os("HOME").and_then(|h| std::fs::read_to_string(std::path::Path::new(&h).join(".config/itemgrid/recovery-grid")).ok()).unwrap_or_default();
    let cases = [
        ("waiting", Status::waiting("903855502467")),
        ("writing", Status::read(&format!("title writing item\ndetail 3.4 of 5.4 gb\nsteps {steps}\nstep 1\nprogress 0.62\nstate work\nid 00001"), "903855502467")),
        ("done", Status::read(&format!("title item is on\ndetail setup next\nsteps {steps}\nstep 5\nstate done\nid 00001"), "903855502467")),
        ("error", Status::read(&format!("title stopped\ndetail the image came|incomplete|retry from|item/grid\nsteps {steps}\nstep 2\nstate error"), "903855502467")),
    ];
    let (w, h) = (1350, 1800);
    for (name, mut s) in cases {
        let g = Status::read(&grid, "");
        (s.line, s.strength, s.width) = (g.line, g.strength, g.width);
        for which in 0..2 {
            let mut px = vec![0u32; w * h];
            p.panel(which, &s, &mut px, w, h, w);
            let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
            for v in px {
                out.extend_from_slice(&[(v >> 16) as u8, (v >> 8) as u8, v as u8]);
            }
            let _ = std::fs::write(format!("{dir}/{name}-{which}.ppm"), out);
        }
    }
    0
}

/// Where the daemon says the host has connected (before any status).
pub const CONNECTED: &str = "/run/item-adb";

/// The screen kept as the status says, on whatever panels DRM gives
/// (a thread of PID 1's; it gives up quietly if there is nothing to show
/// on: the recovery works by the cable alone).
pub fn run(serial: String) {
    crate::drm::brightest();
    let Ok(bytes) = std::fs::read(FONT) else {
        crate::say!("screen: no font at {FONT}");
        return;
    };
    let Some(mut painter) = Painter::new(&bytes) else { return };
    // The panels may come up after init starts.
    let mut display = None;
    for _ in 0..20 {
        display = crate::drm::Display::open();
        if display.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let Some(mut display) = display else { return };
    let mut shown: Option<Status> = None;
    loop {
        let status = match std::fs::read_to_string(STATUS) {
            Ok(text) => Status::read(&text, &serial),
            Err(_) if std::path::Path::new(CONNECTED).exists() => Status { title: "item/grid is here".into(), detail: "waiting for its word".into(), ..Status::waiting(&serial) },
            Err(_) => Status::waiting(&serial),
        };
        if shown.as_ref() != Some(&status) {
            draw(&mut painter, &mut display, &status);
            shown = Some(status);
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// Both panels drawn: each connector its panel (in their order, left
/// first), or one wide connector as both with the hinge's gap between.
fn draw(painter: &mut Painter, display: &mut crate::drm::Display, s: &Status) {
    let single = display.outputs.len() == 1;
    for (i, o) in display.outputs.iter_mut().enumerate() {
        let (w, h, stride) = (o.width, o.height, o.stride);
        let panels: Vec<(usize, usize, usize)> = if single && w > h {
            let pw = (h * 3 / 4).min(w / 2);
            vec![(0, 0, pw), (1, w - pw, pw)]
        } else {
            vec![(i.min(1), 0, w)]
        };
        let px = o.pixels();
        px.fill(PAPER);
        for (which, x0, pw) in panels {
            let mut buf = vec![0u32; pw * h];
            painter.panel(which, s, &mut buf, pw, h, pw);
            for y in 0..h {
                px[y * stride + x0..y * stride + x0 + pw].copy_from_slice(&buf[y * pw..(y + 1) * pw]);
            }
        }
    }
    display.flush();
}
