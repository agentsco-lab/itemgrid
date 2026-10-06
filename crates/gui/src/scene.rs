//! What item/grid's frames are laid out by: its steps from the start on,
//! each with where its parts lie (in the table's squares) and how the eye
//! looks at them, and the table's looks for all of them. Laid in the
//! layout editor (F2, editor.rs); kept in ~/.config/itemgrid/layout (and
//! the editor's draft beside it, layout.draft).
//!
//! The file, a line each (older files - a part's place for all the steps,
//! `shot` lines - are read too):
//!
//!   paper white | lines black | line-width 1.6 | font lato
//!   step logo zoom 0.75 pan 0.1 0.1 top 0.36 words 0 15 duo 11 4 ...
//!   fixed logo agentsco

/// The steps, as the start and what comes after show them: the word alone,
/// the credit come with the squares, the eye down (the cubes standing), the
/// buttons up, the phone there, the menu open, a section open.
pub const STEPS: [&str; 7] = ["logo", "agentsco", "cubes", "buttons", "phone", "menu", "section"];

/// The parts the editor moves (their index is the part's number
/// everywhere): the phone's words, the Duo, the word, the buttons, the
/// menu, the credit.
pub const PARTS: [&str; 6] = ["phone words", "duo", "logo", "buttons", "menu", "credit"];

/// The table's paper (by day; the night keeps its dark), the lines' ink (by
/// day), their width, the letters' face - names as kept in the file; the
/// fonts those tools/fetch-fonts.sh brings (light), Lato first.
pub const PAPERS: [(&str, [f64; 3]); 10] = [
    ("white", [1.0, 1.0, 1.0]),
    ("warm", [0.980, 0.969, 0.941]),
    ("cream", [0.961, 0.937, 0.878]),
    ("sand", [0.949, 0.910, 0.847]),
    ("stone", [0.933, 0.933, 0.925]),
    ("cool", [0.945, 0.957, 0.973]),
    ("sky", [0.918, 0.949, 0.984]),
    ("mint", [0.933, 0.965, 0.945]),
    ("rose", [0.984, 0.941, 0.941]),
    ("lilac", [0.953, 0.941, 0.984]),
];
pub const INKS: [(&str, [f64; 3]); 10] = [
    ("black", [0.0, 0.0, 0.0]),
    ("graphite", [0.23, 0.23, 0.25]),
    ("blue", [0.11, 0.31, 0.85]),
    ("navy", [0.12, 0.16, 0.47]),
    ("teal", [0.06, 0.46, 0.43]),
    ("green", [0.08, 0.50, 0.24]),
    ("red", [0.73, 0.11, 0.11]),
    ("orange", [0.76, 0.25, 0.05]),
    ("brown", [0.49, 0.29, 0.12]),
    ("violet", [0.43, 0.16, 0.85]),
];
/// The lines' width, px (1.6 the old one).
pub const WIDTHS: [f64; 8] = [0.6, 1.0, 1.3, 1.6, 2.0, 2.5, 3.2, 4.5];
pub const FONTS: [(&str, &str); 20] = [
    ("lato", "Lato"),
    ("inter", "Inter"),
    ("roboto", "Roboto"),
    ("open sans", "Open Sans"),
    ("montserrat", "Montserrat"),
    ("poppins", "Poppins"),
    ("raleway", "Raleway"),
    ("nunito", "Nunito"),
    ("work sans", "Work Sans"),
    ("fira sans", "Fira Sans"),
    ("plex", "IBM Plex Sans"),
    ("jetbrains", "JetBrains Mono"),
    ("grotesk", "Space Grotesk"),
    ("manrope", "Manrope"),
    ("rubik", "Rubik"),
    ("quicksand", "Quicksand"),
    ("comfortaa", "Comfortaa"),
    ("josefin", "Josefin Sans"),
    ("playfair", "Playfair Display"),
    ("merri", "Merriweather"),
];

/// The looks chosen: indices into PAPERS, INKS, WIDTHS, FONTS.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Style {
    pub paper: usize,
    pub ink: usize,
    pub width: usize,
    pub font: usize,
}

impl Default for Style {
    fn default() -> Style {
        Style { paper: 0, ink: 0, width: 3, font: 0 }
    }
}

/// Where a step's parts lie, in the table's squares: the phone's words
/// (their first letter's square) and the Duo (its top left open flat; none:
/// in the middle of what is right of its words) from the sheet's top left;
/// the word, the buttons, the menu and the credit moved from where they
/// come by themselves; the Duo's size.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Place {
    pub words: (f32, f32),
    pub duo: Option<(f32, f32)>,
    pub word: (f32, f32),
    pub buttons: (f32, f32),
    pub menu: (f32, f32),
    pub credit: (f32, f32),
    pub duo_scale: f32,
}

impl Default for Place {
    fn default() -> Place {
        Place { words: (0.0, 4.0), duo: None, word: (0.0, 0.0), buttons: (0.0, 0.0), menu: (0.0, 0.0), credit: (0.0, 0.0), duo_scale: 1.0 }
    }
}

fn lerp2(a: (f32, f32), b: (f32, f32), f: f32) -> (f32, f32) {
    (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f)
}

impl Place {
    /// On the way to `to` (`f` 0..1): each part moving there.
    pub fn mix(self, to: Place, f: f32) -> Place {
        Place {
            words: lerp2(self.words, to.words, f),
            duo: match (self.duo, to.duo) {
                (Some(a), Some(b)) => Some(lerp2(a, b, f)),
                (a, b) => if f < 0.5 { a } else { b },
            },
            word: lerp2(self.word, to.word, f),
            buttons: lerp2(self.buttons, to.buttons, f),
            menu: lerp2(self.menu, to.menu, f),
            credit: lerp2(self.credit, to.credit, f),
            duo_scale: self.duo_scale + (to.duo_scale - self.duo_scale) * f,
        }
    }

    /// Part `i`'s place (the Duo's: none while it is where it comes by
    /// itself).
    pub fn part(&self, i: usize) -> Option<(f32, f32)> {
        match i {
            0 => Some(self.words),
            1 => self.duo,
            2 => Some(self.word),
            3 => Some(self.buttons),
            4 => Some(self.menu),
            _ => Some(self.credit),
        }
    }

    pub fn set_part(&mut self, i: usize, to: (f32, f32)) {
        match i {
            0 => self.words = to,
            1 => self.duo = Some(to),
            2 => self.word = to,
            3 => self.buttons = to,
            4 => self.menu = to,
            _ => self.credit = to,
        }
    }
}

/// A step's eye: its lens (1 as it comes by itself), how far it is moved
/// over the table (squares), and how far it has risen toward straight
/// above (0 as it comes by itself .. 1 straight above, the row of the word
/// and the buttons in the page's middle).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shot {
    pub zoom: f32,
    pub pan: (f32, f32),
    pub top: f32,
}

impl Default for Shot {
    fn default() -> Shot {
        Shot { zoom: 1.0, pan: (0.0, 0.0), top: 0.0 }
    }
}

impl Shot {
    pub fn mix(self, to: Shot, f: f32) -> Shot {
        Shot { zoom: self.zoom + (to.zoom - self.zoom) * f, pan: lerp2(self.pan, to.pan, f), top: self.top + (to.top - self.top) * f }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Step {
    pub place: Place,
    pub shot: Shot,
}

impl Step {
    pub fn mix(self, to: Step, f: f32) -> Step {
        Step { place: self.place.mix(to.place, f), shot: self.shot.mix(to.shot, f) }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Layout {
    pub steps: [Step; STEPS.len()],
    pub style: Style,
    /// The steps fixed (ok pressed on them, nothing changed since).
    pub fixed: [bool; STEPS.len()],
}

impl Layout {
    /// The step the frame is at, as the start and what comes after go on:
    /// `marks` how far each next step has come (the squares grown, the eye
    /// down, the buttons up, the phone there, the menu open, a section
    /// open) - each step's parts and eye on the way into the next; and the
    /// step it is at most (the last more than half come).
    pub fn at(&self, marks: [f32; STEPS.len() - 1]) -> (Step, usize) {
        let mut step = self.steps[0];
        let mut now = 0;
        for (i, f) in marks.into_iter().enumerate() {
            step = step.mix(self.steps[i + 1], f);
            if f >= 0.5 {
                now = i + 1;
            }
        }
        (step, now)
    }

    /// The layout in its file's words.
    pub fn read(text: &str) -> Layout {
        let mut l = Layout::default();
        // An older file: the parts' places for all the steps, the eye's
        // lens for all, the steps' eyes (`shot`).
        let mut all = Place::default();
        let mut all_zoom: Option<f32> = None;
        let mut shots: [Option<(f32, (f32, f32))>; STEPS.len()] = [None; STEPS.len()];
        let mut steps: [Option<Step>; STEPS.len()] = [None; STEPS.len()];
        let step_named = |n: &str| STEPS.iter().position(|s| *s == n).or(["word", "grid"].iter().position(|s| *s == n));
        for line in text.lines() {
            let w: Vec<&str> = line.split_whitespace().collect();
            let num = |i: usize| w.get(i).and_then(|v| v.parse::<f32>().ok());
            let xy = |i: usize| Some((num(i)?, num(i + 1)?));
            match w.first() {
                Some(&"words") => all.words = xy(1).unwrap_or(all.words),
                Some(&"duo") => all.duo = xy(1).or(all.duo),
                Some(&"word") => all.word = xy(1).unwrap_or(all.word),
                Some(&"buttons") => all.buttons = xy(1).unwrap_or(all.buttons),
                Some(&"menu") => all.menu = xy(1).unwrap_or(all.menu),
                Some(&"credit") => all.credit = xy(1).unwrap_or(all.credit),
                Some(&"duo-scale") => all.duo_scale = num(1).unwrap_or(all.duo_scale),
                Some(&"zoom") => all_zoom = num(1).or(all_zoom),
                Some(&"shot") => {
                    if let (Some(i), Some(zoom), Some(pan)) = (w.get(1).and_then(|n| step_named(n)), num(2), xy(3)) {
                        shots[i] = Some((zoom, pan));
                    }
                }
                Some(&"step") => {
                    let Some(i) = w.get(1).and_then(|n| step_named(n)) else { continue };
                    let mut s = Step::default();
                    let mut k = 2;
                    while let Some(key) = w.get(k) {
                        let two = xy(k + 1);
                        let one = num(k + 1);
                        k += match *key {
                            "zoom" => one.map(|v| s.shot.zoom = v).map_or(1, |_| 2),
                            "top" => one.map(|v| s.shot.top = v).map_or(1, |_| 2),
                            "duo-scale" => one.map(|v| s.place.duo_scale = v).map_or(1, |_| 2),
                            "pan" => two.map(|v| s.shot.pan = v).map_or(1, |_| 3),
                            "words" => two.map(|v| s.place.words = v).map_or(1, |_| 3),
                            "duo" => two.map(|v| s.place.duo = Some(v)).map_or(1, |_| 3),
                            "word" => two.map(|v| s.place.word = v).map_or(1, |_| 3),
                            "buttons" => two.map(|v| s.place.buttons = v).map_or(1, |_| 3),
                            "menu" => two.map(|v| s.place.menu = v).map_or(1, |_| 3),
                            "credit" => two.map(|v| s.place.credit = v).map_or(1, |_| 3),
                            _ => 1,
                        };
                    }
                    steps[i] = Some(s);
                }
                Some(&"fixed") => {
                    for n in &w[1..] {
                        if let Some(i) = STEPS.iter().position(|s| s == n) {
                            l.fixed[i] = true;
                        }
                    }
                }
                Some(&"paper") => l.style.paper = named(&PAPERS.map(|p| p.0), &w[1..]).unwrap_or(l.style.paper),
                Some(&"lines") => l.style.ink = named(&INKS.map(|p| p.0), &w[1..]).unwrap_or(l.style.ink),
                Some(&"line-width") => l.style.width = num(1).and_then(|v| WIDTHS.iter().position(|w| (*w - v as f64).abs() < 0.01)).unwrap_or(l.style.width),
                Some(&"font") => l.style.font = named(&FONTS.map(|p| p.0), &w[1..]).unwrap_or(l.style.font),
                _ => {}
            }
        }
        for i in 0..STEPS.len() {
            l.steps[i] = steps[i].unwrap_or_else(|| {
                // From an older file: the places for all, the step's eye (the
                // lens for all before the steps had their own). The eye rose
                // with the lens then (drawn back): kept so.
                let (zoom, pan) = shots[i].unwrap_or((all_zoom.unwrap_or(1.0), (0.0, 0.0)));
                let up = ((1.0 - zoom) / 0.6).clamp(0.0, 1.0);
                Step { place: all, shot: Shot { zoom, pan, top: up * up * (3.0 - 2.0 * up) } }
            });
        }
        l
    }

    /// The layout in its file's words.
    pub fn write(&self) -> String {
        let s = self.style;
        let mut text = format!("paper {}\nlines {}\nline-width {}\nfont {}\n", PAPERS[s.paper].0, INKS[s.ink].0, WIDTHS[s.width], FONTS[s.font].0);
        for (i, st) in self.steps.iter().enumerate() {
            let (p, e) = (st.place, st.shot);
            text.push_str(&format!("step {} zoom {:.3} pan {:.2} {:.2} top {:.3}", STEPS[i], e.zoom, e.pan.0, e.pan.1, e.top));
            text.push_str(&format!(" words {} {}", p.words.0, p.words.1));
            if let Some(d) = p.duo {
                text.push_str(&format!(" duo {} {}", d.0, d.1));
            }
            text.push_str(&format!(" word {} {} buttons {} {} menu {} {} credit {} {} duo-scale {:.3}\n", p.word.0, p.word.1, p.buttons.0, p.buttons.1, p.menu.0, p.menu.1, p.credit.0, p.credit.1, p.duo_scale));
        }
        let fixed: Vec<&str> = (0..STEPS.len()).filter(|&i| self.fixed[i]).map(|i| STEPS[i]).collect();
        if !fixed.is_empty() {
            text.push_str(&format!("fixed {}\n", fixed.join(" ")));
        }
        text
    }

    /// Whether `other` lays the frames out the same (the fixed marks
    /// aside).
    pub fn same(&self, other: &Layout) -> bool {
        self.steps == other.steps && self.style == other.style
    }
}

/// Which of `names` the words `w` are.
fn named(names: &[&str], w: &[&str]) -> Option<usize> {
    let name = w.join(" ");
    names.iter().position(|n| *n == name)
}

fn dir() -> std::path::PathBuf {
    gtk::glib::user_config_dir().join("itemgrid")
}

/// The layout kept, else the default.
pub fn read_saved() -> Layout {
    Layout::read(&std::fs::read_to_string(dir().join("layout")).unwrap_or_default())
}

pub fn save(l: &Layout) {
    let _ = std::fs::create_dir_all(dir());
    let _ = std::fs::write(dir().join("layout"), l.write());
}

/// The editor's draft (what it shows, not yet kept with ok), if any.
pub fn read_draft() -> Option<Layout> {
    std::fs::read_to_string(dir().join("layout.draft")).ok().map(|t| Layout::read(&t))
}

pub fn save_draft(l: &Layout) {
    let _ = std::fs::create_dir_all(dir());
    let _ = std::fs::write(dir().join("layout.draft"), l.write());
}

pub fn drop_draft() {
    let _ = std::fs::remove_file(dir().join("layout.draft"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_file_is_read_for_every_step() {
        let l = Layout::read("words 0 15\nduo 11 4\nmenu 0 1\nzoom 0.751\npaper lilac\nfont open sans\nfixed logo\n");
        for s in &l.steps {
            assert_eq!(s.place.words, (0.0, 15.0));
            assert_eq!(s.place.duo, Some((11.0, 4.0)));
            assert_eq!(s.place.menu, (0.0, 1.0));
            assert!((s.shot.zoom - 0.751).abs() < 1e-4);
            assert!(s.shot.top > 0.3 && s.shot.top < 0.4);
        }
        assert_eq!(PAPERS[l.style.paper].0, "lilac");
        assert_eq!(FONTS[l.style.font].0, "open sans");
        assert!(l.fixed[0] && !l.fixed[1]);
    }

    #[test]
    fn a_layout_comes_back_from_its_file() {
        let mut l = Layout::default();
        l.steps[2].place.word = (3.0, -1.0);
        l.steps[4].place.duo = Some((9.0, 4.0));
        l.steps[4].shot = Shot { zoom: 0.8, pan: (1.5, -2.25), top: 0.2 };
        l.style.font = 18;
        l.fixed[4] = true;
        let back = Layout::read(&l.write());
        assert_eq!(back, l);
    }

    #[test]
    fn the_steps_go_one_into_the_next() {
        let mut l = Layout::default();
        l.steps[1].place.word = (2.0, 0.0);
        let (s, now) = l.at([0.5, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(s.place.word, (1.0, 0.0));
        assert_eq!(now, 1);
    }
}
