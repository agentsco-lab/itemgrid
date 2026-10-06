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

/// The steps in the editor's line of time: the start, the phone looked for
/// and not found, the menu and a section with none, then the phone found,
/// opened, on the cable, the menu and a section with it, the wallpaper.
pub const ORDER: [usize; STEPS.len()] = [0, 1, 2, 3, 4, 5, 11, 12, 6, 7, 8, 9, 10, 13];

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

/// The sections a section step can show (the menu's keys; with no phone
/// only the first two are there).
pub const SECTIONS: [&str; 10] = ["settings", "itemgrid", "overview", "agent", "look", "battery", "storage", "about", "updates", "repair"];

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
pub const LOGO: usize = 0;
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

/// The steps one after the other on one line of time (the editor's play,
/// its scrubber; ORDER).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Script {
    /// When each step's way begins to be due, and when it is all there.
    pub due: [f32; STEPS.len()],
    pub reached: [f32; STEPS.len()],
    pub end: f32,
}

/// What the script has at a moment (each step's way: 0..1, its delay and
/// all, not yet eased): the start's seconds; the wave's seconds and the
/// note while they show; the steps' ways; the boards open (the menu, a
/// section; with the phone's lines or with none).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ScriptAt {
    pub start_s: f32,
    pub wave: Option<f32>,
    pub note: bool,
    pub raw: Marks,
    pub menu_open: bool,
    pub section_open: bool,
    pub phone_boards: bool,
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

/// Seconds between one event step being there and the next being brought.
const PAUSE: f32 = 0.8;

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
    /// The section the editor's section steps open (SECTIONS).
    pub section: usize,
    /// The steps fixed (ok pressed on them, nothing changed since).
    pub fixed: [bool; STEPS.len()],
}

impl Default for Layout {
    fn default() -> Layout {
        Layout { steps: std::array::from_fn(|i| Step { trans: TRANS[i], ..Step::default() }), style: Style::default(), section: 0, fixed: [false; STEPS.len()] }
    }
}

impl Layout {
    /// The ways in, step by step.
    pub fn timing(&self) -> [Trans; STEPS.len()] {
        self.steps.map(|s| s.trans)
    }

    /// The steps on one line of time (ORDER).
    pub fn script(&self) -> Script {
        let t = self.timing();
        let mut due = [0.0f32; STEPS.len()];
        let mut reached = [0.0f32; STEPS.len()];
        // The start's: each after the one before began.
        let mut began = 0.0;
        for i in 0..START_STEPS {
            began += t[i].delay;
            due[i] = began;
            reached[i] = began + t[i].secs;
        }
        // Then each brought a little after the one before is there.
        let mut at = reached[START_STEPS - 1] + PAUSE;
        for &i in &ORDER[START_STEPS..] {
            due[i] = at;
            reached[i] = at + t[i].all();
            at = reached[i] + PAUSE;
        }
        Script { due, reached, end: at }
    }
}

impl Script {
    /// The script at `t` seconds.
    pub fn at(&self, timing: &[Trans; STEPS.len()], t: f32) -> ScriptAt {
        let raw = |i: usize| ((t - self.due[i]) / timing[i].all()).clamp(0.0, 1.0);
        // The boards with no phone close over the pause before the phone
        // comes; the phone's before the wallpaper.
        let close_bare = ((t - self.reached[SECTION_BARE]) / PAUSE).clamp(0.0, 1.0);
        let close_phone = ((t - self.reached[SECTION]) / PAUSE).clamp(0.0, 1.0);
        let phone_side = t >= self.due[PHONE];
        let (menu, section) = if phone_side { (raw(MENU) * (1.0 - close_phone), raw(SECTION) * (1.0 - close_phone)) } else { (raw(MENU_BARE) * (1.0 - close_bare), raw(SECTION_BARE) * (1.0 - close_bare)) };
        let (menu_due, section_due, closing) = if phone_side { (self.due[MENU], self.due[SECTION], close_phone) } else { (self.due[MENU_BARE], self.due[SECTION_BARE], close_bare) };
        ScriptAt {
            start_s: t.min(self.reached[START_STEPS - 1] + 0.2),
            wave: (t >= self.due[SEARCH] && t < self.due[NOT_FOUND]).then(|| t - self.due[SEARCH]),
            note: t >= self.due[NOT_FOUND] && t < self.due[MENU_BARE],
            raw: Marks { search: raw(SEARCH), not_found: raw(NOT_FOUND), phone: raw(PHONE), open: raw(PHONE_OPEN), cable: raw(CABLE), menu, section, wall: raw(WALLPAPER) },
            menu_open: t >= menu_due && closing < 0.5,
            section_open: t >= section_due && closing < 0.5,
            phone_boards: phone_side,
        }
    }

    /// The step all there at `t` (the last reached in the script's order;
    /// before the first, it).
    pub fn step_at(&self, t: f32) -> usize {
        ORDER.iter().rev().copied().find(|&i| t + 1e-3 >= self.reached[i]).unwrap_or(LOGO)
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
                Some(&"fixed") => {
                    for n in &w[1..] {
                        if let Some(i) = STEPS.iter().position(|s| s.replace(' ', "-") == *n) {
                            l.fixed[i] = true;
                        }
                    }
                }
                Some(&"paper") => l.style.paper = named(&PAPERS.map(|p| p.0), &w[1..]).unwrap_or(l.style.paper),
                Some(&"lines") => l.style.ink = named(&INKS.map(|p| p.0), &w[1..]).unwrap_or(l.style.ink),
                Some(&"line-width") => l.style.width = num(1).and_then(|v| WIDTHS.iter().position(|w| (*w - v as f64).abs() < 0.01)).unwrap_or(l.style.width),
                Some(&"font") => l.style.font = named(&FONTS.map(|p| p.0), &w[1..]).unwrap_or(l.style.font),
                Some(&"section") => l.section = w.get(1).and_then(|n| SECTIONS.iter().position(|s| s == n)).unwrap_or(l.section),
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
        let mut text = format!("paper {}\nlines {}\nline-width {}\nfont {}\nsection {}\n", PAPERS[s.paper].0, INKS[s.ink].0, WIDTHS[s.width], FONTS[s.font].0, SECTIONS[self.section.min(SECTIONS.len() - 1)]);
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
        let fixed: Vec<String> = (0..STEPS.len()).filter(|&i| self.fixed[i]).map(|i| STEPS[i].replace(' ', "-")).collect();
        if !fixed.is_empty() {
            text.push_str(&format!("fixed {}\n", fixed.join(" ")));
        }
        text
    }

    /// Whether `other` lays the frames out the same (the fixed marks
    /// aside).
    pub fn same(&self, other: &Layout) -> bool {
        self.steps == other.steps && self.style == other.style && self.section == other.section
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
        l.fixed[13] = true;
        l.style.font = 18;
        l.steps[3].trans = Trans { delay: 2.0, secs: 0.75, ease: Ease::Out };
        l.fixed[4] = true;
        let back = Layout::read(&l.write());
        assert_eq!(back, l);
    }

    #[test]
    fn the_script_keeps_the_start_as_it_was() {
        let l = Layout::default();
        let s = l.script();
        // The eye down at 1.9 .. 3.6 s, the buttons from 3.6 s.
        assert!((s.due[2] - 1.9).abs() < 1e-4 && (s.reached[2] - 3.6).abs() < 1e-4);
        assert!((s.due[3] - 3.6).abs() < 1e-4);
        assert_eq!(s.step_at(0.0), 0);
        assert_eq!(s.step_at(s.reached[MENU]), MENU);
        let at = s.at(&l.timing(), s.reached[PHONE]);
        assert!(at.raw.phone > 0.999 && !at.menu_open && at.phone_boards);
        let bare = s.at(&l.timing(), s.reached[MENU_BARE]);
        assert!(bare.menu_open && !bare.phone_boards && bare.raw.phone == 0.0);
        // The Duo seen in the last moment of its way, as before.
        assert!(l.steps[PHONE].trans.eased(0.8) == 0.0 && l.steps[PHONE].trans.eased(1.0) == 1.0);
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
