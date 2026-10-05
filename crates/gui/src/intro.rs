//! The start: the word on five squares, seen from straight above; the
//! table's squares grow out around them; the eye comes down to where the
//! Duo is seen from - and the squares are the tops of five cubes standing
//! on the table, the word written on them. They stand while there is no
//! phone, and sink into the table one after the other when it comes: the
//! Duo is where they were. Gone again, they rise.
//!
//! Then the credit is set in the table's squares under the word,
//! the eye comes near it and back, and it goes.
//!
//! A cube clicked - or any of the table's squares - looks for the phone at
//! once: the cube pressed in (a square jumps up out of the table and back),
//! a wave along the word while it is looked for; not found, a note under
//! it.

use std::time::Instant;

pub const WORD: [&str; 5] = ["h", "y", "t", "h", "e"];

/// Under the word, a letter a square.
pub const CREDIT: [&str; 3] = ["designed &", "developed", "by agentsco"];

/// Seconds from the start: the word coming, the squares growing out, the
/// eye coming down.
const WORD_IN: f32 = 0.5;
const GRID: (f32, f32) = (0.55, 1.7);
const EYE: (f32, f32) = (1.6, 3.0);
/// The credit set letter by letter; the eye near it and back; it goes.
const CREDIT_SET: (f32, f32) = (3.1, 4.3);
const FOCUS_IN: (f32, f32) = (3.4, 4.9);
const FOCUS_OUT: (f32, f32) = (6.0, 7.5);
const CREDIT_OUT: (f32, f32) = (6.6, 7.4);
const END: f32 = 7.5;
/// Seconds for the cubes to sink (or rise), the last starting a little
/// after the first.
const SINK_S: f32 = 1.1;

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
    /// How near the eye is to the note (eased toward 1 while it shows).
    near_note: f32,
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
        // HYTHE_INTRO=0: started at its end (the cubes up, the eye down).
        let skip = std::env::var("HYTHE_INTRO").is_ok_and(|v| v == "0");
        let start = skip.then(|| Instant::now() - std::time::Duration::from_secs_f32(END));
        Intro { start, last: None, sink: 0.0, sink_to: 0.0, ended: false, pressed: None, search: None, note: None, tapped: None, near_note: 0.0 }
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

    pub fn done(&self) -> bool {
        self.begun() && self.t() >= END
    }

    /// The word and its squares: 0 not yet .. 1 there.
    pub fn word(&self) -> f32 {
        smooth(self.t() / WORD_IN)
    }

    /// How far the table's squares have grown out around the cubes: 0 not
    /// at all .. 1 all of them.
    pub fn grid(&self) -> f32 {
        smooth((self.t() - GRID.0) / (GRID.1 - GRID.0))
    }

    /// The eye near the note (not found): 0 .. 1.
    pub fn near_note(&self) -> f32 {
        smoother(self.near_note)
    }

    /// The eye near the credit: 0 where the Duo is seen from .. 1 near.
    pub fn focus(&self) -> f32 {
        let t = self.t();
        smoother((t - FOCUS_IN.0) / (FOCUS_IN.1 - FOCUS_IN.0)) * (1.0 - smoother((t - FOCUS_OUT.0) / (FOCUS_OUT.1 - FOCUS_OUT.0)))
    }

    /// The credit: how much of it is set (0 .. 1, letter by letter) and
    /// its strength; none before or after.
    pub fn credit(&self) -> Option<(f32, f32)> {
        let t = self.t();
        if !self.begun() || t < CREDIT_SET.0 || t >= CREDIT_OUT.1 {
            return None;
        }
        let set = ((t - CREDIT_SET.0) / (CREDIT_SET.1 - CREDIT_SET.0)).clamp(0.0, 1.0);
        Some((set, 1.0 - smooth((t - CREDIT_OUT.0) / (CREDIT_OUT.1 - CREDIT_OUT.0))))
    }

    /// The eye: 0 straight above .. 1 where the Duo is seen from.
    pub fn eye(&self) -> f32 {
        smoother((self.t() - EYE.0) / (EYE.1 - EYE.0))
    }

    /// Each cube: there (0 sunk .. 1 standing; one after the other), and
    /// its height. Seen from above they are the table's squares themselves
    /// (risen, their tops nearer the eye showed larger than the squares):
    /// they rise out of the table as the eye comes down.
    pub fn cubes(&self) -> [(f32, f32); 5] {
        let risen = smooth(self.eye() / 0.7);
        std::array::from_fn(|i| {
            let there = 1.0 - smooth((self.sink - i as f32 * 0.1) / 0.6);
            (there, there * risen * (1.0 + self.hop(i)))
        })
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
        if let Some((since, _)) = self.search {
            if self.searching() {
                let t = since.elapsed().as_secs_f32();
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
        smooth((self.sink - 0.85) / 0.15)
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
        if !self.done() {
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
        // Toward the note while it shows (as it comes), away when it goes:
        // a second each way.
        let to = if self.note().is_some() && self.sink_to < 0.5 { 1.0 } else { 0.0 };
        let near_moving = (to - self.near_note).abs() > 1e-4;
        if near_moving {
            let d = to - self.near_note;
            self.near_note += d.signum() * dt.min(d.abs());
        }
        let moving = moving || near_moving;
        let d = self.sink_to - self.sink;
        if d.abs() < 1e-4 {
            self.sink = self.sink_to;
            return moving;
        }
        self.sink += d.signum() * (dt / SINK_S).min(d.abs());
        true
    }
}
