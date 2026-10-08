//! What item/grid's frames are laid out by: its steps from the start on,
//! each with where its parts lie (in the table's squares) and how the eye
//! looks at them and how it comes on, and the table's looks for all of
//! them - the layout in data/layout (laid out with the owner, kept in the
//! app; a line each):
//!
//!   paper white | lines black | line-width 1.6 | font lato
//!   step logo zoom 0.75 pan 0.1 0.1 top 0.36 tilt 0 turn 0 delay 0 secs 0.5
//!       ease smooth words 0 15 duo 11 4 word 0 0 ... [show .. bright ..
//!       height ..]

/// The frame's own size: laid out at this, shown scaled to the page.
pub const REF: (f32, f32) = (1000.0, 800.0);
/// As the wallpaper: a screen's (16:9).
pub const REF_WALL: (f32, f32) = (1600.0, 900.0);

/// The steps, as the start and what comes after show them: the word alone,
/// the credit come with the squares, the eye down (the cubes standing), the
/// buttons up; no phone: looked for (the wave), not found (the note); the
/// phone there (on Wi-Fi, shut), opened (a click), on the cable; the menu
/// and a section open (with the phone, and with none: "bare"); the
/// wallpaper.
pub const STEPS: [&str; 14] = [
    "logo", "agentsco", "cubes", "buttons", "search", "not found", "phone", "phone open", "cable", "menu", "section", "menu bare", "section bare", "wallpaper",
];

/// Where a step not yet laid out (not in the file) takes its places and
/// eye from: the one it comes from.
const PARENT: [usize; STEPS.len()] = [0, 0, 1, 2, 3, 4, 3, 6, 6, 6, 9, 9, 10, 3];

/// The parts laid out (their index is the part's number everywhere): the
/// phone's words, the Duo, the word, the buttons, the menu, the credit.
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
    /// Each part (PARTS) shown (1) or hidden (0; between, fading), how
    /// strong it is drawn (1 as it comes), and how high it stands: the
    /// word's cubes (times as high as they come), the buttons (raised as
    /// boxes, squares).
    pub show: [f32; PARTS.len()],
    pub bright: [f32; PARTS.len()],
    pub height: [f32; PARTS.len()],
}

/// The parts' heights as they come: the word's cubes as they stand, the
/// buttons flat.
pub const HEIGHTS: [f32; PARTS.len()] = [1.0, 1.0, 1.0, 0.0, 1.0, 1.0];

impl Default for Place {
    fn default() -> Place {
        Place {
            words: (0.0, 4.0),
            duo: None,
            word: (0.0, 0.0),
            buttons: (0.0, 0.0),
            menu: (0.0, 0.0),
            credit: (0.0, 0.0),
            duo_scale: 1.0,
            show: [1.0; PARTS.len()],
            bright: [1.0; PARTS.len()],
            height: HEIGHTS,
        }
    }
}

fn lerp_all<const N: usize>(a: [f32; N], b: [f32; N], f: f32) -> [f32; N] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * f)
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
            show: lerp_all(self.show, to.show, f),
            bright: lerp_all(self.bright, to.bright, f),
            height: lerp_all(self.height, to.height, f),
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
    /// The eye tipped further down (degrees: more, nearer straight above;
    /// less, nearer the table) and turned round the table (degrees).
    pub tilt: f32,
    pub turn: f32,
}

impl Default for Shot {
    fn default() -> Shot {
        Shot { zoom: 1.0, pan: (0.0, 0.0), top: 0.0, tilt: 0.0, turn: 0.0 }
    }
}

impl Shot {
    pub fn mix(self, to: Shot, f: f32) -> Shot {
        let l = |a: f32, b: f32| a + (b - a) * f;
        Shot { zoom: l(self.zoom, to.zoom), pan: lerp2(self.pan, to.pan, f), top: l(self.top, to.top), tilt: l(self.tilt, to.tilt), turn: l(self.turn, to.turn) }
    }
}

/// How a step comes on: its curve.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Ease {
    Linear,
    Smooth,
    Smoother,
    Out,
    In,
}

pub const EASES: [(&str, Ease); 5] = [("linear", Ease::Linear), ("smooth", Ease::Smooth), ("smoother", Ease::Smoother), ("ease out", Ease::Out), ("ease in", Ease::In)];

impl Ease {
    /// Its value `x` of the way (0..1).
    pub fn at(self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Ease::Linear => x,
            Ease::Smooth => x * x * (3.0 - 2.0 * x),
            Ease::Smoother => x * x * x * (x * (6.0 * x - 15.0) + 10.0),
            Ease::Out => 1.0 - (1.0 - x).powi(3),
            Ease::In => x * x * x,
        }
    }

    pub fn name(self) -> &'static str {
        EASES.iter().find(|e| e.1 == self).map_or("smooth", |e| e.0)
    }
}

/// The way into a step: how long after it is due it begins (the start's
/// steps: after the step before began; the others: after what brings them
/// - the phone found, the menu or a section opened), how long it takes,
/// its curve.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Trans {
    pub delay: f32,
    pub secs: f32,
    pub ease: Ease,
}

impl Trans {
    /// How far on (0..1, eased) at `raw` of the whole way (delay and all,
    /// 0..1).
    pub fn eased(self, raw: f32) -> f32 {
        let all = (self.delay + self.secs).max(1e-3);
        self.ease.at((raw * all - self.delay) / self.secs.max(1e-3))
    }

    /// The whole way's seconds.
    pub fn all(self) -> f32 {
        (self.delay + self.secs).max(1e-3)
    }
}

/// Each step's way in as it came by itself: the word (half a second), the
/// squares grown and the credit (from 0.55 s), the eye down (from 1.9 s),
/// the cubes down and the buttons up (as the eye stops); the wave, the
/// note; the Duo the last moment of 1.1 s once found, opened, the cable;
/// the menu a third of a second; a section 0.9 s; the wallpaper.
pub const TRANS: [Trans; STEPS.len()] = [
    Trans { delay: 0.0, secs: 0.5, ease: Ease::Smooth },
    Trans { delay: 0.55, secs: 1.55, ease: Ease::Smooth },
    Trans { delay: 1.35, secs: 1.7, ease: Ease::Smoother },
    Trans { delay: 1.7, secs: 1.2, ease: Ease::Linear },
    Trans { delay: 0.0, secs: 0.3, ease: Ease::Smooth },
    Trans { delay: 0.0, secs: 0.4, ease: Ease::Smooth },
    Trans { delay: 0.94, secs: 0.16, ease: Ease::Smooth },
    Trans { delay: 0.0, secs: 0.6, ease: Ease::Smoother },
    Trans { delay: 0.0, secs: 0.6, ease: Ease::Smooth },
    Trans { delay: 0.0, secs: 0.35, ease: Ease::Smooth },
    Trans { delay: 0.0, secs: 0.9, ease: Ease::Smooth },
    Trans { delay: 0.0, secs: 0.35, ease: Ease::Smooth },
    Trans { delay: 0.0, secs: 0.9, ease: Ease::Smooth },
    Trans { delay: 0.0, secs: 0.8, ease: Ease::Smooth },
];

/// The steps by name (their index).
pub const SEARCH: usize = 4;
pub const NOT_FOUND: usize = 5;
pub const PHONE: usize = 6;
pub const PHONE_OPEN: usize = 7;
pub const CABLE: usize = 8;
pub const MENU: usize = 9;
pub const SECTION: usize = 10;
pub const MENU_BARE: usize = 11;
pub const SECTION_BARE: usize = 12;
pub const WALLPAPER: usize = 13;

/// The start's steps: those that come by themselves, one after the other.
pub const START_STEPS: usize = 4;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Step {
    pub place: Place,
    pub shot: Shot,
    pub trans: Trans,
}

impl Default for Step {
    fn default() -> Step {
        Step { place: Place::default(), shot: Shot::default(), trans: TRANS[0] }
    }
}

impl Step {
    /// On the way to `to`: its parts and eye (the way in stays the
    /// step's).
    pub fn mix(self, to: Step, f: f32) -> Step {
        Step { place: self.place.mix(to.place, f), shot: self.shot.mix(to.shot, f), trans: self.trans }
    }
}

/// How far each step after the start has come (0..1): its way, not yet
/// eased (the start's own come from its seconds).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Marks {
    pub search: f32,
    pub not_found: f32,
    pub phone: f32,
    pub open: f32,
    pub cable: f32,
    pub menu: f32,
    pub section: f32,
    pub wall: f32,
}


/// The steps' weights in a frame (summing to 1).
#[derive(Clone, Copy, Debug)]
struct Blend([f32; STEPS.len()]);

impl Blend {
    fn of(i: usize) -> Blend {
        let mut w = [0.0; STEPS.len()];
        w[i] = 1.0;
        Blend(w)
    }

    fn to(self, other: Blend, f: f32) -> Blend {
        let f = f.clamp(0.0, 1.0);
        Blend(std::array::from_fn(|i| self.0[i] * (1.0 - f) + other.0[i] * f))
    }

    fn into_step(self, i: usize, f: f32) -> Blend {
        self.to(Blend::of(i), f)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Layout {
    pub steps: [Step; STEPS.len()],
    pub style: Style,
}

impl Default for Layout {
    fn default() -> Layout {
        Layout { steps: std::array::from_fn(|i| Step { trans: TRANS[i], ..Step::default() }), style: Style::default() }
    }
}

impl Layout {
    /// The ways in, step by step.
    pub fn timing(&self) -> [Trans; STEPS.len()] {
        self.steps.map(|s| s.trans)
    }

}

impl Layout {
    /// The frame's step, as the start and what comes after go on: the
    /// start's steps one into the next (`start`: the squares grown, the eye
    /// down, the buttons up); then, with no phone, looked for and not
    /// found; with it (`m.phone`), shut, opened, on the cable; the menu and
    /// a section open (the phone's or the bare ones, as the phone is
    /// there); the wallpaper. `m` eased. Each step's parts and eye weighed
    /// in; the step weighing most.
    pub fn at(&self, start: [f32; 3], m: Marks) -> (Step, usize) {
        let mut b = Blend::of(0).into_step(1, start[0]).into_step(2, start[1]).into_step(3, start[2]);
        b = b.into_step(SEARCH, m.search).into_step(NOT_FOUND, m.not_found);
        let phone = Blend::of(PHONE).into_step(PHONE_OPEN, m.open).into_step(CABLE, m.cable);
        b = b.to(phone, m.phone);
        b = b.to(Blend::of(MENU_BARE).into_step(MENU, m.phone), m.menu);
        b = b.to(Blend::of(SECTION_BARE).into_step(SECTION, m.phone), m.section);
        b = b.into_step(WALLPAPER, m.wall);
        // Weighed in one by one (each a mean of those before and it).
        let mut order: Vec<usize> = (0..STEPS.len()).filter(|&i| b.0[i] > 1e-5).collect();
        order.sort_by(|x, y| b.0[*y].partial_cmp(&b.0[*x]).unwrap_or(std::cmp::Ordering::Equal));
        let most = order.first().copied().unwrap_or(0);
        let mut step = self.steps[most];
        let mut acc = b.0[most];
        for &i in order.iter().skip(1) {
            let w = b.0[i];
            step = step.mix(self.steps[i], w / (acc + w));
            acc += w;
        }
        step.trans = self.steps[most].trans;
        (step, most)
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
        let step_named = |n: &str| STEPS.iter().position(|s| s.replace(' ', "-") == n).or(["word", "grid"].iter().position(|s| *s == n));
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
                    let mut s = Step { trans: TRANS[i], ..Step::default() };
                    let mut k = 2;
                    while let Some(key) = w.get(k) {
                        let two = xy(k + 1);
                        let one = num(k + 1);
                        k += match *key {
                            "zoom" => one.map(|v| s.shot.zoom = v).map_or(1, |_| 2),
                            "top" => one.map(|v| s.shot.top = v).map_or(1, |_| 2),
                            "delay" => one.map(|v| s.trans.delay = v.max(0.0)).map_or(1, |_| 2),
                            "secs" => one.map(|v| s.trans.secs = v.max(0.01)).map_or(1, |_| 2),
                            // A curve's name may have a space ("ease out").
                            "ease" => {
                                let two_words = w.get(k + 1).zip(w.get(k + 2)).map(|(a, b)| format!("{a} {b}"));
                                if let Some(e) = two_words.as_deref().and_then(|n| EASES.iter().find(|e| e.0 == n)) {
                                    s.trans.ease = e.1;
                                    3
                                } else if let Some(e) = w.get(k + 1).and_then(|n| EASES.iter().find(|e| e.0 == *n)) {
                                    s.trans.ease = e.1;
                                    2
                                } else {
                                    1
                                }
                            }
                            "duo-scale" => one.map(|v| s.place.duo_scale = v).map_or(1, |_| 2),
                            "pan" => two.map(|v| s.shot.pan = v).map_or(1, |_| 3),
                            "tilt" => one.map(|v| s.shot.tilt = v).map_or(1, |_| 2),
                            "turn" => one.map(|v| s.shot.turn = v).map_or(1, |_| 2),
                            "show" | "bright" | "height" => {
                                let six: Option<Vec<f32>> = (1..=PARTS.len()).map(|j| num(k + j)).collect();
                                match six {
                                    Some(v) => {
                                        let a: [f32; PARTS.len()] = std::array::from_fn(|j| v[j]);
                                        match *key {
                                            "show" => s.place.show = a,
                                            "bright" => s.place.bright = a,
                                            _ => s.place.height = a,
                                        }
                                        1 + PARTS.len()
                                    }
                                    None => 1,
                                }
                            }
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
                Some(&"paper") => l.style.paper = named(&PAPERS.map(|p| p.0), &w[1..]).unwrap_or(l.style.paper),
                Some(&"lines") => l.style.ink = named(&INKS.map(|p| p.0), &w[1..]).unwrap_or(l.style.ink),
                Some(&"line-width") => l.style.width = num(1).and_then(|v| WIDTHS.iter().position(|w| (*w - v as f64).abs() < 0.01)).unwrap_or(l.style.width),
                Some(&"font") => l.style.font = named(&FONTS.map(|p| p.0), &w[1..]).unwrap_or(l.style.font),
                _ => {}
            }
        }
        // The steps there were before the editor had them all (older files:
        // the places for all, each its eye or the one lens for all); a
        // step not laid out yet, the one it comes from (PARENT: laid out
        // before it).
        const FIRST: [&str; 7] = ["logo", "agentsco", "cubes", "buttons", "phone", "menu", "section"];
        for i in 0..STEPS.len() {
            l.steps[i] = match steps[i] {
                Some(s) => s,
                None if !FIRST.contains(&STEPS[i]) => Step { trans: TRANS[i], ..l.steps[PARENT[i]] },
                None => {
                    // The eye rose with the lens then (drawn back): kept so.
                    let (zoom, pan) = shots[i].unwrap_or((all_zoom.unwrap_or(1.0), (0.0, 0.0)));
                    let up = ((1.0 - zoom) / 0.6).clamp(0.0, 1.0);
                    Step { place: all, shot: Shot { zoom, pan, top: up * up * (3.0 - 2.0 * up), ..Shot::default() }, trans: TRANS[i] }
                }
            };
        }
        l
    }

    /// The layout in its file's words.
    pub fn write(&self) -> String {
        let s = self.style;
        let mut text = format!("paper {}\nlines {}\nline-width {}\nfont {}\n", PAPERS[s.paper].0, INKS[s.ink].0, WIDTHS[s.width], FONTS[s.font].0);
        for (i, st) in self.steps.iter().enumerate() {
            let (p, e) = (st.place, st.shot);
            let t = st.trans;
            // (A step's name may have a space: written with a dash.)
            text.push_str(&format!("step {} zoom {:.3} pan {:.2} {:.2} top {:.3} tilt {:.1} turn {:.1} delay {:.2} secs {:.2} ease {}", STEPS[i].replace(' ', "-"), e.zoom, e.pan.0, e.pan.1, e.top, e.tilt, e.turn, t.delay, t.secs, t.ease.name()));
            for (key, v, by) in [("show", p.show, [1.0; PARTS.len()]), ("bright", p.bright, [1.0; PARTS.len()]), ("height", p.height, HEIGHTS)] {
                if v != by {
                    text.push_str(&format!(" {key}"));
                    for x in v {
                        text.push_str(&format!(" {x:.2}"));
                    }
                }
            }
            text.push_str(&format!(" words {} {}", p.words.0, p.words.1));
            if let Some(d) = p.duo {
                text.push_str(&format!(" duo {} {}", d.0, d.1));
            }
            text.push_str(&format!(" word {} {} buttons {} {} menu {} {} credit {} {} duo-scale {:.3}\n", p.word.0, p.word.1, p.buttons.0, p.buttons.1, p.menu.0, p.menu.1, p.credit.0, p.credit.1, p.duo_scale));
        }
        text
    }
}

/// Which of `names` the words `w` are.
fn named(names: &[&str], w: &[&str]) -> Option<usize> {
    let name = w.join(" ");
    names.iter().position(|n| *n == name)
}

/// Where the parts moved on the table (E, main.rs) are kept: read over
/// the layout laid out if it is there, to be laid into data/layout.
pub fn moved_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"));
    base.join("itemgrid/layout-moved")
}

/// The layout laid out with the owner (data/layout), or the one with its
/// parts moved (moved_path).
pub fn layout() -> Layout {
    if let Ok(text) = std::fs::read_to_string(moved_path()) {
        return Layout::read(&text);
    }
    Layout::read(include_str!("../data/layout"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_file_is_read_for_every_step() {
        let l = Layout::read("words 0 15\nduo 11 4\nmenu 0 1\nzoom 0.751\npaper lilac\nfont open sans\n");
        for s in &l.steps {
            assert_eq!(s.place.words, (0.0, 15.0));
            assert_eq!(s.place.duo, Some((11.0, 4.0)));
            assert_eq!(s.place.menu, (0.0, 1.0));
            assert!((s.shot.zoom - 0.751).abs() < 1e-4);
            assert!(s.shot.top > 0.3 && s.shot.top < 0.4);
        }
        assert_eq!(PAPERS[l.style.paper].0, "lilac");
        assert_eq!(FONTS[l.style.font].0, "open sans");
        // The new steps from the ones they come from.
        assert_eq!(l.steps[PHONE_OPEN].place.duo, Some((11.0, 4.0)));
        assert_eq!(l.steps[PHONE_OPEN].trans, TRANS[PHONE_OPEN]);
    }

    #[test]
    fn a_layout_comes_back_from_its_file() {
        let mut l = Layout::default();
        l.steps[2].place.word = (3.0, -1.0);
        l.steps[4].place.duo = Some((9.0, 4.0));
        l.steps[4].shot = Shot { zoom: 0.8, pan: (1.5, -2.25), top: 0.2, tilt: -5.0, turn: 12.5 };
        l.steps[7].place.show[3] = 0.0;
        l.steps[7].place.height[3] = 0.5;
        l.style.font = 18;
        l.steps[3].trans = Trans { delay: 2.0, secs: 0.75, ease: Ease::Out };
        let back = Layout::read(&l.write());
        assert_eq!(back, l);
    }

    #[test]
    fn the_layout_kept_in_the_app_is_read() {
        let l = layout();
        assert_eq!(FONTS[l.style.font].0, "open sans");
        assert_eq!(l.steps[PHONE].place.duo, Some((11.0, 4.0)));
        assert_eq!(Layout::read(&l.write()), l);
    }

    #[test]
    fn the_steps_go_one_into_the_next() {
        let mut l = Layout::default();
        l.steps[1].place.word = (2.0, 0.0);
        let (s, _) = l.at([0.5, 0.0, 0.0], Marks::default());
        assert_eq!(s.place.word, (1.0, 0.0));
        l.steps[MENU].place.menu = (4.0, 0.0);
        let (s, now) = l.at([1.0; 3], Marks { phone: 1.0, menu: 1.0, ..Marks::default() });
        assert_eq!((s.place.menu, now), ((4.0, 0.0), MENU));
        let (_, now) = l.at([1.0; 3], Marks { menu: 1.0, ..Marks::default() });
        assert_eq!(now, MENU_BARE);
    }
}

