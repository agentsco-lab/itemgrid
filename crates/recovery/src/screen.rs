//! What the recovery shows on the phone, drawn as item/grid draws its
//! table: a dark grid of squares, a letter a square (the owner's rule: text
//! only in the squares; a big word is a letter on 2x2 of them), lowercase;
//! as little of it as will do (the owner, 2026-10-29): the stage in a word
//! or two, big; how long is left; keep the cable in. The way is shown by
//! light, not words: a lamp under the table, its light through the slits
//! between the squares in item/grid's accent, goes left to right across
//! both panels as the job goes. item's water drop, clear (the table seen
//! larger through it), a square and a half wide, rides over the lamp on a
//! spring, sways and breathes: something always moves. item/grid quiet for
//! a while, the drop stops and breathes slowly, the light dims.
//!
//! Words are typed in a square at a time and erased the same way. A moment
//! worth it (ready, item is on, stopped) is zoomed on: the words erased,
//! the squares grown, the words typed again in the bigger squares; held at
//! least 10 s; back out the same way. Nothing is ever cut.
//!
//! item/grid tells it in /run/item-screen, a line a field:
//!   title writing|item     (the stage; "|" between its words)
//!   detail 2 min left      (the small line under it)
//!   foot keep the cable in (the line at the bottom; "|" between lines)
//!   path 0.54              (how much of the whole way is gone: the lamp)
//!   moment 1               (zoom on it, if its words are short enough)
//!   state work|done|error
//!   accent #86d68f         (item/grid's accent: the light's colour)
//!   line #ffffff           (the grid as item/grid's window draws it:
//!   strength 1.0            its lines' colour by night, how strong,
//!   width 2.0               how wide)
//! Before it does: ready for item/grid.

use std::time::Instant;

use fontdue::{Font, FontSettings};

pub const STATUS: &str = "/run/item-screen";
/// The font in the initramfs (item/grid's own: Open Sans Light, OFL).
pub const FONT: &str = "/usr/share/item-recovery/font.ttf";

#[derive(Clone, Default, PartialEq)]
pub struct Status {
    pub title: String,
    pub detail: String,
    pub foot: String,
    pub path: Option<f32>,
    pub moment: bool,
    pub state: String,
    pub accent: Option<u32>,
    /// The grid's lines: colour, strength (1 plain), width (px on the
    /// computer's table).
    pub line: Option<u32>,
    pub strength: f32,
    pub width: f32,
    /// A test picture in place of all of it (the panels' colour looked
    /// at: greys in steps, colour bars, the same on both).
    pub pattern: bool,
}

impl Status {
    pub fn waiting() -> Status {
        Status { title: "ready".into(), detail: "for item/grid".into(), foot: "keep the cable in".into(), state: "wait".into(), moment: true, ..Default::default() }
    }

    /// The status as item/grid wrote it.
    pub fn read(text: &str) -> Status {
        let mut s = Status { state: "work".into(), ..Default::default() };
        for line in text.lines() {
            let (k, v) = line.split_once(' ').unwrap_or((line, ""));
            let v = v.trim().to_lowercase();
            match k {
                "title" => s.title = v,
                "detail" => s.detail = v,
                "foot" => s.foot = v,
                "path" => s.path = v.parse::<f32>().ok().map(|p| p.clamp(0.0, 1.0)),
                "moment" => s.moment = v == "1" || v == "yes",
                "state" => s.state = v,
                "accent" => s.accent = hex(&v),
                "line" => s.line = hex(&v),
                "strength" => s.strength = v.parse().unwrap_or(0.0),
                "width" => s.width = v.parse().unwrap_or(0.0),
                "pattern" => s.pattern = v == "1" || v == "yes",
                _ => {}
            }
        }
        if s.state != "work" {
            s.moment = true;
        }
        s
    }
}

fn hex(v: &str) -> Option<u32> {
    u32::from_str_radix(v.trim_start_matches('#'), 16).ok()
}

/// Colours, 0xRRGGBB: the table at night, the letters (lit, dim).
const PAPER: u32 = 0x242526;
const DIM: u32 = 0x77797c;
const LIT: u32 = 0xe4e6e8;
/// item/grid's own green (its accent cube's), when it says none; an
/// error's light.
const ACCENT: u32 = 0x86d68f;
const RED: u32 = 0xe8695e;

/// A panel's squares across (the Duo's 1350 px: 75 px squares).
const ACROSS: usize = 18;
/// The square row the words begin on, and the zoom is anchored on.
const TITLE_ROW: f32 = 6.0;
/// The lamp's height on the panels (of the height).
const LAMP_Y: f32 = 0.62;
/// item's bezel edge (edge.frag's default): how soft, its corners' radius
/// (px).
const EDGE_SOFT: f32 = 12.0;
const EDGE_CORNER: f32 = 24.0;
/// The drop's radius, of a square; how far from the hinge (of its radius)
/// it waits on its side.
const DROP_R: f32 = 1.1;
const HINGE_ZONE: f32 = 1.25;
/// item/grid quiet this long while at work: the drop stops, the light dims.
const QUIET_S: f32 = 12.0;
/// A letter typed, erased; a zoom; a zoom held at least.
const TYPE_S: f32 = 0.055;
const ERASE_S: f32 = 0.028;
const ZOOM_S: f32 = 0.9;
const HOLD_S: f32 = 10.0;

/// What the left panel says: the stage's words, the small line, the foot.
#[derive(Clone, PartialEq, Default)]
struct Words {
    title: Vec<String>,
    detail: String,
    foot: Vec<String>,
    moment: bool,
}

impl Words {
    fn of(s: &Status, quiet: bool) -> Words {
        let detail = if quiet { "waiting for item/grid".to_owned() } else { s.detail.clone() };
        let foot = if s.foot.is_empty() {
            vec![match s.state.as_str() {
                "done" => "restarting now".to_owned(),
                _ => "keep the cable in".to_owned(),
            }]
        } else {
            s.foot.split('|').map(str::to_owned).collect()
        };
        Words { title: s.title.split('|').map(str::to_owned).filter(|w| !w.is_empty()).collect(), detail, foot, moment: s.moment }
    }

    /// The squares bigger for a moment as far as its longest word still
    /// fits (a letter on 2x2, from the second square), and only if that is
    /// a real change (12 across at most); else 1.
    fn scale(&self) -> f32 {
        if !self.moment {
            return 1.0;
        }
        let n = self.title.iter().map(|w| w.chars().count()).max().unwrap_or(1);
        let across = (2 * n + 2).max(8);
        if across <= 12 {
            ACROSS as f32 / across as f32
        } else {
            1.0
        }
    }

    fn letters(&self) -> usize {
        self.title.iter().map(|w| w.chars().count()).sum::<usize>() + self.detail.chars().count() + self.foot.iter().map(|l| l.chars().count()).sum::<usize>()
    }
}

fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// What moves, frame to frame.
pub struct Scene {
    began: Instant,
    last: f32,
    /// The words shown, typed since `typed_at`; or being erased (since
    /// when, and what comes after).
    words: Option<Words>,
    typed_at: f32,
    erasing: Option<(Words, f32, Words)>,
    /// The left panel's scale (1 calm), going from `from` to `to` since
    /// `zoom_at`; zoomed in since `zoomed_at`.
    scale: f32,
    from: f32,
    to: f32,
    zoom_at: f32,
    zoomed_at: f32,
    /// The drop across both panels (0 to 2 panel widths), its speed (panel
    /// widths a second), how much it wobbles.
    drop_x: f32,
    drop_v: f32,
    wobble: f32,
    started: bool,
}

/// A frame's motion, as the panels draw it.
#[derive(Clone)]
pub struct Frame {
    pub t: f32,
    pub scale: f32,
    words: Words,
    /// Letters shown of the words (typed so far, or kept while erasing).
    shown: usize,
    pub drop_x: f32,
    pub drop_v: f32,
    pub wobble: f32,
    pub lamp_x: f32,
    pub glow: f32,
    pub quiet: bool,
}

impl Scene {
    pub fn new() -> Scene {
        Scene { began: Instant::now(), last: 0.0, words: None, typed_at: 0.0, erasing: None, scale: 1.0, from: 1.0, to: 1.0, zoom_at: -100.0, zoomed_at: -100.0, drop_x: 0.0, drop_v: 0.0, wobble: 0.0, started: false }
    }

    pub fn frame(&mut self, s: &Status, quiet: bool) -> Frame {
        let t = self.began.elapsed().as_secs_f32();
        let dt = (t - self.last).clamp(0.0, 0.1);
        self.last = t;
        let want = Words::of(s, quiet);
        if self.words.is_none() {
            self.scale = want.scale();
            self.to = self.scale;
            if self.scale > 1.0 {
                self.zoomed_at = t;
            }
            self.words = Some(want.clone());
            self.typed_at = t;
        }
        if let Some((old, at, next)) = self.erasing.clone() {
            if t - at > old.letters() as f32 * ERASE_S + 0.12 {
                let target = next.scale();
                let zoom = (target - self.scale).abs() > 0.01;
                if zoom {
                    self.from = self.scale;
                    self.to = target;
                    self.zoom_at = t;
                    if target > 1.0 {
                        self.zoomed_at = t;
                    }
                }
                self.typed_at = t + if zoom { ZOOM_S } else { 0.0 };
                self.words = Some(next);
                self.erasing = None;
            }
        } else if t - self.zoom_at >= ZOOM_S {
            let cur = self.words.clone().unwrap_or_default();
            if cur.title != want.title || cur.moment != want.moment {
                // A zoom stays at least HOLD_S before it goes back out.
                let target = want.scale();
                let held = self.to > 1.0 && (target - self.to).abs() > 0.01 && t - self.zoomed_at < HOLD_S;
                if !held {
                    self.erasing = Some((cur, t, want));
                }
            } else if cur.detail != want.detail || cur.foot != want.foot {
                // The small lines change in place.
                self.words = Some(Words { detail: want.detail, foot: want.foot, ..cur });
            }
        }
        let z = (t - self.zoom_at) / ZOOM_S;
        self.scale = if z < 1.0 { self.from + (self.to - self.from) * ease(z) } else { self.to };
        // The lamp: where the way has got to (done: the hinge, all lit).
        let lamp_x = match s.state.as_str() {
            "done" => 1.0,
            "wait" => 0.3,
            _ => s.path.unwrap_or(0.0) * 2.0,
        };
        let glow = match s.state.as_str() {
            "done" => 1.0,
            "error" => 0.8,
            _ if quiet => 0.35 + 0.1 * (t * 1.1).sin(),
            _ => 0.85 + 0.15 * (t * 2.4).sin(),
        };
        // The drop on a spring to the lamp, swaying a little; still while
        // item/grid is quiet.
        if !self.started {
            self.drop_x = lamp_x;
            self.started = true;
        }
        if quiet {
            self.drop_v *= 0.9;
            self.wobble *= 0.95;
        } else {
            let sway = (t / 0.9).sin() * 0.6 / ACROSS as f32;
            // By the hinge the drop waits on its side until the lamp is
            // past it, then goes over at once (a crossing, not a crawl).
            let mut target = lamp_x + sway;
            let hz = HINGE_ZONE * DROP_R / ACROSS as f32;
            if (target - 1.0).abs() < hz {
                target = if self.drop_x < 1.0 { 1.0 - hz } else { 1.0 + hz };
            }
            let a = 18.0 * (target - self.drop_x) - 7.0 * self.drop_v;
            self.drop_v += a * dt;
            self.drop_x += self.drop_v * dt;
            self.wobble = (self.wobble * 0.96 + a.abs() * 0.002).min(1.0);
        }
        let (words, shown) = match &self.erasing {
            Some((w, at, _)) => (w.clone(), w.letters().saturating_sub(((t - at) / ERASE_S) as usize)),
            None => {
                let w = self.words.clone().unwrap_or_default();
                let n = if t < self.typed_at { 0 } else { ((t - self.typed_at) / TYPE_S) as usize };
                (w, n)
            }
        };
        Frame { t, scale: self.scale, words, shown, drop_x: self.drop_x, drop_v: self.drop_v, wobble: self.wobble, lamp_x, glow, quiet }
    }
}

/// A vignette's table: the panel and its size.
type KeepKey = (usize, usize, usize);
/// A still light's: the panel, its size, the zoom, the lamp's place, the
/// glow and the accent.
type LitKey = (usize, usize, usize, u32, i32, i32, u32);

/// How long each part of the last panel took (ms): the table copied, the
/// light, the words, the drop - for `preview`'s bench.
pub static PARTS: std::sync::Mutex<[f32; 4]> = std::sync::Mutex::new([0.0; 4]);

pub struct Painter {
    font: Font,
    cache: std::collections::HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
    /// The rim's keeping a pixel (255 kept whole), for the panel drawn now,
    /// and the other's.
    keep: Option<(KeepKey, Vec<u8>)>,
    keeps: Vec<(KeepKey, Vec<u8>)>,
    /// The paper a panel, darkened to its outer edges (dithered): the
    /// grid's lines are drawn over it each frame, at any zoom.
    papers: Vec<(KeepKey, Vec<u32>)>,
    /// Pixels under the drop, as they were before it.
    under: Vec<u32>,
    /// A panel with a still light on it, and what it was made of.
    lit: Option<(LitKey, Vec<u32>)>,
}

impl Painter {
    pub fn new(font_bytes: &[u8]) -> Option<Painter> {
        let font = Font::from_bytes(font_bytes, FontSettings::default()).ok()?;
        Some(Painter { font, cache: Default::default(), keep: None, keeps: Vec::new(), papers: Vec::new(), under: Vec::new(), lit: None })
    }

    /// The panel `which` (0 left, 1 right) of `w` x `h` pixels, 0xXXRRGGBB
    /// a pixel, rows of `stride` pixels, as `f` has it moving.
    #[allow(clippy::too_many_arguments)]
    pub fn panel(&mut self, which: usize, s: &Status, f: &Frame, px: &mut [u32], w: usize, h: usize, stride: usize) {
        if s.pattern {
            test_picture(px, w, h, stride);
            return;
        }
        let base_sq = w as f32 / ACROSS as f32;
        // One table over both panels, zoomed as one about the words' first
        // square (the left panel's column 1, row TITLE_ROW): its lines go
        // on across the hinge at any zoom.
        let zoom = f.scale.max(1.0);
        let sq = base_sq * zoom;
        let ax = base_sq * (1.0 - zoom);
        let ox = ax - which as f32 * w as f32;
        let oy = TITLE_ROW * base_sq * (1.0 - zoom);
        self.keep_for(which, w, h);
        let key = (which, w, h);
        if !self.papers.iter().any(|(k, _)| *k == key) {
            let paper = self.paper(w, h);
            self.papers.push((key, paper));
        }
        let t0 = Instant::now();
        let paper = &self.papers.iter().find(|(k, _)| *k == key).unwrap().1;
        for y in 0..h {
            px[y * stride..y * stride + w].copy_from_slice(&paper[y * w..(y + 1) * w]);
        }
        let keep = self.keep.take().unwrap();
        grid_lines(px, w, h, stride, &keep.1, s, sq, ox, oy);
        let t1 = Instant::now();
        let accent = if s.state == "error" { RED } else { s.accent.unwrap_or(ACCENT) };
        // The light under the table, round the lamp - a still one (done,
        // an error) worked out once and kept.
        let lamp = (f.lamp_x * w as f32 - which as f32 * w as f32, h as f32 * LAMP_Y);
        let reach = base_sq * if s.state == "done" { 13.0 } else { 4.5 };
        let still = s.state == "done" || s.state == "error";
        let lit_key = (which, w, h, (zoom * 1000.0) as u32, lamp.0 as i32, (f.glow * 64.0) as i32, accent);
        if still && self.lit.as_ref().is_some_and(|(k, _)| *k == lit_key) {
            let lit = &self.lit.as_ref().unwrap().1;
            for y in 0..h {
                px[y * stride..y * stride + w].copy_from_slice(&lit[y * w..(y + 1) * w]);
            }
        } else {
            light(px, w, h, stride, &keep.1, s, sq, ox, oy, lamp, reach, f.glow, accent);
            if still {
                let mut lit = vec![0u32; w * h];
                for y in 0..h {
                    lit[y * w..(y + 1) * w].copy_from_slice(&px[y * stride..y * stride + w]);
                }
                self.lit = Some((lit_key, lit));
            }
        }
        let t2 = Instant::now();
        // The words, as typed so far, a letter a square.
        if which == 0 {
            let mut g = Grid { px: &mut *px, stride, w, h, sq, ox, oy, keep: &keep.1 };
            let mut left = f.shown;
            let mut take = |word: &str| -> String {
                let n = word.chars().count().min(left);
                left -= n;
                word.chars().take(n).collect()
            };
            let mut r = TITLE_ROW;
            for word in &f.words.title {
                let shown = take(word);
                self.words(&mut g, 1.0, r, &shown, LIT, 2);
                r += 2.0;
            }
            // The small lines broken between words to the squares there are
            // now (fewer, zoomed in): nothing cut.
            let cols = (((w as f32 - ox) / sq).floor() as usize).saturating_sub(1).max(4);
            r += 1.0;
            for l in wrap(&f.words.detail, cols) {
                let shown = take(&l);
                self.words(&mut g, 1.0, r, &shown, DIM, 1);
                r += 1.0;
            }
            let foot: Vec<String> = f.words.foot.iter().flat_map(|l| wrap(l, cols)).collect();
            let bottom = ((h as f32 - oy) / sq).floor() - 2.0 - foot.len() as f32;
            for (i, l) in foot.iter().enumerate() {
                let shown = take(l);
                self.words(&mut g, 1.0, bottom + i as f32, &shown, DIM, 1);
            }
        }
        let t3 = Instant::now();
        // The drop, over all of it. By the hinge, as item's dock drops
        // cross it: nearing it, held - shorter along its way, fuller
        // across; across it, in two, each part against its edge (ending
        // straight there: the rest is in the hinge, unseen), the water going
        // over from one to the other - the one shrinking, the other coming
        // out small and round and growing.
        let r = DROP_R * base_sq / w as f32;
        let cy = lamp.1 - base_sq * 0.2;
        let over = ((f.drop_x - (1.0 - r)) / (2.0 * r)).clamp(0.0, 1.0);
        if over > 0.0 && over < 1.0 {
            let share = if which == 0 { 1.0 - over } else { over };
            let k = share.sqrt();
            let rx = DROP_R * base_sq * k;
            let cx = if which == 0 { w as f32 - rx * 0.55 } else { rx * 0.55 };
            if k > 0.05 {
                self.drop(px, w, h, stride, &keep.1, cx, cy, base_sq, f, k, k);
            }
        } else {
            let dx = f.drop_x * w as f32 - which as f32 * w as f32;
            if dx > -base_sq * 3.0 && dx < w as f32 + base_sq * 3.0 {
                // Held as it nears the hinge (within a third of a square).
                let gap = ((f.drop_x - 1.0).abs() - r) * w as f32 / base_sq;
                let held = (1.0 - gap / 0.35).clamp(0.0, 1.0);
                self.drop(px, w, h, stride, &keep.1, dx, cy, base_sq, f, 1.0 - 0.1 * held, 1.0 + 0.08 * held);
            }
        }
        self.keep = Some(keep);
        let ms = |a: Instant, b: Instant| (b - a).as_secs_f32() * 1000.0;
        *PARTS.lock().unwrap() = [ms(t0, t1), ms(t1, t2), ms(t2, t3), ms(t3, Instant::now())];
    }

    /// How much of a pixel of panel `which` is kept (255 whole): item's
    /// vignette (vignette.frag, its default: strength 0.55, reach 0.5,
    /// soft 0.6), soft as on the phone's own screens - but over both
    /// panels as one, so only their outer edges and corners darken, not
    /// the hinge's (the owner, 2026-10-29).
    fn keep_for(&mut self, which: usize, w: usize, h: usize) {
        let key = (which, w, h);
        if self.keep.as_ref().is_some_and(|k| k.0 == key) {
            return;
        }
        if let Some(i) = self.keeps.iter().position(|k| k.0 == key) {
            let k = self.keeps.remove(i);
            if let Some(old) = self.keep.take() {
                self.keeps.push(old);
            }
            self.keep = Some(k);
            return;
        }
        const STRENGTH: f32 = 0.55;
        let start = 1.3 + (0.12 - 1.3) * 0.5;
        let width = 0.1 + (1.15 - 0.1) * 0.6;
        let tw = 2.0 * w as f32;
        // item's bezel edge (edge.frag, its default): outside a rectangle
        // with corners rounded EDGE_CORNER px, black, soft EDGE_SOFT px in;
        // the rectangle going on past the hinge, so nothing there.
        let extra = EDGE_SOFT + EDGE_CORNER + 2.0;
        let (lo, hi) = (if which == 1 { -extra } else { 0.0 }, w as f32 + if which == 0 { extra } else { 0.0 });
        let (mx, hx) = ((lo + hi) / 2.0, (hi - lo) / 2.0 - EDGE_CORNER);
        let (my, hy) = (h as f32 / 2.0, h as f32 / 2.0 - EDGE_CORNER);
        let mut keep = vec![255u8; w * h];
        for y in 0..h {
            let qy = ((y as f32 + 0.5) / h as f32 - 0.5) * 2.0 * 0.92;
            let ey = (y as f32 + 0.5 - my).abs() - hy;
            for x in 0..w {
                let qx = (((x + which * w) as f32 + 0.5) / tw - 0.5) * 2.0;
                let t = (((qx * qx + qy * qy).sqrt() - start) / width).clamp(0.0, 1.0);
                let av = STRENGTH * t * t * (3.0 - 2.0 * t);
                let ex = (x as f32 + 0.5 - mx).abs() - hx;
                let inside = EDGE_CORNER - ((ex.max(0.0).powi(2) + ey.max(0.0).powi(2)).sqrt() + ex.max(ey).min(0.0));
                let e = (inside / EDGE_SOFT).clamp(0.0, 1.0);
                let edge = e * e * (3.0 - 2.0 * e);
                keep[y * w + x] = ((1.0 - av) * edge * 255.0).round() as u8;
            }
        }
        if let Some(old) = self.keep.take() {
            self.keeps.push(old);
        }
        self.keeps.truncate(3);
        self.keep = Some((key, keep));
        self.papers.retain(|(k, _)| k.0 != which);
    }

    /// The paper of a panel, darkened as `keep` has it, dithered (a 4x4
    /// ordered threshold: the paper darkened in 8 bits a channel falls
    /// into wide steps otherwise). Black stays black.
    fn paper(&self, w: usize, h: usize) -> Vec<u32> {
        const BAYER: [u32; 16] = [8, 136, 40, 168, 200, 72, 232, 104, 56, 184, 24, 152, 248, 120, 216, 88];
        let keep = &self.keep.as_ref().unwrap().1;
        let mut px = vec![PAPER; w * h];
        for y in 0..h {
            for x in 0..w {
                let k = keep[y * w + x] as u32;
                if k == 255 {
                    continue;
                }
                let t = BAYER[(y & 3) * 4 + (x & 3)];
                let ch = |v: u32| ((v & 0xff) * k + t) >> 8;
                px[y * w + x] = (ch(PAPER >> 16) << 16) | (ch(PAPER >> 8) << 8) | ch(PAPER);
            }
        }
        px
    }

    /// `text` from square (`col`, `row`), a letter a square - or, `big`, a
    /// letter on `big` x `big` of them.
    fn words(&mut self, g: &mut Grid, col: f32, row: f32, text: &str, colour: u32, big: usize) {
        for (i, ch) in text.chars().enumerate() {
            if ch != ' ' {
                self.letter(g, col + (i * big) as f32, row, ch, colour, big);
            }
        }
    }

    fn letter(&mut self, g: &mut Grid, col: f32, row: f32, ch: char, colour: u32, big: usize) {
        let span = g.sq * big as f32;
        let size = (span * 0.52).round() as u32;
        if size < 4 {
            return;
        }
        let font = &self.font;
        let (m, bitmap) = self.cache.entry((ch, size)).or_insert_with(|| font.rasterize(ch, size as f32));
        // Centred in its squares, on a baseline a little under their middle.
        let x0 = (g.ox + col * g.sq + (span - m.width as f32) / 2.0).round() as i64;
        let base = g.oy + row * g.sq + span / 2.0 + size as f32 * 0.36;
        let y0 = base.round() as i64 - m.height as i64 - m.ymin as i64;
        for y in 0..m.height {
            let py = y0 + y as i64;
            if py < 0 || py >= g.h as i64 {
                continue;
            }
            for x in 0..m.width {
                let a = bitmap[y * m.width + x] as u32;
                let pxx = x0 + x as i64;
                if a == 0 || pxx < 0 || pxx >= g.w as i64 {
                    continue;
                }
                let (py, pxx) = (py as usize, pxx as usize);
                let k = g.keep[py * g.w + pxx] as u32;
                let p = &mut g.px[py * g.stride + pxx];
                *p = blend(*p, shade(colour, k), a);
            }
        }
    }

    /// item's water drop on the table, clear: what is under it seen larger
    /// (a lens), a little darker to its rim, a light ring along its edge,
    /// light gathered inside along its bottom, a small sharp highlight, a
    /// soft shadow; a square and a half wide, stretched as it moves,
    /// wobbling, breathing.
    #[allow(clippy::too_many_arguments)]
    fn drop(&mut self, px: &mut [u32], w: usize, h: usize, stride: usize, keep: &[u8], cx: f32, cy: f32, sq: f32, f: &Frame, sx: f32, sy: f32) {
        let breath = 1.0 + 0.035 * (f.t * if f.quiet { 0.9 } else { 1.9 }).sin();
        let stretch = 1.0 + (f.drop_v.abs() * 0.9).min(0.35);
        let wob = f.wobble * (f.t * 22.0).sin() * 0.08;
        let (rx, ry) = (sq * DROP_R * breath * (stretch + wob) * sx, sq * DROP_R * breath / stretch.sqrt() * (1.0 - wob) * sy);
        let (xa, xb) = (((cx - rx * 1.35) as i64).max(0) as usize, ((cx + rx * 1.35) as i64).clamp(0, w as i64) as usize);
        let (ya, yb) = (((cy - ry * 1.1) as i64).max(0) as usize, ((cy + ry * 1.7) as i64).clamp(0, h as i64) as usize);
        if xa >= xb || ya >= yb {
            return;
        }
        let bw = xb - xa;
        self.under.clear();
        for y in ya..yb {
            self.under.extend_from_slice(&px[y * stride + xa..y * stride + xb]);
        }
        let under = &self.under;
        let at = |x: usize, y: usize| under[(y - ya) * bw + (x - xa)];
        for y in ya..yb {
            for x in xa..xb {
                let (ux, uy) = ((x as f32 + 0.5 - cx) / rx, (y as f32 + 0.5 - cy) / ry);
                let r2 = ux * ux + uy * uy;
                let p = &mut px[y * stride + x];
                if r2 >= 1.0 {
                    // The shadow, soft, a little under it.
                    let (sx, sy) = (ux / 1.15, (y as f32 + 0.5 - cy - ry * 0.35) / (ry * 1.1));
                    let sd = (sx * sx + sy * sy).sqrt();
                    if sd < 1.0 {
                        *p = blend(*p, 0, (80.0 * (1.0 - sd).powf(1.5)) as u32);
                    }
                    continue;
                }
                let r = r2.sqrt();
                // The lens: the middle magnified, the edge pulled in.
                let m = 0.55 + 0.45 * r2;
                let sx = ((cx + ux * rx * m) as i64).clamp(xa as i64, xb as i64 - 1) as usize;
                let sy = ((cy + uy * ry * m) as i64).clamp(ya as i64, yb as i64 - 1) as usize;
                let src = at(sx, sy);
                let dark = 1.04 - 0.1 * r2;
                let rim = if r > 0.82 { ((r - 0.82) / 0.13).min(1.0) * (1.0 - ((r - 0.96) / 0.04).max(0.0)) } else { 0.0 };
                let low = if uy > 0.3 && r < 0.92 { ((uy - 0.3) / 0.6).min(1.0) * (1.0 - r) * 0.3 } else { 0.0 };
                let (hx, hy) = (ux + 0.36, uy + 0.42);
                let high = hx * hx + hy * hy < 0.022;
                let ch = |v: u32| -> u32 {
                    let mut c = (v & 0xff) as f32 * dark;
                    c += (255.0 - c) * (rim * 0.45 + low);
                    if high {
                        c = 250.0;
                    }
                    c.clamp(0.0, 255.0) as u32
                };
                let k = keep[y * w + x] as u32;
                let water = shade((ch(src >> 16) << 16) | (ch(src >> 8) << 8) | ch(src), if high || rim > 0.0 { k } else { 255 });
                let edge = ((1.0 - r) * rx.min(ry) * 0.9).clamp(0.0, 1.0);
                *p = blend(at(x, y), water, (edge * 255.0) as u32);
            }
        }
    }
}

/// The grid's lines over the paper, as item/grid's window has them (their
/// colour on the dark table, how strong, how wide; else a faint white),
/// darkened with it: only the lines' pixels touched, so any zoom is cheap.
#[allow(clippy::too_many_arguments)]
fn grid_lines(px: &mut [u32], w: usize, h: usize, stride: usize, keep: &[u8], s: &Status, sq: f32, ox: f32, oy: f32) {
    let line = blend(PAPER, s.line.unwrap_or(0xffffff), line_alpha(s));
    let thick = line_thick(s, sq).round().max(1.0) as i64;
    let first = |o: f32| ((-o) / sq).floor() as i64 - 1;
    let mut k = first(ox);
    loop {
        let x0 = (ox + k as f32 * sq - thick as f32 / 2.0).round() as i64;
        if x0 >= w as i64 {
            break;
        }
        for x in x0.max(0)..(x0 + thick).min(w as i64) {
            let x = x as usize;
            for y in 0..h {
                px[y * stride + x] = shade(line, keep[y * w + x] as u32);
            }
        }
        k += 1;
    }
    let mut k = first(oy);
    loop {
        let y0 = (oy + k as f32 * sq - thick as f32 / 2.0).round() as i64;
        if y0 >= h as i64 {
            break;
        }
        for y in y0.max(0)..(y0 + thick).min(h as i64) {
            let y = y as usize;
            for x in 0..w {
                px[y * stride + x] = shade(line, keep[y * w + x] as u32);
            }
        }
        k += 1;
    }
}

/// The test picture: the upper half greys in 16 steps (black to white,
/// left to right, in two rows: the darks larger below), the lower half
/// bars of red, green, blue, cyan, magenta, yellow, white at full and
/// half; nothing darkened, nothing dithered: the panels as they are.
fn test_picture(px: &mut [u32], w: usize, h: usize, stride: usize) {
    for y in 0..h {
        for x in 0..w {
            let c = if y < h / 4 {
                let v = (x * 16 / w) as u32 * 255 / 15;
                (v << 16) | (v << 8) | v
            } else if y < h / 2 {
                // The darks: 0 to 15 of 255, in 16 steps.
                let v = (x * 16 / w) as u32;
                (v << 16) | (v << 8) | v
            } else {
                const BARS: [u32; 7] = [0xff0000, 0x00ff00, 0x0000ff, 0x00ffff, 0xff00ff, 0xffff00, 0xffffff];
                let bar = BARS[(x * 7 / w).min(6)];
                if y < h * 3 / 4 {
                    bar
                } else {
                    (bar >> 1) & 0x7f7f7f
                }
            };
            px[y * stride + x] = c;
        }
    }
}

/// The light's falloff by the distance squared (of its reach, in 1024
/// steps): the lines' (1 - d)^1.6 and the paper's (1 - d)^1.3 - worked
/// out once (a root and two powers a pixel cost 120 ms a frame).
fn falloff() -> &'static ([f32; 1025], [f32; 1025]) {
    static F: std::sync::OnceLock<([f32; 1025], [f32; 1025])> = std::sync::OnceLock::new();
    F.get_or_init(|| {
        let mut lines = [0f32; 1025];
        let mut paper = [0f32; 1025];
        for i in 0..=1024 {
            let d = (i as f32 / 1024.0).sqrt();
            lines[i] = (1.0 - d).powf(1.6);
            paper[i] = (1.0 - d).powf(1.3);
        }
        (lines, paper)
    })
}

/// The lamp's light round `lamp` as far as `reach`: the lines near it lit
/// in the accent, wider and stronger the nearer, and a soft light over the
/// paper between them, as much as `glow`.
#[allow(clippy::too_many_arguments)]
fn light(px: &mut [u32], w: usize, h: usize, stride: usize, keep: &[u8], s: &Status, sq: f32, ox: f32, oy: f32, lamp: (f32, f32), reach: f32, glow: f32, accent: u32) {
    let xa = (lamp.0 - reach).max(0.0) as usize;
    let xb = (lamp.0 + reach).clamp(0.0, w as f32) as usize;
    let ya = (lamp.1 - reach / 1.25).max(0.0) as usize;
    let yb = (lamp.1 + reach / 1.25).clamp(0.0, h as f32) as usize;
    if xa >= xb || ya >= yb {
        return;
    }
    let (lines, paper) = falloff();
    let base_line = line_alpha(s) as f32 / 255.0;
    let half = line_thick(s, sq) / 2.0;
    let ink = s.line.unwrap_or(0xffffff);
    let inv = 1024.0 / (reach * reach);
    // Each column's distance to its line and to the lamp, once.
    let cols: Vec<(f32, f32)> = (xa..xb).map(|x| (dist_to_line(x as f32 + 0.5, ox, sq), (x as f32 + 0.5 - lamp.0).powi(2))).collect();
    for y in ya..yb {
        let dy = dist_to_line(y as f32 + 0.5, oy, sq);
        let ey2 = ((y as f32 + 0.5 - lamp.1) * 1.25).powi(2);
        let row = y * stride;
        for (i, &(dx, ex2)) in cols.iter().enumerate() {
            let q = (ex2 + ey2) * inv;
            if q >= 1024.0 {
                continue;
            }
            let qi = q as usize;
            let k = lines[qi] * glow;
            let x = xa + i;
            let kp = keep[y * w + x] as u32;
            let p = &mut px[row + x];
            // The soft light over the paper.
            let mut c = blend(*p, shade(accent, kp), (0.10 * glow * paper[qi] * 255.0) as u32);
            // A line here, as wide as the light makes it.
            let on = (half + 1.1 * k - dx.min(dy) + 0.5).clamp(0.0, 1.0);
            if on > 0.0 {
                let colour = blend(ink, accent, (k * 255.0) as u32);
                c = blend(c, shade(colour, kp), ((base_line + 0.55 * k) * on * 255.0) as u32);
            }
            *p = c;
        }
    }
}

struct Grid<'a> {
    px: &'a mut [u32],
    stride: usize,
    w: usize,
    h: usize,
    sq: f32,
    ox: f32,
    oy: f32,
    keep: &'a [u8],
}

/// How far `v` is from the nearest of the lines at `o + k * sq`.
fn dist_to_line(v: f32, o: f32, sq: f32) -> f32 {
    let m = (v - o).rem_euclid(sq);
    m.min(sq - m)
}

fn line_alpha(s: &Status) -> u32 {
    let strength = if s.strength > 0.0 { s.strength } else { 1.0 };
    ((0.16 * strength).clamp(0.04, 0.5) * 255.0) as u32
}

fn line_thick(s: &Status, sq: f32) -> f32 {
    if s.width > 0.0 {
        (s.width * 0.8 * sq / 75.0).clamp(1.0, 6.0)
    } else {
        1.0
    }
}

/// `text` broken between words into lines of at most `wide` letters.
fn wrap(text: &str, wide: usize) -> Vec<String> {
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

/// A colour darkened by the vignette's `k` (255 kept whole).
fn shade(c: u32, k: u32) -> u32 {
    if k >= 255 {
        return c;
    }
    let ch = |v: u32| (v & 0xff) * k / 255;
    (ch(c >> 16) << 16) | (ch(c >> 8) << 8) | ch(c)
}

fn blend(under: u32, over: u32, a: u32) -> u32 {
    let a = a.min(255);
    let ch = |c: u32, s: u32| ((c >> s & 0xff) * (255 - a) + (over >> s & 0xff) * a) / 255;
    (ch(under, 16) << 16) | (ch(under, 8) << 8) | ch(under, 0)
}

/// Both panels as portable pixmaps in `dir`, for looking at the design on
/// a computer (`item-recovery preview DIR FONT`): DIR's *.status files if
/// it has any (a status as item/grid writes it; "waiting" alone the
/// waiting), else a few of an install's - each a few seconds into it (the
/// words typed, the drop come to rest).
pub fn preview(dir: &str, font: &str) -> i32 {
    let Ok(bytes) = std::fs::read(font) else {
        eprintln!("{font}: not read");
        return 1;
    };
    let Some(mut p) = Painter::new(&bytes) else { return 1 };
    let grid = std::env::var_os("HOME").and_then(|h| std::fs::read_to_string(std::path::Path::new(&h).join(".config/itemgrid/recovery-grid")).ok()).unwrap_or_default();
    let own: Vec<(String, Status)> = std::fs::read_dir(dir)
        .map(|d| {
            let mut v: Vec<_> = d
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().strip_suffix(".status")?.to_owned();
                    let text = std::fs::read_to_string(e.path()).ok()?;
                    Some((name, if text.trim() == "waiting" { Status::waiting() } else { Status::read(&text) }))
                })
                .collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v
        })
        .unwrap_or_default();
    let builtin = [
        ("waiting", Status::waiting()),
        ("writing", Status::read("title writing|item\ndetail 2 min left\npath 0.55\nstate work")),
        ("done", Status::read("title item|is on\ndetail unlock with your pin\npath 1\nstate done")),
        ("error", Status::read("title stopped\ndetail the cable dropped\nfoot nothing is lost|plug the cable in|install again\npath 0.5\nstate error")),
    ];
    let cases: Vec<(String, Status)> = if own.is_empty() { builtin.into_iter().map(|(n, s)| (n.to_owned(), s)).collect() } else { own };
    let (w, h) = (1350, 1800);
    for (name, mut s) in cases {
        let g = Status::read(&grid);
        (s.line, s.strength, s.width) = (g.line, g.strength, g.width);
        if s.accent.is_none() {
            s.accent = Some(ACCENT);
        }
        // A scene run on a few seconds: the words typed, the zoom done.
        let mut scene = Scene::new();
        let mut f = scene.frame(&s, false);
        for _ in 0..80 {
            scene.began -= std::time::Duration::from_millis(50);
            f = scene.frame(&s, false);
        }
        for which in 0..2 {
            let mut px = vec![0u32; w * h];
            p.panel(which, &s, &f, &mut px, w, h, w);
            if std::env::var_os("ITEM_BENCH").is_some() {
                let t = Instant::now();
                for _ in 0..20 {
                    p.panel(which, &s, &f, &mut px, w, h, w);
                }
                let parts = *PARTS.lock().unwrap();
                eprintln!("{name} panel {which}: {:.1} ms a frame; the last: table {:.1}, light {:.1}, words {:.1}, drop {:.1}", t.elapsed().as_secs_f32() * 50.0, parts[0], parts[1], parts[2], parts[3]);
            }
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

/// The screen kept moving as the status says, on whatever panels DRM gives
/// (a thread of PID 1's; it gives up quietly if there is nothing to show
/// on: the recovery works by the cable alone). About 30 frames a second;
/// how long a frame takes to draw and to put on the panels is said to the
/// kernel's log every 10 s (`item-recovery log`).
pub fn run(_serial: String) {
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
    let mut scene = Scene::new();
    let mut bufs: Vec<Vec<u32>> = Vec::new();
    let mut colour_last: Option<String> = None;
    let (mut frames, mut draw_ms, mut flush_ms, mut since) = (0u32, 0f32, 0f32, Instant::now());
    const FRAME: std::time::Duration = std::time::Duration::from_millis(33);
    loop {
        let start = Instant::now();
        let read = std::fs::read_to_string(STATUS);
        let quiet_for = std::fs::metadata(STATUS).and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).map_or(0.0, |d| d.as_secs_f32());
        let status = match read {
            Ok(text) => Status::read(&text),
            Err(_) if std::path::Path::new(CONNECTED).exists() => Status { detail: "item/grid is here".into(), ..Status::waiting() },
            Err(_) => Status::waiting(),
        };
        // The panels' colour, as item/grid asks (colour.rs).
        if let Some(want) = crate::colour::wanted(&mut colour_last) {
            crate::colour::apply(&display, &want);
        }
        let f = scene.frame(&status, status.state == "work" && quiet_for > QUIET_S);
        draw(&mut painter, &mut display, &status, &f, &mut bufs);
        let drawn = Instant::now();
        display.flush();
        draw_ms += (drawn - start).as_secs_f32() * 1000.0;
        flush_ms += drawn.elapsed().as_secs_f32() * 1000.0;
        frames += 1;
        if since.elapsed().as_secs() >= 10 {
            let s = since.elapsed().as_secs_f32();
            crate::say!("screen: {:.1} frames/s, drawing {:.0} ms, putting on the panels {:.0} ms a frame", frames as f32 / s, draw_ms / frames as f32, flush_ms / frames as f32);
            (frames, draw_ms, flush_ms, since) = (0, 0.0, 0.0, Instant::now());
        }
        if let Some(rest) = FRAME.checked_sub(start.elapsed()) {
            std::thread::sleep(rest);
        }
    }
}

/// Both panels drawn in memory of our own and copied into the buffers not
/// shown, a row at a time: the buffers are the card's, not cached - drawn
/// into straight, every blend's read of them cost a frame 100 ms on the
/// phone (5 ms here in memory).
fn draw(painter: &mut Painter, display: &mut crate::drm::Display, s: &Status, f: &Frame, bufs: &mut Vec<Vec<u32>>) {
    let single = display.outputs.len() == 1;
    let mut b = 0;
    for (i, o) in display.outputs.iter_mut().enumerate() {
        let (w, h, stride) = (o.width, o.height, o.stride);
        let panels: Vec<(usize, usize, usize)> = if single && w > h {
            let pw = (h * 3 / 4).min(w / 2);
            vec![(0, 0, pw), (1, w - pw, pw)]
        } else {
            vec![(i.min(1), 0, w)]
        };
        for (which, x0, pw) in panels {
            if bufs.len() <= b {
                bufs.push(Vec::new());
            }
            let buf = &mut bufs[b];
            buf.resize(pw * h, 0);
            painter.panel(which, s, f, buf, pw, h, pw);
            let px = o.pixels();
            for y in 0..h {
                px[y * stride + x0..y * stride + x0 + pw].copy_from_slice(&buf[y * pw..(y + 1) * pw]);
            }
            b += 1;
        }
    }
}
