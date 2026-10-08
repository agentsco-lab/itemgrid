//! Boards on the table: lines of words set a letter a square, turning up
//! as a departures board's squares do (each square turning over whole
//! through a few letters to its own, quick at first and slowing to the
//! last), one line and one
//! letter a little after another; a line under the pointer darker; a line
//! clicked tells its key; closed, a board fades. The table is where item/grid
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
    /// Raised off the table as a cube (its height, squares): a line that
    /// can be clicked, under the pointer.
    pub lift: f32,
}

pub struct Line {
    pub key: String,
    pub text: String,
    /// Set again since (its own turning up, the board's others left).
    pub since: Option<Instant>,
    /// Letters (from, to: their places in it) in a colour of their own.
    pub accent: Option<(usize, usize, (f64, f64, f64))>,
}

impl Line {
    pub fn new(key: impl Into<String>, text: impl Into<String>) -> Line {
        Line { key: key.into(), text: text.into(), since: None, accent: None }
    }
}

/// Seconds: a square's turning up; after the line before, after the letter
/// before; the fading when closed.
const TURN_S: f32 = 1.3;
const LINE_AFTER: f32 = 0.1;
const LETTER_AFTER: f32 = 0.05;
const FADE_S: f32 = 0.3;
/// A line's letters rising under the pointer.
const HOVER_S: f32 = 0.15;

pub struct Board {
    pub lines: Vec<Line>,
    opened: Instant,
    closed: Option<Instant>,
    pub hover: Option<usize>,
    /// Since when the line under the pointer is (its letters rising).
    hover_since: Option<Instant>,
    /// A line chosen (kept darker).
    pub chosen: Option<usize>,
}

impl Board {
    pub fn open(lines: Vec<Line>) -> Board {
        Board { lines, opened: crate::clock::now(), closed: None, hover: None, hover_since: None, chosen: None }
    }

    /// Line `i` set anew: its squares turn up again to the new words.
    pub fn set_line(&mut self, i: usize, text: impl Into<String>) {
        if let Some(l) = self.lines.get_mut(i) {
            l.text = text.into();
            l.since = Some(crate::clock::now());
        }
    }

    /// The widest line, in squares.
    pub fn width(&self) -> usize {
        self.lines.iter().map(|l| l.text.chars().count()).max().unwrap_or(0)
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
        self.closed.is_some_and(|c| crate::clock::secs_since(c) >= FADE_S)
    }

    /// Still turning up or fading.
    /// The line under the pointer (none: none): a line that can be
    /// clicked (it has a key) rises; whether that changed.
    pub fn set_hover(&mut self, line: Option<usize>) -> bool {
        let line = line.filter(|&l| self.lines.get(l).is_some_and(|l| !l.key.is_empty()));
        if self.hover == line {
            return false;
        }
        self.hover = line;
        self.hover_since = line.map(|_| crate::clock::now());
        true
    }

    /// How high line `r`'s letters stand (squares).
    fn lift(&self, r: usize) -> f32 {
        match (self.hover, self.hover_since) {
            (Some(h), Some(since)) if h == r && !self.lines[r].key.is_empty() => 0.3 * smoother(crate::clock::secs_since(since) / HOVER_S),
            _ => 0.0,
        }
    }

    pub fn moving(&self) -> bool {
        let longest = self.lines.iter().map(|l| l.text.chars().count()).max().unwrap_or(0);
        let all = self.lines.len() as f32 * LINE_AFTER + longest as f32 * LETTER_AFTER + TURN_S;
        let line = longest as f32 * LETTER_AFTER + TURN_S;
        crate::clock::secs_since(self.opened) < all + 0.05
            || self.lines.iter().any(|l| l.since.is_some_and(|s| crate::clock::secs_since(s) < line + 0.05))
            || self.closed.is_some_and(|c| crate::clock::secs_since(c) < FADE_S + 0.05)
            || self.hover_since.is_some_and(|s| crate::clock::secs_since(s) < HOVER_S + 0.05)
    }

    /// Its squares from `origin` (the first line's first square's far left
    /// corner), a line a row, squares of `side`.
    pub fn tiles(&self, origin: (f32, f32), side: f32) -> Vec<Tile> {
        let t = crate::clock::secs_since(self.opened);
        let fade = self.closed.map_or(1.0, |c| 1.0 - smoother(crate::clock::secs_since(c) / FADE_S));
        let mut out = Vec::new();
        for (r, line) in self.lines.iter().enumerate() {
            let grey = if self.hover == Some(r) || self.chosen == Some(r) { 0.12 } else { 0.42 };
            let lift = self.lift(r);
            // Set again: from then, alone.
            let (t, r_after) = line.since.map_or((t, r as f32 * LINE_AFTER), |s| (crate::clock::secs_since(s), 0.0));
            for (c, ch) in line.text.chars().enumerate() {
                if ch == ' ' {
                    continue;
                }
                let p = (t - r_after - c as f32 * LETTER_AFTER) / TURN_S;
                let (cr, cg, cb) = match line.accent {
                    Some((from, to, colour)) if (from..to).contains(&c) => colour,
                    _ => (grey, grey, grey * 1.02),
                };
                out.push(Tile {
                    at: (origin.0 + c as f32 * side, origin.1 + r as f32 * side),
                    flap: flap(r * 13 + c, ch, p),
                    rgba: (cr, cg, cb, 0.9 * fade as f64),
                    lift,
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
