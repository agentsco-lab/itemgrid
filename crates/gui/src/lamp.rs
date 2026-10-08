//! The light under the table as a lamp: not the whole table lit through
//! its slits but a spot, falling off from where the lamp stands - the
//! owner: "как бы прожектор светит снизу" (2026-10-08). How strong, how
//! far, and where: placed (dragged in E, a square at a time), under the
//! phone, or at the pointer. Kept in ~/.config/itemgrid/lamp; before it
//! there was ~/.config/itemgrid/light (on or off), read if the lamp's file
//! is not there.

use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::Mutex;

use gtk::glib;

/// How strong the light is (times the one light there was).
pub const STRENGTHS: [(&str, f32); 5] = [("off", 0.0), ("dim", 0.5), ("soft", 1.0), ("bright", 1.7), ("full", 2.6)];
/// How far the spot reaches, in the table's squares (0: the whole table,
/// as the light was).
pub const REACHES: [(&str, f32); 4] = [("near", 4.0), ("middle", 7.0), ("wide", 12.0), ("all", 0.0)];
/// Where the lamp stands.
pub const MODES: [&str; 3] = ["placed", "under the phone", "at the pointer"];
pub const PLACED: usize = 0;
pub const PHONE: usize = 1;
pub const POINTER: usize = 2;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Lamp {
    pub strength: usize,
    pub reach: usize,
    pub mode: usize,
    /// Where it was put (squares from the sheet's top left); none: under
    /// the phone's middle, until dragged.
    pub at: Option<(f32, f32)>,
}

impl Default for Lamp {
    fn default() -> Lamp {
        Lamp { strength: 0, reach: 1, mode: PLACED, at: None }
    }
}

static LAMP: Mutex<Option<Lamp>> = Mutex::new(None);

fn file() -> std::path::PathBuf {
    glib::user_config_dir().join("itemgrid/lamp")
}

fn old_light_file() -> std::path::PathBuf {
    glib::user_config_dir().join("itemgrid/light")
}

fn read() -> Lamp {
    let mut l = Lamp::default();
    let Ok(text) = std::fs::read_to_string(file()) else {
        // The light there was: on, as the one light, over the whole table.
        if std::fs::read_to_string(old_light_file()).is_ok_and(|t| t.trim() == "on") {
            l.strength = 2;
            l.reach = 3;
        }
        return l;
    };
    for line in text.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        let rest = w.get(1..).map(|r| r.join(" ")).unwrap_or_default();
        match w.first().copied() {
            Some("light") => l.strength = STRENGTHS.iter().position(|s| s.0 == rest).unwrap_or(l.strength),
            Some("reach") => l.reach = REACHES.iter().position(|s| s.0 == rest).unwrap_or(l.reach),
            Some("lamp") => l.mode = MODES.iter().position(|s| *s == rest).unwrap_or(l.mode),
            Some("at") => {
                if let (Some(x), Some(y)) = (w.get(1).and_then(|v| v.parse().ok()), w.get(2).and_then(|v| v.parse().ok())) {
                    l.at = Some((x, y));
                }
            }
            _ => {}
        }
    }
    l
}

/// The lamp as set (read from its file the first time).
pub fn get() -> Lamp {
    let mut g = LAMP.lock().unwrap_or_else(|e| e.into_inner());
    *g.get_or_insert_with(read)
}

/// The lamp set, and kept.
pub fn set(l: Lamp) {
    *LAMP.lock().unwrap_or_else(|e| e.into_inner()) = Some(l);
    let mut text = format!("light {}\nreach {}\nlamp {}\n", STRENGTHS[l.strength].0, REACHES[l.reach].0, MODES[l.mode]);
    if let Some((x, y)) = l.at {
        text.push_str(&format!("at {x} {y}\n"));
    }
    let _ = std::fs::create_dir_all(file().parent().unwrap_or(std::path::Path::new(".")));
    let _ = std::fs::write(file(), text);
}

/// How strong the light is now (0: off).
pub fn strength() -> f32 {
    STRENGTHS[get().strength.min(STRENGTHS.len() - 1)].1
}

/// How far the spot reaches (squares; 0: the whole table).
pub fn reach_squares() -> f32 {
    REACHES[get().reach.min(REACHES.len() - 1)].1
}

/// The next of `n` after `i`, round.
pub fn next(i: usize, n: usize) -> usize {
    (i + 1) % n
}

// The table's point under the pointer (table px; none: off the table),
// for the lamp at the pointer - set as the pointer moves.
static POINTER_AT: [AtomicU32; 3] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];

pub fn pointer_at(p: Option<(f32, f32)>) {
    match p {
        Some((x, y)) => {
            POINTER_AT[0].store(x.to_bits(), Relaxed);
            POINTER_AT[1].store(y.to_bits(), Relaxed);
            POINTER_AT[2].store(1, Relaxed);
        }
        None => POINTER_AT[2].store(0, Relaxed),
    }
}

pub fn pointer() -> Option<(f32, f32)> {
    (POINTER_AT[2].load(Relaxed) == 1).then(|| (f32::from_bits(POINTER_AT[0].load(Relaxed)), f32::from_bits(POINTER_AT[1].load(Relaxed))))
}

// Where the lamp stood as laid this frame (squares from the sheet's top
// left: `at`, or under the phone), so a drag can start from it; and where
// it stood as the drag began.
static LAID: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
static DRAG_FROM: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];

pub fn laid(at: (f32, f32)) {
    LAID[0].store(at.0.to_bits(), Relaxed);
    LAID[1].store(at.1.to_bits(), Relaxed);
}

fn laid_at() -> (f32, f32) {
    (f32::from_bits(LAID[0].load(Relaxed)), f32::from_bits(LAID[1].load(Relaxed)))
}

pub fn drag_begin() {
    let (x, y) = laid_at();
    DRAG_FROM[0].store(x.to_bits(), Relaxed);
    DRAG_FROM[1].store(y.to_bits(), Relaxed);
}

/// The lamp moved `d` squares from where the drag began; kept.
pub fn drag_to(d: (f32, f32)) {
    let from = (f32::from_bits(DRAG_FROM[0].load(Relaxed)), f32::from_bits(DRAG_FROM[1].load(Relaxed)));
    set(Lamp { at: Some((from.0 + d.0, from.1 + d.1)), ..get() });
}
