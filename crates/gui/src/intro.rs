//! The start: the word on five squares, seen from straight above; the
//! table's squares grow out around them; the eye comes down to where the
//! Duo is seen from - and the squares are the tops of five cubes standing
//! on the table, the word written on them. They stand while there is no
//! phone and when it comes (the Duo in the page's middle, they in its
//! upper left; their fronts are the window's buttons).
//!
//! The credit turns up in the table's own squares as the growing reaches
//! them - each turning over as a departures board's flap does, through a
//! few letters to its own, slowing as it comes to it - and fades as the
//! eye comes down, gone when it stops.
//!
//! A cube clicked - or any of the table's squares - looks for the phone at
//! once: the cube pressed in (a square jumps up out of the table and back),
//! a wave along the word while it is looked for; not found, a note under
//! it.

use std::time::Instant;

pub const WORD: [&str; 9] = ["i", "t", "e", "m", "/", "g", "r", "i", "d"];

/// The word's letters on cubes: item/ (grid lies in the table's squares,
/// flat).
pub const ON_CUBES: usize = 5;

/// The credit, a letter a square of the table (none for the space).
pub const CREDIT: &str = "by AgentsCo";

/// A credit square now: which letter's place; turning over from showing
/// `from` to showing `to` (`turn` 0..1; 0 still, showing `from`); its
/// strength.
#[derive(Clone, Copy, PartialEq)]
pub struct Flip {
    pub i: usize,
    pub from: char,
    pub to: char,
    pub turn: f32,
    pub strength: f32,
}

/// When the start's steps come and how (the word coming, the squares
/// growing out, the eye coming down, the cubes down and the buttons up):
/// the layout's (scene.rs; the editor sets them), as it came by itself
/// until it is given them.
use crate::scene::{Trans, PHONE, START_STEPS, TRANS};

/// The buttons' step as it was laid out (its parts' times inside it, in
/// its seconds): the buttons this long after the cubes begin to go down.
const BUTTONS_AFTER: f32 = 0.35;
const BUTTONS_S: f32 = 1.2;

/// How the start goes - the ways laid out for the owner to look at and
/// choose between (2026-10-08; `start` in the settings lists them, ↻ plays
/// the start again). Kept in ~/.config/itemgrid/start; ITEMGRID_START
/// over it.
///
/// - Flight: the word stands as the logo in the page's upper left and is
///   carried down in a straight line to its place as the eye comes down.
/// - Still: the eye where it stays all along; the word at its place, the
///   table growing out under it.
/// - Zoom: the word at its place; the table far, drawn in to it.
/// - Corner: as the flight, from the page's middle to the corner.
/// - Tilt: the word at its place; the table seen from straight above, tipped
///   to the usual view.
/// - Wave: as still, the squares turning over as the table grows out.
/// - Quick: the table there, the word in; the buttons and the phone at once.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Start {
    Flight,
    Still,
    Zoom,
    Corner,
    Tilt,
    Wave,
    Quick,
}

pub const STARTS: [Start; 7] = [Start::Flight, Start::Still, Start::Zoom, Start::Corner, Start::Tilt, Start::Wave, Start::Quick];

impl Start {
    pub fn name(self) -> &'static str {
        match self {
            Start::Flight => "flight",
            Start::Still => "still",
            Start::Zoom => "zoom",
            Start::Corner => "corner",
            Start::Tilt => "tilt",
            Start::Wave => "wave",
            Start::Quick => "quick",
        }
    }

    pub fn of(name: &str) -> Option<Start> {
        STARTS.iter().copied().find(|s| s.name() == name.trim())
    }

    /// Whether the eye stands where it stays from the first frame (no
    /// camera movement at the start).
    pub fn still_eye(self) -> bool {
        matches!(self, Start::Still | Start::Wave | Start::Quick)
    }

    /// As set (the file, or ITEMGRID_START over it); the flight if neither.
    pub fn set() -> Start {
        if let Some(s) = std::env::var("ITEMGRID_START").ok().and_then(|v| Start::of(&v)) {
            return s;
        }
        std::fs::read_to_string(start_file()).ok().and_then(|t| Start::of(&t)).unwrap_or(Start::Flight)
    }

    pub fn keep(self) {
        let path = start_file();
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
        let _ = std::fs::write(path, format!("{}\n", self.name()));
    }
}

fn start_file() -> std::path::PathBuf {
    crate::scene::moved_path().with_file_name("start")
}

/// The quick start's steps: the table there, the word in, the buttons
/// a moment after.
const QUICK: [Trans; START_STEPS] = [
    Trans { delay: 0.0, secs: 0.4, ease: crate::scene::Ease::Smooth },
    Trans { delay: 0.0, secs: 0.01, ease: crate::scene::Ease::Linear },
    Trans { delay: 0.0, secs: 0.01, ease: crate::scene::Ease::Linear },
    Trans { delay: 0.25, secs: 0.8, ease: crate::scene::Ease::Linear },
];

pub struct Intro {
    start: Option<Instant>,
    last: Option<Instant>,
    /// 0 the cubes standing .. 1 all sunk; and where it goes.
    pub sink: f32,
    pub sink_to: f32,
    /// The start's last frame drawn (at its very end).
    ended: bool,
    /// A cube clicked: which, when; the looking begun then and when it
    /// ended; the note after it (not found) and since when.
    pressed: Option<(usize, Instant)>,
    search: Option<(Instant, Option<Instant>)>,
    note: Option<(String, Instant)>,
    /// A square of the table clicked (its middle, px) and when.
    tapped: Option<((f32, f32), Instant)>,
    /// The steps' ways in (the layout's).
    pub timing: [Trans; crate::scene::STEPS.len()],
    /// How the start goes.
    pub kind: Start,
}

/// The wave goes on at least so long (a look over the cable alone is over
/// in a moment, and the cubes would only twitch).
const SEARCH_MIN_S: f32 = 1.2;
/// A press: in and back.
const PRESS_S: f32 = 0.35;
/// A square of the table: up out of it and back.
const TAP_S: f32 = 0.6;

impl Default for Intro {
    fn default() -> Intro {
        // ITEMGRID_INTRO=0: started at its end (the cubes up, the eye down).
        let skip = std::env::var("ITEMGRID_INTRO").is_ok_and(|v| v == "0");
        let start = skip.then(|| Instant::now() - std::time::Duration::from_secs_f32(30.0));
        Intro { start, last: None, sink: 0.0, sink_to: 0.0, ended: false, pressed: None, search: None, note: None, tapped: None, timing: TRANS, kind: Start::set() }
    }
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Eased in and out more softly (the eye's travel).
fn smoother(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * x * (x * (6.0 * x - 15.0) + 10.0)
}

impl Intro {
    fn t(&self) -> f32 {
        self.start.map_or(0.0, |s| s.elapsed().as_secs_f32())
    }

    /// Seconds since the start (for looking from outside).
    pub fn seconds(&self) -> f32 {
        self.t()
    }

    pub fn begun(&self) -> bool {
        self.start.is_some()
    }

    pub fn begin(&mut self) {
        self.start.get_or_insert_with(Instant::now);
    }

    /// From the beginning again, the cubes up (to look at it once more);
    /// they go where they were going after it.
    pub fn replay(&mut self) {
        self.start = Some(Instant::now());
        self.ended = false;
        self.sink = 0.0;
    }

    /// The start's step `i` as timed: the layout's, or the quick start's
    /// own.
    fn trans(&self, i: usize) -> Trans {
        if self.kind == Start::Quick && i < START_STEPS {
            return QUICK[i];
        }
        self.timing[i]
    }

    /// When the start's step `i` begins (each after the one before began).
    fn due(&self, i: usize) -> f32 {
        (0..=i.min(START_STEPS - 1)).map(|j| self.trans(j).delay).sum()
    }

    /// The start's step `i` on its way (0..1, its curve's).
    fn on(&self, i: usize) -> f32 {
        let t = self.trans(i);
        t.ease.at((self.t() - self.due(i)) / t.secs.max(1e-3))
    }

    /// The buttons' step, in its seconds as laid out (BUTTONS_S): its
    /// curve over its own time.
    fn buttons_s(&self) -> f32 {
        self.on(3) * BUTTONS_S
    }

    /// The eye down: the start's own part over (the credit gone).
    pub fn done(&self) -> bool {
        self.begun() && self.t() >= self.due(2) + self.trans(2).secs
    }

    /// The word and its squares: 0 not yet .. 1 there.
    pub fn word(&self) -> f32 {
        self.on(0)
    }

    /// How far the table's squares have grown out around the cubes: 0 not
    /// at all .. 1 all of them.
    pub fn grid(&self) -> f32 {
        self.on(1)
    }

    /// The credit's square `i` (its letter `ch`) as the squares' growing
    /// comes across it (`reached` 0..1): turning over as a board's flap,
    /// through a few letters to its own, each turn slower than the one
    /// before; fading as the eye comes down, gone as it stops.
    pub fn credit_flip(&self, i: usize, ch: char, reached: f32) -> Flip {
        let strength = 1.0 - smoother((self.eye() - 0.15) / 0.75);
        let blank = Flip { i, from: ' ', to: ' ', turn: 0.0, strength };
        if !self.begun() || self.done() || reached <= 0.0 || strength <= 0.0 {
            return blank;
        }
        // As a board's square turns up (board.rs).
        let f = crate::board::flap(i, ch, reached);
        Flip { i, from: f.from, to: f.to, turn: f.turn, strength }
    }

    /// Button `i` coming: 0 not yet .. 1 there - a second after the eye
    /// stops, one after the other.
    pub fn button(&self, i: usize) -> f32 {
        if !self.begun() {
            return 0.0;
        }
        smooth((self.buttons_s() - BUTTONS_AFTER - i as f32 * 0.08) / 0.5)
    }

    /// The word's cubes going down at the end (0 standing .. 1 in the
    /// table), one after the other.
    fn down(&self, i: usize) -> f32 {
        if !self.begun() {
            return 0.0;
        }
        smooth((self.buttons_s() - i as f32 * 0.06) / 0.6)
    }

    /// The eye: 0 straight above .. 1 where the Duo is seen from.
    pub fn eye(&self) -> f32 {
        self.on(2)
    }

    /// Each cube: there (0 sunk .. 1 standing; one after the other), and
    /// its height. Seen from above they are the table's squares themselves
    /// (risen, their tops nearer the eye showed larger than the squares):
    /// they rise out of the table as the eye comes down.
    pub fn cubes(&self) -> [(f32, f32); WORD.len()] {
        let risen = smooth(self.eye() / 0.7);
        std::array::from_fn(|i| {
            // They stay when the phone comes (in the page's upper left, the
            // Duo in its middle; their fronts are the buttons).
            let there = 1.0;
            // item/ half a square high until the eye stops, then down into
            // the table (grid lies in it all along); a letter hops still
            // as the phone is looked for.
            // (The quick start: the word lies in the table from the first.)
            let stand = if i < ON_CUBES && self.kind != Start::Quick { 0.5 * risen * (1.0 - self.down(i)) } else { 0.0 };
            let hop = self.hop(i).max(0.0) * 0.5;
            (there, (stand * (1.0 + self.hop(i)) + if stand <= 0.0 { hop } else { 0.0 }).max(0.0))
        })
    }

    /// The note (not found) taken away (a board goes where it was).
    pub fn clear_note(&mut self) {
        self.note = None;
    }

    /// A square of the table clicked: it jumps, the phone looked for.
    pub fn tap(&mut self, at: (f32, f32)) {
        self.tapped = Some((at, Instant::now()));
        self.look();
    }

    /// The square jumping: its middle and its height now (a part of its
    /// side).
    pub fn tapped(&self) -> Option<((f32, f32), f32)> {
        let (at, when) = self.tapped?;
        let t = when.elapsed().as_secs_f32() / TAP_S;
        (t < 1.0).then(|| (at, 0.6 * (t * std::f32::consts::PI).sin().powf(0.7)))
    }

    /// A cube pressed: the phone looked for from now.
    pub fn press(&mut self, i: usize) {
        self.pressed = Some((i, Instant::now()));
        self.look();
    }

    fn look(&mut self) {
        let now = Instant::now();
        if !self.searching() {
            self.search = Some((now, None));
        }
        self.note = None;
    }

    /// The wave still going (looking, or not yet long enough).
    pub fn searching(&self) -> bool {
        match self.search {
            Some((since, None)) => since.elapsed().as_secs_f32() < 30.0,
            Some((since, Some(_))) => since.elapsed().as_secs_f32() < SEARCH_MIN_S,
            None => false,
        }
    }

    /// The looking over: its seconds; a note to leave if not found.
    pub fn found(&mut self, note: Option<String>) -> Option<f32> {
        let (since, end) = self.search.as_mut()?;
        if end.is_some() {
            return None;
        }
        *end = Some(Instant::now());
        let took = since.elapsed().as_secs_f32();
        // Shown once the wave is over.
        let at = *since + std::time::Duration::from_secs_f32(SEARCH_MIN_S.max(took));
        self.note = note.map(|n| (n, at));
        Some(took)
    }

    /// Each cube's lift above its height (a part of it): the press, the
    /// wave along the word.
    fn hop(&self, i: usize) -> f32 {
        let mut lift = 0.0;
        if let Some((p, at)) = self.pressed {
            let t = at.elapsed().as_secs_f32() / PRESS_S;
            if p == i && t < 1.0 {
                lift -= 0.25 * (t * std::f32::consts::PI).sin();
            }
        }
        let wave_t = self.search.filter(|_| self.searching()).map(|(since, _)| since.elapsed().as_secs_f32());
        {
            if let Some(t) = wave_t {
                // Softly in at the start; along the word, a hop a cube.
                let phase = t * 1.6 - i as f32 * 0.14;
                let hop = (phase * std::f32::consts::TAU).sin().max(0.0).powi(2);
                lift += 0.22 * hop * smooth(t / 0.3);
            }
        }
        lift
    }

    /// The note under the word (not found) and its strength.
    pub fn note(&self) -> Option<(&str, f32)> {
        let (text, at) = self.note.as_ref()?;
        let t = Instant::now().saturating_duration_since(*at).as_secs_f32();
        (Instant::now() >= *at).then_some((text.as_str(), smooth(t / 0.4)))
    }

    /// The Duo: seen as the last cube goes down, gone as the first rises.
    pub fn duo(&self) -> f32 {
        self.timing[PHONE].eased(self.sink)
    }

    /// A frame on: the cubes toward where they go (once the start is
    /// over). Whether anything still moves.
    pub fn step(&mut self) -> bool {
        let now = Instant::now();
        let dt = self.last.map_or(0.0, |l| now.duration_since(l).as_secs_f32()).min(0.1);
        self.last = Some(now);
        if !self.begun() {
            return false;
        }
        if !self.done() || self.t() < self.due(3) + self.trans(3).secs + 0.1 {
            return true;
        }
        if !self.ended {
            self.ended = true;
            return true;
        }
        // Pressed, looking, the note coming.
        let moving = self.pressed.is_some_and(|(_, at)| at.elapsed().as_secs_f32() < PRESS_S + 0.05)
            || self.tapped.is_some_and(|(_, at)| at.elapsed().as_secs_f32() < TAP_S + 0.05)
            || self.search.is_some_and(|(since, _)| since.elapsed().as_secs_f32() < 30.0 && (self.searching() || since.elapsed().as_secs_f32() < SEARCH_MIN_S + 0.4))
            || self.note.as_ref().is_some_and(|(_, at)| Instant::now().saturating_duration_since(*at).as_secs_f32() < 0.5);
        let sink_to = self.sink_to;
        let d = sink_to - self.sink;
        if d.abs() < 1e-4 {
            self.sink = sink_to;
            return moving;
        }
        self.sink += d.signum() * (dt / self.timing[PHONE].all()).min(d.abs());
        true
    }
}
