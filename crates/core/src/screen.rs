//! What item/grid's own recovery shows on the phone while a job runs there
//! (crates/recovery's screen: its header tells the lines): the stage in a
//! word or two, how long is left, the whole way gone as the lamp's place,
//! item/grid's accent and grid. Written to /run/item-screen over adb; with
//! TWRP, nothing.

use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Shown {
    serial: String,
    steps: Vec<String>,
    step: usize,
    /// The step's title and word as last written, and how far it was: a
    /// note written under it keeps them.
    title: String,
    detail: String,
    fraction: Option<f64>,
    total: u64,
    began: Instant,
    /// Stopped by an error: nothing more goes over it, the lamp left where
    /// the job stopped.
    over: bool,
    /// When the phone was last written to: each write is an adb round of
    /// its own, side by side with the job's (unthrottled, a reader that
    /// told it every read sent 0.5 MB/s, 2026-10-29).
    shown: Option<Instant>,
}

static SHOWN: Mutex<Option<Shown>> = Mutex::new(None);

/// A job's steps on the phone `serial`, if its recovery is item/grid's.
pub fn begin(serial: &str, steps: &[&str]) {
    let ours = crate::full::ours(serial);
    *SHOWN.lock().unwrap() = ours.then(|| Shown {
        serial: serial.to_owned(),
        steps: steps.iter().map(|s| s.to_string()).collect(),
        step: 0,
        title: String::new(),
        detail: String::new(),
        fraction: None,
        total: 0,
        began: Instant::now(),
        over: false,
        shown: None,
    });
}

/// Step `i` at work: what it does, and a word more (said until there is a
/// time left to say).
pub fn step(i: usize, title: &str, detail: &str) {
    let mut g = SHOWN.lock().unwrap();
    let Some(s) = g.as_mut() else { return };
    s.step = i;
    s.total = 0;
    s.title = title.to_owned();
    s.detail = detail.to_owned();
    s.fraction = None;
    write(s, "work");
}

/// The step's bytes to go (for progress()).
pub fn total(bytes: u64) {
    if let Some(s) = SHOWN.lock().unwrap().as_mut() {
        s.total = bytes;
    }
}

/// So much of the step's total done: the lamp on, the time left anew - at
/// most once a second.
pub fn progress(title: &str, done: u64) {
    let mut g = SHOWN.lock().unwrap();
    let Some(s) = g.as_mut() else { return };
    // Not while the send itself holds the phone's interface (a second
    // connection would stall it); the screen moves on its own meanwhile.
    if s.total == 0 || s.over || s.shown.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) || crate::adbusb::in_use() {
        return;
    }
    s.title = title.to_owned();
    s.fraction = Some(done.min(s.total) as f64 / s.total as f64);
    write(s, "work");
}

/// The job's line of this moment (activity.rs's): the window says it; the
/// phone's screen keeps to the stage and the time (as little as will do,
/// the owner) and is only told again, at most once a second, that the job
/// goes on - quiet for 12 s it would say item/grid is waiting.
pub fn note(_line: &str) {
    let mut g = SHOWN.lock().unwrap();
    let Some(s) = g.as_mut() else { return };
    // Nothing after the end (the phone leaving the recovery: a write would
    // wait for adb), nor more than one a second.
    if s.title.is_empty() || s.over || s.step >= s.steps.len() || s.shown.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) || crate::adbusb::in_use() {
        return;
    }
    write(s, "work");
}

/// The job done.
pub fn done(title: &str, detail: &str) {
    let mut g = SHOWN.lock().unwrap();
    if let Some(s) = g.as_mut() {
        s.step = s.steps.len();
        s.title = title.to_owned();
        s.detail = detail.to_owned();
        s.fraction = None;
        write(s, "done");
    }
}

/// The job stopped: what went wrong, short, and where to go on from.
pub fn error(what: &str) {
    let mut g = SHOWN.lock().unwrap();
    if let Some(s) = g.as_mut() {
        s.title = "stopped".to_owned();
        s.detail = what.replace('|', " ").chars().take(48).collect();
        // Over: no note goes over the error.
        s.over = true;
        write(s, "error");
    }
}

fn clean(t: &str) -> String {
    t.chars().map(|c| if c == '\n' || c == '\'' || c == '\\' || c == '%' { ' ' } else { c }).collect::<String>().to_lowercase()
}

/// A stage's title as the screen's big words: its first word, then the
/// rest ("writing item" - writing|item, "item is on" - item|is on).
fn words(title: &str) -> String {
    let t = title.trim();
    match t.split_once(' ') {
        Some((a, b)) => format!("{a}|{}", b.trim()),
        None => t.to_owned(),
    }
}

/// How much of the whole way is gone: the steps done, and the share of the
/// one at work; all of it at the end.
fn path(s: &Shown) -> f64 {
    let n = s.steps.len().max(1) as f64;
    if s.step >= s.steps.len() {
        return 1.0;
    }
    ((s.step as f64 + s.fraction.unwrap_or(0.0)) / n).clamp(0.0, 1.0)
}

/// The time left as the screen's small line, from the time gone and the way
/// gone - once there is enough of both to tell it (a tenth of the way, 15 s).
fn time_left(gone: Duration, path: f64) -> Option<String> {
    if path < 0.1 || gone < Duration::from_secs(15) || path >= 1.0 {
        return None;
    }
    let left = gone.as_secs_f64() * (1.0 - path) / path;
    Some(if left < 90.0 { "a minute left".to_owned() } else { format!("{} min left", (left / 60.0).round() as u64) })
}

fn write(s: &mut Shown, state: &str) {
    s.shown = Some(Instant::now());
    let text = status(s, state);
    let _ = crate::full::adb_send(&s.serial, "item-recovery replace /run/item-screen", &mut text.as_bytes());
}

/// The status as the recovery's screen reads it.
fn status(s: &Shown, state: &str) -> String {
    let p = path(s);
    let detail = match state {
        "work" => time_left(s.began.elapsed(), p).unwrap_or_else(|| s.detail.clone()),
        _ => s.detail.clone(),
    };
    let mut text = format!("title {}\ndetail {}\nstate {state}\npath {p:.3}\n", clean(&words(&s.title)), clean(&detail));
    if state == "error" {
        text.push_str("foot nothing is lost|retry from|item/grid\n");
    }
    // The phone's item id, as this computer knows it (club.rs).
    if let Some(d) = crate::club::known(&s.serial) {
        text.push_str(&format!("id {}\n", clean(&d.number)));
    }
    // The table's grid and light as item/grid draws them (its window keeps
    // them there): the screen's the same.
    if let Some(grid) = dirs_grid() {
        text.push_str(&grid);
    }
    if let Some(accent) = accent() {
        text.push_str(&format!("accent {accent}\n"));
    }
    text
}

/// item/grid's accent (~/.config/itemgrid/accent: its window's choice, a
/// name of item's palette) as #rrggbb; none for its plain ink.
fn accent() -> Option<&'static str> {
    let home = std::env::var_os("HOME")?;
    let name = std::fs::read_to_string(std::path::Path::new(&home).join(".config/itemgrid/accent")).ok()?;
    crate::look::PALETTE.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())).map(|(_, hex)| *hex)
}

/// item/grid's window's grid for the recovery (~/.config/itemgrid/
/// recovery-grid: line #rrggbb, strength, width), its known lines only.
fn dirs_grid() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let text = std::fs::read_to_string(std::path::Path::new(&home).join(".config/itemgrid/recovery-grid")).ok()?;
    let ok = |l: &&str| {
        let (k, v) = l.split_once(' ').unwrap_or(("", ""));
        match k {
            "line" => v.len() == 7 && v.starts_with('#') && v[1..].chars().all(|c| c.is_ascii_hexdigit()),
            "strength" | "width" => v.parse::<f32>().is_ok(),
            _ => false,
        }
    };
    Some(text.lines().filter(ok).map(|l| format!("{l}\n")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_as_big_words() {
        assert_eq!(words("writing item"), "writing|item");
        assert_eq!(words("item is on"), "item|is on");
        assert_eq!(words("checking"), "checking");
    }

    /// The statuses of real jobs' stages into ITEM_SCREEN_SAMPLES (a
    /// directory), for `item-recovery preview` to draw.
    #[test]
    fn samples() {
        let Some(dir) = std::env::var_os("ITEM_SCREEN_SAMPLES") else { return };
        let dir = std::path::PathBuf::from(dir);
        let steps = ["clean", "item 0.3.1", "check", "link", "restart"].map(String::from).to_vec();
        let at = |step: usize, title: &str, detail: &str, fraction: Option<f64>, secs: u64| Shown {
            serial: String::new(),
            steps: steps.clone(),
            step,
            title: title.into(),
            detail: detail.into(),
            fraction,
            total: 0,
            began: Instant::now() - Duration::from_secs(secs),
            over: false,
            shown: None,
        };
        let cases = [
            ("1-cleaning", at(0, "cleaning", "old data goes", None, 3), "work"),
            ("2-writing", at(1, "writing item", "", Some(0.4), 70), "work"),
            ("3-checking", at(2, "checking", "every byte", None, 150), "work"),
            ("4-erasing", at(1, "erasing linux", "about 7 min", None, 20), "work"),
            ("5-android", at(3, "android data", "", Some(0.6), 400), "work"),
            ("6-done", at(5, "item is on", "setup next", None, 280), "done"),
            ("7-stopped", at(1, "stopped", "the phone left the recovery", Some(0.5), 100), "error"),
        ];
        for (name, s, state) in cases {
            std::fs::write(dir.join(format!("{name}.status")), status(&s, state)).unwrap();
        }
    }

    #[test]
    fn the_way_and_the_time_left() {
        let mut s = Shown { serial: String::new(), steps: vec!["a".into(), "b".into(), "c".into(), "d".into()], step: 1, title: String::new(), detail: String::new(), fraction: Some(0.5), total: 0, began: Instant::now(), over: false, shown: None };
        assert!((path(&s) - 0.375).abs() < 1e-9);
        s.step = 4;
        assert_eq!(path(&s), 1.0);
        assert_eq!(time_left(Duration::from_secs(60), 0.5).as_deref(), Some("a minute left"));
        assert_eq!(time_left(Duration::from_secs(300), 0.5).as_deref(), Some("5 min left"));
        assert_eq!(time_left(Duration::from_secs(5), 0.5), None);
        assert_eq!(time_left(Duration::from_secs(60), 0.05), None);
    }
}
