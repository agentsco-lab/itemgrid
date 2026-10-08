//! The table's rim: how it goes see-through toward the page's edges, and
//! how the lines fade from the table's middle. Settings lines, for the
//! owner to look at them (2026-10-09: "давай поработаем с тем как
//! выглядят края, и смягчение и переход в прозрачное"). Kept in
//! ~/.config/itemgrid/rim.

use std::sync::Mutex;

use gtk::glib;

/// How wide the rim is, of the page's shorter side.
pub const WIDTHS: [(&str, f32); 5] = [("thin", 0.04), ("soft", 0.09), ("wide", 0.15), ("vast", 0.25), ("whole", 0.5)];
/// How the rim goes from the table to nothing across its width: even -
/// straight; soft - eased both ends; thin - most of the rim still the
/// table, gone quickly at the very edge; deep - see-through far in,
/// the table only well inside.
pub const CURVES: [&str; 4] = ["even", "soft", "thin", "deep"];
/// The rim's form: a frame along the page's edges (the corners where two
/// sides meet); rounded - a frame with rounded corners; oval - an
/// ellipse in the page, the corners gone.
pub const SHAPES: [&str; 3] = ["frame", "rounded", "oval"];
/// How far from the table's middle the lines fade out (times the
/// table's reach; 0: not at all - the lines to the rim).
pub const FADES: [(&str, f32); 4] = [("near", 0.65), ("soft", 1.0), ("far", 1.6), ("none", 0.0)];

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rim {
    pub width: usize,
    pub curve: usize,
    pub shape: usize,
    pub fade: usize,
}

impl Default for Rim {
    fn default() -> Rim {
        Rim { width: 1, curve: 1, shape: 0, fade: 1 }
    }
}

static RIM: Mutex<Option<Rim>> = Mutex::new(None);

fn file() -> std::path::PathBuf {
    glib::user_config_dir().join("itemgrid/rim")
}

fn read() -> Rim {
    let mut r = Rim::default();
    let Ok(text) = std::fs::read_to_string(file()) else { return r };
    for line in text.lines() {
        let Some((key, rest)) = line.trim().split_once(' ') else { continue };
        match key {
            "rim" => r.width = WIDTHS.iter().position(|w| w.0 == rest).unwrap_or(r.width),
            "curve" => r.curve = CURVES.iter().position(|c| *c == rest).unwrap_or(r.curve),
            "shape" => r.shape = SHAPES.iter().position(|c| *c == rest).unwrap_or(r.shape),
            "lines" => r.fade = FADES.iter().position(|f| f.0 == rest).unwrap_or(r.fade),
            _ => {}
        }
    }
    r
}

/// The rim as set (read from its file the first time).
pub fn get() -> Rim {
    let mut g = RIM.lock().unwrap_or_else(|e| e.into_inner());
    *g.get_or_insert_with(read)
}

/// The rim set, and kept.
pub fn set(r: Rim) {
    *RIM.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
    let text = format!("rim {}\ncurve {}\nshape {}\nlines {}\n", WIDTHS[r.width].0, CURVES[r.curve], SHAPES[r.shape], FADES[r.fade].0);
    let _ = std::fs::create_dir_all(file().parent().unwrap_or(std::path::Path::new(".")));
    let _ = std::fs::write(file(), text);
}

/// The rim's width (of the page's shorter side).
pub fn width() -> f32 {
    WIDTHS[get().width.min(WIDTHS.len() - 1)].1
}

/// How far the lines fade (times the reach; 0: not at all).
pub fn fade() -> f32 {
    FADES[get().fade.min(FADES.len() - 1)].1
}
