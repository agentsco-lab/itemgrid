//! The start: the word on five squares, seen from straight above; the
//! table's squares grow out around them; the eye comes down to where the
//! Duo is seen from - and the squares are the tops of five cubes standing
//! on the table, the word written on them. They stand while there is no
//! phone, and sink into the table one after the other when it comes: the
//! Duo is where they were. Gone again, they rise.

use std::time::Instant;

pub const WORD: [&str; 5] = ["h", "y", "t", "h", "e"];

/// Seconds from the start: the word coming, the squares growing out, the
/// eye coming down.
const WORD_IN: f32 = 0.5;
const GRID: (f32, f32) = (0.55, 1.7);
const EYE: (f32, f32) = (1.6, 3.0);
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
}

impl Default for Intro {
    fn default() -> Intro {
        // HYTHE_INTRO=0: started at its end (the cubes up, the eye down).
        let skip = std::env::var("HYTHE_INTRO").is_ok_and(|v| v == "0");
        let start = skip.then(|| Instant::now() - std::time::Duration::from_secs_f32(EYE.1));
        Intro { start, last: None, sink: 0.0, sink_to: 0.0, ended: false }
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
        self.begun() && self.t() >= EYE.1
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
            (there, there * risen)
        })
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
        let d = self.sink_to - self.sink;
        if d.abs() < 1e-4 {
            self.sink = self.sink_to;
            return false;
        }
        self.sink += d.signum() * (dt / SINK_S).min(d.abs());
        true
    }
}
