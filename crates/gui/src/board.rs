//! Boards on the table: lines of words set a letter a square, turning up
//! as a departures board's flaps do (each square through a few letters to
//! its own, quick at first and slowing to the last), one line and one
//! letter a little after another; a line under the pointer darker; a line
//! clicked tells its key; closed, a board fades. The table is where Hythe
//! talks with whoever is at it: the sections' menu first.

use std::time::Instant;

/// A square turning over from showing `from` to showing `to` (`turn`
/// 0..1; 0 still, showing `from`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Flap {
    pub from: char,
    pub to: char,
    pub turn: f32,
}

fn smoother(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * x * (x * (6.0 * x - 15.0) + 10.0)
}

/// A square of a board (its letter `ch`, `seed` telling it from the others)
/// so far along (`p` 0..1) in turning up: from blank through a few letters
/// to its own, each turn slower than the one before.
pub fn flap(seed: usize, ch: char, p: f32) -> Flap {
    if p <= 0.0 || ch == ' ' {
        return Flap { from: ' ', to: ' ', turn: 0.0 };
    }
    let letters = b"abcdefghijklmnopqrstuvwxyz";
    let n = 3 + (seed * 5) % 3;
    let at = |k: usize| match k {
        0 => ' ',
        k if k >= n => ch,
        k => letters[(seed * 31 + k * 17 + 7) % 26] as char,
    };
    let eased = 1.0 - (1.0 - p.clamp(0.0, 1.0)).powi(3);
    let turns = eased * n as f32;
    let k = turns.floor() as usize;
    if k >= n {
        return Flap { from: ch, to: ch, turn: 0.0 };
    }
    Flap { from: at(k), to: at(k + 1), turn: smoother(turns - k as f32) }
}

/// A square to draw: its far left corner on the table (px), its flap, its
/// letter's colour.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Tile {
    pub at: (f32, f32),
    pub flap: Flap,
    pub rgba: (f64, f64, f64, f64),
}

pub struct Line {
    pub key: String,
    pub text: String,
}

/// Seconds: a square's turning up; after the line before, after the letter
/// before; the fading when closed.
const TURN_S: f32 = 1.3;
const LINE_AFTER: f32 = 0.1;
const LETTER_AFTER: f32 = 0.05;
const FADE_S: f32 = 0.3;

pub struct Board {
    pub lines: Vec<Line>,
    opened: Instant,
    closed: Option<Instant>,
    pub hover: Option<usize>,
}

impl Board {
    pub fn open(lines: Vec<Line>) -> Board {
        Board { lines, opened: Instant::now(), closed: None, hover: None }
    }

    pub fn close(&mut self) {
        self.closed.get_or_insert_with(Instant::now);
        self.hover = None;
    }

    pub fn closing(&self) -> bool {
        self.closed.is_some()
    }

    /// Faded away (to be dropped).
    pub fn gone(&self) -> bool {
        self.closed.is_some_and(|c| c.elapsed().as_secs_f32() >= FADE_S)
    }

    /// Still turning up or fading.
    pub fn moving(&self) -> bool {
        let longest = self.lines.iter().map(|l| l.text.chars().count()).max().unwrap_or(0);
        let all = self.lines.len() as f32 * LINE_AFTER + longest as f32 * LETTER_AFTER + TURN_S;
        self.opened.elapsed().as_secs_f32() < all + 0.05 || self.closed.is_some_and(|c| c.elapsed().as_secs_f32() < FADE_S + 0.05)
    }

    /// Its squares from `origin` (the first line's first square's far left
    /// corner), a line a row, squares of `side`.
    pub fn tiles(&self, origin: (f32, f32), side: f32) -> Vec<Tile> {
        let t = self.opened.elapsed().as_secs_f32();
        let fade = self.closed.map_or(1.0, |c| 1.0 - smoother(c.elapsed().as_secs_f32() / FADE_S));
        let mut out = Vec::new();
        for (r, line) in self.lines.iter().enumerate() {
            let grey = if self.hover == Some(r) { 0.12 } else { 0.42 };
            for (c, ch) in line.text.chars().enumerate() {
                if ch == ' ' {
                    continue;
                }
                let p = (t - r as f32 * LINE_AFTER - c as f32 * LETTER_AFTER) / TURN_S;
                out.push(Tile {
                    at: (origin.0 + c as f32 * side, origin.1 + r as f32 * side),
                    flap: flap(r * 13 + c, ch, p),
                    rgba: (grey, grey, grey * 1.02, 0.9 * fade as f64),
                });
            }
        }
        out
    }

    /// The line at a square of the table (counted from `origin` in squares
    /// of `side`): on its letters or the square after them.
    pub fn line_at(&self, origin: (f32, f32), side: f32, at: (f32, f32)) -> Option<usize> {
        if self.closing() {
            return None;
        }
        let (c, r) = (((at.0 - origin.0) / side).floor(), ((at.1 - origin.1) / side).floor());
        if c < 0.0 || r < 0.0 {
            return None;
        }
        let (c, r) = (c as usize, r as usize);
        self.lines.get(r).filter(|l| c <= l.text.chars().count()).map(|_| r)
    }
}
