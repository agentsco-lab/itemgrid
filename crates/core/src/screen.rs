//! What item/grid's own recovery shows on the phone while a job runs there
//! (crates/recovery's screen): the steps, the one at work, how far, the
//! end. Written to /run/item-screen over adb; with TWRP, nothing.

use std::sync::Mutex;

struct Shown {
    serial: String,
    steps: Vec<String>,
    step: usize,
    total: u64,
}

static SHOWN: Mutex<Option<Shown>> = Mutex::new(None);

/// A job's steps on the phone `serial`, if its recovery is item/grid's.
pub fn begin(serial: &str, steps: &[&str]) {
    let ours = crate::full::ours(serial);
    *SHOWN.lock().unwrap() = ours.then(|| Shown { serial: serial.to_owned(), steps: steps.iter().map(|s| s.to_string()).collect(), step: 0, total: 0 });
}

/// Step `i` at work: what it does, and a word more.
pub fn step(i: usize, title: &str, detail: &str) {
    let mut g = SHOWN.lock().unwrap();
    let Some(s) = g.as_mut() else { return };
    s.step = i;
    s.total = 0;
    write(s, title, detail, None, "work");
}

/// The step's bytes to go (for progress()).
pub fn total(bytes: u64) {
    if let Some(s) = SHOWN.lock().unwrap().as_mut() {
        s.total = bytes;
    }
}

/// So much of the step's total done.
pub fn progress(title: &str, done: u64) {
    let mut g = SHOWN.lock().unwrap();
    let Some(s) = g.as_mut() else { return };
    if s.total == 0 {
        return;
    }
    let gb = |b: u64| format!("{:.1}", b as f64 / (1u64 << 30) as f64);
    let detail = format!("{} of {} gb", gb(done.min(s.total)), gb(s.total));
    write(s, title, &detail, Some(done as f64 / s.total as f64), "work");
}

/// The job done.
pub fn done(title: &str, detail: &str) {
    let mut g = SHOWN.lock().unwrap();
    if let Some(s) = g.as_mut() {
        s.step = s.steps.len();
        write(s, title, detail, None, "done");
    }
}

/// The job stopped: what went wrong, and that nothing else is changed.
pub fn error(what: &str) {
    let mut g = SHOWN.lock().unwrap();
    if let Some(s) = g.as_mut() {
        let short: String = what.chars().take(60).collect();
        write(s, &short, "run it again from item/grid", None, "error");
    }
}

fn clean(t: &str) -> String {
    t.chars().map(|c| if c == '\n' || c == '\'' || c == '\\' || c == '%' { ' ' } else { c }).collect::<String>().to_lowercase()
}

fn write(s: &Shown, title: &str, detail: &str, progress: Option<f64>, state: &str) {
    let mut text = format!("title {}\ndetail {}\nsteps {}\nstep {}\nstate {state}\n", clean(title), clean(detail), s.steps.iter().map(|x| clean(x)).collect::<Vec<_>>().join("|"), s.step);
    // The phone's item id, as this computer knows it (club.rs).
    if let Some(d) = crate::club::known(&s.serial) {
        text.push_str(&format!("id {}\n", clean(&d.number)));
    }
    // The table's grid as item/grid draws it (its window keeps it there):
    // the screen's grid the same.
    if let Some(grid) = dirs_grid() {
        text.push_str(&grid);
    }
    if let Some(p) = progress {
        text.push_str(&format!("progress {:.3}\n", p.clamp(0.0, 1.0)));
    }
    let _ = crate::full::adb_shell(&s.serial, &format!("printf '%s' '{text}' > /run/item-screen.new && mv /run/item-screen.new /run/item-screen"));
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
