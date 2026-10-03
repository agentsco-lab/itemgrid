//! The card that tells a job as it goes: its title, a bar of its stages - the
//! one under way filling smoothly with a light running over it - the stage in
//! plain words and what it means, the time left, and the raw steps folded
//! away under Details.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use adw::prelude::*;
use gtk::glib;

use crate::journey;

#[derive(Clone, Copy, PartialEq)]
enum Outcome {
    Running,
    Done,
    Stopped,
}

/// What the bar draws, eased toward what it should show.
struct Anim {
    /// Each stage's fill now, and what it is heading for.
    shown: Vec<f64>,
    target: Vec<f64>,
    outcome: Outcome,
    /// The light's place, 0..1, moving while running.
    sheen: f64,
    last_frame: Option<i64>,
}

pub struct Card {
    pub root: gtk::Box,
    title: gtk::Label,
    when: gtk::Label,
    step: gtk::Label,
    explain: gtk::Label,
    count: gtk::Label,
    details: gtk::Label,
    pub dismiss: gtk::Button,
    bar: gtk::DrawingArea,
    anim: Rc<RefCell<Anim>>,
    /// The stage shown and since when (for the time within it).
    at: RefCell<(String, usize, Instant)>,
}

pub const CSS: &str = "
.activity-card {
  border-radius: 18px;
  padding: 18px 20px 14px 20px;
  background: alpha(@accent_bg_color, 0.09);
  border: 1px solid alpha(@accent_bg_color, 0.28);
}
.activity-card.done { background: alpha(@success_color, 0.09); border-color: alpha(@success_color, 0.35); }
.activity-card.stopped { background: alpha(@error_color, 0.08); border-color: alpha(@error_color, 0.4); }
.activity-title { font-weight: 800; font-size: 1.25em; }
.activity-step { font-weight: 700; }
.activity-spinner { animation: activity-breathe 1.6s ease-in-out infinite; }
@keyframes activity-breathe { 0% { opacity: 0.45; } 50% { opacity: 1; } 100% { opacity: 0.45; } }
.activity-details { font-family: monospace; font-size: 0.85em; opacity: 0.7; }
";

impl Card {
    pub fn new() -> Rc<Card> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
        root.add_css_class("activity-card");
        root.set_visible(false);

        let head = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let pulse = gtk::Image::from_icon_name("emblem-synchronizing-symbolic");
        pulse.add_css_class("activity-spinner");
        let title = gtk::Label::builder().xalign(0.0).hexpand(true).css_classes(["activity-title"]).build();
        let when = gtk::Label::builder().xalign(1.0).css_classes(["dim-label", "numeric"]).build();
        let dismiss = gtk::Button::builder().icon_name("window-close-symbolic").css_classes(["flat", "circular"]).tooltip_text("Hide").valign(gtk::Align::Center).build();
        dismiss.set_visible(false);
        head.append(&pulse);
        head.append(&title);
        head.append(&when);
        head.append(&dismiss);
        root.append(&head);

        let bar = gtk::DrawingArea::builder().content_height(12).hexpand(true).margin_top(4).build();
        root.append(&bar);

        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let step = gtk::Label::builder().xalign(0.0).hexpand(true).wrap(true).css_classes(["activity-step"]).build();
        let count = gtk::Label::builder().xalign(1.0).css_classes(["dim-label", "caption"]).build();
        line.append(&step);
        line.append(&count);
        root.append(&line);
        let explain = gtk::Label::builder().xalign(0.0).wrap(true).max_width_chars(70).css_classes(["dim-label"]).build();
        root.append(&explain);

        let details = gtk::Label::builder().xalign(0.0).yalign(0.0).wrap(true).wrap_mode(gtk::pango::WrapMode::WordChar).selectable(true).css_classes(["activity-details"]).build();
        let scroll = gtk::ScrolledWindow::builder().child(&details).min_content_height(120).max_content_height(180).hscrollbar_policy(gtk::PolicyType::Never).build();
        let expander = gtk::Expander::builder().label("Details").child(&scroll).build();
        root.append(&expander);

        let anim = Rc::new(RefCell::new(Anim { shown: Vec::new(), target: Vec::new(), outcome: Outcome::Running, sheen: 0.0, last_frame: None }));
        bar.set_draw_func({
            let anim = anim.clone();
            move |area, cr, w, h| draw(area, cr, w as f64, h as f64, &anim.borrow())
        });
        bar.add_tick_callback({
            let anim = anim.clone();
            let pulse = pulse.clone();
            move |area, clock| {
                let mut a = anim.borrow_mut();
                let now = clock.frame_time();
                let dt = a.last_frame.map(|t| (now - t) as f64 / 1e6).unwrap_or(0.0).min(0.1);
                a.last_frame = Some(now);
                // Eased: a fifth of the way each tenth of a second.
                let k = 1.0 - (-dt * 2.2).exp();
                let target = a.target.clone();
                for (s, t) in a.shown.iter_mut().zip(target) {
                    *s += (t - *s) * k;
                }
                if a.outcome == Outcome::Running {
                    a.sheen = (a.sheen + dt / 1.8) % 1.0;
                }
                pulse.set_visible(a.outcome == Outcome::Running);
                area.queue_draw();
                glib::ControlFlow::Continue
            }
        });

        Rc::new(Card { root, title, when, step, explain, count, details, dismiss, bar, anim, at: RefCell::new((String::new(), 0, Instant::now())) })
    }

    /// The job shown as it stands: its steps so far, seconds since it began,
    /// and how it ended if it has (None: running; Some(None): done;
    /// Some(Some(why)): stopped). `elsewhere`: run by the command line.
    pub fn show(&self, job: &str, lines: &[String], secs: u64, ended: Option<Option<String>>, elsewhere: bool) {
        let stages = journey::stages(job);
        let at = journey::locate(&stages, lines);
        {
            let mut cur = self.at.borrow_mut();
            if cur.0 != job || cur.1 != at {
                *cur = (job.to_owned(), at, Instant::now());
            }
        }
        let secs_in = self.at.borrow().2.elapsed().as_secs_f64();
        self.root.set_visible(true);
        let by = if elsewhere { " · from the command line" } else { "" };
        self.title.set_label(journey::title(job));
        let mut a = self.anim.borrow_mut();
        if a.shown.len() != stages.len() {
            a.shown = vec![0.0; stages.len()];
        }
        a.target = (0..stages.len())
            .map(|i| match &ended {
                Some(None) => 1.0,
                _ if i < at => 1.0,
                _ if i == at => journey::within(&stages[i], secs_in),
                _ => 0.0,
            })
            .collect();
        for c in ["done", "stopped"] {
            self.root.remove_css_class(c);
        }
        let stage = &stages[at];
        match &ended {
            None => {
                a.outcome = Outcome::Running;
                let left = journey::left(&stages, at, secs_in);
                self.when.set_label(&format!("{} · {}{by}", clock(secs), about(left)));
                self.step.set_label(stage.title);
                self.explain.set_label(stage.explain);
                self.count.set_label(&format!("Step {} of {}", at + 1, stages.len()));
                self.dismiss.set_visible(false);
            }
            Some(None) => {
                a.outcome = Outcome::Done;
                self.root.add_css_class("done");
                self.when.set_label(&format!("took {}{by}", clock(secs)));
                self.step.set_label("Done");
                self.explain.set_label(journey::after(job));
                self.count.set_label("");
                self.dismiss.set_visible(true);
            }
            Some(Some(why)) => {
                a.outcome = Outcome::Stopped;
                self.root.add_css_class("stopped");
                self.when.set_label(&format!("after {}{by}", clock(secs)));
                self.step.set_label(&format!("Stopped at: {}", stage.title));
                self.explain.set_label(&format!("{}\n\nNothing past this step was done. The steps below show where it stopped.", plain(why)));
                self.count.set_label(&format!("Step {} of {}", at + 1, stages.len()));
                self.dismiss.set_visible(true);
            }
        }
        drop(a);
        // The raw steps, newest last, a transfer's notes thinned.
        let shown: Vec<&str> = lines.iter().map(String::as_str).filter(|l| !l.starts_with("  ")).collect();
        let tail = shown.len().saturating_sub(60);
        let mut text = shown[tail..].join("\n");
        if let Some(last) = lines.last().filter(|l| l.starts_with("  ")) {
            text.push('\n');
            text.push_str(last.trim());
        }
        self.details.set_label(&text);
        self.bar.queue_draw();
    }

    /// How the phone looks at this stage (for the Duo drawn), while running.
    pub fn phone(&self, job: &str, lines: &[String]) -> &'static str {
        let stages = journey::stages(job);
        stages[journey::locate(&stages, lines)].phone
    }

    pub fn hide(&self) {
        self.root.set_visible(false);
        let mut a = self.anim.borrow_mut();
        a.shown.clear();
        a.target.clear();
    }
}

/// The core's words, a little friendlier at the start.
fn plain(why: &str) -> String {
    let mut s = why.trim().to_owned();
    if let Some(c) = s.get(..1) {
        s.replace_range(..1, &c.to_uppercase());
    }
    if !s.ends_with('.') {
        s.push('.');
    }
    s
}

fn clock(secs: u64) -> String {
    if secs < 60 {
        format!("{secs} s")
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

fn about(left: f64) -> String {
    let min = (left / 60.0).round() as u64;
    match min {
        0 => "almost done".into(),
        1 => "about a minute left".into(),
        m => format!("about {m} min left"),
    }
}

fn draw(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, w: f64, h: f64, a: &Anim) {
    let n = a.shown.len();
    if n == 0 {
        return;
    }
    let fg = area.color();
    let gap = if n > 1 { 5.0 } else { 0.0 };
    let seg = (w - gap * (n - 1) as f64) / n as f64;
    let r = h / 2.0;
    let (c1, c2) = match a.outcome {
        Outcome::Running => ((0.20, 0.47, 0.96), (0.42, 0.68, 1.0)),
        Outcome::Done => ((0.15, 0.68, 0.38), (0.30, 0.82, 0.50)),
        Outcome::Stopped => ((0.85, 0.22, 0.24), (0.95, 0.40, 0.38)),
    };
    for (i, fill) in a.shown.iter().enumerate() {
        let x = i as f64 * (seg + gap);
        // The track.
        crate::rounded(cr, x, 0.0, seg, h, r);
        cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.13);
        let _ = cr.fill();
        let fw = seg * fill.clamp(0.0, 1.0);
        if fw < 0.5 {
            continue;
        }
        let _ = cr.save();
        crate::rounded(cr, x, 0.0, seg, h, r);
        cr.clip();
        let g = gtk::cairo::LinearGradient::new(x, 0.0, x + seg, 0.0);
        g.add_color_stop_rgb(0.0, c1.0, c1.1, c1.2);
        g.add_color_stop_rgb(1.0, c2.0, c2.1, c2.2);
        cr.rectangle(x, 0.0, fw, h);
        let _ = cr.set_source(&g);
        let _ = cr.fill();
        // The light running over the stage under way.
        let under_way = a.outcome == Outcome::Running && *fill < 0.999 && a.target.get(i).is_some_and(|t| *t < 0.999 && *t > 0.0);
        if under_way {
            let cx = x + (a.sheen * 1.6 - 0.3) * fw;
            let sheen = gtk::cairo::LinearGradient::new(cx - 40.0, 0.0, cx + 40.0, 0.0);
            sheen.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.0);
            sheen.add_color_stop_rgba(0.5, 1.0, 1.0, 1.0, 0.45);
            sheen.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
            cr.rectangle(x, 0.0, fw, h);
            let _ = cr.set_source(&sheen);
            let _ = cr.fill();
        }
        let _ = cr.restore();
    }
}
