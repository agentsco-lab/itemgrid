//! item/grid's window, in the spirit of Finder's page for a connected iPhone.
//!
//! Simple by default: the Duo on the left; on the right one sentence on how
//! it is (a coloured dot), one button for what to do now, backups, updates
//! and the storage; Settings (the menu) hold Repair & Reset - the backup,
//! a reinstall, the whole system back, Android - and Developer Mode, which shows what was here before - slots,
//! images from RAM, every kind of backup, the logs.
//!
//! Before the simple page, the window was:
//! the Duo on the left - its two panels showing what is on them - with its
//! name, mode and battery; on the right Software, Backups, Screen and System;
//! the storage as one bar along the bottom. Over itemgrid-core: the same
//! actions and safety rules as the command line. Everything that waits on the
//! phone runs off the main thread (gio::spawn_blocking); the window only
//! shows.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use itemgrid_core::{screenshot, status, Mode};
use gtk::{gdk, gio, glib};

mod board;
mod cable;
mod duo3d;
mod floor_area;
mod grid_gl;
mod intro;
// The job's card (GTK): the table tells jobs now; kept for the pages.
#[allow(dead_code)]
mod card;
mod control;
mod journey;
mod place;
mod saver;
mod scene;
mod sections;

const APP_ID: &str = "lab.agentsco.ItemGrid";
const REFRESH_S: u32 = 5;
/// The screens on the Duo drawn here, taken again this often while the
/// window is in front on General (each frame is ~20 MB over USB).
const SCREENS_EVERY: u32 = 3;
/// The Duo drawn on the left (data/duo-body.py, mm): its body, a panel, the
/// panels' left edges and their top; drawn at this many px a mm.
const DUO_BODY: (f64, f64) = (186.9, 145.2);
const DUO_PANEL: (f64, f64) = (86.654, 115.539);
const DUO_SCREEN_X: (f64, f64) = (4.1, 96.146);
const DUO_SCREEN_TOP: f64 = 14.831;
// Small enough for the sections under it (#169; was 2.35).
const DUO_PX_PER_MM: f64 = 2.0;
/// The eye's middle in the frame (its px, scene::REF): where the
/// 1000×800 window had it (the Duo's room's middle).
const FRAME_MIDDLE: (f32, f32) = (500.0, 410.0);
/// Transparent room round each drawn half and its shadow, px: their edges
/// smoothed as they turn.
const DUO_PAD: f32 = 3.0;
/// A half's thickness (the Duo's 4.8 mm), px, and the layers that make it.
const DUO_THICK: f32 = 4.8 * DUO_PX_PER_MM as f32;
const DUO_EDGE_LAYERS: usize = 9;
/// The hinge's barrels' width, mm.
const DUO_HINGE_W: f64 = 10.2;
/// The USB-C cable (mm): its plug's housing, where it is on the right half's
/// bottom edge (from the spine), and the room its picture has.
const CABLE_PLUG: (f64, f64) = (11.0, 19.0);
/// The port's middle from the spine (the plug's edge ~1.5 cm from it).
const CABLE_PORT_X: f64 = duo3d::PORT_X as f64;
/// The plug's thickness (mm) and its layers, drawn as the halves' are.
const CABLE_PLUG_T: f32 = 5.0;
const CABLE_PLUG_LAYERS: usize = 7;
const CABLE_ROOM: (f64, f64) = (90.0, 80.0);
const CABLE_PAD: f64 = 4.0;
/// The floor's squares (mm), and the depth of the hole the cord goes down.
const FLOOR_SQUARE: f64 = 20.0;

/// Half the word's width, in squares (its cubes laid about their middle).
const WORD_HALF: f32 = intro::WORD.len() as f32 / 2.0;

/// Night: the table dark, its lines light - by the night button on the
/// table (kept in ~/.config/itemgrid/night: day or night).
static NIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// How night is set: "night" or "day" (an old "auto": day).
fn night_mode() -> String {
    match std::fs::read_to_string(night_file()).ok().as_deref().map(str::trim) {
        Some("night") => "night".into(),
        _ => "day".into(),
    }
}

/// Whether it should be night now, as set.
fn night_due() -> bool {
    night_mode() == "night"
}

/// How long a wave takes to cross the table (s).
const NIGHT_FLIP_S: f32 = 3.0;

fn night() -> bool {
    NIGHT.load(std::sync::atomic::Ordering::Relaxed)
}

fn night_file() -> std::path::PathBuf {
    glib::user_config_dir().join("itemgrid/night")
}

/// The table's own grey (what is white by day).
const NIGHT_TABLE: f64 = 0.142;

thread_local! {
    /// Night for what is being drawn now: while the squares turn over
    /// (the night button), each thing on the table is drawn as the squares
    /// under it are - turned or not yet; else as night is.
    static DRAW_NIGHT: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

fn draw_night() -> bool {
    DRAW_NIGHT.with(|c| c.get()).unwrap_or_else(night)
}

/// What is drawn next at the table's point `p`: as night or day by the
/// waves of the squares turning over (grid_gl's), if any run - each that
/// has passed the point turned it over once.
fn draw_night_at(fv: &FloorView, p: (f32, f32)) {
    if fv.waves.is_empty() {
        DRAW_NIGHT.with(|c| c.set(None));
        return;
    }
    let step = square() * fv.k;
    let mut now = night();
    for (how_far, from) in &fv.waves {
        if wave_passed(fv.reach, step, *how_far, *from, p) {
            now = !now;
        }
    }
    DRAW_NIGHT.with(|c| c.set(Some(now)));
}

/// Whether a wave `how_far` on its way from `from` has turned the square
/// at `p` over (as grid_gl's shader counts it: a band of six squares
/// turning, the middle of it).
fn wave_passed(reach: f32, step: f32, how_far: f32, from: (f32, f32), p: (f32, f32)) -> bool {
    let spread = 6.0 * step;
    let d = ((p.0 - from.0).powi(2) + (p.1 - from.1).powi(2)).sqrt();
    (how_far * (reach * 1.6 + spread) - d) / spread > 0.5
}

fn draw_night_off() {
    DRAW_NIGHT.with(|c| c.set(None));
}

use scene::{Style, FONTS, INKS, PAPERS, WIDTHS};

thread_local! {
    static STYLE: std::cell::Cell<Style> = std::cell::Cell::new(Style::default());
}

fn style() -> Style {
    STYLE.with(|s| s.get())
}

/// The paper's colour by day.
fn paper_rgb() -> [f64; 3] {
    PAPERS[style().paper.min(PAPERS.len() - 1)].1
}

/// The lines' width (px) chosen, and how many times the old one.
fn line_width() -> f64 {
    WIDTHS[style().width.min(WIDTHS.len() - 1)]
}

/// A face lit `light` as drawn: by day the paper's colour that lit, by
/// night the grey.
fn face_rgb(light: f64) -> (f64, f64, f64) {
    let l = paper(light);
    if night() {
        (l, l, l * 1.005)
    } else {
        let p = paper_rgb();
        (l * p[0], l * p[1], l * p[2] * 1.005)
    }
}

/// The page's paper as CSS (by day; the night's own stays). See-through
/// (the home page, the squares on the GPU), the window shows nothing of
/// its own: the paper is the table's, grid_gl's, fading out at the page's
/// edges - the owner asked for see-through edges (2026-10-08). The shadow
/// is kept, unseen, for the resize grab round the window; adwaita's 1 px
/// outline (a thin frame in the air) goes.
fn style_css() -> String {
    let p = paper_rgb().map(|v| (v * 255.0).round() as u8);
    let hex = format!("#{:02x}{:02x}{:02x}", p[0], p[1], p[2]);
    format!(
        "window:not(.night), window.background:not(.night), window:not(.night) headerbar, window.wall-cover:not(.night), window.wall-move:not(.night) .wall-card {{ background: {hex}; }}\n\
         window.see-through, window.see-through.background:not(.night), window.see-through.background.night, window.see-through headerbar {{ background: transparent; }}\n\
         window.see-through.csd {{ box-shadow: 0 0 0 12px transparent; outline: none; }}"
    )
}

thread_local! {
    static STYLE_CSS: gtk::CssProvider = gtk::CssProvider::new();
}

/// The looks `s` taken: kept for the drawing, the page's paper, the
/// letters made anew in the face.
fn set_style(s: Style) {
    let font_changed = style().font != s.font;
    STYLE.with(|c| c.set(s));
    STYLE_CSS.with(|css| css.load_from_string(&style_css()));
    if font_changed {
        GLYPHS.with(|g| g.borrow_mut().clear());
    }
}

/// A face's grey: by day `light` itself (1 the table's white); by night
/// the table's dark, the less lit faces lighter (lit from the viewer).
fn paper(light: f64) -> f64 {
    if night() {
        NIGHT_TABLE + (1.0 - light) * 1.1
    } else {
        light
    }
}

/// The lines' colour at `alpha`: black by day, white by night.
fn ink(cr: &gtk::cairo::Context, alpha: f64) {
    if night() {
        cr.set_source_rgba(1.0, 1.0, 1.0, alpha * 1.15);
    } else {
        let c = INKS[style().ink.min(INKS.len() - 1)].1;
        cr.set_source_rgba(c[0], c[1], c[2], alpha);
    }
}

/// A letter's colour: by night the light one for the dark one.
fn letter_rgba(c: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    // (A colour - red, green - stays itself: only greys turn.)
    let spread = c.0.max(c.1).max(c.2) - c.0.min(c.1).min(c.2);
    if draw_night() && spread < 0.15 {
        (1.04 - c.0, 1.04 - c.1, 1.04 - c.2, c.3)
    } else {
        c
    }
}

/// For now, to find the floor's look: its squares' size (mm) set with the
/// scroll wheel, and where they lie (mm) by dragging the table - kept
/// while the window runs (f32 bits).
static SQUARE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// Where the wheel has sent the size: eased there each frame.
static SQUARE_TO: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static GRID_AT: [std::sync::atomic::AtomicU32; 2] = [std::sync::atomic::AtomicU32::new(0), std::sync::atomic::AtomicU32::new(0)];

fn square() -> f32 {
    match SQUARE.load(std::sync::atomic::Ordering::Relaxed) {
        0 => FLOOR_SQUARE as f32,
        b => f32::from_bits(b),
    }
}

/// A frame's step (`dt` s) of the squares' size toward where the wheel
/// sent it, by the time and not the frames (the window gets 30 a second
/// here: X11 and GNOME's compositor); whether it moved.
fn ease_square(dt: f32) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    let to = match SQUARE_TO.load(Relaxed) {
        0 => return false,
        b => f32::from_bits(b),
    };
    let now = square();
    if (to - now).abs() < 0.01 {
        if now != to {
            SQUARE.store(to.to_bits(), Relaxed);
            return true;
        }
        return false;
    }
    let k = 1.0 - (-dt / 0.12).exp();
    SQUARE.store((now + (to - now) * k).to_bits(), Relaxed);
    true
}

/// The cubes' middle on the table (px, f32 bits; 0 0 not yet): the
/// squares are laid from it (show_fold sets it).
static CUBES_AT: [std::sync::atomic::AtomicU32; 2] = [std::sync::atomic::AtomicU32::new(0), std::sync::atomic::AtomicU32::new(0)];

fn grid_at() -> (f32, f32) {
    let g = |i: usize| f32::from_bits(GRID_AT[i].load(std::sync::atomic::Ordering::Relaxed));
    (g(0), g(1))
}
const HOLE_DEPTH: f64 = 30.0;
/// How far the plug's housing goes into the edge (mm).
const CABLE_IN: f64 = 0.7;
/// The strain relief's length after the housing (mm).
const CABLE_RELIEF: f64 = 6.0;
const DUO_FLOOR_PAD: f32 = 30.0;
/// The drawn Duo's room, in its body's heights and widths: the raised half
/// above it, its near edge wider in perspective.
const DUO_ROOM: f64 = 1.25;
const DUO_ROOM_W: f64 = 1.3;
/// The storage bar's parts' colours, in the order of `Parts::list`.
const PART_COLOURS: [(f64, f64, f64); 4] = [(0.21, 0.52, 0.89), (0.20, 0.82, 0.48), (1.0, 0.47, 0.0), (0.57, 0.25, 0.67)];

const CSS: &str = "
/* LOOK: minimal - white, small type, light weights; colour only where it
   says something (the status dot, the accent on the chosen section). */
window, window.background { background: #ffffff; font-size: 9.5pt; }

window.night, window.night.background { background: #242427; color: #e6e6ea; }
window.wallpaper, window.wallpaper.csd { border-radius: 0; box-shadow: none; outline: none; margin: 0; }
/* On its way into the wallpaper or back: the window the monitor's, seen
   through; what it shows a card growing or shrinking in it. */
window.wall-move, window.wall-move.background { background: transparent; }
window.wall-move .wall-card { background: #ffffff; border-radius: 12px; }
window.night.wall-move .wall-card { background: #242427; }
window.wall-cover { background: #ffffff; }
window.wall-cover.night { background: #242427; }
window.night headerbar { background: #242427; color: #e6e6ea; }
headerbar { background: #ffffff; box-shadow: none; border-bottom: none; }
.navigation-sidebar { background: transparent; }
.navigation-sidebar > row { min-height: 30px; padding: 0 10px; border-radius: 8px; }
.navigation-sidebar > row:selected { background: alpha(black, 0.05); }
.navigation-sidebar > row:selected label { font-weight: 600; }
.navigation-sidebar image { opacity: 0.6; }
.boxed-list { background: #ffffff; box-shadow: none; border: 1px solid alpha(black, 0.08); }
.navigation-sidebar > row.nav-apart { margin-top: 14px; }
.boxed-list button.pill { padding: 3px 14px; min-height: 26px; font-weight: 500; }
.boxed-list button.pill.destructive-action { background: alpha(#e01b24, 0.08); color: #c01c28; }
.status-bar { padding: 10px 24px; border-top: 1px solid alpha(black, 0.07); }
.bar-title { font-weight: 600; }
.duo-name { font-weight: 600; font-size: 1.25em; }
.duo-panel {
  background: #0b0b0d;
  border-radius: 16px;
  padding: 5px;
  border: 1px solid alpha(white, 0.14);
  box-shadow: 0 8px 24px alpha(black, 0.35);
}
.duo-screen { border-radius: 11px; background: #000; }
.duo-hinge {
  min-width: 7px;
  margin: 18px 0;
  border-radius: 3px;
  background: linear-gradient(to right, #2a2a2e, #4a4a50, #2a2a2e);
}
.section-title { font-weight: 600; font-size: 0.85em; letter-spacing: 0.04em; opacity: 0.55; }
.fact-name { opacity: 0.55; }
.storage-legend-dot { min-width: 10px; min-height: 10px; border-radius: 5px; }
.dot-free { background: alpha(currentColor, 0.18); }
.bottom-bar { padding: 14px 24px 16px 24px; }
.live-badge { color: #ff4f4f; font-weight: 700; font-size: 0.85em; letter-spacing: 1px; }
.duo-half-left { border-radius: 23px 0 0 23px; }
.duo-half-right { border-radius: 0 23px 23px 0; }
.duo-back { border-radius: 23px 0 0 23px; background: linear-gradient(to left, #b9bcb4, #d4d7cf); }
.duo-floor { background: alpha(black, 0.5); border-radius: 0 23px 23px 0; filter: blur(14px); }
.duo-shade { background: black; }
.duo-mode {
  background: alpha(black, 0.62);
  color: white;
  border-radius: 14px;
  padding: 10px 16px;
  font-weight: 700;
}
.duo-mode.moving { animation: duo-breathe 1.8s ease-in-out infinite; }
@keyframes duo-breathe { 0% { opacity: 0.55; } 50% { opacity: 1; } 100% { opacity: 0.55; } }
.mode-card {
  border-radius: 18px;
  padding: 18px 20px;
  background: alpha(currentColor, 0.05);
  border: 1px solid alpha(currentColor, 0.10);
}
.mode-card.moving image { animation: duo-breathe 1.8s ease-in-out infinite; }
.mode-title { font-weight: 600; font-size: 1.1em; }
.status-dot { min-width: 12px; min-height: 12px; border-radius: 6px; }
.status-dot.fine { background: #33d17a; }
.status-dot.look { background: #f6d32d; }
.status-dot.busy { background: #62a0ea; }
.status-dot.away { background: #77767b; }
.status-title { font-weight: 600; font-size: 1.45em; }
.repair-row-title { font-weight: 700; }
.free-label { opacity: 0.6; font-size: 0.9em; }
";

fn main() -> glib::ExitCode {
    itemgrid_core::moved::from_old_names();
    // Ubuntu 24.04 lets no unconfined program make user namespaces, and
    // WebKit's sandbox needs them: without item/grid's AppArmor profile
    // (data/apparmor) the Microsoft window would bring the whole app down.
    // Then WebKit runs unsandboxed - and that window goes to Microsoft's
    // sign-in and support pages only (see microsoft_only).
    let restricted = std::fs::read_to_string("/proc/sys/kernel/apparmor_restrict_unprivileged_userns").is_ok_and(|v| v.trim() == "1");
    if restricted && !std::path::Path::new("/etc/apparmor.d/itemgrid-gui").exists() {
        std::env::set_var("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", "1");
    }
    let app = adw::Application::builder().application_id(APP_ID).build();
    // A picture taken (ITEMGRID_SHOT), or a window tried from outside
    // (ITEMGRID_CONTROL, itemgrid-mcp): an instance of its own, beside the
    // owner's running window.
    if std::env::var_os("ITEMGRID_SHOT").is_some() || std::env::var("ITEMGRID_CONTROL").ok().as_deref() == Some("1") {
        app.set_flags(gio::ApplicationFlags::NON_UNIQUE);
    }
    app.connect_activate(build);
    app.run()
}

/// Where the phone is, as item/grid sees it.
#[derive(Clone, Debug, Default, PartialEq)]
enum Place {
    /// Linux up, over ssh at this host.
    Linux(String),
    Fastboot(String),
    Recovery(String),
    /// Android with USB debugging on.
    Android(String),
    /// On the USB, but neither adb nor fastboot answers: Android starting,
    /// or without USB debugging.
    Quiet(String),
    /// The port's kernel with no system on userdata (after a return to
    /// Android, a plain restart): Halium's initramfs on the USB.
    NoSystem,
    #[default]
    Gone,
}

impl Place {
    fn serial(&self) -> Option<&str> {
        match self {
            Place::Fastboot(s) | Place::Recovery(s) | Place::Android(s) | Place::Quiet(s) => Some(s),
            _ => None,
        }
    }
}

/// This window's job as it goes.
struct OwnJob {
    kind: &'static str,
    lines: Vec<String>,
    started: std::time::Instant,
    ended: Option<Option<String>>,
    /// Its length, once over.
    took: Option<u64>,
}

/// What the window knows between looks.
#[derive(Default)]
struct State {
    host: Option<String>,
    place: Place,
    /// When the phone was last seen anywhere: a phone gone a moment is
    /// restarting, not unplugged.
    last_seen: Option<std::time::Instant>,
    /// A job of this window's under way: no looks meanwhile.
    busy: bool,
    job: Option<OwnJob>,
    /// A job of the command line's under way: no looks meanwhile either.
    elsewhere: bool,
    /// The end of a job (its time) already put away.
    dismissed: Option<u64>,
    /// The phone's screens were taken once since it came: not again on each
    /// look (a frame takes seconds).
    pictured: bool,
}

/// One half of the drawn Duo: its front, its back, its shade.
#[derive(Clone)]
struct DuoHalf {
    front: gtk::Picture,
    back: gtk::Picture,
    shade: gtk::Picture,
    /// Its thickness: its silhouette in the chassis' colour, layer on layer
    /// from the screen's plane to the back's.
    edge: Vec<gtk::Picture>,
    /// The light on its screen, as it faces the light.
    glare: gtk::Picture,
    /// Its shadow on the table.
    floor: gtk::Picture,
    /// Its pictures far to near, as last drawn.
    order: Rc<RefCell<Vec<gtk::Picture>>>,
}

struct Ui {
    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    banner: adw::Banner,
    /// The phone's page, or the page asking for it.
    pages: gtk::Stack,
    switcher: adw::ViewSwitcher,
    screens: [gtk::Picture; 2],
    /// Over the Duo drawn: how the phone looks when it is not in Linux.
    duo_mode: gtk::Box,
    duo_mode_label: gtk::Label,
    name: gtk::Label,
    join: gtk::Button,
    serial: RefCell<String>,
    /// Serials whose club number was asked for this run (once each).
    claimed: RefCell<Vec<String>>,
    name_sub: gtk::Label,
    battery: gtk::Label,
    software: gtk::Label,
    /// Microsoft's packages on this computer, in a line.
    stock_line: gtk::Label,
    /// The job under way, told.
    card: Rc<card::Card>,
    /// Where the phone is when it is not in Linux, and what can be done.
    mode: gtk::Box,
    mode_icon: gtk::Image,
    mode_title: gtk::Label,
    mode_text: gtk::Label,
    mode_buttons: gtk::Box,
    /// What needs Linux: the sections and the facts.
    linux_only: gtk::Box,
    /// The sections' list, in the menu.
    nav: gtk::ListBox,
    /// The Duo drawn: the right half (and its back) turned as the phone
    /// folds; the angle shown and the one to go to.
    duo: gtk::Fixed,
    /// Its halves (left, right): front, back and shade each.
    halves: [DuoHalf; 2],
    spine: gtk::Picture,
    /// The USB-C cable in the right half's port, while the phone is on it:
    /// its cord, and its plug in layers (bottom to top).
    cable: gtk::DrawingArea,
    rope: Rc<RefCell<cable::Rope>>,
    /// The Duo in 3D (duo3d.rs), and what it draws.
    gl3d: gtk::GLArea,
    scene3d: Rc<RefCell<duo3d::Scene>>,
    /// The table under the Duo as the page's floor: the view of the phone
    /// at rest (not turned by the pointer, nor as it is held).
    floor: floor_area::FloorArea,
    /// The squares under it, on the GPU (grid_gl).
    floor_gl: gtk::GLArea,
    floor_view: Rc<RefCell<FloorView>>,
    /// The start, and the cubes with the word standing while there is no
    /// phone (intro.rs).
    intro: RefCell<intro::Intro>,
    /// The table's buttons under the pointer and pressed.
    buttons: RefCell<Buttons>,
    /// The board on the table open now (the sections' menu), if any.
    board: RefCell<Option<board::Board>>,
    /// A section set on the table beside the menu (its key, its board), and
    /// how far the eye has drawn back to see it all (eased toward 1 while
    /// it is there).
    page: RefCell<Option<(String, board::Board)>>,
    /// What can be done with the phone where it is, under its words (on
    /// stock Android: item installed) - a line to click.
    act: RefCell<Option<board::Board>>,
    /// The table's parts being moved (E): which, from where (the table's
    /// point), and the layout and the Duo's corner as the drag began.
    editing: std::cell::Cell<bool>,
    /// Waves of the squares turning over (the night button, a press a
    /// wave): since when, from where on the table, whether it has reached
    /// the page's bottom (the window's colours turned with it).
    night_waves: RefCell<Vec<(std::time::Instant, (f32, f32), bool)>>,
    edit_from: RefCell<Option<(&'static str, (f32, f32), scene::Layout, (f32, f32))>>,
    /// The menu opened for the phone's line's question only: put away with it.
    menu_for_ask: std::cell::Cell<bool>,
    page_back: std::cell::Cell<f32>,
    /// The section's board's size (squares across, lines) the eye is
    /// drawn back for, eased: one section for another, the eye goes over
    /// (it jumped).
    page_dims: std::cell::Cell<Option<(f32, f32)>>,
    /// The screen saver on since (saver.rs), and how far the eye has gone
    /// over to drifting (eased).
    saver: std::cell::Cell<Option<std::time::Instant>>,
    saver_mix: std::cell::Cell<f32>,
    /// Where the word stood on the table before the saver came.
    cubes_rest: std::cell::Cell<(f32, f32)>,
    /// Where the pointer was as the saver came.
    saver_pointer: std::cell::Cell<Option<(i32, i32)>>,
    /// The window as the second monitor's wallpaper: where it was before
    /// (its X frame) and its size as GTK had it.
    wallpaper: std::cell::Cell<Option<((i32, i32, i32, i32), (i32, i32))>>,
    /// The window on its way into the wallpaper or back: since when, from
    /// and to (its frame on the screen), into it.
    wall_move: std::cell::Cell<Option<WallMove>>,
    /// How far the card is on its way (0..1), while it moves: the word and
    /// the buttons then not kept to whole squares (they jumped a square at
    /// a time as the card grew), but for its first and last quarter.
    wall_t: std::cell::Cell<Option<f32>>,
    /// Over Wi-Fi (not followed, drawn shut): opened by a click on it, shut
    /// by another - a picture of opening, nothing asked of the phone.
    wifi_open: std::cell::Cell<bool>,
    /// Where the Duo lies on the table's sheet (table px, its middle open
    /// flat), and where that is in its own drawing (for its GL room).
    duo_on_sheet: std::cell::Cell<Option<(f32, f32)>>,
    /// The frames' layout (scene.rs: each step's places and eye).
    layout: std::cell::Cell<scene::Layout>,
    /// The step the frame is at now (scene.rs).
    step_now: std::cell::Cell<usize>,
    /// The steps after the start that the app has no way of its own for:
    /// looked for, not found, opened (on Wi-Fi), on the cable - each's way
    /// (0..1, not yet eased) as it goes.
    marks_raw: std::cell::Cell<[f32; 4]>,
    /// The Duo put away while the menu is open (eased 0 .. 1).
    duo_away: std::cell::Cell<f32>,
    duo_drawn_at: std::cell::Cell<Option<(f32, f32)>>,
    /// What the window shows (moved in it on the way into the wallpaper),
    /// on its stage.
    shown: gtk::Widget,
    stage: gtk::Overlay,
    /// How far the eye has gone over to the wallpaper's (eased).
    wallpaper_mix: std::cell::Cell<f32>,
    /// The wheel's lens: how near now, and where it goes.
    zoom: std::cell::Cell<(f32, f32)>,
    /// The sections' facts as words, from the last look (for their boards).
    section_words: RefCell<std::collections::HashMap<&'static str, Vec<(&'static str, String)>>>,
    /// The updates' details (when built, its commit) shown.
    details_open: std::cell::Cell<bool>,
    /// A question on the table before a job (repair, update): its board
    /// beside the menu.
    asking: RefCell<Option<Ask>>,
    cable_plug: Vec<gtk::Picture>,
    /// A half's width and the body's height, px.
    duo_size: (f32, f32),
    /// The angle the drawn Duo shows, and the one to go to.
    fold: std::cell::Cell<(f64, f64)>,
    /// How the phone is tipped from lying flat (pitch about its width, roll
    /// about its length, degrees, from its gravity): shown, and to go to.
    tilt: std::cell::Cell<([f64; 2], [f64; 2])>,
    /// duo-motion's orientation: shown (eased), to, and whether it came;
    /// the yaw it started at (taken off: drawn from its usual side); and
    /// whether the window follows the phone through duo-motion.
    orient: std::cell::Cell<([f64; 4], [f64; 4], bool)>,
    /// The turn about the vertical taken off duo-motion's world (x to
    /// magnetic north) so the viewer is in front of the drawing (rad); and
    /// the one at the computer's bearing from north, as the last look told
    /// it (kept: ~/.config/itemgrid/user-heading).
    yaw_ref: std::cell::Cell<Option<f64>>,
    /// Where the reference goes (after a look): eased there.
    yaw_ref_to: std::cell::Cell<Option<f64>>,
    /// duo-motion's world is north's (it heeds the field: --north); else
    /// its yaw starts anywhere and only looks set the reference.
    absolute: std::cell::Cell<bool>,
    user_heading: std::cell::Cell<Option<f64>>,
    motion_on: std::cell::Cell<bool>,
    /// The posture in words under the Duo; what it is made from: the
    /// hinge's posture by name, the gravities lately (in a hand: they move).
    pose: gtk::Label,
    pose_name: RefCell<String>,
    gravities: RefCell<std::collections::VecDeque<(std::time::Instant, [f64; 3])>>,
    /// The view turned by the pointer (yaw about the table's up, pitch
    /// added to the tilt), shown and to go to; double click: back.
    orbit: std::cell::Cell<([f32; 2], [f32; 2])>,
    /// No phone: the drawn one waits, opening and closing.
    idle: std::cell::Cell<bool>,
    /// The phone was closed and went (asleep): drawn shut, lying on the
    /// table, not waiting open; and the hinge's last angle read.
    shut_away: std::cell::Cell<bool>,
    /// How the phone was last seen (kept on disk): asleep, its words on the
    /// table say so.
    last_seen: RefCell<Option<LastSeen>>,
    /// When the lid was shut (and not opened since), and when the last
    /// gravity came: shut, the display goes dark and the gravity with it.
    lid_shut_at: std::cell::Cell<Option<std::time::Instant>>,
    gravity_at: std::cell::Cell<Option<std::time::Instant>>,
    /// The phone's USB link up at the last second's check.
    usb_was: std::cell::Cell<bool>,
    /// A look on its way (the quick ones after the link came wait for it).
    looking: std::cell::Cell<bool>,
    /// Nothing done at the computer for a while (saver::rest_after_ms()): the
    /// phone let go (not followed, not looked for over Wi-Fi).
    resting: std::cell::Cell<bool>,
    last_angle: std::cell::Cell<Option<f64>>,
    /// The hinge followed (posture.rs): where, and its stop.
    following: RefCell<Option<(String, itemgrid_core::posture::Stop)>>,
    /// The simple page's status (not shown: said on the table, show_fold).
    status_dot: gtk::Box,
    status_title: gtk::Label,
    status_lines: gtk::Label,
    backups_row: adw::ActionRow,
    updates_row: adw::ActionRow,
    repair_note: gtk::Label,
    free_label: gtk::Label,
    refresh: gtk::Button,
    /// What takes the phone out of Linux: the cable only (tracker #156).
    cable_only: Vec<(gtk::Button, Option<glib::GString>)>,
    /// Said over Wi-Fi: those need the cable.
    cable_note: gtk::Label,
    actions: gtk::Box,
    slots: gtk::ListBox,
    backups: gtk::ListBox,
    facts: gtk::Grid,
    storage: gtk::DrawingArea,
    legend: gtk::Box,
    bottom: gtk::Box,
    tabs: adw::ViewStack,
    /// The system disk by part, for the bar; counted when the phone comes.
    parts: RefCell<Option<itemgrid_core::storage::Parts>>,
    /// The live view running (its stop), and when it last failed - not
    /// tried again for a while (an item without a mirror).
    live: RefCell<Option<itemgrid_core::live::Stop>>,
    live_failed: RefCell<Option<std::time::Instant>>,
    live_badge: gtk::Label,
    state: RefCell<State>,
    sections: Rc<sections::Pages>,
}

/// A phone not seen for this long is away; before that, restarting.
const GONE_AFTER_S: u64 = 90;

/// An angle brought to -pi..pi.
fn wrap(a: f64) -> f64 {
    (a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

/// Where the one at this computer was, from north, as the phone last saw.
fn user_heading_file() -> std::path::PathBuf {
    glib::user_config_dir().join("itemgrid/user-heading")
}

/// ITEMGRID_TRACE=1: what the window hears and does, with the time (to see
/// where the drawn Duo lags the phone).
fn trace(what: std::fmt::Arguments) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ON.get_or_init(|| std::env::var_os("ITEMGRID_TRACE").is_some()) {
        let t = glib::DateTime::now_local().ok().and_then(|d| d.format("%T.%f").ok()).map(|s| s[..12].to_string()).unwrap_or_default();
        eprintln!("{t} {what}");
    }
}

/// The sections under the Duo (#169): key, title, icon.
const NAV: &[(&str, &str, &str)] = &[
    ("overview", "Overview", "phone-symbolic"),
    ("agent", "Agent", "system-users-symbolic"),
    ("look", "Wallpapers & Look", "preferences-desktop-wallpaper-symbolic"),
    ("battery", "Battery", "battery-good-symbolic"),
    ("storage", "Storage", "drive-harddisk-symbolic"),
    ("about", "About", "help-about-symbolic"),
    ("updates", "Updates & Backups", "software-update-available-symbolic"),
    ("repair", "Repair & Reset", "applications-engineering-symbolic"),
    ("developer", "Developer", "utilities-terminal-symbolic"),
];


fn build(app: &adw::Application) {
    // Light whatever the desktop's scheme: white, small type (LOOK) - or
    // dark, if the night was chosen on the table.
    NIGHT.store(night_due(), std::sync::atomic::Ordering::Relaxed);
    adw::StyleManager::default().set_color_scheme(if night() { adw::ColorScheme::ForceDark } else { adw::ColorScheme::ForceLight });
    let css = gtk::CssProvider::new();
    css.load_from_string(&format!("{CSS}{}{}", card::CSS, sections::CSS));
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        STYLE_CSS.with(|s| gtk::style_context_add_provider_for_display(&display, s, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1));
    }
    set_style(scene::layout().style);
    let window = adw::ApplicationWindow::builder().application(app).title("item/grid").default_width(1000).default_height(800).build();
    if night() {
        window.add_css_class("night");
    }
    // Where it was last (place.rs).
    place::restore(&window);
    place::keep(&window);

    // The tabs, in the header as Finder has them.
    let stack = adw::ViewStack::new();
    let switcher = adw::ViewSwitcher::builder().stack(&stack).policy(adw::ViewSwitcherPolicy::Wide).visible(false).build();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&switcher));
    let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh.set_tooltip_text(Some("Look again"));
    header.pack_end(&refresh);
    refresh.set_visible(developer_mode());

    // General: the Duo on the left and the sections under it (#169), the
    // section chosen on the right.
    // The Duo in the middle, the sections from the menu (top right) in its
    // place, how it is along the bottom (#169).
    let general = gtk::Box::new(gtk::Orientation::Vertical, 0);

    let device = gtk::Box::new(gtk::Orientation::Vertical, 8);
    device.set_valign(gtk::Align::Center);
    device.set_halign(gtk::Align::Center);
    device.set_vexpand(true);
    // The Duo as it lies on a table (data/duo-body.py: agentsco.uk's
    // drawing, in mm): each half one picture - its body with its live screen
    // in it (duo_half) - turned whole in 3D by show_fold; the spine and the
    // hinges over them. Laid over a placeholder with room above for the
    // raised half: its perspective does not widen the column.
    let duo = gtk::Fixed::new();
    let px = |mm: f64| (mm * DUO_PX_PER_MM).round() as i32;
    let (bw, bh) = (px(DUO_BODY.0), px(DUO_BODY.1));
    let mid = bw / 2;
    let room = (bh as f64 * DUO_ROOM) as i32;
    let texture = |svg: &'static [u8], w: i32| -> Option<gdk::Texture> {
        let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_static(svg));
        let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_stream_at_scale(&stream, w * 2, bh * 2, false, gio::Cancellable::NONE).ok()?;
        #[allow(deprecated)]
        Some(gdk::Texture::for_pixbuf(&pixbuf))
    };
    let leaf = |paintable: &gdk::Paintable, w: i32, h: i32| gtk::Picture::builder().content_fit(gtk::ContentFit::Fill).width_request(w).height_request(h).can_shrink(true).can_target(false).paintable(paintable).build();
    let pad = DUO_PAD as i32;
    // The live screens: pictures held, not shown - each half drawn with its.
    let screens = [0, 1].map(|_| gtk::Picture::new());
    let bodies = [texture(include_bytes!("../data/duo-left.svg"), bw), texture(include_bytes!("../data/duo-right.svg"), bw)];
    // Each half: its front (body and screen), its glacier back, and its
    // shade - the last two its silhouette, so their edges are as smooth as
    // the body's; all with transparent room round them for the same.
    let halves = [0, 1].map(|i| {
        let body = bodies[i].as_ref();
        let front = leaf(&duo_half(body, None, i, bw, bh), mid + 2 * pad, bh + 2 * pad);
        let back = leaf(&duo_back(body, i, bw, bh), mid + 2 * pad, bh + 2 * pad);
        let shade = leaf(&duo_silhouette(body, i, bw, bh, DUO_PAD, [0.0, 0.0, 0.0], 0.0), mid + 2 * pad, bh + 2 * pad);
        shade.set_opacity(0.0);
        // The layers lighter toward the screen: the rim catches the light.
        let edge = (0..DUO_EDGE_LAYERS)
            .map(|k| {
                let t = k as f32 / (DUO_EDGE_LAYERS - 1) as f32;
                let c = 0.62 + 0.18 * (1.0 - t);
                leaf(&duo_silhouette(body, i, bw, bh, DUO_PAD, [c, c + 0.01, c - 0.03], 0.0), mid + 2 * pad, bh + 2 * pad)
            })
            .collect();
        let glare = leaf(&duo_glare(i, bw, bh), mid + 2 * pad, bh + 2 * pad);
        glare.set_opacity(0.0);
        let fpad = DUO_FLOOR_PAD as i32;
        let floor = leaf(&duo_silhouette(body, i, bw, bh, DUO_FLOOR_PAD, [0.0, 0.0, 0.0], 12.0), mid + 2 * fpad, bh + 2 * fpad);
        DuoHalf { front, back, shade, edge, glare, floor, order: Rc::default() }
    });
    for i in 0..2 {
        let body = bodies[i].clone();
        let front = halves[i].front.clone();
        screens[i].connect_paintable_notify(move |p| front.set_paintable(Some(&duo_half(body.as_ref(), p.paintable().as_ref(), i, bw, bh))));
    }
    // The hinge: a strip the barrels' width, turned to face the viewer - a
    // cylinder.
    let hinge_w = px(DUO_HINGE_W);
    let spine = leaf(&texture(include_bytes!("../data/duo-hinge.svg"), hinge_w).map(|t| t.upcast::<gdk::Paintable>()).unwrap_or_else(|| gdk::Paintable::new_empty(hinge_w, bh)), hinge_w, bh);
    for h in &halves {
        duo.put(&h.floor, 0.0, 0.0);
    }
    let cable_px = |mm: f64| (mm * DUO_PX_PER_MM) as f32;
    let (cw, ch) = (cable_px(CABLE_ROOM.0) as i32, cable_px(CABLE_ROOM.1) as i32);
    // The cord (cable.rs): drawn over the whole room and well past it, as
    // it hangs and lies.
    let rope: Rc<RefCell<cable::Rope>> = Rc::default();
    let past = 320;
    let cable = gtk::DrawingArea::builder()
        .content_width((bw as f64 * DUO_ROOM_W) as i32 + 2 * past)
        .content_height(room + 2 * past)
        .can_target(false)
        .visible(false)
        .build();
    rope.borrow_mut().offset = (-past as f32, -past as f32);
    cable.set_draw_func({
        let rope = rope.clone();
        move |_, cr, _, _| rope.borrow().draw(cr)
    });
    cable.add_tick_callback({
        let rope = rope.clone();
        let last = std::cell::Cell::new(0i64);
        move |area, clock| {
            let now = clock.frame_time();
            let dt = if last.get() == 0 { 1.0 / 60.0 } else { (now - last.get()) as f32 / 1e6 };
            last.set(now);
            if area.is_visible() && rope.borrow_mut().step(dt) {
                area.queue_draw();
            }
            glib::ControlFlow::Continue
        }
    });
    duo.put(&cable, -past as f64, -past as f64);
    // The Duo itself, in 3D, over the same room as the cord.
    let scene3d: Rc<RefCell<duo3d::Scene>> = Rc::default();
    // More room above: held up and tipped, the phone's top went past it.
    let past_top = 700;
    let gl3d = duo3d::area(scene3d.clone(), (bw as f64 * DUO_ROOM_W) as i32 + 2 * past, room + past + past_top, DUO_PX_PER_MM as f32);
    duo.put(&gl3d, -past as f64, -past_top as f64);
    rope.borrow_mut().gl_past_top = past_top as f32;
    // The phone's screens on the 3D halves.
    for i in 0..2 {
        let (scene3d, gl3d) = (scene3d.clone(), gl3d.clone());
        screens[i].connect_paintable_notify(move |p| {
            let mut sc = scene3d.borrow_mut();
            let tex = p.paintable().and_downcast::<gdk::Texture>();
            // Picturing them (ITEMGRID_SCREENS): kept while the window, with
            // no phone, clears them.
            if tex.is_none() && std::env::var_os("ITEMGRID_SCREENS").is_some() {
                return;
            }
            sc.screens[i] = tex;
            sc.screens_changed[i] += 1;
            gl3d.queue_render();
        });
    }
    rope.borrow_mut().gl_past = past as f32;

    let cable_plug: Vec<gtk::Picture> = (0..CABLE_PLUG_LAYERS)
        .map(|i| {
            let t = i as f64 / (CABLE_PLUG_LAYERS - 1) as f64;
            let p = leaf(&duo_cable_plug(t, i == CABLE_PLUG_LAYERS - 1), cw, ch);
            p.set_visible(false);
            duo.put(&p, 0.0, 0.0);
            p
        })
        .collect();
    for h in &halves {
        for w in std::iter::once(&h.back).chain(&h.edge).chain([&h.front, &h.glare, &h.shade]) {
            duo.put(w, 0.0, 0.0);
        }
    }
    duo.put(&spine, 0.0, 0.0);
    let duo_sized = gtk::Overlay::builder().halign(gtk::Align::Center).build();
    duo_sized.set_child(Some(&gtk::Box::builder().width_request((bw as f64 * DUO_ROOM_W) as i32).height_request(room).build()));
    duo_sized.add_overlay(&duo);
    // Turned by the pointer: dragged sideways about the table's up, up and
    // down tipped more or less; a double click puts it back.
    let drag = gtk::GestureDrag::new();
    duo_sized.add_controller(drag.clone());
    let turn_back = gtk::GestureClick::new();
    duo_sized.add_controller(turn_back.clone());
    // How the phone looks when not in Linux, over its screens.
    let duo_mode = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    duo_mode.add_css_class("duo-mode");
    duo_mode.set_halign(gtk::Align::Center);
    duo_mode.set_valign(gtk::Align::Center);
    let duo_mode_label = gtk::Label::new(None);
    duo_mode.append(&duo_mode_label);
    duo_mode.set_visible(false);
    let duo_over = gtk::Overlay::new();
    duo_over.set_child(Some(&duo_sized));
    duo_over.add_overlay(&duo_mode);
    device.append(&duo_over);
    // The posture, in words.
    // Room above it for the cable, which lies toward the viewer.
    let pose = gtk::Label::builder().css_classes(["dim-label"]).margin_top(44).build();
    device.append(&pose);
    let live_badge = gtk::Label::builder().label("● LIVE").css_classes(["live-badge"]).build();
    live_badge.set_visible(false);
    device.append(&live_badge);
    let name = gtk::Label::builder().label("Surface Duo").css_classes(["duo-name"]).margin_top(6).build();
    let join = gtk::Button::builder().label("Join the Club…").css_classes(["pill"]).halign(gtk::Align::Center).build();
    join.set_tooltip_text(Some("A number for this Duo in the owners' club on agentsco.uk (00001...)"));
    join.set_visible(false);
    let name_sub = gtk::Label::builder().css_classes(["dim-label"]).build();
    let battery = gtk::Label::new(None);
    device.append(&name);
    device.append(&name_sub);
    device.append(&battery);
    device.append(&join);
    // Kept for what they say (set as ever), not shown: the table says it, in
    // its squares, under the Duo (show_fold). Shown, their first layout came
    // in the frame the phone appeared in (a 100 ms hitch).
    for l in [&pose, &name, &name_sub, &battery] {
        l.set_visible(false);
    }
    // The sections, as item Settings has them on the phone: what the phone
    // is and what is set from here, Repair & Reset at the bottom, apart.
    let nav = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).css_classes(["navigation-sidebar"]).width_request(230).build();
    for (key, title, icon) in NAV {
        let r = gtk::ListBoxRow::builder().name(*key).build();
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        b.append(&gtk::Image::from_icon_name(icon));
        b.append(&gtk::Label::builder().label(*title).xalign(0.0).build());
        r.set_child(Some(&b));
        if *key == "repair" {
            r.add_css_class("nav-apart");
        }
        if *key == "developer" {
            r.set_visible(developer_mode());
        }
        nav.append(&r);
    }
    nav.select_row(nav.row_at_index(0).as_ref());
    // Opened from the menu's square on the table (draw_buttons).
    let nav_pop = gtk::Popover::builder().child(&nav).has_arrow(false).position(gtk::PositionType::Bottom).build();
    // In a section: back to the Duo, and the section's name.
    let back_home = gtk::Button::builder().icon_name("go-previous-symbolic").tooltip_text("Back to your Duo").visible(false).build();
    let section_name = gtk::Label::builder().css_classes(["heading"]).visible(false).build();
    header.pack_start(&back_home);
    header.pack_start(&section_name);
    // At home no header (the table is all); in a section, its way back.
    header.set_visible(false);

    let sections = gtk::Box::new(gtk::Orientation::Vertical, 22);
    sections.set_hexpand(true);
    let section = |title: &str| {
        let b = gtk::Box::new(gtk::Orientation::Vertical, 8);
        b.append(&gtk::Label::builder().label(title).css_classes(["section-title"]).xalign(0.0).build());
        b
    };
    // Wrapped at a reading width: unbounded, they widened the window.
    let body = |text: &str| gtk::Label::builder().label(text).wrap(true).max_width_chars(64).xalign(0.0).css_classes(["dim-label"]).build();
    let pill = |label: &str| {
        let b = gtk::Button::with_label(label);
        b.add_css_class("pill");
        b
    };
    let row = || {
        let r = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        r.set_margin_top(4);
        r
    };

    // The job under way, on top.
    let card = card::Card::new();
    sections.append(&card.root);

    // Where the phone is, when not in Linux.
    let mode = gtk::Box::new(gtk::Orientation::Vertical, 10);
    mode.add_css_class("mode-card");
    let mode_head = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let mode_icon = gtk::Image::builder().pixel_size(32).build();
    let mode_title = gtk::Label::builder().xalign(0.0).wrap(true).css_classes(["mode-title"]).build();
    mode_head.append(&mode_icon);
    mode_head.append(&mode_title);
    mode.append(&mode_head);
    let mode_text = gtk::Label::builder().xalign(0.0).wrap(true).max_width_chars(70).css_classes(["dim-label"]).build();
    mode.append(&mode_text);
    let mode_buttons = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    mode_buttons.set_margin_top(4);
    mode.append(&mode_buttons);
    mode.set_visible(false);
    sections.append(&mode);

    let linux_only = gtk::Box::new(gtk::Orientation::Vertical, 22);
    let cable_note = gtk::Label::builder()
        .label("On Wi-Fi: status, logs, Update item, backups of home and settings and screenshots work from here. What takes the phone out of Linux - Erase and Install, Return to Android, the whole-system backup, images from RAM - needs the cable.")
        .wrap(true)
        .max_width_chars(70)
        .xalign(0.0)
        .css_classes(["dim-label", "caption"])
        .visible(false)
        .build();
    linux_only.append(&cable_note);

    // Software.
    let soft = section("Software");
    let software = gtk::Label::builder().xalign(0.0).wrap(true).build();
    soft.append(&software);
    soft.append(&body("Update builds item from your tree, installs it and restarts the phone. Enter the PIN when it is back."));
    let actions = gtk::Box::new(gtk::Orientation::Vertical, 22);
    let soft_row = row();
    let update = pill("Update item");
    update.add_css_class("suggested-action");
    let reboot = pill("Restart");
    let reinstall = pill("Erase and Install…");
    reinstall.set_tooltip_text(Some("A fresh item from a release image: everything on the phone's data partition goes (backed up first)"));
    soft_row.append(&update);
    soft_row.append(&reboot);
    soft_row.append(&reinstall);
    soft.append(&soft_row);
    actions.append(&soft);

    // Slots: the two boot slots and what is in them, read only.
    let slot_sec = section("Slots");
    slot_sec.append(&body("The phone boots from one of two slots. Read from the phone's partition table; nothing here changes them."));
    let slots = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).margin_top(6).build();
    slots.append(&adw::ActionRow::builder().title("Reading the slots…").build());
    slot_sec.append(&slots);
    let slot_row = row();
    let try_ram = pill("Try an Image from RAM…");
    try_ram.set_tooltip_text(Some("fastboot boot: the image runs once, nothing is flashed, the slots stay as they are"));
    slot_row.append(&try_ram);
    slot_sec.append(&slot_row);
    actions.append(&slot_sec);

    // Backups.
    let back = section("Backups");
    back.append(&body("Back up the boot chain and your home and settings to this computer - and, once, the device data no image can give back (radio calibration, IMEI, keys). Everything copies the whole system from the recovery (about 20 minutes)."));
    let back_row = row();
    let backup = pill("Back Up Now");
    let backup_all = pill("Back Up Everything…");
    let restore = pill("Restore Backup…");
    restore.set_tooltip_text(Some("A slot's boot chain put back from a backup - one slot at a time, a changed boot tried from RAM first"));
    let back_full = pill("Back to a Full Backup…");
    back_full.set_tooltip_text(Some("The whole system as the newest full backup has it - exactly, part by part; what the phone holds now goes"));
    back_row.append(&backup);
    back_row.append(&backup_all);
    back_row.append(&restore);
    back_row.append(&back_full);
    back.append(&back_row);
    let backups = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).margin_top(6).build();
    back.append(&backups);
    actions.append(&back);

    // Android: the phone's own Android, for a while.
    let android = section("Android");
    android.append(&body("Stock Android can come back for a while. item/grid backs everything up first and tests the way back, then clears Linux's data and starts Android. Back to Linux puts it all back from the backup."));
    let android_row = row();
    let to_android = pill("Return to Android…");
    let get_android = pill("Get Android from Microsoft…");
    get_android.set_tooltip_text(Some("Microsoft's own package for this Duo, by its serial number: the stock kernel for the return, and a full repair"));
    android_row.append(&to_android);
    android_row.append(&get_android);
    android.append(&android_row);
    let stock_line = gtk::Label::builder().xalign(0.0).wrap(true).css_classes(["dim-label", "caption"]).build();
    android.append(&stock_line);
    actions.append(&android);

    // Screen.
    let scr = section("Screen");
    scr.append(&body("What both panels show, on the Duo here and saved to ~/itemgrid-shots."));
    let scr_row = row();
    let shot = pill("Take Screenshot");
    let folder = pill("Open Folder");
    scr_row.append(&shot);
    scr_row.append(&folder);
    scr.append(&scr_row);
    actions.append(&scr);
    linux_only.append(&actions);

    // System: a few facts.
    let sys = section("System");
    let facts = gtk::Grid::builder().row_spacing(6).column_spacing(18).build();
    sys.append(&facts);
    linux_only.append(&sys);
    // The simple page: how the Duo is, what to do now.
    let home = gtk::Box::new(gtk::Orientation::Horizontal, 22);
    home.add_css_class("status-bar");
    let status_head = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let status_dot = gtk::Box::builder().css_classes(["status-dot", "fine"]).valign(gtk::Align::Center).build();
    let status_title = gtk::Label::builder().label("Your Duo is fine").xalign(0.0).css_classes(["bar-title"]).build();
    status_head.append(&status_dot);
    status_head.append(&status_title);
    let status_lines = gtk::Label::builder().xalign(0.0).hexpand(true).ellipsize(gtk::pango::EllipsizeMode::End).css_classes(["dim-label"]).build();
    let status_box = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    status_box.set_hexpand(true);
    status_box.append(&status_head);
    status_box.append(&status_lines);
    home.append(&status_box);
    let home_list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build();
    let updates_row = adw::ActionRow::builder().title("Updates").build();
    home_list.append(&updates_row);
    home.append(&home_list);
    // Updates come as a row once there is one to give (from releases).
    home_list.set_visible(false);
    home.set_visible(false);

    // Overview: the job under way and the phone's mode over the Duo, the
    // Duo in the middle.
    sections.set_hexpand(false);
    sections.set_halign(gtk::Align::Center);
    sections.set_width_request(560);
    let overview = gtk::Box::new(gtk::Orientation::Vertical, 18);
    overview.set_margin_top(16);
    overview.set_margin_bottom(16);
    overview.append(&sections);
    overview.append(&device);
    // Not in a scrolled window: it clipped the drawn Duo at its edge, and
    // GTK's bounds for a half turned in 3D are loose - the raised half went
    // whole when it neared the top.
    let scroll = &overview;
    // Behind it the table, in centimetre squares, across the whole page and
    // still (it does not turn with the Duo).
    let floor = floor_area::FloorArea::new();
    let floor_view: Rc<RefCell<FloorView>> = Rc::default();
    floor.set_draw_func({
        let fv = floor_view.clone();
        move |cr, w, h| {
            // ITEMGRID_FRAMES=1: each drawing's time in the trace.
            static FRAMES: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            let t = std::time::Instant::now();
            draw_floor(&fv.borrow(), cr, w, h);
            if *FRAMES.get_or_init(|| std::env::var_os("ITEMGRID_FRAMES").is_some()) {
                trace(format_args!("floor drawn in {:.1} ms", t.elapsed().as_secs_f64() * 1000.0));
            }
        }
    });
    let floor_gl = grid_gl::area({
        let fv = floor_view.clone();
        move || grid_of(&fv.borrow())
    });
    let home_page = gtk::Overlay::new();
    home_page.set_child(Some(&floor_gl));
    home_page.add_overlay(&floor);
    home_page.add_overlay(scroll);
    // Nothing on the way to the page cuts the drawn Duo off: held up and
    // tipped it reaches far past its room (an overlay clipped it there).
    {
        let mut w: Option<gtk::Widget> = Some(duo.clone().upcast());
        while let Some(x) = w {
            x.set_overflow(gtk::Overflow::Visible);
            if x == *home_page.upcast_ref::<gtk::Widget>() {
                break;
            }
            w = x.parent();
        }
    }
    // Developer: what Developer Mode shows - software, slots, backups,
    // Android, the screen, the system.
    let developer = gtk::Box::new(gtk::Orientation::Vertical, 16);
    developer.append(&gtk::Label::builder().label("Developer").xalign(0.0).css_classes(["status-title"]).build());
    developer.append(&linux_only);
    // Repair & Reset: what is done once in a while, each asking first, all
    // on the cable.
    let repair = gtk::Box::new(gtk::Orientation::Vertical, 16);
    repair.append(&gtk::Label::builder().label("Repair & Reset").xalign(0.0).css_classes(["status-title"]).build());
    // Developer Mode: what the simple page leaves out.
    let dev_list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build();
    let dev_row = adw::SwitchRow::builder().title("Developer Mode").subtitle("Slots, images from RAM, every kind of backup, item built from your tree, the logs.").active(developer_mode()).build();
    dev_list.append(&dev_row);
    // The agent: what is set once and wants a keyboard (#166).
    let agent = gtk::Box::new(gtk::Orientation::Vertical, 16);
    agent.append(&gtk::Label::builder().label("Agent").xalign(0.0).css_classes(["status-title"]).build());
    agent.append(&body("item's agent does its heavy work through OpenRouter, with your own key and within your limit."));
    let agent_list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build();
    let key_row = adw::ActionRow::builder().title("OpenRouter key").subtitle("The agent's heavy work goes through OpenRouter with your own key. Kept in the phone's keyring, never on this computer.").subtitle_lines(3).build();
    let key_set = gtk::Button::builder().label("Set Key…").valign(gtk::Align::Center).css_classes(["pill"]).build();
    let key_forget = gtk::Button::builder().label("Remove").valign(gtk::Align::Center).css_classes(["pill", "flat"]).visible(false).build();
    key_row.add_suffix(&key_forget);
    key_row.add_suffix(&key_set);
    let model_row = adw::EntryRow::builder().title("Model (OpenRouter id, empty: item's default)").show_apply_button(true).build();
    let limit_row = adw::EntryRow::builder().title("Monthly limit, $ (empty: none)").show_apply_button(true).input_purpose(gtk::InputPurpose::Number).build();
    agent_list.append(&key_row);
    agent_list.append(&model_row);
    agent_list.append(&limit_row);
    agent.append(&agent_list);
    repair.append(&body("Things to do once in a while. Each asks before it starts; all but the backup need the USB cable."));
    let repair_list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build();
    let repair_row = |title: &str, text: &str, button: &str, destructive: bool| {
        let row = adw::ActionRow::builder().title(title).subtitle(text).subtitle_lines(4).build();
        let b = gtk::Button::builder().label(button).valign(gtk::Align::Center).css_classes(["pill"]).build();
        if destructive {
            b.add_css_class("destructive-action");
        }
        row.add_suffix(&b);
        repair_list.append(&row);
        b
    };
    // A copy of home and settings: here, out of the way - asked for only
    // when wanted (over Wi-Fi too).
    let backups_row = adw::ActionRow::builder().title("Back up your files").subtitle("Your home folder and settings, copied to this computer. About a minute, over the cable or Wi-Fi.").subtitle_lines(4).build();
    let back_up_now = gtk::Button::builder().label("Back Up Now").valign(gtk::Align::Center).css_classes(["pill"]).build();
    backups_row.add_suffix(&back_up_now);
    repair_list.append(&backups_row);
    let r_reinstall = repair_row("Reinstall item", "A fresh system from the latest release. You choose: keep your files and Wi-Fi, or erase everything. About 10 minutes.", "Reinstall…", false);
    let r_restore = repair_row("Restore the whole system", "The phone exactly as it was in your last full backup. What is on it now goes. About 40 minutes.", "Restore…", true);
    let r_android = repair_row("Go back to Android", "Microsoft's Android, for a while. Everything is backed up first and the way back is tested. About 40 minutes.", "Android…", false);
    repair.append(&repair_list);
    let repair_note = body("On Wi-Fi now: plug in the cable for all but the backup.");
    repair.append(&repair_note);
    repair.append(&gtk::Label::builder().label("Developer").xalign(0.0).css_classes(["section-title"]).margin_top(10).build());
    repair.append(&dev_list);
    let page = |child: &gtk::Box| {
        child.set_margin_top(20);
        child.set_margin_bottom(24);
        child.set_margin_start(40);
        child.set_margin_end(40);
        gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(child).hexpand(true).build()
    };
    let right = gtk::Stack::builder().transition_type(gtk::StackTransitionType::Crossfade).transition_duration(150).hexpand(true).build();
    right.add_named(&home_page, Some("overview"));
    right.add_named(&page(&agent), Some("agent"));
    right.add_named(&page(&repair), Some("repair"));
    right.add_named(&page(&developer), Some("developer"));
    // The sections showing the phone itself (sections.rs).
    let (section_ui, section_pages) = sections::build();
    let section_ui = Rc::new(section_ui);
    for (key, b) in &section_pages {
        right.add_named(&page(b), Some(key));
    }
    nav.connect_row_activated({
        let (right, nav_pop) = (right.clone(), nav_pop.clone());
        move |_, r| {
            right.set_visible_child_name(&r.widget_name());
            nav_pop.popdown();
        }
    });
    right.connect_visible_child_name_notify({
        let (back_home, section_name, header_bar) = (back_home.clone(), section_name.clone(), header.clone());
        move |r| {
            let key = r.visible_child_name().unwrap_or_default();
            let home = key == "overview";
            back_home.set_visible(!home);
            // The header only in a section (its way back); at home the
            // table is all, its buttons on the word's cubes.
            header_bar.set_visible(!home);
            // The page says its name itself.
            section_name.set_visible(false);
            section_name.set_label(NAV.iter().find(|n| n.0 == key.as_str()).map_or("", |n| n.1));
        }
    });
    back_home.connect_clicked({
        let (right, nav) = (right.clone(), nav.clone());
        move |_| {
            right.set_visible_child_name("overview");
            nav.select_row(nav.row_at_index(0).as_ref());
        }
    });
    right.set_vexpand(true);
    general.append(&right);
    general.append(&home);

    // The storage along the bottom.
    let storage = gtk::DrawingArea::builder().content_height(8).hexpand(true).build();
    let legend = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let bottom = gtk::Box::new(gtk::Orientation::Vertical, 8);
    bottom.add_css_class("bottom-bar");
    bottom.append(&storage);
    bottom.append(&legend);
    let free_label = gtk::Label::builder().xalign(0.0).css_classes(["free-label"]).build();
    bottom.append(&free_label);

    general.set_vexpand(true);
    stack.add_titled_with_icon(&general, Some("general"), "General", "phone-symbolic");

    // Logs.
    let owner: Rc<RefCell<Option<Rc<Ui>>>> = Rc::default();
    let logs_page = logs_view(owner.clone());
    stack.add_titled_with_icon(&logs_page, Some("logs"), "Logs", "text-x-generic-symbolic");

    // The page asking for the phone, when it has not been seen for a while.
    let away = adw::StatusPage::builder()
        .icon_name("phone-symbolic")
        .title("Looking for your Duo")
        .description("Plug it in with the USB cable, or connect it to the same Wi-Fi as this computer. If it is off, hold the power key for a few seconds.")
        .build();

    let pages = gtk::Stack::builder().transition_type(gtk::StackTransitionType::Crossfade).transition_duration(400).build();
    pages.add_named(&stack, Some("phone"));
    pages.add_named(&away, Some("away"));
    // The start is the page's own (intro.rs): the word on its cubes, while
    // the phone is first looked for.
    pages.set_visible_child_name("phone");
    // See-through: on the home page, the squares on the GPU, the window
    // shows nothing of its own round the table (style_css).
    let see_through = {
        let (window, right, pages) = (window.clone(), right.clone(), pages.clone());
        move || {
            let on = grid_gl::on() && pages.visible_child_name().as_deref() == Some("phone") && right.visible_child_name().as_deref() == Some("overview");
            if on {
                window.add_css_class("see-through");
            } else {
                window.remove_css_class("see-through");
            }
        }
    };
    floor_gl.connect_realize({
        let see_through = see_through.clone();
        move |_| see_through()
    });
    right.connect_visible_child_name_notify({
        let see_through = see_through.clone();
        move |_| see_through()
    });
    pages.connect_visible_child_name_notify(move |_| see_through());

    let banner = adw::Banner::new("");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&banner);
    content.append(&pages);
    pages.set_vexpand(true);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&content));
    // (A plain box, not adw's ToolbarView: that one kept 30 px under the
    // content, bare, with the header hidden - 2026-10-08.)
    let view = gtk::Box::new(gtk::Orientation::Vertical, 0);
    view.append(&header);
    toasts.set_vexpand(true);
    view.append(&toasts);
    view.add_css_class("wall-card");
    // The storage bar at the top of Storage.
    if let Some((_, b)) = section_pages.iter().find(|(k, _)| *k == "storage") {
        b.insert_child_after(&bottom, b.first_child().as_ref());
    }
    bottom.set_visible(false);
    // What the window shows over a stage of its own (on the way into the
    // wallpaper it is moved and sized there: no say then in the window's
    // size).
    let stage = gtk::Overlay::new();
    stage.set_child(Some(&gtk::Box::new(gtk::Orientation::Vertical, 0)));
    stage.add_overlay(&view);
    stage.set_measure_overlay(&view, true);
    window.set_content(Some(&stage));

    let ui = Rc::new(Ui {
        window: window.clone(),
        toasts,
        banner,
        pages,
        switcher,
        screens,
        duo_mode,
        duo_mode_label,
        name,
        join: join.clone(),
        serial: RefCell::default(),
        claimed: RefCell::default(),
        name_sub,
        battery,
        software,
        stock_line,
        card: card.clone(),
        mode,
        mode_icon,
        mode_title,
        mode_text,
        mode_buttons,
        linux_only,
        nav: nav.clone(),
        cable_only: [&reinstall, &restore, &try_ram, &backup_all, &back_full, &to_android, &r_reinstall, &r_restore, &r_android].into_iter().map(|b| (b.clone(), b.tooltip_text())).collect(),
        duo: duo.clone(),
        halves: halves.clone(),
        spine: spine.clone(),
        cable: cable.clone(),
        rope: rope.clone(),
        gl3d: gl3d.clone(),
        scene3d: scene3d.clone(),
        floor: floor.clone(),
        floor_gl: floor_gl.clone(),
        floor_view: floor_view.clone(),
        intro: RefCell::default(),
        buttons: RefCell::default(),
        board: RefCell::default(),
        page: RefCell::default(),
        act: RefCell::default(),
        editing: std::cell::Cell::new(false),
        night_waves: RefCell::default(),
        edit_from: RefCell::default(),
        menu_for_ask: std::cell::Cell::new(false),
        page_back: std::cell::Cell::new(0.0),
        page_dims: std::cell::Cell::new(None),
        saver: std::cell::Cell::new(None),
        saver_mix: std::cell::Cell::new(0.0),
        cubes_rest: std::cell::Cell::new((0.0, 0.0)),
        saver_pointer: std::cell::Cell::new(None),
        wallpaper: std::cell::Cell::new(None),
        wall_move: std::cell::Cell::new(None),
        wall_t: std::cell::Cell::new(None),
        wifi_open: std::cell::Cell::new(false),
        duo_on_sheet: std::cell::Cell::new(None),
        layout: std::cell::Cell::new(scene::layout()),
        step_now: std::cell::Cell::new(0),
        marks_raw: std::cell::Cell::new([0.0; 4]),
        duo_away: std::cell::Cell::new(0.0),
        duo_drawn_at: std::cell::Cell::new(None),
        shown: view.clone().upcast(),
        stage: stage.clone(),
        wallpaper_mix: std::cell::Cell::new(0.0),
        zoom: std::cell::Cell::new((1.0, 1.0)),
        section_words: RefCell::default(),
        details_open: std::cell::Cell::new(false),
        asking: RefCell::new(None),
        cable_plug: cable_plug.clone(),
        duo_size: (mid as f32, bh as f32),
        fold: std::cell::Cell::new((180.0, 180.0)),
        following: RefCell::default(),
        orbit: std::cell::Cell::new(([0.0; 2], [0.0; 2])),
        idle: std::cell::Cell::new(false),
        shut_away: std::cell::Cell::new(false),
        last_seen: RefCell::new(None),
        usb_was: std::cell::Cell::new(false),
        looking: std::cell::Cell::new(false),
        resting: std::cell::Cell::new(false),
        lid_shut_at: std::cell::Cell::new(None),
        gravity_at: std::cell::Cell::new(None),
        last_angle: std::cell::Cell::new(None),
        tilt: std::cell::Cell::new(([0.0; 2], [0.0; 2])),
        orient: std::cell::Cell::new(([1.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], false)),
        yaw_ref: std::cell::Cell::new(None),
        yaw_ref_to: std::cell::Cell::new(None),
        absolute: std::cell::Cell::new(false),
        user_heading: std::cell::Cell::new(std::fs::read_to_string(user_heading_file()).ok().and_then(|s| s.trim().parse().ok())),
        motion_on: std::cell::Cell::new(false),
        pose: pose.clone(),
        pose_name: RefCell::default(),
        gravities: RefCell::default(),
        status_dot,
        status_title,
        status_lines,
        backups_row,
        updates_row,
        repair_note,
        free_label,
        refresh: refresh.clone(),
        cable_note,
        actions,
        slots,
        backups,
        facts,
        storage: storage.clone(),
        legend,
        bottom,
        tabs: stack.clone(),
        parts: RefCell::default(),
        live: RefCell::default(),
        live_failed: RefCell::default(),
        live_badge,
        state: RefCell::default(),
        sections: section_ui.clone(),
    });
    // The letters the page sets, made ahead.
    glyphs_ahead("abcdefghijklmnopqrstuvwxyz?-&./");
    glyphs_ahead(intro::CREDIT);
    // The saver: idle five minutes, on; anything done since it came, off.
    // Idle ten, the phone let go (resting); anything done, looked for at
    // once. Asked each 2 s, not waited for (it was a call each second on the
    // main thread).
    {
        let weak = Rc::downgrade(&ui);
        let asking = Rc::new(std::cell::Cell::new(false));
        glib::timeout_add_local(std::time::Duration::from_secs(2), move || {
            let Some(ui) = weak.upgrade() else { return glib::ControlFlow::Break };
            if asking.replace(true) {
                return glib::ControlFlow::Continue;
            }
            let asking = asking.clone();
            glib::spawn_future_local(async move {
                let idle = saver::idle_ms_async().await;
                asking.set(false);
                if let Some(idle) = idle {
                    idle_now(&ui, idle);
                }
            });
            glib::ControlFlow::Continue
        });
    }
    // Looked at from outside (ITEMGRID_CONTROL=1, control.rs; itemgrid-mcp).
    {
        let weak = Rc::downgrade(&ui);
        control::start(move |request| {
            let Some(ui) = weak.upgrade() else { return serde_json::json!({ "error": "gone" }) };
            match request["cmd"].as_str() {
                Some("shot") => control::shot(&ui.window),
                Some("renderer") => serde_json::json!({ "renderer": ui.window.native().and_then(|n| n.renderer()).map(|r| r.type_().name().to_string()) }),
                Some("replay") => {
                    ui.intro.borrow_mut().replay();
                    serde_json::json!({ "ok": true })
                }
                Some("wallpaper") => {
                    wallpaper(&ui);
                    serde_json::json!({ "ok": true })
                }
                Some("state") => {
                    let intro = ui.intro.borrow();
                    let st = ui.state.borrow();
                    serde_json::json!({
                        "window": {
                            "content_on_screen": place::content_origin(&ui.window),
                            "width": ui.window.width(),
                            "height": ui.window.height(),
                            "maximized": ui.window.is_maximized(),
                            "active": ui.window.is_active(),
                            "fps": ui.duo.frame_clock().map(|c| c.fps()),
                            "zoom": ui.zoom.get().0,
                        },
                        "intro": {
                            "seconds": intro.seconds(),
                            "done": intro.done(),
                            "eye": intro.eye(),
                            "grid": intro.grid(),
                            "sink": intro.sink,
                            "sink_to": intro.sink_to,
                            "duo_shown": intro.duo(),
                        },
                        "floor": {
                            "square_mm": square(),
                            "square_to_mm": f32::from_bits(SQUARE_TO.load(std::sync::atomic::Ordering::Relaxed)),
                            "grid_at_mm": grid_at(),
                            "cubes_at_px": ui.floor_view.borrow().cubes_at,
                        },
                        "duo": {
                            "fold": ui.fold.get(),
                            "orbit": ui.orbit.get(),
                            "tilt": ui.tilt.get(),
                            "motion": ui.motion_on.get(),
                        },
                        "phone": {
                            "place": format!("{:?}", st.place),
                            "host": st.host,
                            "seen_s_ago": st.last_seen.map(|t| t.elapsed().as_secs_f32()),
                        },
                        "page": ui.pages.visible_child_name().map(|s| s.to_string()),
                    })
                }
                _ => serde_json::json!({ "error": "cmd: shot, state or replay" }),
            }
        });
    }
    // For now: the squares' size with the scroll wheel, where they lie by
    // dragging the table (not the Duo: dragging it turns it).
    {
        let page = ui.floor.parent().expect("the home page");
        fn redraw(ui: &Ui) {
            show_fold(ui, ui.fold.get().0);
            ui.floor.queue_draw();
            ui.floor_gl.queue_render();
            ui.cable.queue_draw();
        }
        let said: Rc<RefCell<Option<adw::Toast>>> = Rc::default();
        // One note, its words changed as the wheel turns (a new one each step
        // queued up); a new one once it has gone.
        let say = move |ui: &Ui, text: String| {
            if let Some(t) = said.borrow().as_ref() {
                t.set_title(&text);
                return;
            }
            let t = adw::Toast::builder().title(&text).timeout(2).build();
            let s = said.clone();
            t.connect_dismissed(move |_| *s.borrow_mut() = None);
            ui.toasts.add_toast(t.clone());
            *said.borrow_mut() = Some(t);
        };
        let say = Rc::new(say);
        let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let (weak, s) = (Rc::downgrade(&ui), say.clone());
        let pointer = Rc::new(std::cell::Cell::new((0.0f64, 0.0f64)));
        let wheel_at = gtk::EventControllerMotion::new();
        wheel_at.connect_motion({
            let pointer = pointer.clone();
            move |_, x, y| pointer.set((x, y))
        });
        page.add_controller(wheel_at);
        wheel.connect_scroll(move |c, _, dy| {
            let Some(ui) = weak.upgrade() else { return glib::Propagation::Proceed };
            // The wheel: the eye nearer or further, as a lens (a tenth a
            // notch), eased there.
            if !c.current_event_state().contains(gdk::ModifierType::CONTROL_MASK) {
                // No nearer than the usual view; back as far as the squares go.
                let to = (ui.zoom.get().1 * 1.1f32.powf(-dy as f32)).clamp(0.4, 1.0);
                ui.zoom.set((ui.zoom.get().0, to));
                return glib::Propagation::Stop;
            }
            // With Ctrl (for now) the squares' size: a notch a millimetre
            // (a touchpad's flow as it comes), eased there frame by frame.
            let from = match SQUARE_TO.load(std::sync::atomic::Ordering::Relaxed) {
                0 => square(),
                b => f32::from_bits(b),
            };
            let size = (from - dy as f32).clamp(8.0, 50.0);
            SQUARE_TO.store(size.to_bits(), std::sync::atomic::Ordering::Relaxed);
            s(&ui, format!("Squares {size:.0} mm"));
            glib::Propagation::Stop
        });
        page.add_controller(wheel);
        let drag = gtk::GestureDrag::new();
        let grid_drag = Rc::new(std::cell::Cell::new(false));
        let grid_drag2 = grid_drag.clone();
        // Where the table was and its point under the pointer as the drag
        // began: that point kept under the pointer.
        let from = Rc::new(std::cell::Cell::new(((0.0f32, 0.0f32), None::<(f32, f32)>, (0.0f64, 0.0f64))));
        let (weak, f, pg) = (Rc::downgrade(&ui), from.clone(), page.clone());
        drag.connect_drag_begin(move |g, x, y| {
            let Some(ui) = weak.upgrade() else { return };
            // Moving the table's parts (E): the one under the pointer taken.
            ui.edit_from.borrow_mut().take();
            if ui.editing.get() {
                let fv = ui.floor_view.borrow();
                let hit = table_under(&fv, (x, y)).and_then(|t| fv.edit_boxes.iter().find(|(_, b)| t.0 >= b[0] && t.0 < b[2] && t.1 >= b[1] && t.1 < b[3]).map(|(n, _)| (*n, t)));
                let corner = fv.duo_corner;
                drop(fv);
                if let Some((name, t)) = hit {
                    *ui.edit_from.borrow_mut() = Some((name, t, ui.layout.get(), corner));
                    f.set((grid_at(), Some(t), (x, y)));
                    grid_drag.set(false);
                    g.set_state(gtk::EventSequenceState::Claimed);
                    return;
                }
            }
            let holder = ui.duo.parent().filter(|_| ui.intro.borrow().duo() > 0.5);
            let on_duo = pg.pick(x, y, gtk::PickFlags::DEFAULT).is_some_and(|w| holder.as_ref().is_some_and(|h| w == *h || w.is_ancestor(h)));
            if on_duo {
                g.set_state(gtk::EventSequenceState::Denied);
                return;
            }
            f.set((grid_at(), table_under(&ui.floor_view.borrow(), (x, y)), (x, y)));
            // With Ctrl the squares are dragged (for now); else, once it
            // moves, the window is (no header at home).
            grid_drag.set(g.current_event_state().contains(gdk::ModifierType::CONTROL_MASK));
        });
        let say_edit = say.clone();
        let say_end = say.clone();
        let drag_end = drag.clone();
        let (weak, f, pg, gd) = (Rc::downgrade(&ui), from, page.clone(), grid_drag2);
        drag.connect_drag_update(move |g, dx, dy| {
            let Some(ui) = weak.upgrade() else { return };
            // A part moved (E): the layout from the drag's start, the part
            // moved by whole squares, every step alike.
            let moving = *ui.edit_from.borrow();
            if let Some((name, from, base, corner)) = moving {
                let (_, _, (x, y)) = f.get();
                let fv = ui.floor_view.borrow();
                let cur = square() * fv.k;
                let now = table_under(&fv, (x + dx, y + dy));
                drop(fv);
                let Some(now) = now else { return };
                let d = (((now.0 - from.0) / cur).round(), ((now.1 - from.1) / cur).round());
                let mut l = base;
                for st in l.steps.iter_mut() {
                    let p = &mut st.place;
                    match name {
                        "word" => p.word = (p.word.0 + d.0, p.word.1 + d.1),
                        "buttons" => p.buttons = (p.buttons.0 + d.0, p.buttons.1 + d.1),
                        "words" => p.words = (p.words.0 + d.0, p.words.1 + d.1),
                        "duo" => p.duo = Some((corner.0 + d.0, corner.1 + d.1)),
                        "credit" => p.credit = (p.credit.0 + d.0, p.credit.1 + d.1),
                        "menu" => p.menu = (p.menu.0 + d.0, p.menu.1 + d.1),
                        _ => {}
                    }
                }
                ui.layout.set(l);
                redraw(&ui);
                say(&ui, format!("{name} moved {:+.0}, {:+.0} (E keeps it)", d.0, d.1));
                return;
            }
            if !gd.get() {
                if dx.hypot(dy) > 4.0 {
                    let ((_, _), _, (x, y)) = f.get();
                    let device = g.current_event_device();
                    let surface = ui.window.surface().and_then(|s| s.downcast::<gdk::Toplevel>().ok());
                    let at = pg.compute_point(&ui.window, &gtk::graphene::Point::new((x + dx) as f32, (y + dy) as f32));
                    let shadow = ui.window.native().map_or((0.0, 0.0), |n| n.surface_transform());
                    if let (Some(device), Some(surface), Some(at)) = (device, surface, at) {
                        surface.begin_move(&device, g.current_button() as i32, at.x() as f64 + shadow.0, at.y() as f64 + shadow.1, g.current_event_time());
                    }
                    // The window manager has the button now: its release
                    // never comes here - the gesture let go of at once (left
                    // waiting, the next drag did nothing).
                    g.set_state(gtk::EventSequenceState::Denied);
                    g.reset();
                }
                return;
            }
            let k = DUO_PX_PER_MM as f32;
            let ((fx, fy), began, (x, y)) = f.get();
            let now = table_under(&ui.floor_view.borrow(), (x + dx, y + dy));
            let at = match (began, now) {
                (Some(a), Some(b)) => (fx + (b.0 - a.0) / k, fy + (b.1 - a.1) / k),
                // Past the horizon: as near as the view allows.
                _ => (fx + dx as f32 / k, fy + dy as f32 / k / 50f32.to_radians().cos()),
            };
            GRID_AT[0].store(at.0.to_bits(), std::sync::atomic::Ordering::Relaxed);
            GRID_AT[1].store(at.1.to_bits(), std::sync::atomic::Ordering::Relaxed);
            redraw(&ui);
            say(&ui, format!("Squares moved {:.0}, {:.0} mm", at.0, at.1));
        });
        page.add_controller(drag);
        // E: the table's parts moved by dragging - the word, the buttons,
        // the phone's words, the Duo, the credit, the open menu - a square
        // at a time, every step alike; E again keeps them (layout-moved,
        // read at the start: to be laid into data/layout).
        let (weak, s) = (Rc::downgrade(&ui), say_edit);
        let edit_keys = gtk::EventControllerKey::new();
        edit_keys.connect_key_pressed(move |_, key, _, state| {
            let Some(ui) = weak.upgrade() else { return glib::Propagation::Proceed };
            if !matches!(key, gdk::Key::e | gdk::Key::E) || state.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK) || ui.asking.borrow().is_some() {
                return glib::Propagation::Proceed;
            }
            let on = !ui.editing.get();
            ui.editing.set(on);
            if on {
                s(&ui, "moving: drag the word, the buttons, the phone, its words, the credit, the menu - E ends".into());
            } else {
                s(&ui, keep_moved(&ui));
            }
            redraw(&ui);
            glib::Propagation::Stop
        });
        ui.window.add_controller(edit_keys);
        // Each part let go of: kept at once (E ending the moving kept them
        // too, but a press that went to another window kept nothing).
        let (weak, s) = (Rc::downgrade(&ui), say_end);
        drag_end.connect_drag_end(move |_, _, _| {
            let Some(ui) = weak.upgrade() else { return };
            if ui.edit_from.borrow_mut().take().is_some() {
                s(&ui, keep_moved(&ui));
            }
        });
        // The phone asleep when last seen (before a restart): lying shut on the
    // table, saying so, not looked for as gone.
    if let Some(seen) = LastSeen::read() {
        if seen.asleep() {
            ui.shut_away.set(true);
            ui.scene3d.borrow_mut().sticker = seen.droidian;
            // Shut, from the first frame.
            ui.fold.set((0.0, 0.0));
        }
        *ui.last_seen.borrow_mut() = Some(seen);
    }
    // A question on the table: its number typed, Enter, Escape.
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed({
        let weak = Rc::downgrade(&ui);
        move |_, key, _, _| {
            let Some(ui) = weak.upgrade() else { return glib::Propagation::Proceed };
            if ask_key(&ui, key) { glib::Propagation::Stop } else { glib::Propagation::Proceed }
        }
    });
    ui.window.add_controller(keys);
    // The table's buttons: raised under the pointer, the hand there.
        let motion = gtk::EventControllerMotion::new();
        let (weak, pg) = (Rc::downgrade(&ui), page.clone());
        let hover = move |x: f64, y: f64| {
            let Some(ui) = weak.upgrade() else { return };
            let fv = ui.floor_view.borrow();
            let on = button_at(&fv, (x, y));
            ui.buttons.borrow_mut().hover = on;
            // A line of the open boards that can be clicked: its letters
            // rising on cubes under the pointer, the hand there.
            let under = table_under(&fv, (x, y));
            let side = square() * fv.k;
            let line = under.and_then(|t| ui.board.borrow().as_ref().and_then(|b| b.line_at(fv.board_at, side, t)));
            let page_line = under.and_then(|t| ui.page.borrow().as_ref().and_then(|(_, b)| b.line_at(fv.page_at, side, t)));
            let act_line = under.and_then(|t| ui.act.borrow().as_ref().filter(|b| !b.closing()).and_then(|b| b.line_at(fv.act_at, side, t)));
            drop(fv);
            let mut changed = ui.board.borrow_mut().as_mut().is_some_and(|b| b.set_hover(line));
            changed |= ui.act.borrow_mut().as_mut().is_some_and(|b| b.set_hover(act_line));
            let clickable = ui.page.borrow().as_ref().and_then(|(_, b)| page_line.filter(|&l| !b.lines[l].key.is_empty()));
            changed |= ui.page.borrow_mut().as_mut().is_some_and(|(_, b)| b.set_hover(clickable));
            if changed {
                show_fold(&ui, ui.fold.get().0);
            }
            pg.set_cursor_from_name(on.or(line).or(clickable).or(act_line).map(|_| "pointer"));
        };
        let h2 = hover.clone();
        motion.connect_motion(move |_, x, y| hover(x, y));
        motion.connect_leave(move |_| h2(-1.0, -1.0));
        page.add_controller(motion);
        // A cube clicked: the phone looked for at once.
        let click = gtk::GestureClick::new();
        let weak = Rc::downgrade(&ui);
        let pg2 = page.clone();
        let (win, right, nav) = (ui.window.clone(), right.clone(), nav.clone());
        click.connect_released(move |g, n, x, y| {
            let Some(ui) = weak.upgrade() else { return };
            // The drawn Duo (over Wi-Fi: opened or shut).
            if duo_clicked(&ui, (x, y)) {
                g.set_state(gtk::EventSequenceState::Claimed);
                return;
            }
            // A button of the table's.
            let pressed = button_at(&ui.floor_view.borrow(), (x, y));
            if let Some(i) = pressed {
                g.set_state(gtk::EventSequenceState::Claimed);
                ui.buttons.borrow_mut().pressed = Some((i, std::time::Instant::now()));
                trace(format_args!("button {:?}", FLOOR_BUTTONS[i]));
                let Some(button) = FLOOR_BUTTONS[i] else { return };
                match button {
                    FloorButton::Menu => {
                        // The sections as a board on the table (open, it
                        // closes).
                        let mut b = ui.board.borrow_mut();
                        if b.as_ref().is_some_and(|b| !b.closing()) {
                            b.as_mut().unwrap().close();
                            if let Some((_, p)) = ui.page.borrow_mut().as_mut() {
                                p.close();
                            }
                        } else {
                            *b = Some(board::Board::open(menu_lines(&ui)));
                            drop(b);
                            ui.intro.borrow_mut().clear_note();
                        }
                    }
                    // The start played again (for laying things out by it).
                    FloorButton::Replay => ui.intro.borrow_mut().replay(),
                    // Night (or day) coming: the squares turning over from
                    // this button; the colours follow once they have.
                    FloorButton::Night => {
                        if ui.night_waves.borrow().len() < grid_gl::WAVES {
                            let fv = ui.floor_view.borrow();
                            let cur = square() * fv.k;
                            let from = (fv.buttons_at.0 + (i as f32 + 0.5) * cur, fv.buttons_at.1 + 0.5 * cur);
                            drop(fv);
                            ui.night_waves.borrow_mut().push((std::time::Instant::now(), from, false));
                        }
                    }
                    // As the wallpaper, the window back first (its place and
                    // size kept as it closes are the usual ones).
                    FloorButton::Close => {
                        if ui.wallpaper.get().is_some() {
                            wallpaper(&ui);
                        }
                        win.close();
                    }
                }
                return;
            }
            // The phone's line (install item): the menu opened, its question
            // beside it.
            let act = {
                let fv = ui.floor_view.borrow();
                let side = square() * fv.k;
                table_under(&fv, (x, y)).and_then(|t| ui.act.borrow().as_ref().filter(|b| !b.closing()).and_then(|b| b.line_at(fv.act_at, side, t).map(|l| b.lines[l].key.clone())))
            };
            if let Some(key) = act.filter(|k| k.starts_with("do:")) {
                g.set_state(gtk::EventSequenceState::Claimed);
                if let Some(a) = ui.act.borrow_mut().as_mut() {
                    a.close();
                }
                *ui.board.borrow_mut() = Some(board::Board::open(menu_lines(&ui)));
                ui.intro.borrow_mut().clear_note();
                ui.menu_for_ask.set(true);
                board_action(&ui, &key);
                return;
            }
            // A line of the open board: its section; anywhere else, the
            // board closed (and nothing else done with the click).
            let open = ui.board.borrow().as_ref().is_some_and(|b| !b.closing());
            if open {
                g.set_state(gtk::EventSequenceState::Claimed);
                // A line of the section's board: a setting turned.
                let fv = ui.floor_view.borrow();
                let side = square() * fv.k;
                let page_line = table_under(&fv, (x, y)).and_then(|t| ui.page.borrow().as_ref().and_then(|(_, b)| b.line_at(fv.page_at, side, t)));
                drop(fv);
                if let Some(l) = page_line {
                    let key = ui.page.borrow().as_ref().map(|(_, b)| b.lines[l].key.clone()).unwrap_or_default();
                    match key.as_str() {
                        "set:night" => turn_night_mode(&ui),
                        "set:start" => {
                            turn_start(&ui);
                            return;
                        }
                        "set:developer" => set_developer_mode(!developer_mode()),
                        // Into the wallpaper or back (the boards closed on the
                        // way).
                        "set:wallpaper" => wallpaper(&ui),
                        // The updates' details: opened or closed (their lines
                        // turning up, or gone).
                        k if k.starts_with("do:") || k.starts_with("ask:") => {
                            board_action(&ui, k);
                            return;
                        }
                        "more:updates" => {
                            ui.details_open.set(!ui.details_open.get());
                            let lines = updates_lines(&ui);
                            if let Some((_, b)) = ui.page.borrow_mut().as_mut() {
                                let now = std::time::Instant::now();
                                let head = lines.iter().position(|l| l.key == "more:updates").map_or(2, |i| i + 1);
                                b.lines.truncate(head.min(lines.len()));
                                for (i, mut l) in lines.into_iter().enumerate() {
                                    if i < b.lines.len() {
                                        if b.lines[i].text != l.text {
                                            b.set_line(i, l.text);
                                        }
                                    } else {
                                        l.since = Some(now);
                                        b.lines.push(l);
                                    }
                                }
                            }
                            show_fold(&ui, ui.fold.get().0);
                            return;
                        }
                        _ => return,
                    }
                    trace(format_args!("setting {key} turned"));
                    let _ = l;
                    refresh_settings(&ui);
                    return;
                }
                let fv = ui.floor_view.borrow();
                let line = table_under(&fv, (x, y)).and_then(|t| ui.board.borrow().as_ref().and_then(|b| b.line_at(fv.board_at, square() * fv.k, t)));
                drop(fv);
                let key = line.and_then(|l| ui.board.borrow().as_ref().map(|b| b.lines[l].key.clone()));
                // A line that asks for a job: its question beside the menu.
                if let Some(k) = key.as_deref().filter(|k| k.starts_with("do:")) {
                    if let Some(b) = ui.board.borrow_mut().as_mut() {
                        b.chosen = line;
                    }
                    board_action(&ui, k);
                    return;
                }
                // A section set in words: on the table beside the menu (the
                // menu kept, its line chosen).
                let on_table = key.as_deref().and_then(|k| ui.section_words.borrow().get(k).cloned().map(|rows| (k.to_owned(), rows)));
                let on_table = on_table.or_else(|| key.as_deref().filter(|k| ["about", "storage", "updates"].contains(k)).map(|k| (k.to_owned(), vec![("", "no duo yet".to_owned())])));
                // item/grid's own: its settings (lines to click), about it.
                let own = match key.as_deref() {
                    Some("settings") => Some(settings_lines(&ui)),
                    Some("itemgrid") => Some(vec![board::Line::new("", format!("itemgrid  {}", env!("CARGO_PKG_VERSION")))]),
                    Some("repair") => Some(repair_lines(&ui)),
                    _ => None,
                };
                if let (Some(lines), Some(key)) = (own, key.clone()) {
                    if let Some(b) = ui.board.borrow_mut().as_mut() {
                        b.chosen = line;
                    }
                    trace(format_args!("board: {key} on the table"));
                    *ui.page.borrow_mut() = Some((key, board::Board::open(lines)));
                    return;
                }
                if let Some((key, rows)) = on_table {
                    if let Some(b) = ui.board.borrow_mut().as_mut() {
                        b.chosen = line;
                    }
                    let lines = if key == "updates" { updates_lines(&ui) } else { table_lines(&rows) };
                    trace(format_args!("board: {key} on the table"));
                    *ui.page.borrow_mut() = Some((key, board::Board::open(lines)));
                    return;
                }
                // A question open: a click off it lets it be (cancel or
                // Escape put it away), the menu with it.
                if ui.asking.borrow().is_some() && key.is_none() {
                    return;
                }
                if let Some(b) = ui.board.borrow_mut().as_mut() {
                    b.close();
                }
                if let Some((_, b)) = ui.page.borrow_mut().as_mut() {
                    b.close();
                }
                pg2.set_cursor_from_name(None);
                if let Some(key) = key {
                    trace(format_args!("board: {key}"));
                    let mut i = 0;
                    while let Some(r) = nav.row_at_index(i) {
                        if r.widget_name() == key {
                            nav.select_row(Some(&r));
                        }
                        i += 1;
                    }
                    right.set_visible_child_name(&key);
                }
                return;
            }
            let standing = ui.intro.borrow().done() && ui.intro.borrow().duo() < 0.5;
            if n != 1 || !standing {
                return;
            }
            let fv = ui.floor_view.borrow();
            match cube_at(&fv, (x, y)) {
                Some(i) => {
                    drop(fv);
                    trace(format_args!("cube {i} pressed: looking for the phone"));
                    if let Some(b) = ui.board.borrow_mut().as_mut() {
                        b.close();
                    }
                    ui.intro.borrow_mut().press(i);
                }
                None => {
                    // The table's square under the pointer.
                    let Some(p) = table_under(&fv, (x, y)) else { return };
                    let step = square() * fv.k;
                    let (sx, sy) = floor_shift(fv.k);
                    let at = (sx + (((p.0 - sx) / step).floor() + 0.5) * step, sy + (((p.1 - sy) / step).floor() + 0.5) * step);
                    drop(fv);
                    trace(format_args!("square at {at:?} pressed: looking for the phone"));
                    if let Some(b) = ui.board.borrow_mut().as_mut() {
                        b.close();
                    }
                    ui.intro.borrow_mut().tap(at);
                }
            }
            g.set_state(gtk::EventSequenceState::Claimed);
            if !ui.looking.get() {
                look(&ui);
            }
        });
        page.add_controller(click);
    }
    *owner.borrow_mut() = Some(ui.clone());

    storage.set_draw_func({
        let ui = Rc::downgrade(&ui);
        move |area, cr, w, h| {
            let Some(ui) = ui.upgrade() else { return };
            let (w, h) = (w as f64, h as f64);
            let fg = area.color();
            rounded(cr, 0.0, 0.0, w, h, h / 2.0);
            cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.18);
            let _ = cr.fill();
            let Some(parts) = ui.parts.borrow().clone() else { return };
            let _ = cr.save();
            rounded(cr, 0.0, 0.0, w, h, h / 2.0);
            cr.clip();
            // The parts one after another, a hairline between them.
            let mut x = 0.0;
            for ((_, kib), (r, g, b)) in parts.list().iter().zip(PART_COLOURS) {
                let width = w * *kib as f64 / parts.size as f64;
                cr.rectangle(x, 0.0, (width - 1.5).max(0.0), h);
                cr.set_source_rgb(r, g, b);
                let _ = cr.fill();
                x += width;
            }
            let _ = cr.restore();
        }
    });

    // The agent's settings read from the phone each time its page shows.
    let agent_read = gio::SimpleAction::new("agent-read", None);
    window.add_action(&agent_read);
    right.connect_visible_child_name_notify({
        let agent_read = agent_read.clone();
        move |r| {
            if r.visible_child_name().as_deref() == Some("agent") {
                agent_read.activate(None);
            }
        }
    });
    let agent_again = gio::SimpleAction::new("agent-read-if-shown", None);
    agent_again.connect_activate({
        let (right, agent_read) = (right.clone(), agent_read.clone());
        move |_, _| {
            if right.visible_child_name().as_deref() == Some("agent") {
                agent_read.activate(None);
            }
        }
    });
    window.add_action(&agent_again);
    agent_read.connect_activate({
        let (ui, key_row, key_forget, model_row, limit_row) = (Rc::downgrade(&ui), key_row.clone(), key_forget.clone(), model_row.clone(), limit_row.clone());
        move |_, _| {
            let Some(ui) = ui.upgrade() else { return };
            let Some(host) = ui.state.borrow().host.clone() else {
                key_row.set_subtitle("Your Duo is not here: its settings are read when it is.");
                // Looked for again while the page shows (opened at the start,
                // before the phone was found).
                let window = ui.window.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                    gio::prelude::ActionGroupExt::activate_action(&window, "agent-read-if-shown", None);
                });
                return;
            };
            let (key_row, key_forget, model_row, limit_row) = (key_row.clone(), key_forget.clone(), model_row.clone(), limit_row.clone());
            glib::spawn_future_local(async move {
                let read = gio::spawn_blocking(move || (itemgrid_core::agent::stored(&host), itemgrid_core::agent::choices(&host))).await;
                let Ok((stored, choices)) = read else { return };
                match stored {
                    Ok(Some(last)) => {
                        key_row.set_subtitle(&format!("Set on the phone: …{last}"));
                        key_forget.set_visible(true);
                    }
                    Ok(None) => {
                        key_row.set_subtitle("Not set. The agent's heavy work goes through OpenRouter with your own key - kept in the phone's keyring, never on this computer.");
                        key_forget.set_visible(false);
                    }
                    Err(e) => key_row.set_subtitle(&format!("Could not read the phone's keyring: {e}")),
                }
                if let Ok(c) = choices {
                    model_row.set_text(&c.model);
                    limit_row.set_text(&c.monthly_limit.map(|l| format!("{l:.2}")).unwrap_or_default());
                }
            });
        }
    });
    // Battery and Look read from the phone each time they show.
    let toaster = |ui: &Rc<Ui>| -> Rc<dyn Fn(&str)> {
        let toasts = ui.toasts.clone();
        Rc::new(move |m: &str| toasts.add_toast(adw::Toast::new(m)))
    };
    right.connect_visible_child_name_notify({
        let ui = Rc::downgrade(&ui);
        move |r| {
            let Some(ui) = ui.upgrade() else { return };
            let Some(host) = ui.state.borrow().host.clone() else {
                // Opened before the phone was found: again in a moment.
                let r = r.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || r.notify("visible-child-name"));
                return;
            };
            match r.visible_child_name().as_deref() {
                Some("battery") => sections::load_battery(&ui.sections, host),
                Some("look") => sections::load_look(&ui.sections, host, toaster(&ui)),
                _ => {}
            }
        }
    });
    section_ui.add_walls.connect_clicked({
        let ui = Rc::downgrade(&ui);
        move |_| {
            let Some(ui) = ui.upgrade() else { return };
            let Some(host) = ui.state.borrow().host.clone() else { return };
            sections::add_pictures(&ui.sections, ui.window.upcast_ref(), host, toaster(&ui));
        }
    });
    section_ui.back_up.connect_clicked({
        let ui = ui.clone();
        move |_| run_job(&ui, Job::Backup)
    });
    section_ui.update_tree.connect_clicked({
        let ui = ui.clone();
        move |_| ask(&ui, "Update item?", "item is built from your tree and installed, and the phone restarts. Enter the PIN when it is back.", "Update", Job::Update)
    });
    key_set.connect_clicked({
        let (ui, key_row, key_forget) = (Rc::downgrade(&ui), key_row.clone(), key_forget.clone());
        move |_| {
            let Some(ui) = ui.upgrade() else { return };
            set_agent_key(&ui, &key_row, &key_forget);
        }
    });
    key_forget.connect_clicked({
        let (ui, key_row, key_forget) = (Rc::downgrade(&ui), key_row.clone(), key_forget.clone());
        move |b| {
            let Some(ui) = ui.upgrade() else { return };
            let Some(host) = ui.state.borrow().host.clone() else { return };
            let (ui2, key_row, key_forget, b) = (ui.clone(), key_row.clone(), key_forget.clone(), b.clone());
            b.set_sensitive(false);
            glib::spawn_future_local(async move {
                let done = gio::spawn_blocking(move || itemgrid_core::agent::forget(&host)).await;
                b.set_sensitive(true);
                match done {
                    Ok(Ok(())) => {
                        key_row.set_subtitle("Removed from the phone. The agent's heavy work is off until a key is set.");
                        key_forget.set_visible(false);
                        ui2.toasts.add_toast(adw::Toast::new("Key removed from the phone"));
                    }
                    Ok(Err(e)) => ui2.toasts.add_toast(adw::Toast::new(&e)),
                    Err(_) => {}
                }
            });
        }
    });
    for row in [&model_row, &limit_row] {
        row.connect_apply({
            let (ui, model_row, limit_row) = (Rc::downgrade(&ui), model_row.clone(), limit_row.clone());
            move |_| {
                let Some(ui) = ui.upgrade() else { return };
                let Some(host) = ui.state.borrow().host.clone() else { return };
                let limit = limit_row.text().trim().replace(',', ".");
                let monthly_limit = if limit.is_empty() {
                    None
                } else {
                    match limit.trim_start_matches('$').parse::<f64>() {
                        Ok(v) if v >= 0.0 => Some(v),
                        _ => {
                            ui.toasts.add_toast(adw::Toast::new("The limit is a number of dollars"));
                            return;
                        }
                    }
                };
                let c = itemgrid_core::agent::Choices { model: model_row.text().trim().to_owned(), monthly_limit };
                let ui = ui.clone();
                glib::spawn_future_local(async move {
                    match gio::spawn_blocking(move || itemgrid_core::agent::set_choices(&host, &c)).await {
                        Ok(Ok(())) => ui.toasts.add_toast(adw::Toast::new("Saved on the phone")),
                        Ok(Err(e)) => ui.toasts.add_toast(adw::Toast::new(&e)),
                        Err(_) => {}
                    }
                });
            }
        });
    }
    dev_row.connect_active_notify({
        let ui = Rc::downgrade(&ui);
        move |row| {
            set_developer_mode(row.is_active());
            if let Some(ui) = ui.upgrade() {
                ui.state.borrow_mut().pictured = false;
                look(&ui);
            }
        }
    });
    back_up_now.connect_clicked({
        let ui = ui.clone();
        move |_| run_job(&ui, Job::Backup)
    });
    r_reinstall.connect_clicked({
        let ui = ui.clone();
        move |_| erase_and_install(&ui)
    });
    r_restore.connect_clicked({
        let back_full = back_full.clone();
        move |_| {
            back_full.emit_clicked();
        }
    });
    r_android.connect_clicked({
        let ui = ui.clone();
        move |_| return_to_android(&ui)
    });
    refresh.connect_clicked({
        let ui = ui.clone();
        move |_| {
            ui.state.borrow_mut().pictured = false;
            look(&ui);
        }
    });
    update.connect_clicked({
        let ui = ui.clone();
        move |_| ask(&ui, "Update item?", "item is built from your tree and installed, and the phone restarts. Enter the PIN when it is back.", "Update", Job::Update)
    });
    reboot.connect_clicked({
        let ui = ui.clone();
        move |_| ask(&ui, "Restart the phone?", "Enter the PIN when it is back.", "Restart", Job::Reboot)
    });
    reinstall.connect_clicked({
        let ui = ui.clone();
        move |_| erase_and_install(&ui)
    });
    join.connect_clicked({
        let ui = ui.clone();
        move |_| {
            let serial = ui.serial.borrow().clone();
            ui.claimed.borrow_mut().retain(|s| *s != serial);
            claim_quietly(&ui, &serial, ui.state.borrow().host.clone());
        }
    });
    restore.connect_clicked({
        let ui = ui.clone();
        move |_| choose_restore(&ui)
    });
    try_ram.connect_clicked({
        let ui = ui.clone();
        move |_| choose_ram_image(&ui)
    });
    backup.connect_clicked({
        let ui = ui.clone();
        move |_| run_job(&ui, Job::Backup)
    });
    backup_all.connect_clicked({
        let ui = ui.clone();
        move |_| {
            ask(
                &ui,
                "Back up everything?",
                "The phone restarts into the recovery and its whole system is copied here - about 20 minutes, the phone unusable meanwhile - then it comes back to Linux. Nothing on the phone is changed.",
                "Back Up",
                Job::FullBackup,
            )
        }
    });
    get_android.connect_clicked({
        let ui = ui.clone();
        move |_| get_android_from_microsoft(&ui)
    });
    back_full.connect_clicked({
        let ui = ui.clone();
        move |_| {
            let serial = ui.serial.borrow().clone();
            let newest = itemgrid_core::backup::list(Some(&serial)).into_iter().find(|b| b.manifest.kind == itemgrid_core::backup::Kind::Full);
            let Some(b) = newest else {
                stopped(&ui, "No full backup of this phone yet: Back Up Everything makes one.");
                return;
            };
            let body = format!(
                "The whole system goes back as it was on {} (item {}): the recovery starts, the data partition is made anew, the system and the Android apps' data are written back part by part, each checked. About 40 minutes. What the phone holds now goes.",
                b.manifest.created, b.manifest.item
            );
            let dialog = adw::AlertDialog::new(Some("Back to the full backup?"), Some(&body));
            dialog.add_responses(&[("cancel", "Cancel"), ("go", "Go Back")]);
            dialog.set_response_appearance("go", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");
            let ui2 = ui.clone();
            dialog.connect_response(None, move |_, response| {
                if response == "go" {
                    run_job(&ui2, Job::AndroidBack(serial.clone()));
                }
            });
            dialog.present(Some(&ui.window));
        }
    });
    to_android.connect_clicked({
        let ui = ui.clone();
        move |_| return_to_android(&ui)
    });
    shot.connect_clicked({
        let ui = ui.clone();
        move |_| take_screens(&ui, true)
    });
    folder.connect_clicked(|_| {
        let dir = shots_dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = gio::AppInfo::launch_default_for_uri(&gio::File::for_path(&dir).uri(), gio::AppLaunchContext::NONE);
    });
    card.dismiss.connect_clicked({
        let ui = Rc::downgrade(&ui);
        move |_| {
            let Some(ui) = ui.upgrade() else { return };
            let mut st = ui.state.borrow_mut();
            st.job = None;
            st.dismissed = itemgrid_core::activity::elsewhere(u64::MAX).and_then(|a| a.ended_at).or(st.dismissed);
            drop(st);
            ui.card.hide();
        }
    });

    stack.connect_visible_child_notify({
        let ui = Rc::downgrade(&ui);
        move |_| {
            if let Some(ui) = ui.upgrade() {
                bottom_shown(&ui);
                live_sync(&ui);
            }
        }
    });
    window.connect_is_active_notify({
        let ui = Rc::downgrade(&ui);
        move |_| {
            if let Some(ui) = ui.upgrade() {
                live_sync(&ui);
            }
        }
    });
    glib::timeout_add_local_once(std::time::Duration::from_millis(900), {
        let ui = ui.clone();
        move || look(&ui)
    });
    // Not seen (asleep on Wi-Fi, mostly): its ssh at the address it had
    // tried twice a second, a look at once when it answers - back from sleep
    // it was found only at the next look, seconds later.
    glib::timeout_add_local(std::time::Duration::from_millis(500), {
        let ui = Rc::downgrade(&ui);
        let trying = Rc::new(std::cell::Cell::new(false));
        move || {
            let Some(ui) = ui.upgrade() else { return glib::ControlFlow::Break };
            let asleep = ui.state.borrow().host.is_none() && !ui.resting.get();
            if asleep && !trying.get() && !ui.looking.get() {
                trying.set(true);
                let trying = trying.clone();
                glib::spawn_future_local(async move {
                    let back = gio::spawn_blocking(itemgrid_core::link::wifi_answers_quickly).await.unwrap_or(false);
                    trying.set(false);
                    if back && !ui.looking.get() {
                        trace(format_args!("wifi: back from sleep, looking now"));
                        look(&ui);
                    }
                });
            }
            glib::ControlFlow::Continue
        }
    });
    glib::timeout_add_seconds_local(REFRESH_S, {
        let ui = ui.clone();
        let mut ticks = 0u32;
        move || {
            ticks += 1;
            let (busy, elsewhere) = {
                let st = ui.state.borrow();
                (st.busy, st.elsewhere)
            };
            // Over Wi-Fi a look costs the phone's radio, and only its state
            // is shown there: every 30 s.
            let wifi = ui.state.borrow().host.as_deref().is_some_and(|h| itemgrid_core::link::Via::of(h) == itemgrid_core::link::Via::Wifi);
            let every = if !wifi { 1 } else { 6 };
            if !busy && !elsewhere && !ui.resting.get() && ticks % every == 0 {
                look(&ui);
                live_sync(&ui);
                // Without the live view (an item without a mirror), a picture
                // now and then.
                let front = ui.window.is_active() && ui.tabs.visible_child_name().as_deref() == Some("general");
                if front && !wifi && ui.live.borrow().is_none() && ticks % SCREENS_EVERY == 0 {
                    take_screens(&ui, false);
                }
            }
            glib::ControlFlow::Continue
        }
    });
    drag.connect_drag_update({
        let ui = Rc::downgrade(&ui);
        let start = Rc::new(std::cell::Cell::new([0.0f32; 2]));
        let s2 = start.clone();
        let ui2 = ui.clone();
        drag.connect_drag_begin(move |g, _, _| {
            if let Some(ui) = ui2.upgrade() {
                // Not drawn (the cubes stand): nothing to turn - the drag is
                // the table's.
                if ui.intro.borrow().duo() < 0.5 {
                    g.set_state(gtk::EventSequenceState::Denied);
                    return;
                }
                s2.set(ui.orbit.get().1);
            }
        });
        move |_, dx, dy| {
            let Some(ui) = ui.upgrade() else { return };
            let s = start.get();
            let to = [s[0] - dx as f32 * 0.5, (s[1] + dy as f32 * 0.3).clamp(-45.0, 30.0)];
            ui.orbit.set((ui.orbit.get().0, to));
            let (a, b) = ui.fold.get();
            ui.fold.set((a + 0.1, b));
        }
    });
    // Over Wi-Fi a click on the drawn Duo opens it flat, another shuts it
    // (a drag is no click; the room round it is no Duo).
    turn_back.connect_released({
        let ui = Rc::downgrade(&ui);
        move |g, n, x, y| {
            let Some(ui) = ui.upgrade() else { return };
            let on_floor = g.widget().and_then(|w| w.compute_point(&ui.floor, &gtk::graphene::Point::new(x as f32, y as f32)));
            if n == 1 && on_floor.is_some_and(|p| duo_clicked(&ui, (p.x() as f64, p.y() as f64))) {
                g.set_state(gtk::EventSequenceState::Claimed);
            }
        }
    });
    turn_back.connect_pressed({
        let ui = Rc::downgrade(&ui);
        move |_, n, _, _| {
            if n == 2 {
                if let Some(ui) = ui.upgrade() {
                    ui.orbit.set((ui.orbit.get().0, [0.0, 0.0]));
                    let (a, b) = ui.fold.get();
                    ui.fold.set((a + 0.1, b));
                }
            }
        }
    });
    // The fold, eased toward the hinge's angle each frame; the angle read
    // each second while the window is in front (each 5 s behind it).
    ui.duo.add_tick_callback({
        let ui = Rc::downgrade(&ui);
        let drawn_at = std::cell::Cell::new(-1);
        let last_frame = std::cell::Cell::new(0i64);
        let hooked = std::cell::Cell::new(false);
        move |_, clock| {
            let Some(ui) = ui.upgrade() else { return glib::ControlFlow::Break };
            // ITEMGRID_FRAMES=1: the frame's layout and paint (GTK's own
            // part: snapshot, render, the swap) timed too.
            if !hooked.replace(true) && std::env::var_os("ITEMGRID_FRAMES").is_some() {
                let at = Rc::new(std::cell::Cell::new(std::time::Instant::now()));
                clock.connect_layout({
                    let at = at.clone();
                    move |_| at.set(std::time::Instant::now())
                });
                let painting = Rc::new(std::cell::Cell::new(std::time::Instant::now()));
                clock.connect_paint({
                    let painting = painting.clone();
                    move |_| painting.set(std::time::Instant::now())
                });
                clock.connect_after_paint(move |_| {
                    let (laid, painted) = (painting.get().duration_since(at.get()), painting.get().elapsed());
                    trace(format_args!("layout {:.2} ms paint in {:.2} ms", laid.as_secs_f64() * 1000.0, painted.as_secs_f64() * 1000.0))
                });
            }
            if ui.duo.width() > 1 && !ui.intro.borrow().begun() {
                ui.intro.borrow_mut().begin();
            }
            // Waiting for the phone: the drawn one opens and closes, slowly.
            let working = ui.state.borrow().job.as_ref().is_some_and(|j| j.ended.is_none());
            if ui.idle.get() && !ui.shut_away.get() && !working && std::env::var_os("ITEMGRID_FOLD").is_none() {
                let t = clock.frame_time() as f64 / 1e6;
                ui.fold.set((ui.fold.get().0, 135.0 + 40.0 * (t * 0.6).sin()));
            }
            // Laid down slowly when it was closed and went; else quick, as
            // it follows the phone.
            let k = 1.0 - (-1.0f64 / 60.0 / if ui.shut_away.get() { 0.18 } else { 0.05 }).exp();
            let (shown, to) = ui.fold.get();
            let (tilt, tilt_to) = ui.tilt.get();
            let (orbit, orbit_to) = ui.orbit.get();
            let (oq, oq_to, ohave) = ui.orient.get();
            let qfar = ohave && (0..4).any(|i| (oq[i] - oq_to[i]).abs() > 1e-4);
            if qfar {
                // Toward the phone's quaternion (the short way), quickly:
                // it comes 50 times a second.
                let d: f64 = (0..4).map(|i| oq[i] * oq_to[i]).sum();
                let sign = if d < 0.0 { -1.0 } else { 1.0 };
                let kq = 1.0 - (-1.0f64 / 60.0 / 0.035).exp();
                let mut n = [0.0; 4];
                for i in 0..4 {
                    n[i] = oq[i] + (sign * oq_to[i] - oq[i]) * kq;
                }
                let l = n.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-12);
                ui.orient.set((n.map(|v| v / l), oq_to, true));
            }
            // The reference toward where a look puts it,
            // slowly (about a second): no jump in the drawing.
            let mut rfar = false;
            if let (Some(y), Some(to)) = (ui.yaw_ref.get(), ui.yaw_ref_to.get()) {
                let d = wrap(to - y);
                if d.abs() > 1e-4 {
                    let kr = 1.0 - (-1.0f64 / 60.0 / 0.6).exp();
                    ui.yaw_ref.set(Some(y + d * kr));
                    rfar = true;
                }
            }
            let far = rfar || qfar || (shown - to).abs() > 0.05 || (0..2).any(|i| (tilt[i] - tilt_to[i]).abs() > 0.05 || (orbit[i] - orbit_to[i]).abs() > 0.05);
            // The start, and the cubes rising or sinking; the squares' size
            // eased toward the wheel's.
            let now_us = clock.frame_time();
            static FRAMES: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            if *FRAMES.get_or_init(|| std::env::var_os("ITEMGRID_FRAMES").is_some()) {
                trace(format_args!("tick +{:.1} ms", (now_us - last_frame.get()) as f64 / 1000.0));
            }
            let dt = (now_us - last_frame.replace(now_us)) as f32 / 1e6;
            let resized = ease_square(dt.clamp(0.0, 0.1));
            if resized {
                ui.floor.queue_draw();
                ui.floor_gl.queue_render();
                ui.cable.queue_draw();
            }
            let pressing = ui.buttons.borrow_mut().step(dt.clamp(0.0, 0.1));
            // The board turning up or fading; gone, dropped.
            let boarding = {
                let mut b = ui.board.borrow_mut();
                if b.as_ref().is_some_and(|b| b.gone()) {
                    *b = None;
                    true
                } else {
                    b.as_ref().is_some_and(|b| b.moving())
                }
            };
            // The phone's line to click too.
            let boarding = {
                let mut a = ui.act.borrow_mut();
                if a.as_ref().is_some_and(|b| b.gone()) {
                    *a = None;
                    true
                } else {
                    boarding || a.as_ref().is_some_and(|b| b.moving())
                }
            };
            // The section's board too; the eye drawing back for it, or in.
            let paging = {
                let mut p = ui.page.borrow_mut();
                if p.as_ref().is_some_and(|(_, b)| b.gone()) {
                    *p = None;
                }
                let open = p.as_ref().is_some_and(|(_, b)| !b.closing());
                let moving = p.as_ref().is_some_and(|(_, b)| b.moving());
                let size = p.as_ref().filter(|(_, b)| !b.closing()).map(|(_, b)| (b.width() as f32, b.lines.len() as f32));
                drop(p);
                // The size eased to the open board's (taken at once while the
                // eye is still in: it draws back for it anyway).
                let sizing = match (size, ui.page_dims.get()) {
                    (Some(to), Some(now)) if ui.page_back.get() > 0.0 && now != to => {
                        let k = 1.0 - (-dt.clamp(0.0, 0.1) / 0.25).exp();
                        let mut n = (now.0 + (to.0 - now.0) * k, now.1 + (to.1 - now.1) * k);
                        if (n.0 - to.0).abs() < 0.01 && (n.1 - to.1).abs() < 0.01 {
                            n = to;
                        }
                        ui.page_dims.set(Some(n));
                        true
                    }
                    (Some(to), _) => {
                        ui.page_dims.set(Some(to));
                        false
                    }
                    _ => false,
                };
                // The menu's step (the Duo away) and a section's (the eye
                // drawn back) on their way, as the layout has them.
                let timing = ui.layout.get().timing();
                let menu_open = ui.board.borrow().as_ref().is_some_and(|b| !b.closing());
                let (a, a_to) = (ui.duo_away.get(), if menu_open { 1.0 } else { 0.0 });
                let a_step = (a_to - a).signum() * (dt.clamp(0.0, 0.1) / timing[scene::MENU].all()).min((a_to - a).abs());
                if a_step != 0.0 {
                    ui.duo_away.set(a + a_step);
                }
                // Looked for, not found, opened, on the cable: as they are.
                let marked = {
                    let wifi = ui.state.borrow().host.as_deref().is_some_and(|h| itemgrid_core::link::Via::of(h) == itemgrid_core::link::Via::Wifi);
                    let intro = ui.intro.borrow();
                    let cable = ui.state.borrow().host.as_deref().is_some_and(|h| itemgrid_core::link::Via::of(h) == itemgrid_core::link::Via::Cable);
                    let to = [intro.searching(), intro.note().is_some(), wifi && ui.wifi_open.get(), cable];
                    drop(intro);
                    let now = ui.marks_raw.get();
                    let next: [f32; 4] = std::array::from_fn(|k| {
                        let to = if to[k] { 1.0 } else { 0.0 };
                        let all = timing[[scene::SEARCH, scene::NOT_FOUND, scene::PHONE_OPEN, scene::CABLE][k]].all();
                        now[k] + (to - now[k]).signum() * (dt.clamp(0.0, 0.1) / all).min((to - now[k]).abs())
                    });
                    ui.marks_raw.set(next);
                    next != now
                };
                let (now, to) = (ui.page_back.get(), if open { 1.0 } else { 0.0 });
                let step = (to - now).signum() * (dt.clamp(0.0, 0.1) / timing[scene::SECTION].all()).min((to - now).abs());
                if step != 0.0 {
                    ui.page_back.set(now + step);
                }
                moving || sizing || step != 0.0 || a_step != 0.0 || marked
            };
            // The saver: the eye going over to drifting (and back), the
            // frames going on while it drifts.
            let saving = {
                let (now, to) = (ui.saver_mix.get(), if ui.saver.get().is_some() { 1.0 } else { 0.0 });
                // In over two seconds; back in half of one (the window is
                // its usual size again at once).
                let span = if to > now { 2.0 } else { 0.5 };
                let step = (to - now).signum() * (dt.clamp(0.0, 0.1) / span).min((to - now).abs());
                ui.saver_mix.set(now + step);
                ui.saver.get().is_some() || step != 0.0
            };
            // The wheel's lens eased toward where it was sent.
            let zooming = {
                let (now, to) = ui.zoom.get();
                if (to - now).abs() > 0.0005 {
                    let k = 1.0 - (-dt.clamp(0.0, 0.1) / 0.12).exp();
                    ui.zoom.set((now + (to - now) * k, to));
                    true
                } else if now != to {
                    ui.zoom.set((to, to));
                    true
                } else {
                    false
                }
            };
            // The wallpaper's view eased to and back, as the window grows
            // to the monitor or comes back.
            let walling = {
                let (now, to) = (ui.wallpaper_mix.get(), if ui.wallpaper.get().is_some() { 1.0 } else { 0.0 });
                let step = (to - now).signum() * (dt.clamp(0.0, 0.1) / WALL_S).min((to - now).abs());
                ui.wallpaper_mix.set(now + step);
                step != 0.0
            };
            let walling = wall_step(&ui, now_us) || walling;
            let flipping = !ui.night_waves.borrow().is_empty();
            let pressing = pressing || boarding || paging || saving || zooming || walling || flipping;
            // The start as the layout times it.
            ui.intro.borrow_mut().timing = ui.layout.get().timing();
            let far = ui.intro.borrow_mut().step() || resized || pressing || far;
            if far {
                ui.orbit.set(([0, 1].map(|i| orbit[i] + (orbit_to[i] - orbit[i]) * k as f32), orbit_to));
                let now = shown + (to - shown) * k;
                ui.fold.set((now, to));
                ui.tilt.set(([0, 1].map(|i| tilt[i] + (tilt_to[i] - tilt[i]) * k), tilt_to));
                show_fold(&ui, now);
            } else if ui.duo.width() != drawn_at.get() {
                // Drawn once at the start and when the room's size changes,
                // moving or not (nothing was, until it moved).
                drawn_at.set(ui.duo.width());
                show_fold(&ui, shown);
            }
            glib::ControlFlow::Continue
        }
    });
    // The hinge followed while the phone is in Linux and no job runs: its
    // angle as it changes (posture.rs), over one ssh.
    glib::timeout_add_seconds_local(1, {
        let ui = ui.clone();
        move || {
            follow_hinge(&ui);
            // Shut over a second ago and no gravity since (the display went
            // dark, the accelerometer with it): put down, most likely -
            // drawn lying on the table, not held where it last was (it hung
            // in the air until the phone slept, ten seconds and more).
            if let Some(shut) = ui.lid_shut_at.get() {
                let fresh = ui.gravity_at.get().is_some_and(|g| g > shut + std::time::Duration::from_millis(300));
                if shut.elapsed() > std::time::Duration::from_millis(1200) && !fresh && !ui.shut_away.get() {
                    trace(format_args!("lid shut, no gravity: laid down"));
                    ui.shut_away.set(true);
                    fold_to(&ui, 0.0);
                    tilt_to(&ui, [0.0, 0.0, 1.0]);
                }
            }
            // The phone's USB link came or went: the kept ssh connection
            // over it is from the link before (dead, it held the next look
            // ~6 s until ssh gave it up) - closed. Just come (it woke) and
            // not seen: looked for now and each second for 4 s, not at the
            // next round (up to 5 s) - the phone's end of the link takes
            // its address a moment after this one's.
            let usb = itemgrid_core::link::usb_up();
            let was = ui.usb_was.replace(usb);
            if usb != was {
                gio::spawn_blocking(|| itemgrid_core::phone::close_shared(itemgrid_core::link::CABLE));
            }
            if usb && !was && ui.state.borrow().host.is_none() {
                for s in 0..5u64 {
                    let ui = ui.clone();
                    glib::timeout_add_local_once(std::time::Duration::from_millis(300 + s * 1000), move || {
                        if ui.state.borrow().host.is_none() && !ui.looking.get() {
                            look(&ui);
                        }
                    });
                }
            }
            glib::ControlFlow::Continue
        }
    });
    // ITEMGRID_SCREENS=file.png: the phone's two panels from a screenshot
    // (item/grid's, both side by side), to picture them without the phone.
    if let Some(path) = std::env::var_os("ITEMGRID_SCREENS") {
        if let Ok(pb) = gtk::gdk_pixbuf::Pixbuf::from_file(&path) {
            let w = pb.width() / 2;
            #[allow(deprecated)]
            let halves: Vec<gdk::Texture> = (0..2).map(|i| gdk::Texture::for_pixbuf(&pb.new_subpixbuf(i * w, 0, w, pb.height()))).collect();
            // Again and again: without the phone, the window clears them.
            let ui = ui.clone();
            glib::timeout_add_local(std::time::Duration::from_secs(2), move || {
                for i in 0..2 {
                    if ui.screens[i].paintable().is_none() {
                        ui.screens[i].set_paintable(Some(&halves[i]));
                    }
                }
                glib::ControlFlow::Continue
            });
        }
    }
    // ITEMGRID_FOLD: shown so from the start too, phone or not.
    if std::env::var_os("ITEMGRID_FOLD").is_some() {
        fold_to(&ui, 180.0);
    }
    // ITEMGRID_MENU=1: the sections' menu open at the start (to picture it).
    if std::env::var_os("ITEMGRID_MENU").is_some() {
        let pop = nav_pop.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || pop.popup());
    }
    // ITEMGRID_SECTION=key: that section shown first (agent, repair...).
    if let Ok(key) = std::env::var("ITEMGRID_SECTION") {
        let mut i = 0;
        while let Some(r) = nav.row_at_index(i) {
            if r.widget_name() == key {
                nav.select_row(Some(&r));
                right.set_visible_child_name(&key);
            }
            i += 1;
        }
    }
    // ITEMGRID_SHOT=file.png: the window drawn into a picture 4 s after the start
    // (to see it without a screen grab).
    if let Some(path) = std::env::var_os("ITEMGRID_SHOT") {
        let window = ui.window.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs_f64(std::env::var("ITEMGRID_SHOT_AFTER").ok().and_then(|v| v.parse().ok()).unwrap_or(4.0)), move || {
            let paintable = gtk::WidgetPaintable::new(Some(&window));
            let (w, h) = (window.width() as f64, window.height() as f64);
            let snap = gtk::Snapshot::new();
            paintable.snapshot(&snap, w, h);
            if let (Some(node), Some(native)) = (snap.to_node(), window.native()) {
                let texture = native.renderer().map(|r| r.render_texture(&node, None));
                if let Some(t) = texture {
                    let _ = t.save_to_png(&path);
                }
            }
        });
    }
    // The job under way, told each second: this window's, or the command
    // line's.
    glib::timeout_add_seconds_local(1, {
        let ui = ui.clone();
        move || {
            tell(&ui);
            glib::ControlFlow::Continue
        }
    });
    window.present();
}

/// The card brought up to date: this window's job, else one the command line
/// runs (or ended a little while ago and not put away).
fn tell(ui: &Rc<Ui>) {
    {
        let st = ui.state.borrow();
        if let Some(job) = &st.job {
            let secs = job.took.unwrap_or_else(|| job.started.elapsed().as_secs());
            // Told on the table (its lines under the word), not on a card;
            // the drawn Duo shut and still meanwhile.
            let _ = secs;
            ui.card.hide();
            if job.ended.is_none() {
                drop(st);
                fold_to(ui, 0.0);
                tilt_to(ui, [0.0, 0.0, 1.0]);
            }
            return;
        }
    }
    let other = itemgrid_core::activity::elsewhere(15 * 60);
    let dismissed = ui.state.borrow().dismissed;
    match other {
        Some(a) if a.ended_at.is_none() || a.ended_at != dismissed => {
            let running = a.ended_at.is_none();
            ui.card.hide();
            ui.actions.set_sensitive(!running);
            ui.mode_buttons.set_sensitive(!running);
            if running {
                // The phone is the command line's now: no looks, the page kept.
                ui.state.borrow_mut().elsewhere = true;
                if let Some(stop) = ui.live.borrow_mut().take() {
                    stop.stop();
                }
                ui.pages.set_visible_child_name("phone");
                duo_moving(ui, ui.card.phone(&a.job, &a.lines));
            } else if ui.state.borrow().elsewhere {
                ui.state.borrow_mut().elsewhere = false;
                ui.state.borrow_mut().pictured = false;
                ui.state.borrow_mut().last_seen = Some(std::time::Instant::now());
                look(ui);
            }
        }
        _ => {
            if ui.state.borrow().elsewhere {
                ui.state.borrow_mut().elsewhere = false;
                ui.actions.set_sensitive(true);
                ui.mode_buttons.set_sensitive(true);
                look(ui);
            }
            if !ui.state.borrow().busy {
                ui.card.hide();
            }
        }
    }
}

/// The Duo drawn shown as it looks at a job's stage.
fn duo_moving(ui: &Ui, phone: &str) {
    if phone == "Linux" {
        ui.duo_mode.set_visible(false);
        return;
    }
    for s in &ui.screens {
        s.set_paintable(gdk::Paintable::NONE);
    }
    ui.live_badge.set_visible(false);
    ui.duo_mode_label.set_label(match phone {
        "Restarting" => "Restarting…",
        "TWRP" => "Recovery (TWRP)",
        "Starting" => "Starting…",
        other => other,
    });
    // (Not over the table: the job's lines there say it.)
    ui.duo_mode.set_visible(false);
}


fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

fn shots_dir() -> std::path::PathBuf {
    std::path::Path::new(&std::env::var("HOME").unwrap_or_default()).join("itemgrid-shots")
}

/// Looks at the phone again, off the main thread, and shows what it found.
fn look(ui: &Rc<Ui>) {
    // A job on the phone: it is not asked anything meanwhile (a bootloader
    // spoken to by two at once hangs) - the job says where it is.
    {
        let st = ui.state.borrow();
        if st.busy || st.job.as_ref().is_some_and(|j| j.ended.is_none()) {
            return;
        }
    }
    let ui = ui.clone();
    ui.looking.set(true);
    glib::spawn_future_local(async move {
        let found = gio::spawn_blocking(|| {
            let seen = itemgrid_core::detect();
            let place = match seen.mode {
                Mode::Linux => Place::Linux(seen.via.clone()),
                Mode::Fastboot => Place::Fastboot(seen.via.clone()),
                Mode::Recovery => Place::Recovery(seen.via.clone()),
                Mode::Android => Place::Android(seen.via.clone()),
                Mode::Gone if itemgrid_core::android::port_without_system() => Place::NoSystem,
                Mode::Gone => itemgrid_core::android::on_usb_quietly().map(Place::Quiet).unwrap_or(Place::Gone),
            };
            // What can be done from there: is Android a guest (Linux's data
            // erased), is there a whole backup to come back from.
            let guest = place.serial().is_some_and(|s| itemgrid_core::android::guest(s).is_some());
            let status = if let Place::Linux(host) = &place { Some(status::read(host)) } else { None };
            (place, guest, status)
        })
        .await;
        ui.looking.set(false);
        let Ok((place, guest, status)) = found else { return };
        // Looked for from a cube: the wave ends; not found, a note.
        // (Asleep, it is not missing: no note.)
        let note = (place == Place::Gone && !ui.shut_away.get()).then(|| "plug in usb\nor wi-fi on".to_owned());
        if let Some(took) = ui.intro.borrow_mut().found(note) {
            trace(format_args!("looked for from a cube: {:.2} s, {}", took, if place == Place::Gone { "not found" } else { "found" }));
        }
        let (busy, elsewhere) = {
            let st = ui.state.borrow();
            (st.busy, st.elsewhere)
        };
        if busy || elsewhere {
            return;
        }
        show(&ui, place, guest, status);
    });
}

fn show(ui: &Rc<Ui>, place: Place, guest: bool, status: Option<Result<status::Status, String>>) {
    let shown = std::time::Instant::now();
    show_now(ui, place, guest, status);
    if std::env::var_os("ITEMGRID_FRAMES").is_some() {
        trace(format_args!("look shown in {:.1} ms", shown.elapsed().as_secs_f64() * 1000.0));
    }
}

fn show_now(ui: &Rc<Ui>, place: Place, guest: bool, status: Option<Result<status::Status, String>>) {
    trace(format_args!("look: {} {}", match &place { Place::Linux(h) => format!("linux {h}"), Place::Gone => "gone".into(), _ => "other".into() }, status.as_ref().map_or("-".to_string(), |s| s.as_ref().map(|s| format!("locked {:?} hinge {:?}", s.locked, s.hinge)).unwrap_or_else(|e| e.clone()))));
    let was = ui.state.borrow().host.clone();
    let now = std::time::Instant::now();
    {
        let mut st = ui.state.borrow_mut();
        st.host = match &place {
            Place::Linux(h) => Some(h.clone()),
            _ => None,
        };
        if place != Place::Gone {
            st.last_seen = Some(now);
        }
        st.place = place.clone();
    }
    let Place::Linux(host) = &place else {
        // Seen by its serial (Android, bootloader, recovery): its number
        // asked for quietly too.
        if let Some(serial) = place.serial() {
            *ui.serial.borrow_mut() = serial.to_owned();
            claim_quietly(ui, serial, None);
        }
        away_from_linux(ui, &place, guest);
        return;
    };
    ui.pages.set_visible_child_name("phone");
    ui.intro.borrow_mut().sink_to = 1.0;
    ui.idle.set(false);
    ui.shut_away.set(false);
    let dev = developer_mode();
    ui.switcher.set_visible(dev);
    if !dev {
        ui.tabs.set_visible_child_name("general");
    }
    ui.mode.set_visible(false);
    ui.linux_only.set_visible(dev);
    let mut i = 0;
    while let Some(row) = ui.nav.row_at_index(i) {
        if row.widget_name() == "developer" {
            row.set_visible(dev);
        }
        i += 1;
    }
    ui.battery.set_visible(dev);
    ui.refresh.set_visible(dev);
    ui.legend.set_visible(dev);
    ui.free_label.set_visible(!dev);
    ui.duo_mode.set_visible(false);
    // On the cable or on Wi-Fi: what leaves Linux only on the cable.
    let cable = itemgrid_core::link::Via::of(host) == itemgrid_core::link::Via::Cable;
    ui.name_sub.set_label(&if cable { "Linux · cable".to_owned() } else { format!("Linux · Wi-Fi ({host})") });
    for (b, tip) in &ui.cable_only {
        b.set_sensitive(cable);
        b.set_tooltip_text(if cable { tip.as_deref() } else { Some("Plug in the cable: this takes the phone out of Linux, where Wi-Fi does not reach") });
    }
    ui.cable_note.set_visible(!cable);
    ui.repair_note.set_visible(!cable);
    match status {
        Some(Ok(s)) => fill(ui, &s, if cable { "cable" } else { "Wi-Fi" }),
        // Shut and falling asleep (off the cable): not an error.
        Some(Err(_)) if ui.shut_away.get() => say_status(ui, "away", "Your Duo is closed", "It is asleep. Open it to wake it: item/grid finds it again."),
        Some(Err(e)) => {
            // Said plainly on the simple page; the error itself for developers.
            if dev {
                ui.banner.set_title(&format!("Could not read the phone: {e}"));
                ui.banner.set_revealed(true);
            }
            say_status(ui, "look", "Your Duo is not answering", &format!("It is there, but did not answer just now. item/grid keeps trying.\n{e}"));
        }
        None => {}
    }
    bottom_shown(ui);
    // The screens and the storage, once each time the phone comes (the
    // screens on the cable only: over Wi-Fi it is drawn shut).
    if was.is_none() || !ui.state.borrow().pictured {
        ui.state.borrow_mut().pictured = true;
        if cable {
            take_screens(ui, false);
        } else {
            for s in &ui.screens {
                s.set_paintable(gdk::Paintable::NONE);
            }
        }
        let t = std::time::Instant::now();
        count_storage(ui);
        let t1 = t.elapsed().as_secs_f64() * 1000.0;
        show_backups(ui);
        let t2 = t.elapsed().as_secs_f64() * 1000.0;
        show_slots(ui);
        if std::env::var_os("ITEMGRID_FRAMES").is_some() {
            trace(format_args!("arrived: storage {t1:.1} ms, backups {:.1} ms, slots {:.1} ms", t2 - t1, t.elapsed().as_secs_f64() * 1000.0 - t2));
        }
    }
}

/// The phone outside Linux: said calmly on its page - where it is, what that
/// means, what can be done - and only after a while gone, the page asking
/// for it.
fn away_from_linux(ui: &Rc<Ui>, place: &Place, guest: bool) {
    let st = ui.state.borrow();
    let seen_lately = st.last_seen.is_some_and(|t| t.elapsed().as_secs() < GONE_AFTER_S);
    drop(st);
    ui.state.borrow_mut().pictured = false;
    if let Some(stop) = ui.live.borrow_mut().take() {
        stop.stop();
    }
    ui.live_badge.set_visible(false);
    ui.banner.set_revealed(false);
    // Closed and gone, it is asleep - not restarting, not lost: lying shut
    // on the table, saying so.
    if *place == Place::Gone && ui.shut_away.get() {
        ui.pages.set_visible_child_name("phone");
        ui.tabs.set_visible_child_name("general");
        ui.switcher.set_visible(false);
        ui.linux_only.set_visible(false);
        ui.mode.set_visible(false);
        ui.duo_mode.set_visible(false);
        ui.card.hide();
        ui.updates_row.set_visible(false);
        ui.name_sub.set_label("Asleep");
        ui.battery.set_label("");
        say_status(ui, "away", "Your Duo is closed", "It is asleep. Open it to wake it: item/grid finds it again.");
        // Known to lie there shut: drawn so, not the cubes.
        ui.intro.borrow_mut().sink_to = 1.0;
        ui.idle.set(true);
        bottom_shown(ui);
        return;
    }
    if *place == Place::Gone && !seen_lately {
        // Not seen for a while: the drawn Duo waits, and says how to bring it.
        ui.pages.set_visible_child_name("phone");
        ui.tabs.set_visible_child_name("general");
        ui.switcher.set_visible(false);
        ui.linux_only.set_visible(false);
        ui.mode.set_visible(false);
        ui.duo_mode.set_visible(false);
        // The status along the bottom only when the Duo is drawn (asleep);
        // looked for, the table and the word only.
        ui.updates_row.set_visible(false);
        for s in &ui.screens {
            s.set_paintable(gdk::Paintable::NONE);
        }
        ui.name_sub.set_label("Not seen just now");
        // No phone: the cubes with the word stand where it would be.
        ui.intro.borrow_mut().sink_to = if ui.shut_away.get() { 1.0 } else { 0.0 };
        ui.battery.set_label("");
        if ui.shut_away.get() {
            say_status(ui, "away", "Your Duo is closed", "It is asleep. Open it to wake it: item/grid finds it again.");
        } else {
            say_status(ui, "away", "Looking for your Duo", "Plug it in with the USB cable, or connect it to the same Wi-Fi as this computer.\nIf it is off, hold the power key for a few seconds.");
        }
        ui.idle.set(true);
        bottom_shown(ui);
        return;
    }
    ui.idle.set(false);
    ui.intro.borrow_mut().sink_to = 1.0;
    ui.pages.set_visible_child_name("phone");
    ui.tabs.set_visible_child_name("general");
    ui.switcher.set_visible(false);
    ui.linux_only.set_visible(false);
    ui.mode.set_visible(false);
    bottom_shown(ui);
    for s in &ui.screens {
        s.set_paintable(gdk::Paintable::NONE);
    }
    while let Some(child) = ui.mode_buttons.first_child() {
        ui.mode_buttons.remove(&child);
    }
    let button = |label: &str, suggested: bool, job: Job, ask_first: Option<(&'static str, &'static str)>| {
        let b = gtk::Button::with_label(label);
        b.add_css_class("pill");
        if suggested {
            b.add_css_class("suggested-action");
        }
        ui.mode_buttons.append(&b);
        let ui = ui.clone();
        let label = label.to_owned();
        b.connect_clicked(move |_| match ask_first {
            Some((heading, body)) => ask(&ui, heading, body, &label, job.clone()),
            None => run_job(&ui, job.clone()),
        });
    };
    // Android not letting the computer in yet: drawn shut and still till
    // USB debugging is on.
    if matches!(place, Place::Quiet(_)) {
        fold_to(ui, 0.0);
        tilt_to(ui, [0.0, 0.0, 1.0]);
    }
    const BACK_BODY: &str = "Linux's system and data go back from the newest whole-system backup, each part checked on the phone - about 35 minutes. Android's data on the phone goes.";
    let (icon, duo, title, text, moving) = match place {
        Place::Fastboot(s) => {
            if guest {
                button("Start Android", true, Job::AndroidStart(s.clone()), None);
                button("Back to Linux…", false, Job::AndroidBack(s.clone()), Some(("Back to Linux?", BACK_BODY)));
                ("system-reboot-symbolic", "Bootloader", "The Duo is in its bootloader", "Android runs here as a guest: Linux's data was put away in a backup. Start Android again, or bring Linux back.", false)
            } else {
                button("Start Linux", true, Job::LeaveFastboot(s.clone()), None);
                ("system-reboot-symbolic", "Bootloader", "The Duo is in its bootloader", "Nothing is wrong: the safety catch stopped a restart here, or it was asked for. Start Linux goes on from the same slot.", false)
            }
        }
        Place::Recovery(s) => {
            if guest {
                button("Back to Linux…", true, Job::AndroidBack(s.clone()), Some(("Back to Linux?", BACK_BODY)));
            } else {
                button("Back to Linux", true, Job::RecoveryExit(s.clone()), None);
            }
            ("applications-engineering-symbolic", "Recovery (TWRP)", "The Duo is in the recovery", "TWRP, a small repair system, runs from memory. It has no touch: item/grid drives it from here.", false)
        }
        Place::Android(s) => {
            if guest {
                button("Back to Linux…", true, Job::AndroidBack(s.clone()), Some(("Back to Linux?", BACK_BODY)));
                button("Restart Android", false, Job::AndroidStart(s.clone()), None);
            }
            ("phone-symbolic", "Android", "The Duo runs Android", if guest { "Stock Android, started by item/grid as a guest. Don't restart it from its own menu: Restart Android here does it the right way." } else { "Android runs on the phone." }, false)
        }
        Place::Quiet(_) => (
            "phone-symbolic",
            "Android",
            "The Duo runs Android - or is starting",
            "item/grid sees the phone on the cable but cannot talk to it yet. If Android is up: Settings → About phone → tap Build number seven times → System → Developer options → USB debugging, then allow this computer on the phone.",
            true,
        ),
        Place::NoSystem => (
            "dialog-information-symbolic",
            "No system",
            "The Duo started without a system",
            "Android was restarted plainly, so the phone started Linux's kernel - but Linux's data is in the backup now. Hold Power about 15 seconds until it is off, then hold Volume Down and press Power: the bootloader opens, and item/grid takes it from there.",
            false,
        ),
        _ => ("content-loading-symbolic", "Restarting…", "Waiting for the Duo", "It is restarting, or the cable came out. item/grid keeps looking.", true),
    };
    ui.mode_icon.set_icon_name(Some(icon));
    ui.mode_title.set_label(title);
    ui.mode_text.set_label(text);
    if moving {
        ui.mode.add_css_class("moving");
        ui.duo_mode.add_css_class("moving");
    } else {
        ui.mode.remove_css_class("moving");
        ui.duo_mode.remove_css_class("moving");
    }
    ui.duo_mode_label.set_label(duo);
    // (Where the phone is: said on the table, beside it.)
    ui.duo_mode.set_visible(false);
    ui.name_sub.set_label(match place {
        Place::Fastboot(_) => "Bootloader",
        Place::Recovery(_) => "Recovery",
        Place::Android(_) | Place::Quiet(_) => "Android",
        Place::NoSystem => "No system",
        _ => "Not seen just now",
    });
    ui.battery.set_label("");
}

/// Developer Mode: on, the window shows slots, images from RAM, every kind
/// of backup and the logs (~/.config/itemgrid/gui).
fn developer_mode() -> bool {
    std::fs::read_to_string(gui_settings()).is_ok_and(|t| t.lines().any(|l| l.trim() == "developer=1"))
}

fn set_developer_mode(on: bool) {
    let path = gui_settings();
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = std::fs::write(path, if on { "developer=1\n" } else { "developer=0\n" });
}

fn gui_settings() -> std::path::PathBuf {
    std::path::Path::new(&std::env::var("HOME").unwrap_or_default()).join(".config/itemgrid/gui")
}

/// The simple page's sentence: its dot (fine, look, busy, away), its title
/// and the lines under it.
fn say_status(ui: &Ui, dot: &str, title: &str, lines: &str) {
    for c in ["fine", "look", "busy", "away"] {
        ui.status_dot.remove_css_class(c);
    }
    ui.status_dot.add_css_class(dot);
    ui.status_title.set_label(title);
    ui.status_lines.set_label(lines);
}

/// How long ago, in words: "today at 09:12", "yesterday", "3 days ago".
fn ago(created: &str) -> (String, i64) {
    let day = |d: &str| glib::DateTime::from_local(d.get(0..4).and_then(|y| y.parse().ok()).unwrap_or(1970), d.get(5..7).and_then(|m| m.parse().ok()).unwrap_or(1), d.get(8..10).and_then(|x| x.parse().ok()).unwrap_or(1), 0, 0, 0.0).ok();
    let now = glib::DateTime::now_local().ok();
    let today = now.as_ref().map(|n| n.format("%Y-%m-%d").map(|s| s.to_string()).unwrap_or_default()).unwrap_or_default();
    let days = match (day(created), day(&today)) {
        (Some(a), Some(b)) => b.difference(&a).as_days(),
        _ => 999,
    };
    let time = created.get(11..16).unwrap_or("");
    let words = match days {
        0 => format!("today at {time}"),
        1 => format!("yesterday at {time}"),
        n if n < 999 => format!("{n} days ago"),
        _ => created.to_owned(),
    };
    (words, days)
}

/// The simple page from the phone's state: fine, or what needs a look.
fn simple_status(ui: &Ui, s: &status::Status, problems: &[String], link: &str) {
    use itemgrid_core::backup::{self, Kind};
    let charge = s.battery.map(|b| format!("Battery {b}%")).unwrap_or_else(|| "Battery ?".into());
    let charging = match s.battery_status.as_str() {
        "Charging" => " · charging",
        "Full" => " · full",
        _ => "",
    };
    // The newest backup of the phone's own things (home and settings, or
    // everything).
    let newest = backup::list(Some(&s.serial)).into_iter().find(|b| matches!(b.manifest.kind, Kind::Quick | Kind::Full));
    ui.backups_row.set_subtitle(&match &newest {
        Some(b) => format!("Your home folder and settings, copied to this computer. Last {} · {}.", ago(&b.manifest.created).0, status::size_words(b.size() / 1024)),
        None => "Your home folder and settings, copied to this computer. About a minute, over the cable or Wi-Fi.".to_owned(),
    });
    let version = s.item.split('~').next().unwrap_or(&s.item);
    let dev_build = s.item.contains("~git");
    ui.updates_row.set_subtitle(&format!("item {version}{}", if dev_build { " · a development build" } else { "" }));
    let lines = format!("{charge}{charging} · {link}");
    // The firmware under the phone: item and the system; the port and the
    // kernel on hovering.
    let os = s.os.split(" (").next().unwrap_or(&s.os);
    let built = s.item_built.get(5..10).and_then(|md| {
        let m: usize = md.get(0..2)?.parse().ok()?;
        let d: u32 = md.get(3..5)?.parse().ok()?;
        Some(format!("{d} {}", ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"].get(m.checked_sub(1)?)?))
    });
    ui.name_sub.set_label(&if dev_build { format!("item {version} (dev, {}) · {os}", built.unwrap_or_default()) } else { format!("item {version} · {os}") });
    ui.name_sub.set_tooltip_text(Some(&format!("Port {} · kernel {}", s.port, s.kernel)));
    if !s.item_running {
        say_status(ui, "look", "item is not running", &format!("The phone is up, but its shell is not. A restart usually brings it back.\n{lines}"));
    } else if !problems.is_empty() {
        let plain: Vec<String> = problems
            .iter()
            .filter(|p| !p.contains("item is not running"))
            .map(|p| {
                if p.contains("nearly full") {
                    format!("Storage is almost full ({})", p.rsplit(": ").next().unwrap_or(""))
                } else if p.contains("battery is low") {
                    "The battery is low: plug it in".to_owned()
                } else if p.contains("CPU is hot") {
                    "The phone is hot: let it rest a while".to_owned()
                } else {
                    p.clone()
                }
            })
            .collect();
        say_status(ui, "look", "Your Duo needs a look", &format!("{}\n{lines}", plain.join("\n")));
    } else {
        say_status(ui, "fine", "Your Duo is fine", &lines);
    }
}

/// The hinge followed while the phone is in Linux and no job runs (started
/// again if it ended); stopped otherwise.
fn follow_hinge(ui: &Rc<Ui>) {
    say_pose(ui);
    let want = {
        let st = ui.state.borrow();
        st.host.clone().filter(|_| !st.busy && !st.elsewhere)
    };
    // Followed on the cable only: over Wi-Fi the Duo is drawn shut and still
    // (following there kept its sensors, its radio and its sleep busy - the
    // battery, the sensors hung asleep, slow back from sleep); the cable
    // brings it to life.
    let want = want.filter(|h| itemgrid_core::link::Via::of(h) == itemgrid_core::link::Via::Cable);
    // Nothing done at the computer for a while: let go (on the cable the
    // phone may sleep again; on Wi-Fi its radio saves again).
    let want = want.filter(|_| !ui.resting.get());
    if want.is_none() {
        ui.pose_name.borrow_mut().clear();
    }
    let running = ui.following.borrow().as_ref().map(|(h, _)| h.clone());
    if running == want {
        return;
    }
    if let Some((_, stop)) = ui.following.borrow_mut().take() {
        stop.stop();
    }
    let Some(host) = want else { return };
    // On the cable (charging) the phone is kept awake while followed: the
    // lid and the hinge come at once.
    let cable = itemgrid_core::link::Via::of(&host) == itemgrid_core::link::Via::Cable;
    trace(format_args!("follow: start {host} awake {cable}"));
    // duo-motion (put on the phone as needed), else sfduo-posture through
    // gdbus. On Wi-Fi too: asleep under duo-motion's sensors sensorfw
    // stuck, so there it is stopped as the phone is about to sleep, its
    // sessions let go first (posture.rs).
    let motion_try = itemgrid_core::posture::follow_motion(&host, cable);
    let (follow, motion) = match motion_try {
        Some(Ok(f)) => (Ok(f), true),
        Some(Err(e)) => {
            trace(format_args!("follow: duo-motion failed: {e}"));
            (itemgrid_core::posture::follow(&host, cable), false)
        }
        None => (itemgrid_core::posture::follow(&host, cable), false),
    };
    ui.motion_on.set(motion);
    ui.yaw_ref.set(None);
    ui.yaw_ref_to.set(None);
    ui.absolute.set(false);
    trace(format_args!("follow: through {}", if motion { "duo-motion" } else { "sfduo-posture" }));
    let Ok((mut follow, stop)) = follow else { return };
    *ui.following.borrow_mut() = Some((host, stop.clone()));
    let (tx, rx) = async_channel::bounded::<itemgrid_core::posture::Reading>(16);
    gio::spawn_blocking(move || {
        while let Some(a) = follow.next() {
            if tx.send_blocking(a).is_err() {
                break;
            }
        }
    });
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        // The hinge's sensor reads a few degrees shut (its posture is
        // "closed" up to 10): closed, the Duo is drawn shut.
        let mut raw = None::<f64>;
        let shut = |ui: &Ui, a: f64| if ui.pose_name.borrow().as_str() == "closed" { 0.0 } else { a };
        while let Ok(r) = rx.recv().await {
            if !matches!(r, itemgrid_core::posture::Reading::Gravity(_)) {
                trace(format_args!("reading: {r:?}"));
            }
            match r {
                itemgrid_core::posture::Reading::Angle(a) => {
                    raw = Some(a);
                    ui.last_angle.set(Some(a));
                    // duo-motion tells the angle only: the posture by it, as
                    // sfduo-posture's bands have it (the lid's switch rules
                    // "closed").
                    if ui.motion_on.get() && ui.lid_shut_at.get().is_none() {
                        let name = match a {
                            a if a < 10.0 => "closed",
                            a if (60.0..=130.0).contains(&a) => "laptop",
                            a if (150.0..=210.0).contains(&a) => "flat",
                            a if a >= 340.0 => "folded",
                            _ => "between",
                        };
                        *ui.pose_name.borrow_mut() = name.into();
                    }
                    fold_to(&ui, shut(&ui, a));
                }
                itemgrid_core::posture::Reading::Version(v) => trace(format_args!("duo-motion protocol {v}")),
                itemgrid_core::posture::Reading::North(n) => {
                    // The field heeded: the world is north's - the one at the
                    // computer where they were last seen, at once.
                    if n && !ui.absolute.get() {
                        ui.absolute.set(true);
                        if let Some(h) = ui.user_heading.get() {
                            ui.yaw_ref_to.set(Some(h + std::f64::consts::FRAC_PI_2));
                            if ui.yaw_ref.get().is_none() {
                                ui.yaw_ref.set(Some(h + std::f64::consts::FRAC_PI_2));
                            }
                        }
                    }
                }
                itemgrid_core::posture::Reading::Look(l) => {
                    // Looked at: the one at the computer is where its screen
                    // faced. The reference so that that way is toward the
                    // viewer (-y in that world: the drawing's +y), and, the
                    // world being north's, their bearing kept for next time.
                    let want = l + std::f64::consts::FRAC_PI_2;
                    trace(format_args!("look {l:.3} (reference {:?})", ui.yaw_ref.get()));
                    ui.yaw_ref_to.set(Some(want));
                    if ui.yaw_ref.get().is_none() {
                        ui.yaw_ref.set(Some(want));
                    }
                    if ui.absolute.get() {
                        ui.user_heading.set(Some(wrap(l)));
                        let _ = std::fs::create_dir_all(user_heading_file().parent().unwrap_or(std::path::Path::new(".")));
                        let _ = std::fs::write(user_heading_file(), format!("{}\n", wrap(l)));
                    }
                }
                itemgrid_core::posture::Reading::Quat(q) => {
                    // The reference taken off: a look's, else where it was
                    // at the start.
                    let yaw = |q: [f64; 4]| (2.0 * (q[0] * q[3] + q[1] * q[2])).atan2(1.0 - 2.0 * (q[2] * q[2] + q[3] * q[3]));
                    // No reference yet: drawn as it lay at the start.
                    if ui.yaw_ref.get().is_none() {
                        ui.yaw_ref.set(Some(yaw(q)));
                        ui.yaw_ref_to.set(Some(yaw(q)));
                    }
                    let (shown, _, have) = ui.orient.get();
                    ui.orient.set((if have { shown } else { q }, q, true));
                    // Which way is down, from it: for lying, held, the shadows.
                    let [w, x, y, z] = q;
                    tilt_to(&ui, [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), w * w - x * x - y * y + z * z]);
                }
                itemgrid_core::posture::Reading::Gravity(g) => {
                    ui.gravity_at.set(Some(std::time::Instant::now()));
                    // With duo-motion the quaternion tells the tilt (this is
                    // its accelerometer, the swings in it). Shut off the
                    // cable it lies on the table (Lid below), whatever comes.
                    if !ui.motion_on.get() && (cable || ui.lid_shut_at.get().is_none()) {
                        tilt_to(&ui, g);
                    }
                    let mut gs = ui.gravities.borrow_mut();
                    gs.push_back((std::time::Instant::now(), g));
                    while gs.len() > 40 {
                        gs.pop_front();
                    }
                }
                itemgrid_core::posture::Reading::Posture(p) => {
                    *ui.pose_name.borrow_mut() = p;
                    if let Some(a) = raw {
                        fold_to(&ui, shut(&ui, a));
                    }
                }
                // The lid's switch, at once: shut, drawn shut; opened, drawn
                // opening (a laptop's angle) until the hinge's own reading
                // comes - seconds later, once the display is lit.
                itemgrid_core::posture::Reading::Lid(true) => {
                    ui.lid_shut_at.set(Some(std::time::Instant::now()));
                    *ui.pose_name.borrow_mut() = "closed".into();
                    fold_to(&ui, 0.0);
                    // Off the cable nothing keeps it awake: it sleeps now and
                    // goes from the network. Drawn lying shut on the table at
                    // once, asleep (it hung in the air at its last tilt until
                    // the link was found dead, then was taken for restarting).
                    if !cable {
                        ui.shut_away.set(true);
                        tilt_to(&ui, [0.0, 0.0, 1.0]);
                    }
                }
                itemgrid_core::posture::Reading::Lid(false) => {
                    ui.lid_shut_at.set(None);
                    if ui.pose_name.borrow().as_str() == "closed" {
                        ui.pose_name.borrow_mut().clear();
                    }
                    ui.shut_away.set(false);
                    if raw.is_none_or(|a| a < 30.0) {
                        fold_to(&ui, 110.0);
                    }
                }
            }
            say_pose(&ui);
        }
        trace(format_args!("follow: ended (pose {:?}, last angle {:?}, tilt to {:?})", ui.pose_name.borrow(), ui.last_angle.get(), ui.tilt.get().1));
        // Its quaternion no longer comes: the tilt (laid down if it was
        // closed) draws it from here.
        let (q, qt, _) = ui.orient.get();
        ui.orient.set((q, qt, false));
        ui.motion_on.set(false);
        // It ended (the phone went, or was stopped): started again next second.
        // Gone just as it was being closed - it went to sleep: drawn closing
        // the rest of the way and lying down on the table, gently (it froze
        // in the air at the last angle read).
        let closing = ui.pose_name.borrow().as_str() == "closed" || ui.last_angle.get().is_some_and(|a| a < 60.0);
        if closing {
            ui.shut_away.set(true);
            fold_to(&ui, 0.0);
            tilt_to(&ui, [0.0, 0.0, 1.0]);
        }
        let mine = ui.following.borrow().as_ref().is_some_and(|(_, s)| s.same(&stop));
        if mine {
            ui.following.borrow_mut().take();
        }
    });
}

/// The posture in words, from the hinge's posture, its angle and the right
/// half's gravity (x across, y along, z out of its screen): closed, laptop
/// (the right half lying) or book (held with the spine upright), open
/// flat, tent, folded back, partly open; "in your hand" when its gravity
/// moved within the last two seconds.
fn say_pose(ui: &Ui) {
    let name = ui.pose_name.borrow().clone();
    let angle = ui.fold.get().1;
    let gs = ui.gravities.borrow();
    let g = gs.back().map(|(_, g)| *g).unwrap_or([0.0, 0.0, 1.0]);
    let recent: Vec<[f64; 3]> = gs.iter().filter(|(t, _)| t.elapsed().as_secs_f64() < 2.0).map(|(_, g)| *g).collect();
    let spread = (0..3).map(|i| recent.iter().map(|g| g[i]).fold(f64::MIN, f64::max) - recent.iter().map(|g| g[i]).fold(f64::MAX, f64::min)).fold(0.0, f64::max);
    let held = recent.len() >= 3 && spread > 0.06;
    let words = match name.as_str() {
        "" => "",
        "closed" => "Closed",
        "folded" => "Folded back",
        "flat" => "Open flat",
        "laptop" if g[1].abs() > 0.7 => "Book",
        "laptop" => "Laptop",
        _ if (200.0..340.0).contains(&angle) => "Tent",
        _ => "Partly open",
    };
    let wifi = ui.state.borrow().host.as_deref().is_some_and(|h| itemgrid_core::link::Via::of(h) == itemgrid_core::link::Via::Wifi);
    let line = match (words, held) {
        // Over Wi-Fi not followed: drawn shut (fill).
        _ if wifi => "On Wi-Fi · plug in the cable to see it move".to_owned(),
        ("", _) => String::new(),
        (w, true) => format!("{w} · in your hand"),
        (w, false) => w.to_owned(),
    };
    if ui.pose.label() != line {
        ui.pose.set_label(&line);
    }
}

/// How the phone is tipped, from its gravity (the right half's frame: x
/// across, y toward its top, z out of its screen): pitch, its top raised;
/// roll, its outer edge raised; eased there.
fn tilt_to(ui: &Ui, g: [f64; 2 + 1]) {
    let pitch = g[1].atan2(g[2]).to_degrees();
    let roll = (-g[0]).atan2((g[1] * g[1] + g[2] * g[2]).sqrt()).to_degrees();
    let (shown, _) = ui.tilt.get();
    ui.tilt.set((shown, [pitch, roll]));
}

/// The phone's fold, to be shown: eased there (the tick above).
fn fold_to(ui: &Ui, angle: f64) {
    // ITEMGRID_FOLD=degrees: shown at that angle whatever the phone says (to
    // picture a posture).
    let angle = std::env::var("ITEMGRID_FOLD").ok().and_then(|v| v.parse().ok()).unwrap_or(angle);
    let (shown, _) = ui.fold.get();
    ui.fold.set((shown, angle.clamp(0.0, 360.0)));
}

/// One half of the drawn Duo: its body (the left or the right, drawn over
/// the whole width) and its live screen in its panel.
fn duo_half(body: Option<&gdk::Texture>, screen: Option<&gdk::Paintable>, i: usize, bw: i32, bh: i32) -> gdk::Paintable {
    use gtk::graphene;
    let k = DUO_PX_PER_MM as f32;
    let mid = (bw / 2) as f32;
    let x0 = if i == 0 { 0.0 } else { mid };
    let snap = gtk::Snapshot::new();
    snap.translate(&graphene::Point::new(DUO_PAD, DUO_PAD));
    if let Some(body) = body {
        snap.append_texture(body, &graphene::Rect::new(-x0, 0.0, bw as f32, bh as f32));
    }
    if let Some(screen) = screen {
        let sx = [DUO_SCREEN_X.0, DUO_SCREEN_X.1][i] as f32 * k - x0;
        let (pw, ph) = (DUO_PANEL.0 as f32 * k, DUO_PANEL.1 as f32 * k);
        snap.save();
        snap.translate(&graphene::Point::new(sx, DUO_SCREEN_TOP as f32 * k));
        snap.push_clip(&graphene::Rect::new(0.0, 0.0, pw, ph));
        // Covering the panel, as the picture did.
        let (iw, ih) = (screen.intrinsic_width().max(1) as f32, screen.intrinsic_height().max(1) as f32);
        let scale = (pw / iw).max(ph / ih);
        let (dw, dh) = (iw * scale, ih * scale);
        snap.translate(&graphene::Point::new((pw - dw) / 2.0, (ph - dh) / 2.0));
        screen.snapshot(&snap, dw as f64, dh as f64);
        snap.pop();
        snap.restore();
    }
    flatten(&snap, mid + 2.0 * DUO_PAD, bh as f32 + 2.0 * DUO_PAD)
}

/// What a snapshot drew, made one texture (`w` x `h`, twice as dense): a
/// picture turned in 3D is drawn whole then - GTK drew the clips, masks and
/// blurs of a turned render node in pieces and in the wrong places.
fn flatten(snap: &gtk::Snapshot, w: f32, h: f32) -> gdk::Paintable {
    use gtk::{graphene, gsk};
    thread_local! {
        static RENDERER: Option<gsk::CairoRenderer> = gdk::Display::default().and_then(|d| {
            let r = gsk::CairoRenderer::new();
            let _ = d;
            r.realize(None::<&gdk::Surface>).ok().map(|_| r)
        });
    }
    let node = snap.clone().to_node();
    let texture = node.and_then(|node| {
        let scaled = gsk::TransformNode::new(&node, &gsk::Transform::new().scale(2.0, 2.0));
        RENDERER.with(|r| r.as_ref().map(|r| r.render_texture(&scaled, Some(&graphene::Rect::new(0.0, 0.0, 2.0 * w, 2.0 * h)))))
    });
    match texture {
        Some(t) => t.upcast(),
        None => gdk::Paintable::new_empty(w as i32, h as i32),
    }
}

/// Where the floor's lines are (px from the phone's middle): a column of
/// squares centred on the USB port, so the hole is right in front of it.
/// The corner of the table's square nearest a point (px): where words and
/// boards begin, so that their letters keep to the squares.
fn on_squares(k: f32, p: (f32, f32)) -> (f32, f32) {
    let step = square() * k;
    let (sx, sy) = floor_shift(k);
    (sx + ((p.0 - sx) / step).round() * step, sy + ((p.1 - sy) / step).round() * step)
}

/// Laid from the cubes once they are placed: five squares across them,
/// one along (their middle stays where it is as the size changes).
fn floor_shift(k: f32) -> (f32, f32) {
    let step = square() * k;
    let c = [0, 1].map(|i| f32::from_bits(CUBES_AT[i].load(std::sync::atomic::Ordering::Relaxed)));
    if c != [0.0, 0.0] {
        return ((c[0] - WORD_HALF * step).rem_euclid(step), (c[1] - 0.5 * step).rem_euclid(step));
    }
    ((CABLE_PORT_X as f32 * k - step / 2.0).rem_euclid(step), 0.0)
}

/// The page's floor, as the phone's table at rest is seen.
#[derive(Default)]
struct FloorView {
    matrix: Option<gtk::graphene::Matrix>,
    /// The duo's drawing's origin on the page, and the phone's middle in it.
    off: (f32, f32),
    at: (f32, f32),
    k: f32,
    table: f32,
    /// The hole the cord goes down (its square on the table, its depth).
    hole: Option<([f32; 4], f32)>,
    /// The start (intro.rs): how far the squares have grown out round the
    /// cubes, the word's strength, each cube's being there and height.
    grid: f32,
    word: f32,
    cubes: [(f32, f32); intro::WORD.len()],
    /// How far the eye has come down (0 straight above .. 1).
    eye: f32,
    /// The cubes' middle on the table (px, in the table's squares).
    cubes_at: (f32, f32),
    /// The note under them (not found) and its strength.
    note: Option<(String, f32)>,
    /// A square of the table jumping (clicked): its middle, its height.
    tapped: Option<((f32, f32), f32)>,
    /// Words set in the table's squares.
    texts: Vec<TableText>,
    /// Squares turning up as a board's (board.rs): the credit's, the open
    /// board's; where that board begins.
    tiles: Vec<board::Tile>,
    board_at: (f32, f32),
    /// The parts that can be moved (E) and their boxes on the table (px):
    /// the word, the buttons, the phone's words, the Duo, the credit, the
    /// open menu.
    edit_boxes: Vec<(&'static str, [f32; 4])>,
    /// The waves of the squares turning over: how far each, from where;
    /// the table's point under the page's bottom middle.
    waves: Vec<(f32, (f32, f32))>,
    near_at: (f32, f32),
    /// The page's size (px).
    size: (f32, f32),
    /// The squares turning over as the table grows out (the wave start).
    front: bool,
    /// The Duo's top left corner on the sheet (squares).
    duo_corner: (f32, f32),
    page_at: (f32, f32),
    act_at: (f32, f32),
    /// Across the table, where the eye's line is (boxes drawn farthest
    /// from it first: a box nearer it covers its neighbour's side).
    eye_x: f32,
    /// The buttons' row (its first square's far left corner), their lifts,
    /// the one under the pointer.
    buttons_at: (f32, f32),
    button_lift: [f32; BUTTONS],
    button_in: [f32; BUTTONS],
    hover_button: Option<usize>,
    /// How far the squares are drawn (px) round where the eye looks:
    /// further on a larger page and as the eye draws back.
    reach: f32,
    /// The sheet things are laid on (its left, top, right, bottom, table
    /// px), outlined.
    sheet: Option<[f32; 4]>,
    /// The word's and the buttons' strength as the step has them.
    part_alpha: [f32; 6],
    grid_mid: (f32, f32),
}

/// Words set on the table, a letter a square of `cell` (px; a square of
/// the table's, or a half or a quarter of one - the finer lines shown
/// under them): from `at` (the first letter's square's far left corner),
/// a line a row; so much of it set (0..1, letter by letter); its strength.
#[derive(Clone, PartialEq)]
struct TableText {
    at: (f32, f32),
    cell: f32,
    lines: Vec<String>,
    grey: f64,
    bold: bool,
    set: f32,
    strength: f32,
}

/// The squares for the GPU (grid_gl) from the floor's view: the same
/// lines and fading draw_floor's cairo draws.
fn grid_of(fv: &FloorView) -> Option<grid_gl::Grid> {
    use gtk::graphene;
    let m = fv.matrix?;
    let step = square() * fv.k;
    let z = fv.table;
    let v0 = m.transform_vec4(&graphene::Vec4::new(0.0, 0.0, z, 1.0));
    let vx = m.transform_vec4(&graphene::Vec4::new(1.0, 0.0, 0.0, 0.0));
    let vy = m.transform_vec4(&graphene::Vec4::new(0.0, 1.0, 0.0, 0.0));
    // The table (x, y, 1) to the page (homogeneous, before the offset).
    let h = [[vx.x() as f64, vy.x() as f64, v0.x() as f64], [vx.y() as f64, vy.y() as f64, v0.y() as f64], [vx.w() as f64, vy.w() as f64, v0.w() as f64]];
    let det = h[0][0] * (h[1][1] * h[2][2] - h[1][2] * h[2][1]) - h[0][1] * (h[1][0] * h[2][2] - h[1][2] * h[2][0]) + h[0][2] * (h[1][0] * h[2][1] - h[1][1] * h[2][0]);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = [
        [(h[1][1] * h[2][2] - h[1][2] * h[2][1]) / det, (h[0][2] * h[2][1] - h[0][1] * h[2][2]) / det, (h[0][1] * h[1][2] - h[0][2] * h[1][1]) / det],
        [(h[1][2] * h[2][0] - h[1][0] * h[2][2]) / det, (h[0][0] * h[2][2] - h[0][2] * h[2][0]) / det, (h[0][2] * h[1][0] - h[0][0] * h[1][2]) / det],
        [(h[1][0] * h[2][1] - h[1][1] * h[2][0]) / det, (h[0][1] * h[2][0] - h[0][0] * h[2][1]) / det, (h[0][0] * h[1][1] - h[0][1] * h[1][0]) / det],
    ];
    let night = night();
    Some(grid_gl::Grid {
        inv: inv.map(|r| r.map(|v| v as f32)),
        w_row: h[2].map(|v| v as f32),
        off: fv.off,
        step,
        shift: floor_shift(fv.k),
        mid: fv.grid_mid,
        reach: fv.reach,
        cubes_at: fv.cubes_at,
        grown: fv.grid * (fv.reach + 3.0 * step),
        grid: fv.grid,
        ink: if night { [1.0; 3] } else { INKS[style().ink.min(INKS.len() - 1)].1.map(|v| v as f32) },
        width: (line_width() / 1.6) as f32,
        ink_k: if night { 1.15 } else { 1.0 },
        waves: std::array::from_fn(|i| fv.waves.get(i).copied().unwrap_or((0.0, (0.0, 0.0)))),
        wave_n: fv.waves.len().min(grid_gl::WAVES) as i32,
        base_night: night,
        fill_day: paper_rgb().map(|v| v as f32),
        fill_night: [NIGHT_TABLE as f32, NIGHT_TABLE as f32, (NIGHT_TABLE * 1.02) as f32],
        ink_day: INKS[style().ink.min(INKS.len() - 1)].1.map(|v| v as f32),
        ink_night: [1.0; 3],
        near_at: fv.near_at,
        front: fv.front,
        size: (fv.size.0, fv.size.1),
    })
}

/// The table under the Duo across the page, in 2 cm squares, in the very
/// view the phone at rest is drawn in - so it lies on it; faint, fading
/// with the distance on the table.
fn draw_floor(fv: &FloorView, cr: &gtk::cairo::Context, _w: i32, _h: i32) {
    use gtk::graphene;
    // The squares (and the night's table under them) on the GPU, if it
    // can: grid_gl.
    let gpu = grid_gl::on();
    if night() && !gpu {
        cr.set_source_rgb(NIGHT_TABLE, NIGHT_TABLE, NIGHT_TABLE * 1.02);
        let _ = cr.paint();
    }
    let Some(m) = fv.matrix else { return };
    let k = fv.k;
    let step = square() * k;
    let reach = fv.reach;
    let z = fv.table;
    let project = |x: f32, y: f32| -> Option<(f64, f64)> {
        let v = m.transform_vec4(&graphene::Vec4::new(x, y, z, 1.0));
        if v.w() <= 0.005 {
            return None;
        }
        Some(((v.x() / v.w() + fv.off.0) as f64, (v.y() / v.w() + fv.off.1) as f64))
    };
    // At the start the squares grow out from the cubes' middle: a line
    // there when the growing has reached it.
    let grown = fv.grid * (reach + 3.0 * step);
    // Round where the eye looks (it may look far off the middle).
    let (mx, my) = fv.grid_mid;
    let alpha = |x: f32, y: f32| {
        let d = ((x - mx) * (x - mx) + (y - my) * (y - my)).sqrt() / reach;
        let (cx, cy) = fv.cubes_at;
        let from_cubes = ((x - cx) * (x - cx) + (y - cy) * (y - cy)).sqrt();
        // Grown out (the start over), everywhere.
        let reached = if fv.grid >= 1.0 { 1.0 } else { ((grown - from_cubes) / (2.0 * step)).clamp(0.0, 1.0) };
        0.15 * (1.0 - d).max(0.0).powf(1.3) as f64 * reached as f64
    };
    let n = (reach / step).ceil() as i32 + 1;
    let shift = floor_shift(k);
    // The lines nearest that point first in each direction.
    let (ix, iy) = (((mx - shift.0) / step).round() as i32, ((my - shift.1) / step).round() as i32);
    // Else the lines whole, at full strength, one stroke; how strong each
    // place on the table is, a mask over them (a pixel of it 6 of the
    // page's, looked up on the table through the view turned back): a
    // stroke a piece by strength took 12 ms of a frame to set in pixels.
    if let Some(g) = grid_of(fv).filter(|_| !gpu) {
        const MASK: i32 = 6;
        let (mw, mh) = (_w / MASK + 2, _h / MASK + 2);
        let inv = g.inv.map(|r| r.map(|v| v as f64));
        let w_at = |x: f64, y: f64| g.w_row[0] as f64 * x + g.w_row[1] as f64 * y + g.w_row[2] as f64;
        let Ok(mut mask) = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, mw, mh) else { return draw_cubes(fv, cr) };
        let stride = mask.stride() as usize;
        let mut strongest = 0.0f64;
        if let Ok(mut data) = mask.data() {
            for py in 0..mh {
                for px in 0..mw {
                    let (sx, sy) = ((px * MASK) as f64 - fv.off.0 as f64, (py * MASK) as f64 - fv.off.1 as f64);
                    let q = [inv[0][0] * sx + inv[0][1] * sy + inv[0][2], inv[1][0] * sx + inv[1][1] * sy + inv[1][2], inv[2][0] * sx + inv[2][1] * sy + inv[2][2]];
                    if q[2].abs() < 1e-12 {
                        continue;
                    }
                    let (x, y) = (q[0] / q[2], q[1] / q[2]);
                    // Behind the eye (or at the horizon): nothing.
                    if w_at(x, y) <= 0.005 {
                        continue;
                    }
                    let a = alpha(x as f32, y as f32);
                    strongest = strongest.max(a);
                    // The ink, premultiplied, this strong.
                    let k = (a * g.ink_k as f64 * 255.0).round().clamp(0.0, 255.0) as u8;
                    let c = g.ink.map(|v| (v as f64 * k as f64).round() as u32);
                    let at = py as usize * stride + px as usize * 4;
                    data[at..at + 4].copy_from_slice(&((k as u32) << 24 | c[0] << 16 | c[1] << 8 | c[2]).to_ne_bytes());
                }
            }
        }
        if strongest >= 0.004 {
            // Each line from where it comes in reach to where it leaves (its
            // ends brought in front of the eye).
            for i in -n..=n {
                for along_x in [true, false] {
                    let t = if along_x { (i + iy) as f32 * step + shift.1 } else { (i + ix) as f32 * step + shift.0 };
                    let dt = t - if along_x { my } else { mx };
                    let half = (reach * reach - dt * dt).max(0.0).sqrt();
                    if half <= 0.0 {
                        continue;
                    }
                    let c = if along_x { mx } else { my };
                    let at = |u: f32| if along_x { (u, t) } else { (t, u) };
                    let (mut u0, mut u1) = (c - half, c + half);
                    // w is straight along the line: cut where it is too small.
                    let (w0, w1) = (w_at(at(u0).0 as f64, at(u0).1 as f64), w_at(at(u1).0 as f64, at(u1).1 as f64));
                    if w0 <= 0.06 && w1 <= 0.06 {
                        continue;
                    }
                    let cut = |w0: f64, w1: f64| ((0.06 - w0) / (w1 - w0)) as f32;
                    if w0 <= 0.06 {
                        u0 += (u1 - u0) * cut(w0, w1);
                    } else if w1 <= 0.06 {
                        u1 = u0 + (u1 - u0) * cut(w0, w1);
                    }
                    let (Some(p0), Some(p1)) = (project(at(u0).0, at(u0).1), project(at(u1).0, at(u1).1)) else { continue };
                    cr.move_to(p0.0, p0.1);
                    cr.line_to(p1.0, p1.1);
                }
            }
            cr.set_line_width(line_width());
            mask.mark_dirty();
            let pattern = gtk::cairo::SurfacePattern::create(&mask);
            pattern.set_filter(gtk::cairo::Filter::Bilinear);
            pattern.set_matrix(gtk::cairo::Matrix::new(1.0 / MASK as f64, 0.0, 0.0, 1.0 / MASK as f64, 0.0, 0.0));
            let _ = cr.set_source(&pattern);
            let _ = cr.stroke();
        }
    }
    draw_cubes(fv, cr);
    // The hole: seen through its opening - its floor dark, the walls that
    // face the viewer shaded darker downward, its rim.
    let Some(([x0, y0, x1, y1], depth)) = fv.hole else { return };
    let p3 = |x: f32, y: f32, zz: f32| {
        let v = m.transform_vec4(&graphene::Vec4::new(x, y, zz, 1.0));
        ((v.x() / v.w() + fv.off.0) as f64, (v.y() / v.w() + fv.off.1) as f64)
    };
    let quad = |cr: &gtk::cairo::Context, q: [(f64, f64); 4]| {
        cr.new_path();
        cr.move_to(q[0].0, q[0].1);
        for p in &q[1..] {
            cr.line_to(p.0, p.1);
        }
        cr.close_path();
    };
    let (zt, zb) = (z, z - depth);
    let top = [p3(x0, y0, zt), p3(x1, y0, zt), p3(x1, y1, zt), p3(x0, y1, zt)];
    let bot = [p3(x0, y0, zb), p3(x1, y0, zb), p3(x1, y1, zb), p3(x0, y1, zb)];
    cr.save().ok();
    quad(cr, top);
    cr.clip();
    // All dark first: where the walls and the floor meet, their smoothed
    // edges showed the white table through.
    quad(cr, top);
    cr.set_source_rgb(0.03, 0.03, 0.035);
    let _ = cr.fill();
    quad(cr, bot);
    cr.set_source_rgb(0.04, 0.04, 0.045);
    let _ = cr.fill();
    // A wall: white at its top edge, greyer toward the bottom.
    let wall = |cr: &gtk::cairo::Context, a: usize, b: usize, light: f64| {
        quad(cr, [top[a], top[b], bot[b], bot[a]]);
        let g = gtk::cairo::LinearGradient::new(0.0, top[a].1.min(top[b].1), 0.0, bot[a].1.max(bot[b].1));
        g.add_color_stop_rgb(0.0, light * 0.55, light * 0.55, light * 0.56);
        g.add_color_stop_rgb(0.5, 0.08, 0.08, 0.085);
        g.add_color_stop_rgb(1.0, 0.03, 0.03, 0.035);
        let _ = cr.set_source(&g);
        let _ = cr.fill();
    };
    // The walls that face the viewer only (their corners, taken round from
    // inside the hole, turn the far wall's way on the screen); the far one
    // always does. Drawn, the others left seams at their edges.
    let turn = |q: [(f64, f64); 4]| {
        let mut a = 0.0;
        for i in 0..4 {
            let (p, n) = (q[i], q[(i + 1) % 4]);
            a += p.0 * n.1 - n.0 * p.1;
        }
        a
    };
    let far = turn([top[0], top[1], bot[1], bot[0]]);
    for (a, b, light) in [(0usize, 1usize, 0.9), (1, 2, 0.86), (2, 3, 0.8), (3, 0, 0.8)] {
        if turn([top[a], top[b], bot[b], bot[a]]) * far > 0.0 {
            wall(cr, a, b, light);
        }
    }
    cr.restore().ok();
    quad(cr, top);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.3);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
}

/// The table's point (px) under a point of the page (the floor's own
/// coordinates), found by Newton's way through the floor's view.
fn table_under(fv: &FloorView, at: (f64, f64)) -> Option<(f32, f32)> {
    use gtk::graphene;
    let m = fv.matrix?;
    let shown = |p: (f32, f32)| {
        let v = m.transform_vec4(&graphene::Vec4::new(p.0, p.1, fv.table, 1.0));
        (v.x() / v.w() + fv.off.0, v.y() / v.w() + fv.off.1)
    };
    let want = (at.0 as f32, at.1 as f32);
    let mut p = (want.0 - fv.off.0 - fv.at.0, want.1 - fv.off.1 - fv.at.1);
    for _ in 0..30 {
        let f = shown(p);
        let (fx, fy) = (shown((p.0 + 1.0, p.1)), shown((p.0, p.1 + 1.0)));
        let j = [[fx.0 - f.0, fy.0 - f.0], [fx.1 - f.1, fy.1 - f.1]];
        let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
        if det.abs() < 1e-6 {
            return None;
        }
        let e = (f.0 - want.0, f.1 - want.1);
        p = (p.0 - (j[1][1] * e.0 - j[0][1] * e.1) / det, p.1 - (-j[1][0] * e.0 + j[0][0] * e.1) / det);
    }
    let f = shown(p);
    ((f.0 - want.0).abs() < 1.0 && (f.1 - want.1).abs() < 1.0).then_some(p)
}

type Quad = [(f64, f64); 4];

/// A cube as the page shows it: its shadow, its faces turned to the eye
/// (each with its light), its top; how much it is there and its height.
struct CubeShape {
    i: usize,
    shadow: Quad,
    faces: Vec<(Quad, f64)>,
    top: Quad,
    there: f32,
}

/// The start's cubes (intro.rs): five of the table's squares in a row
/// where the Duo lies, risen out of it as cubes - their edges the squares'
/// own lines, their faces the table's white - the word's letters on their
/// tops; seen from above at first, five of the squares. In the order they
/// are drawn: farthest from the eye's line first (a cube nearer it covers
/// its neighbour's side).
fn cube_shapes(fv: &FloorView) -> Vec<CubeShape> {
    use gtk::graphene;
    let Some(m) = fv.matrix else { return Vec::new() };
    let step = square() * fv.k;
    let p3 = |x: f32, y: f32, z: f32| {
        let v = m.transform_vec4(&graphene::Vec4::new(x, y, z, 1.0));
        ((v.x() / v.w() + fv.off.0) as f64, (v.y() / v.w() + fv.off.1) as f64)
    };
    // Five squares in a row about their middle (laid in the squares by
    // show_fold).
    let (cx, cy) = fv.cubes_at;
    let left = |i: usize| cx + (i as f32 - WORD_HALF) * step;
    let y0 = cy - step / 2.0;
    let z0 = fv.table;
    let mut order: Vec<usize> = (0..intro::WORD.len()).collect();
    let off_eye = |i: usize| (left(i) + step / 2.0 - fv.eye_x).abs();
    order.sort_by(|a, b| off_eye(*b).partial_cmp(&off_eye(*a)).unwrap());
    let mut shapes: Vec<CubeShape> = order
        .into_iter()
        .filter(|i| fv.cubes[*i].0 > 0.0)
        .map(|i| {
            let (there, h) = fv.cubes[i];
            let (shadow, faces, top) = box_shape(&p3, (left(i), y0), step, z0, h);
            CubeShape { i, shadow, faces, top, there }
        })
        .collect();
    // A square of the table jumping (clicked): no letter (i past the word);
    // drawn before the word's if farther than them, else after.
    if let Some(((tx, ty), h)) = fv.tapped {
        let (shadow, faces, top) = box_shape(&p3, (tx - step / 2.0, ty - step / 2.0), step, z0, h);
        let jump = CubeShape { i: usize::MAX, shadow, faces, top, there: 1.0 };
        if ty < cy {
            shapes.insert(0, jump);
        } else {
            shapes.push(jump);
        }
    }
    shapes
}

/// A box on the table (a square of `side` from its near-left corner,
/// raised `h` of its side): its shadow, its faces turned to the eye with
/// their light, its top.
fn box_shape(p3: &dyn Fn(f32, f32, f32) -> (f64, f64), (x0, y0): (f32, f32), side: f32, z0: f32, h: f32) -> (Quad, Vec<(Quad, f64)>, Quad) {
    let (x1, y1) = (x0 + side, y0 + side);
    let zt = z0 + h * side;
    // The shadow toward the viewer.
    let reach = 0.45 * h * side;
    let shadow = [p3(x0, y0, z0), p3(x1, y0, z0), p3(x1 + reach * 0.3, y1 + reach, z0), p3(x0 + reach * 0.3, y1 + reach, z0)];
    // Each face wound so that seen from outside it turns as the top does
    // seen from above: kept only so turned.
    let top = [p3(x0, y0, zt), p3(x1, y0, zt), p3(x1, y1, zt), p3(x0, y1, zt)];
    let facing = area(&top).signum();
    let faces = [
        ([p3(x0, y1, z0), p3(x0, y1, zt), p3(x1, y1, zt), p3(x1, y1, z0)], 0.93),
        ([p3(x1, y0, z0), p3(x1, y0, zt), p3(x0, y0, zt), p3(x0, y0, z0)], 0.99),
        ([p3(x1, y1, z0), p3(x1, y1, zt), p3(x1, y0, zt), p3(x1, y0, z0)], 0.965),
        ([p3(x0, y0, z0), p3(x0, y0, zt), p3(x0, y1, zt), p3(x0, y1, z0)], 0.965),
    ]
    .into_iter()
    .filter(|(q, _)| area(q).signum() == facing)
    .collect();
    (shadow, faces, top)
}

/// Twice a polygon's signed area (its turn).
fn area(q: &[(f64, f64)]) -> f64 {
    (0..q.len()).map(|i| q[i].0 * q[(i + 1) % q.len()].1 - q[(i + 1) % q.len()].0 * q[i].1).sum()
}

fn inside(q: &[(f64, f64)], p: (f64, f64)) -> bool {
    let mut odd = false;
    for i in 0..q.len() {
        let (a, b) = (q[i], q[(i + 1) % q.len()]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0) {
            odd = !odd;
        }
    }
    odd
}

/// The cube under a point of the page, the nearest one drawn over others.
fn cube_at(fv: &FloorView, p: (f64, f64)) -> Option<usize> {
    if fv.word <= 0.0 {
        return None;
    }
    cube_shapes(fv).into_iter().rev().find(|c| c.i < intro::WORD.len() && (inside(&c.top, p) || c.faces.iter().any(|(q, _)| inside(q, p)))).map(|c| c.i)
}

fn draw_cubes(fv: &FloorView, cr: &gtk::cairo::Context) {
    if fv.word <= 0.0 {
        return;
    }
    let path = |q: &[(f64, f64)]| {
        cr.new_path();
        cr.move_to(q[0].0, q[0].1);
        for p in &q[1..] {
            cr.line_to(p.0, p.1);
        }
        cr.close_path();
    };
    let shapes = cube_shapes(fv);
    let _ = cr.push_group();
    for c in &shapes {
        let h = fv.cubes.get(c.i).map_or(fv.tapped.map_or(0.0, |t| t.1), |c| c.1);
        path(&c.shadow);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.05 * (h.min(1.0) * fv.eye) as f64 * if night() { 3.0 } else { 1.0 });
        let _ = cr.fill();
        // Gone down into the table, its sides and top fade into the
        // table's square (the letter alone lying there).
        let up = (h / 0.1).clamp(0.0, 1.0) as f64;
        if up > 0.0 {
            for (q, light) in &c.faces {
                path(q);
                let (r, g, b) = face_rgb(*light);
                cr.set_source_rgba(r, g, b, up);
                let _ = cr.fill_preserve();
                ink(cr, 0.14 * up);
                cr.set_line_width(1.6);
                let _ = cr.stroke();
            }
            path(&c.top);
            let (r, g, b) = face_rgb(1.0);
            cr.set_source_rgba(r, g, b * 1.005, up);
            let _ = cr.fill_preserve();
            ink(cr, 0.14 * up);
            cr.set_line_width(1.6);
            let _ = cr.stroke();
        }
        let Some(letter) = intro::WORD.get(c.i) else { continue };
        // The letter on the top: the face's own frame (its corners), the
        // letter laid in it.
        let ch = letter.chars().next().unwrap_or(' ');
        draw_night_at(fv, (fv.cubes_at.0 + (c.i as f32 - WORD_HALF + 0.5) * square() * fv.k, fv.cubes_at.1));
        draw_glyph(cr, ch, c.top[0], c.top[1], c.top[3], (0.16, 0.16, 0.18, 0.88 * (c.there as f64 / 0.25).min(1.0)));

    }
    let _ = cr.pop_group_to_source();
    let _ = cr.paint_with_alpha((fv.word * fv.part_alpha[2]).min(1.0) as f64);
    draw_texts(fv, cr);
    draw_flips(fv, cr);
    draw_night_off();
    draw_buttons(fv, cr);
}

/// Squares turning over (the credit's, the boards'): each whole - its near
/// edge kept on the table, its far edge up and over, down where the near
/// one was - the old letter on its face, the new on its back; still, the
/// letter on the table.
fn draw_flips(fv: &FloorView, cr: &gtk::cairo::Context) {
    use gtk::graphene;
    let Some(m) = fv.matrix else { return };
    let side = square() * fv.k;
    let z0 = fv.table;
    let p3 = |x: f32, y: f32, z: f32| {
        let v = m.transform_vec4(&graphene::Vec4::new(x, y, z, 1.0));
        ((v.x() / v.w() + fv.off.0) as f64, (v.y() / v.w() + fv.off.1) as f64)
    };
    // The letter in a square whose corners (its top left, top right,
    // bottom left) are these on the page.
    let letter = |a: (f64, f64), b: (f64, f64), d: (f64, f64), ch: char, rgba: (f64, f64, f64, f64)| draw_glyph(cr, ch, a, b, d, rgba);
    // The far rows first; in a row, farthest from the eye's line first.
    let mut tiles = fv.tiles.clone();
    let off_eye = |t: &board::Tile| (t.at.0 + side / 2.0 - fv.eye_x).abs();
    tiles.sort_by(|a, b| a.at.1.partial_cmp(&b.at.1).unwrap().then(off_eye(b).partial_cmp(&off_eye(a)).unwrap()));
    // The raised ones last (over their neighbours).
    tiles.sort_by(|a, b| (a.lift > 0.0).cmp(&(b.lift > 0.0)));
    let path = |q: &[(f64, f64)]| {
        cr.new_path();
        cr.move_to(q[0].0, q[0].1);
        for p in &q[1..] {
            cr.line_to(p.0, p.1);
        }
        cr.close_path();
    };
    for t in &tiles {
        draw_night_at(fv, (t.at.0 + side / 2.0, t.at.1 + side / 2.0));
        let (x0, y0) = t.at;
        let f = t.flap;
        // Raised (a line that can be clicked, under the pointer): a cube
        // with its letter on top, as the buttons are.
        if t.lift > 0.001 && f.turn <= 0.0 {
            let (shadow, faces, top) = box_shape(&p3, (x0, y0), side, z0, t.lift);
            path(&shadow);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.05 * t.rgba.3 * if night() { 3.0 } else { 1.0 });
            let _ = cr.fill();
            for (q, light) in &faces {
                path(q);
                let (r, g, b) = face_rgb(*light);
                cr.set_source_rgba(r, g, b, t.rgba.3);
                let _ = cr.fill_preserve();
                ink(cr, 0.14 * t.rgba.3);
                cr.set_line_width(1.6);
                let _ = cr.stroke();
            }
            path(&top);
            let (r, g, b) = face_rgb(1.0);
            cr.set_source_rgba(r, g, b * 1.005, t.rgba.3);
            let _ = cr.fill_preserve();
            ink(cr, 0.14 * t.rgba.3);
            cr.set_line_width(1.6);
            let _ = cr.stroke();
            letter(top[0], top[1], top[3], f.from, t.rgba);
            continue;
        }
        // The square on the table: a point `u` across, `v` along from its
        // far edge (0..1).
        let at = |u: f32, v: f32| p3(x0 + u * side, y0 + v * side, z0);
        let whole = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0));
        if f.turn <= 0.0 {
            letter(whole.0, whole.1, whole.2, f.from, t.rgba);
        } else {
            // The whole square turning over in its place on the table: its
            // far and near edges closing on its middle (as if turned on
            // edge) and opening again, the old letter on its face, the new
            // on its back - nothing lifted toward the eye (lifted it showed
            // bigger: the letters jumped); no line across it.
            let c = (f.turn * std::f32::consts::PI).cos();
            let ym = y0 + side / 2.0;
            let half = side / 2.0 * c.abs();
            let row = |y: f32, u: f32| p3(x0 + u * side, y, z0);
            let (top, bottom) = (ym - half, ym + half);
            let ch = if c > 0.0 { f.from } else { f.to };
            if half > 0.5 {
                letter(row(top, 0.0), row(top, 1.0), row(bottom, 0.0), ch, t.rgba);
            }
        }
    }
}

/// Words set in the table's squares (TableText): the finer squares under
/// them, then each letter laid in its square on the table.
fn draw_texts(fv: &FloorView, cr: &gtk::cairo::Context) {
    use gtk::graphene;
    let Some(m) = fv.matrix else { return };
    let z = fv.table;
    let p3 = |x: f32, y: f32| {
        let v = m.transform_vec4(&graphene::Vec4::new(x, y, z, 1.0));
        ((v.x() / v.w() + fv.off.0) as f64, (v.y() / v.w() + fv.off.1) as f64)
    };
    let big = square() * fv.k;
    for t in &fv.texts {
        if t.strength <= 0.0 {
            continue;
        }
        draw_night_at(fv, (t.at.0 + t.cell, t.at.1 + t.cell));
        let cols = t.lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        let rows = t.lines.len();
        let (x1, y1) = (t.at.0 + cols as f32 * t.cell, t.at.1 + rows as f32 * t.cell);
        // The finer lines where the words are (not on the table's own).
        if t.cell < big - 0.5 {
            cr.set_line_width(1.0);
            ink(cr, 0.07 * t.strength as f64);
            let fine = |v: f32, from: f32| ((v - from) / big).fract().abs() > 0.01 && ((v - from) / big).fract().abs() < 0.99;
            for c in 1..cols {
                let x = t.at.0 + c as f32 * t.cell;
                if fine(x, t.at.0) {
                    let (a, b) = (p3(x, t.at.1), p3(x, y1));
                    cr.move_to(a.0, a.1);
                    cr.line_to(b.0, b.1);
                }
            }
            for r in 1..rows {
                let y = t.at.1 + r as f32 * t.cell;
                if fine(y, t.at.1) {
                    let (a, b) = (p3(t.at.0, y), p3(x1, y));
                    cr.move_to(a.0, a.1);
                    cr.line_to(b.0, b.1);
                }
            }
            let _ = cr.stroke();
        }
        let total: usize = t.lines.iter().map(|l| l.chars().count()).sum();
        let shown = (t.set * total as f32).ceil() as usize;
        let mut n = 0;
        // As the word's letters on its cubes.
        for (r, line) in t.lines.iter().enumerate() {
            for (c, ch) in line.chars().enumerate() {
                n += 1;
                if n > shown {
                    break;
                }
                if ch == ' ' {
                    continue;
                }
                let (x, y) = (t.at.0 + c as f32 * t.cell, t.at.1 + r as f32 * t.cell);
                draw_glyph(cr, ch, p3(x, y), p3(x + t.cell, y), p3(x, y + t.cell), (t.grey, t.grey, t.grey * 1.02, t.strength as f64));
            }
        }
    }
}

/// A letter's outline in a square of GLYPH units, as the word's are laid
/// on its cubes (light; its ink centred across, the line's up and down so
/// that letters keep one baseline): made once a letter and kept - set with
/// pango each frame, the letters took most of a frame's time (up to 30 ms,
/// and 0.2 s the first time a capital came).
const GLYPH: f64 = 100.0;

thread_local! {
    static GLYPHS: RefCell<std::collections::HashMap<char, Option<gtk::cairo::Path>>> = RefCell::default();
}

fn glyph_made(ch: char) -> Option<gtk::cairo::Path> {
    let surface = gtk::cairo::ImageSurface::create(gtk::cairo::Format::A8, 1, 1).ok()?;
    let cr = gtk::cairo::Context::new(&surface).ok()?;
    let layout = pangocairo::functions::create_layout(&cr);
    let mut font = gtk::pango::FontDescription::from_string(&format!("{}, Lato, Ubuntu Sans, Ubuntu, sans-serif", FONTS[style().font.min(FONTS.len() - 1)].1));
    font.set_weight(gtk::pango::Weight::Light);
    font.set_absolute_size(0.7 * GLYPH * gtk::pango::SCALE as f64);
    layout.set_font_description(Some(&font));
    layout.set_text(&ch.to_string());
    let (ink, logical) = layout.pixel_extents();
    cr.move_to((GLYPH - ink.width() as f64) / 2.0 - ink.x() as f64, (GLYPH - logical.height() as f64) / 2.0 - logical.y() as f64 - 0.03 * GLYPH);
    pangocairo::functions::layout_path(&cr, &layout);
    cr.copy_path().ok()
}

/// The letters of `text` made ahead (at the start, not as they first come).
fn glyphs_ahead(text: &str) {
    GLYPHS.with(|g| {
        let mut g = g.borrow_mut();
        for ch in text.chars().filter(|c| *c != ' ') {
            g.entry(ch).or_insert_with(|| glyph_made(ch));
        }
    });
}

/// The letter `ch` laid in the square whose top left, top right and bottom
/// left corners are `a`, `b`, `d` on the page.
fn draw_glyph(cr: &gtk::cairo::Context, ch: char, a: (f64, f64), b: (f64, f64), d: (f64, f64), rgba: (f64, f64, f64, f64)) {
    if ch == ' ' || rgba.3 <= 0.0 {
        return;
    }
    GLYPHS.with(|g| {
        let mut g = g.borrow_mut();
        let Some(path) = g.entry(ch).or_insert_with(|| glyph_made(ch)) else { return };
        cr.save().ok();
        cr.transform(gtk::cairo::Matrix::new((b.0 - a.0) / GLYPH, (b.1 - a.1) / GLYPH, (d.0 - a.0) / GLYPH, (d.1 - a.1) / GLYPH, a.0, a.1));
        cr.new_path();
        cr.append_path(path);
        let c = letter_rgba(rgba);
        cr.set_source_rgba(c.0, c.1, c.2, c.3);
        let _ = cr.fill();
        cr.restore().ok();
    });
}

/// The table's buttons (three squares under the word): the sections'
/// menu, day or night, the start again.
/// The saver on: boards closed, the window filling the second monitor,
/// the pointer hidden over it.
fn saver_on(ui: &Ui) {
    if ui.saver.get().is_some() {
        return;
    }
    trace(format_args!("saver: on"));
    if let Some(b) = ui.board.borrow_mut().as_mut() {
        b.close();
    }
    if let Some((_, b)) = ui.page.borrow_mut().as_mut() {
        b.close();
    }
    ui.saver.set(Some(std::time::Instant::now()));
    ui.saver_pointer.set(place::pointer(&ui.window));
    let display = WidgetExt::display(&ui.window);
    match saver::second_monitor(&display) {
        Some(m) => ui.window.fullscreen_on_monitor(&m),
        None => ui.window.fullscreen(),
    }
    ui.window.present();
    if let Some(page) = ui.floor.parent() {
        page.set_cursor_from_name(Some("none"));
    }
}

/// The window as the second monitor's wallpaper (under every window, the
/// whole monitor, worked with as ever), or back where it was.
///
/// On the way the window is the monitor's, seen through, and what it shows
/// a card growing to it (or shrinking back) - moved and sized in the
/// window, not the window each frame (each a round with the window manager
/// and all laid out again: 20-40 ms a frame). The window made the
/// monitor's (or its own size again) is not one step on X11: moved at
/// once, sized and framed when GTK has drawn it so, a frame or two showing
/// it half done (it blinked). So meanwhile a cover over it: a picture of
/// the card exactly where it is, the window unseen under it till it is
/// done.
fn wallpaper(ui: &Ui) {
    let display = WidgetExt::display(&ui.window);
    let Some(m) = saver::second_monitor(&display) else { return };
    let g = m.geometry();
    let monitor = (g.x(), g.y(), g.width(), g.height());
    // Midway: turned back from where the card is (not while the window is
    // being made over).
    if let Some(w) = ui.wall_move.get() {
        if !matches!(w.phase, WallPhase::Move(_)) {
            return;
        }
        let Some(back) = WALL_BACK.with(|b| b.get()) else { return };
        trace(format_args!("wallpaper: turned back midway"));
        let (to, into) = if w.into { (back.was, false) } else { (monitor, true) };
        if into {
            ui.wallpaper.set(Some((back.was, back.size)));
        } else {
            ui.wallpaper.set(None);
            place::desktop(&ui.window, false, None);
        }
        ui.wall_move.set(Some(WallMove { phase: WallPhase::Move(glib::monotonic_time()), from: w.now, to, now: w.now, into, monitor }));
        return;
    }
    if let Some((was, _)) = ui.wallpaper.take() {
        trace(format_args!("wallpaper: off, back to {was:?}"));
        // A window again first (still the monitor's), the card drawn back
        // to where it was, then the window made its own again (wall_step).
        place::desktop(&ui.window, false, None);
        ui.window.add_css_class("wall-move");
        ui.wall_move.set(Some(WallMove { phase: WallPhase::Move(glib::monotonic_time()), from: monitor, to: was, now: monitor, into: false, monitor }));
        return;
    }
    // Where its content is (the window's shadow aside): where it comes
    // back to; its shadow's widths round it.
    let (Some((cx, cy)), Some(f)) = (place::content_origin(&ui.window), place::frame(&ui.window)) else { return };
    let size = (ui.window.width(), ui.window.height());
    let was = (cx.round() as i32, cy.round() as i32, size.0, size.1);
    let (left, top) = (was.0 - f.0, was.1 - f.1);
    let shadow = (left, top, f.2 - size.0 - left, f.3 - size.1 - top);
    if let Some(b) = ui.board.borrow_mut().as_mut() {
        b.close();
    }
    if let Some((_, b)) = ui.page.borrow_mut().as_mut() {
        b.close();
    }
    trace(format_args!("wallpaper: on {}x{}+{}+{} (from {was:?}, shadow {shadow:?})", g.width(), g.height(), g.x(), g.y()));
    WALL_BACK.with(|b| b.set(Some(WallBack { was, size, shadow })));
    ui.wallpaper.set(Some((was, size)));
    place::keep_composited(&ui.window);
    wall_cover(ui, was);
    ui.wall_move.set(Some(WallMove { phase: WallPhase::Cover(std::time::Instant::now()), from: was, to: monitor, now: was, into: true, monitor }));
}

/// Where the window was and comes back to: its content, GTK's own size of
/// it, its shadow's widths (left, top, right, bottom).
#[derive(Clone, Copy, Debug)]
struct WallBack {
    was: (i32, i32, i32, i32),
    size: (i32, i32),
    shadow: (i32, i32, i32, i32),
}

/// Where the way into the wallpaper (or back) is: the cover being shown;
/// the window made unseen (that drawn and shown before it is moved: else
/// its last picture showed where it was moved to); the window being made
/// over under it (how many frames in a row it has been right); the window shown again, the cover
/// still up a moment; the card moving (since when).
#[derive(Clone, Copy, Debug, PartialEq)]
enum WallPhase {
    Cover(std::time::Instant),
    Hidden(std::time::Instant),
    Over(std::time::Instant, u32),
    Shown(std::time::Instant),
    /// Since when, in the frame clock's microseconds (the card where it
    /// is when the frame is shown, not when it is worked out).
    Move(i64),
}

/// The window's way into the wallpaper or back: the card's place on the
/// screen from and to, where it is now.
#[derive(Clone, Copy, Debug)]
struct WallMove {
    phase: WallPhase,
    from: (i32, i32, i32, i32),
    to: (i32, i32, i32, i32),
    now: (i32, i32, i32, i32),
    into: bool,
    monitor: (i32, i32, i32, i32),
}

/// Seconds the card takes to the monitor, or back.
const WALL_S: f32 = 0.8;

thread_local! {
    static WALL_BACK: std::cell::Cell<Option<WallBack>> = const { std::cell::Cell::new(None) };
    /// The cover over the window while it is made over.
    static WALL_COVER: std::cell::RefCell<Option<gtk::Window>> = const { std::cell::RefCell::new(None) };
}

/// A picture of the card as it is, in a window of its own exactly over
/// it at `r` (above everything, no window manager's say).
fn wall_cover(ui: &Ui, r: (i32, i32, i32, i32)) {
    let picture = gtk::Picture::for_paintable(&gtk::WidgetPaintable::new(Some(&ui.shown)).current_image());
    picture.set_can_shrink(true);
    picture.set_content_fit(gtk::ContentFit::Fill);
    let cover = gtk::Window::builder().decorated(false).default_width(r.2).default_height(r.3).build();
    cover.add_css_class("wall-cover");
    if night() {
        cover.add_css_class("night");
    }
    cover.set_child(Some(&picture));
    WidgetExt::realize(&cover);
    place::keep_composited(&cover);
    if !place::own_over(&cover, r) {
        return;
    }
    cover.present();
    WALL_COVER.with(|c| c.replace(Some(cover)));
}

fn wall_uncover() {
    if let Some(c) = WALL_COVER.with(|c| c.take()) {
        c.destroy();
    }
}

/// The card at `r` on the screen: its margins in the window, the window
/// taken where it is meant to be (the monitor at its size, else where it
/// was) - not where X has it (moved before GTK has its new size).
fn wall_card(ui: &Ui, w: &WallMove, r: (i32, i32, i32, i32)) {
    ui.stage.set_measure_overlay(&ui.shown, false);
    let (ww, wh) = (ui.window.width(), ui.window.height());
    let back = WALL_BACK.with(|b| b.get());
    let (cx, cy) = if (ww, wh) == (w.monitor.2, w.monitor.3) {
        (w.monitor.0, w.monitor.1)
    } else if let Some(b) = back.filter(|b| (ww, wh) == (b.was.2, b.was.3)) {
        (b.was.0, b.was.1)
    } else {
        // Neither (no window manager sizing it so): where X has it.
        match place::content_origin(&ui.window) {
            Some((x, y)) => (x.round() as i32, y.round() as i32),
            None => return,
        }
    };
    let card = &ui.shown;
    card.set_margin_start((r.0 - cx).max(0));
    card.set_margin_top((r.1 - cy).max(0));
    card.set_margin_end((cx + ww - (r.0 + r.2)).max(0));
    card.set_margin_bottom((cy + wh - (r.1 + r.3)).max(0));
    static FRAMES: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *FRAMES.get_or_init(|| std::env::var_os("ITEMGRID_FRAMES").is_some()) {
        trace(format_args!(
            "wall: {:?} card {r:?} window {ww}x{wh} at {:?} margins {} {} {} {}",
            w.phase,
            place::frame(&ui.window),
            card.margin_start(),
            card.margin_top(),
            card.margin_end(),
            card.margin_bottom()
        ));
    }
}

fn wall_card_off(ui: &Ui) {
    ui.stage.set_measure_overlay(&ui.shown, true);
    let card = &ui.shown;
    card.set_margin_start(0);
    card.set_margin_top(0);
    card.set_margin_end(0);
    card.set_margin_bottom(0);
}

/// A frame of the window's way into the wallpaper or back (true while on
/// it).
fn wall_step(ui: &Ui, frame_us: i64) -> bool {
    let Some(mut w) = ui.wall_move.get() else { return false };
    let Some(back) = WALL_BACK.with(|b| b.get()) else {
        ui.wall_move.set(None);
        return false;
    };
    // The eye stays as it is while the window is made over (before the card
    // moves, and after it is back).
    if !matches!(w.phase, WallPhase::Move(_)) {
        ui.wallpaper_mix.set(0.0);
        ui.wall_t.set(None);
    }
    let now = std::time::Instant::now();
    let after = |since: std::time::Instant, ms: u64| now.duration_since(since) >= std::time::Duration::from_millis(ms);
    match w.phase {
        // The cover up (drawn a couple of frames): the window unseen.
        WallPhase::Cover(since) => {
            if !after(since, 50) {
                ui.wall_move.set(Some(w));
                return true;
            }
            // Unseen by the compositor itself: GTK's own (0) drew nothing,
            // its last picture left to be shown where the window was moved.
            place::seen(&ui.window, false);
            w.phase = WallPhase::Hidden(now);
        }
        // Unseen on the screen (a few frames): made over.
        WallPhase::Hidden(since) => {
            if !after(since, 50) {
                ui.wall_move.set(Some(w));
                return true;
            }
            if w.into {
                ui.window.add_css_class("wall-move");
                ui.window.add_css_class("wallpaper");
                ui.window.set_decorated(false);
                ui.window.set_default_size(w.monitor.2, w.monitor.3);
                place::move_resize(&ui.window, w.monitor);
            } else {
                ui.window.remove_css_class("wall-move");
                ui.window.remove_css_class("wallpaper");
                wall_card_off(ui);
                ui.window.set_default_size(back.size.0, back.size.1);
                ui.window.set_decorated(true);
                let s = back.shadow;
                // Mutter takes the place asked for a framed window as its
                // content's (the shadow round it its own to add).
                place::move_resize(&ui.window, (back.was.0, back.was.1, back.was.2 + s.0 + s.2, back.was.3 + s.1 + s.3));
            }
            w.phase = WallPhase::Over(now, 0);
        }
        // Made over once it is where and as big as meant, a few frames in a
        // row (or after a while, nudged there): shown again under the
        // cover.
        WallPhase::Over(since, right) => {
            let target = if w.into { w.monitor } else { back.was };
            let size_ok = (ui.window.width(), ui.window.height()) == (target.2, target.3);
            let at = place::content_origin(&ui.window).map(|(x, y)| (x.round() as i32, y.round() as i32));
            let place_ok = at == Some((target.0, target.1));
            if !w.into && std::env::var_os("ITEMGRID_FRAMES").is_some() {
                trace(format_args!("wall: back {:?} at {at:?} size {}x{} frame {:?} want {target:?}", w.phase, ui.window.width(), ui.window.height(), place::frame(&ui.window)));
            }
            if w.into {
                wall_card(ui, &w, w.from);
            }
            if size_ok && place_ok && right < 4 {
                w.phase = WallPhase::Over(since, right + 1);
                ui.wall_move.set(Some(w));
                return true;
            }
            if !(size_ok && place_ok) {
                w.phase = WallPhase::Over(since, 0);
                if size_ok && after(since, 40) {
                    // Framed but a little off (its shadow's widths not as
                    // they were): put right.
                    if let (Some(a), Some(f)) = (at, place::frame(&ui.window)) {
                        place::move_to(&ui.window, f.0 + target.0 - a.0, f.1 + target.1 - a.1);
                    }
                }
                if !after(since, 600) {
                    ui.wall_move.set(Some(w));
                    return true;
                }
            }
            place::seen(&ui.window, true);
            w.phase = WallPhase::Shown(now);
        }
        // Drawn again (a few frames): the cover gone, the card on its way
        // (or there, back).
        WallPhase::Shown(since) => {
            if w.into {
                wall_card(ui, &w, w.from);
            }
            if !after(since, 100) {
                ui.wall_move.set(Some(w));
                return true;
            }
            wall_uncover();
            if !w.into {
                ui.wall_move.set(None);
                WALL_BACK.with(|b| b.set(None));
                return false;
            }
            w.phase = WallPhase::Move(frame_us);
        }
        WallPhase::Move(since) => {
            let t = ((frame_us - since) as f32 / 1e6 / WALL_S).clamp(0.0, 1.0);
            // The eye going over by the same clock.
            ui.wallpaper_mix.set(if w.into { t } else { 1.0 - t });
            ui.wall_t.set((t < 1.0).then_some(t));
            let e = t * t * t * (t * (6.0 * t - 15.0) + 10.0);
            let at = |a: i32, b: i32| (a as f32 + (b - a) as f32 * e).round() as i32;
            w.now = (at(w.from.0, w.to.0), at(w.from.1, w.to.1), at(w.from.2, w.to.2), at(w.from.3, w.to.3));
            wall_card(ui, &w, w.now);
            if t < 1.0 {
                ui.wall_move.set(Some(w));
                return true;
            }
            if w.into {
                // There: the desktop's.
                ui.wall_move.set(None);
                ui.window.remove_css_class("wall-move");
                wall_card_off(ui);
                if !place::desktop(&ui.window, true, Some(w.monitor)) {
                    ui.window.remove_css_class("wallpaper");
                    ui.window.set_decorated(true);
                    ui.wallpaper.set(None);
                }
                return false;
            }
            // Back where it was: the cover up, the window made its own
            // again under it.
            wall_cover(ui, back.was);
            w.phase = WallPhase::Cover(now);
        }
    }
    ui.wall_move.set(Some(w));
    true
}

/// What the idle time (ms) at the computer means: the saver on or off;
/// resting (the phone let go) or back from it (looked for at once).
fn idle_now(ui: &Rc<Ui>, idle: u64) {
    let rest = idle >= saver::rest_after_ms();
    if rest != ui.resting.get() {
        ui.resting.set(rest);
        trace(format_args!("{}", if rest { "resting: nothing done at the computer, the phone let go" } else { "back: looking for the phone" }));
        follow_hinge(ui);
        if !rest {
            look(ui);
        }
    }
    match ui.saver.get() {
        None if idle >= saver::AFTER_MS => saver_on(&ui),
        // Something done since it came (idle less than it has been
        // on), after its first second: the pointer moved well away
        // (a hand resting on the mouse stirs it a little), or not at
        // all (a key).
        Some(since) if since.elapsed().as_millis() > 1000 && idle + 700 < since.elapsed().as_millis() as u64 => {
            let now = place::pointer(&ui.window);
            let moved = match (ui.saver_pointer.get(), now) {
                (Some(a), Some(b)) => ((a.0 - b.0).pow(2) + (a.1 - b.1).pow(2)) as f64,
                _ => f64::MAX,
            };
            if moved == 0.0 || moved > 40.0 * 40.0 {
                saver_off(&ui);
            }
        }
        _ => {}
    }
}

/// The menu's lines: without the phone only item/grid's own (its
/// settings, about it); with it, the phone's sections, then those.
/// A section's facts as lines for the table, a letter a square: "name
/// value" when short enough; else the name, then the value under it,
/// wrapped at its words (the table's column stays narrow: the eye draws
/// back little).
fn table_lines(rows: &[(&str, String)]) -> Vec<board::Line> {
    const WIDE: usize = 19;
    let name_w = rows.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0);
    let mut lines = Vec::new();
    for (k, v) in rows {
        let k = k.to_lowercase();
        let one = if k.is_empty() { v.clone() } else { format!("{k:name_w$}  {v}") };
        if one.chars().count() <= WIDE {
            lines.push(board::Line::new(k.clone(), one));
            continue;
        }
        if !k.is_empty() {
            lines.push(board::Line::new(k.clone(), k.clone()));
        }
        let mut cur = String::new();
        for word in v.split_whitespace() {
            for piece in word.chars().collect::<Vec<_>>().chunks(WIDE) {
                let piece: String = piece.iter().collect();
                if !cur.is_empty() && cur.chars().count() + 1 + piece.chars().count() > WIDE {
                    lines.push(board::Line::new(k.clone(), std::mem::take(&mut cur)));
                }
                if !cur.is_empty() {
                    cur.push(' ');
                }
                cur.push_str(&piece);
            }
        }
        if !cur.is_empty() {
            lines.push(board::Line::new(k.clone(), cur));
        }
    }
    lines
}

/// The updates on the table: item's version on the phone - green and
/// "(fresh)" if item's tree here has nothing newer, "(3 newer)" if it has
/// (nothing said if it is not here) - and its details (when built, its
/// commit) under "› details", opened and closed by a click.
fn updates_lines(ui: &Ui) -> Vec<board::Line> {
    let rows = ui.section_words.borrow().get("updates").cloned().unwrap_or_default();
    let get = |k: &str| rows.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone());
    let version = get("item").unwrap_or_else(|| "?".to_owned());
    let newer = get("commit").as_deref().and_then(itemgrid_core::update::newer_than);
    let tag = match newer {
        Some(0) => " (fresh)".to_owned(),
        Some(n) => format!(" ({n} newer)"),
        None => String::new(),
    };
    let mut first = board::Line::new("", format!("item  {version}{tag}"));
    if newer == Some(0) {
        first.accent = Some((6, 6 + version.chars().count(), (0.10, 0.60, 0.34)));
    }
    let open = ui.details_open.get();
    let mut lines = vec![first];
    // Newer in item's tree here: built from it and put on the phone.
    if newer.is_some_and(|n| n > 0) {
        lines.push(board::Line::new("do:update", "› update"));
    }
    lines.push(board::Line::new("more:updates", if open { "⌄ details" } else { "› details" }));
    if open {
        let details: Vec<(&str, String)> = ["built", "commit"].into_iter().filter_map(|k| get(k).map(|v| (k, v))).collect();
        lines.extend(table_lines(&details));
    }
    lines
}

/// The phone's serial when it runs its stock Android (adb on) or sits in
/// its bootloader - not the Android item/grid started as a guest.
fn stock_android(ui: &Ui) -> Option<String> {
    let st = ui.state.borrow();
    match &st.place {
        Place::Android(s) | Place::Fastboot(s) if itemgrid_core::android::guest(s).is_none() => Some(s.clone()),
        _ => None,
    }
}

/// The phone's words on the table when it is not in Linux (eleven
/// letters at most: the open phone lies past them): where it is,
/// and - Android without USB debugging - how to let item/grid reach it.
fn away_words(ui: &Ui) -> Option<Vec<String>> {
    let st = ui.state.borrow();
    let guest = st.place.serial().is_some_and(|s| itemgrid_core::android::guest(s).is_some());
    Some(match &st.place {
        Place::Android(_) if guest => vec!["android".into(), "as a guest".into()],
        Place::Android(_) => vec!["android".into(), "on cable".into()],
        Place::Quiet(_) => vec!["android".into(), "turn on".into(), "usb debugging".into()],
        Place::Fastboot(_) => vec!["bootloader".into()],
        Place::Recovery(_) => vec!["recovery".into()],
        Place::NoSystem => vec!["no system".into(), "reinstall".into()],
        _ => return None,
    })
}

/// The way back to the phone's stock Android ("stock" on the menu) - on
/// the cable only (off it, said so, not to be clicked).
fn repair_lines(ui: &Ui) -> Vec<board::Line> {
    let cable = ui.state.borrow().host.as_deref().is_some_and(|h| itemgrid_core::link::Via::of(h) == itemgrid_core::link::Via::Cable);
    if cable {
        vec![board::Line::new("do:android", "› android")]
    } else {
        vec![board::Line::new("", "android"), board::Line::new("", "plug in the cable")]
    }
}

/// A question on the table before a job: what it is, what it does (short
/// lines), its choices, the phone's number to type for what cannot be
/// undone, and what goes on (with the choice) once asked.
struct Ask {
    title: String,
    info: Vec<String>,
    choices: Vec<(&'static str, String)>,
    chosen: usize,
    word: Option<String>,
    typed: String,
    /// Read yet (a plan read off the phone first: "checking").
    ready: bool,
    /// The go line's word ("erase", "install", "update").
    verb: &'static str,
    go: Option<Rc<dyn Fn(&Rc<Ui>, &'static str)>>,
}

impl Ask {
    fn new(title: &str, info: Vec<String>) -> Ask {
        Ask { title: title.into(), info, choices: Vec::new(), chosen: 0, word: None, typed: String::new(), ready: true, verb: "go", go: None }
    }

    fn can_go(&self) -> bool {
        self.ready && self.go.is_some() && self.word.as_ref().is_none_or(|w| *w == self.typed)
    }

    /// The question's board: its title dark; a blank; what it does, grey;
    /// a blank; its choices (the one taken dark, ● ○); the phone's number
    /// to type (its digits faint, darkening as they are typed - a wrong one
    /// red); then go (only when typed) and cancel.
    fn lines(&self) -> Vec<board::Line> {
        const DARK: (f64, f64, f64) = (0.10, 0.10, 0.11);
        const WRONG: (f64, f64, f64) = (0.72, 0.16, 0.14);
        let dark = |key: String, text: String| {
            let mut l = board::Line::new(key, text.clone());
            l.accent = Some((0, text.chars().count(), DARK));
            l
        };
        let mut lines = vec![dark(String::new(), self.title.clone()), board::Line::new("", "")];
        lines.extend(self.info.iter().map(|i| board::Line::new("", i.clone())));
        if !self.choices.is_empty() {
            lines.push(board::Line::new("", ""));
            for (i, (_, label)) in self.choices.iter().enumerate() {
                let key = format!("ask:choice:{i}");
                if i == self.chosen {
                    lines.push(dark(key, format!("● {label}")));
                } else {
                    lines.push(board::Line::new(key, format!("○ {label}")));
                }
            }
        }
        if let (true, Some(w)) = (self.ready && self.go.is_some(), &self.word) {
            lines.push(board::Line::new("", ""));
            lines.push(dark(String::new(), format!("type {w}")));
            lines.push(board::Line::new("", "on the keyboard:"));
            // Where it goes: a blank a digit, the typed ones in them (right
            // so far dark, wrong red).
            let n = w.chars().count();
            let typed: Vec<char> = self.typed.chars().collect();
            let text: String = (0..n).map(|i| typed.get(i).copied().unwrap_or('_')).collect();
            let mut num = board::Line::new("", text);
            if !typed.is_empty() {
                let right = w.starts_with(&self.typed);
                num.accent = Some((0, typed.len(), if right { DARK } else { WRONG }));
            }
            lines.push(num);
        }
        lines.push(board::Line::new("", ""));
        if self.can_go() {
            let text = format!("› {}", self.verb);
            let mut go = board::Line::new("ask:go", text.clone());
            go.accent = Some((2, text.chars().count(), WRONG));
            lines.push(go);
        }
        lines.push(board::Line::new("ask:cancel", "› cancel"));
        lines
    }
}

/// Words wrapped to the table's narrow column (19 squares).
fn wrapped(text: &str) -> Vec<String> {
    table_lines(&[("", text.to_lowercase())]).into_iter().map(|l| l.text).collect()
}

/// The question on its board beside the menu (laid anew; the lines that
/// changed turning up again).
fn show_ask(ui: &Rc<Ui>) {
    let Some(lines) = ui.asking.borrow().as_ref().map(Ask::lines) else { return };
    let mut page = ui.page.borrow_mut();
    match page.as_mut() {
        Some((key, b)) if key == "ask" && !b.closing() => {
            let now = std::time::Instant::now();
            b.lines.truncate(lines.len());
            for (i, mut l) in lines.into_iter().enumerate() {
                if i < b.lines.len() {
                    if b.lines[i].text != l.text || b.lines[i].key != l.key {
                        let text = l.text.clone();
                        b.lines[i].key = l.key;
                        b.lines[i].accent = l.accent;
                        b.set_line(i, text);
                    }
                } else {
                    l.since = Some(now);
                    b.lines.push(l);
                }
            }
        }
        _ => *page = Some(("ask".to_owned(), board::Board::open(lines))),
    }
    drop(page);
    show_fold(ui, ui.fold.get().0);
}

fn open_ask(ui: &Rc<Ui>, ask: Ask) {
    *ui.asking.borrow_mut() = Some(ask);
    *ui.page.borrow_mut() = None;
    show_ask(ui);
}

fn close_ask(ui: &Rc<Ui>) {
    ui.asking.borrow_mut().take();
    // Asked from the phone's line: back to the phone, the menu away too.
    if ui.menu_for_ask.replace(false) {
        if let Some(b) = ui.board.borrow_mut().as_mut() {
            b.close();
        }
    }
    if let Some((key, p)) = ui.page.borrow_mut().as_mut() {
        if key == "ask" {
            p.close();
        }
    }
    show_fold(ui, ui.fold.get().0);
}

/// A line of a board clicked that asks for a job ("do:"), or answers its
/// question ("ask:").
fn board_action(ui: &Rc<Ui>, key: &str) {
    match key {
        "do:update" => {
            let mut ask = Ask::new("update item", vec!["keeps your files".into(), "restarts the phone".into(), "about 3 min".into()]);
            ask.verb = "update";
            ask.go = Some(Rc::new(|ui, _| run_job(ui, Job::Update)));
            open_ask(ui, ask);
        }
        "do:reinstall" => {
            let word = itemgrid_core::android::confirm_word(&ui.serial.borrow());
            let ask = match itemgrid_core::install::releases().pop() {
                None => Ask::new("reinstall item", wrapped("no release image on this computer yet")),
                Some(release) => {
                    let version = release.item.split(['~', '-', '+']).next().unwrap_or(&release.item).to_owned();
                    let mut ask = Ask::new("reinstall item", vec!["erases the phone".into(), format!("new item {version}"), "keep the cable in".into()]);
                    ask.choices = vec![("erase", "erase all · 10 min".into()), ("keep", "keep files · 15 min".into())];
                    ask.word = Some(word);
                    ask.go = Some(Rc::new(move |ui, choice| {
                        let mode = if choice == "keep" { itemgrid_core::install::Mode::KeepFiles } else { itemgrid_core::install::Mode::Erase };
                        run_job(ui, Job::Install(Box::new(release.clone()), mode));
                    }));
                    ask
                }
            };
            open_ask(ui, ask);
        }
        "do:android" => {
            let Some(host) = ui.state.borrow().host.clone() else { return };
            let mut checking = Ask::new("back to android", vec!["checking…".into()]);
            checking.ready = false;
            open_ask(ui, checking);
            let ui = ui.clone();
            glib::spawn_future_local(async move {
                let h = host.clone();
                let read = gio::spawn_blocking(move || {
                    let plan = itemgrid_core::android::clean_plan(&h)?;
                    let serial = itemgrid_core::backup::serial(&h)?;
                    Ok::<_, String>((plan, itemgrid_core::android::confirm_word(&serial)))
                })
                .await
                .unwrap_or_else(|_| Err("the work stopped".into()));
                // Asked about something else meanwhile: let be.
                if ui.asking.borrow().as_ref().is_none_or(|a| a.title != "back to android") {
                    return;
                }
                let ask = match read {
                    Err(e) => Ask::new("back to android", wrapped(&format!("cannot just now: {e}"))),
                    Ok((plan, _)) if !plan.stops.is_empty() => Ask::new("back to android", plan.stops.iter().flat_map(|s| wrapped(s)).take(6).collect()),
                    Ok((plan, word)) => {
                        let mut ask = Ask::new("back to android", vec!["erases item".into(), "android as it came".into(), "10 min · cable in".into()]);
                        ask.word = Some(word);
                        ask.verb = "erase";
                        ask.go = Some(Rc::new(move |ui, choice| run_job(ui, Job::AndroidClean(Box::new(plan.clone()), choice == "copy"))));
                        ask
                    }
                };
                *ui.asking.borrow_mut() = Some(ask);
                show_ask(&ui);
            });
        }
        "do:install" => {
            let Some(serial) = stock_android(ui) else { return };
            let word = itemgrid_core::android::confirm_word(&serial);
            let ask = match itemgrid_core::install::releases().into_iter().filter(|r| r.boot.is_some() && r.vbmeta.is_some()).next_back() {
                None => Ask::new("install item", wrapped("no release with a boot image on this computer yet")),
                Some(release) => {
                    let version = release.item.split(['~', '-', '+']).next().unwrap_or(&release.item).to_owned();
                    let mut ask = Ask::new("install item", vec!["erases android".into(), format!("puts item {version}"), "15 min · cable in".into()]);
                    ask.word = Some(word);
                    ask.verb = "install";
                    ask.go = Some(Rc::new(move |ui, _| run_job(ui, Job::InstallStock(Box::new(release.clone()), serial.clone()))));
                    ask
                }
            };
            open_ask(ui, ask);
        }
        "ask:go" => {
            let go = ui.asking.borrow().as_ref().filter(|a| a.can_go()).and_then(|a| a.go.clone().map(|g| (g, a.choices.get(a.chosen).map_or("", |c| c.0))));
            if let Some((go, choice)) = go {
                close_ask(ui);
                if let Some(b) = ui.board.borrow_mut().as_mut() {
                    b.close();
                }
                go(ui, choice);
            }
        }
        "ask:cancel" => close_ask(ui),
        k => {
            if let Some(i) = k.strip_prefix("ask:choice:").and_then(|i| i.parse::<usize>().ok()) {
                if let Some(a) = ui.asking.borrow_mut().as_mut() {
                    a.chosen = i.min(a.choices.len().saturating_sub(1));
                }
                show_ask(ui);
            }
        }
    }
}

/// A key while a question is on the table: the number typed, Enter to go,
/// Escape to let it be. Whether it was the question's.
fn ask_key(ui: &Rc<Ui>, key: gdk::Key) -> bool {
    let open = ui.page.borrow().as_ref().is_some_and(|(k, b)| k == "ask" && !b.closing());
    if !open || ui.asking.borrow().is_none() {
        return false;
    }
    match key {
        gdk::Key::Escape => close_ask(ui),
        gdk::Key::Return | gdk::Key::KP_Enter => board_action(ui, "ask:go"),
        gdk::Key::BackSpace => {
            if let Some(a) = ui.asking.borrow_mut().as_mut() {
                a.typed.pop();
            }
            show_ask(ui);
        }
        k => {
            let Some(ch) = k.to_unicode().filter(|c| c.is_ascii_alphanumeric()) else { return false };
            {
                let mut asking = ui.asking.borrow_mut();
                let Some(a) = asking.as_mut() else { return false };
                let Some(w) = a.word.clone() else { return false };
                if a.typed.chars().count() < w.chars().count() {
                    a.typed.push(ch);
                }
            }
            show_ask(ui);
        }
    }
    true
}

/// The phone's sections not on the menu for now (the owner, 2026-10-06:
/// not needed yet).
const MENU_LATER: [&str; 6] = ["overview", "agent", "look", "battery", "storage", "about"];

fn menu_lines(ui: &Ui) -> Vec<board::Line> {
    let phone = ui.state.borrow().host.is_some();
    // On stock Android (not the guest item/grid started): item put on it.
    if let Some(serial) = stock_android(ui) {
        let _ = serial;
        // (install: under the phone's words, its line to click.)
        return vec![board::Line::new("settings", "settings"), board::Line::new("itemgrid", "about")];
    }
    let mut lines: Vec<board::Line> = if phone {
        // (repair shown as "stock": the way back to the phone's Android.)
        NAV.iter().filter(|(key, ..)| !MENU_LATER.contains(key) && (*key != "developer" || developer_mode())).map(|(key, ..)| board::Line::new(*key, if *key == "repair" { "stock" } else { *key })).collect()
    } else {
        Vec::new()
    };
    lines.push(board::Line::new("settings", "settings"));
    if !phone {
        lines.push(board::Line::new("itemgrid", "about"));
    }
    lines
}

/// The settings' board, if open, set to the settings as they are now: its
/// lines turned up anew where changed; laid anew if lines came or went.
fn refresh_settings(ui: &Ui) {
    let lines = settings_lines(ui);
    let mut page = ui.page.borrow_mut();
    let Some((key, b)) = page.as_mut() else { return };
    if key != "settings" || b.closing() {
        return;
    }
    if b.lines.len() != lines.len() {
        *b = board::Board::open(lines);
        return;
    }
    for (i, new) in lines.into_iter().enumerate() {
        if b.lines[i].text != new.text {
            b.set_line(i, new.text);
        }
    }
}

/// A click at `p` on the page (the floor's px): on the drawn Duo over Wi-Fi,
/// it is opened flat or shut (a picture only); whether it was on it.
fn duo_clicked(ui: &Ui, p: (f64, f64)) -> bool {
    let wifi = ui.state.borrow().host.as_deref().is_some_and(|h| itemgrid_core::link::Via::of(h) == itemgrid_core::link::Via::Wifi);
    let Some(d) = ui.duo_on_sheet.get() else { return false };
    if !wifi || ui.intro.borrow().duo() <= 0.0 || ui.duo_away.get() > 0.5 {
        return false;
    }
    let Some(t) = table_under(&ui.floor_view.borrow(), p) else { return false };
    let (mid, h) = ui.duo_size;
    let left = if ui.wifi_open.get() { d.0 - mid } else { d.0 };
    let on = (left..=d.0 + mid).contains(&t.0) && (d.1 - h / 2.0..=d.1 + h / 2.0).contains(&t.1);
    if on {
        let open = !ui.wifi_open.get();
        ui.wifi_open.set(open);
        fold_to(ui, if open { 180.0 } else { 0.0 });
    }
    on
}

/// The saver off: the window back where it was.
fn saver_off(ui: &Ui) {
    if ui.saver.take().is_none() {
        return;
    }
    trace(format_args!("saver: off"));
    ui.window.unfullscreen();
    if let Some(page) = ui.floor.parent() {
        page.set_cursor_from_name(None);
    }
}

/// item/grid's settings as a board's lines (each clicked turns it).
fn settings_lines(ui: &Ui) -> Vec<board::Line> {
    let onoff = |on: bool| if on { "on" } else { "off" };
    let night = match night_mode().as_str() {
        "night" => "on".to_owned(),
        "day" => "off".to_owned(),
        _ => "auto".to_owned(),
    };
    // (Night is the button on the table; the wallpaper and the developer
    // mode are not offered: 2026-10-08. More comes here.)
    let _ = (onoff, night);
    vec![board::Line::new("set:start", format!("start  {}", ui.intro.borrow().kind.name()))]
}

/// The start turned to the next way (intro::Start): kept, the boards
/// closed, the start played again to be looked at.
fn turn_start(ui: &Ui) {
    let next = ui.intro.borrow().kind.next();
    next.keep();
    trace(format_args!("start: {}", next.name()));
    if let Some(b) = ui.board.borrow_mut().as_mut() {
        if !b.closing() {
            b.close();
        }
    }
    if let Some((_, p)) = ui.page.borrow_mut().as_mut() {
        p.close();
    }
    let mut intro = ui.intro.borrow_mut();
    intro.kind = next;
    intro.replay();
}

/// Night or day shown as due: the table, the window.
fn show_night(ui: &Ui) {
    let on = night_due();
    if on == night() {
        return;
    }
    set_night_now(ui, on);
}

/// Night (or day) shown now: the window's colours, the table's.
fn set_night_now(ui: &Ui, on: bool) {
    NIGHT.store(on, std::sync::atomic::Ordering::Relaxed);
    set_night_css(ui, on);
}

/// The window's own colours (its paper beyond the table, the toasts): as
/// the wave reaches the page's bottom, ahead of the table's own switch.
fn set_night_css(ui: &Ui, on: bool) {
    if on {
        ui.window.add_css_class("night");
    } else {
        ui.window.remove_css_class("night");
    }
    adw::StyleManager::default().set_color_scheme(if on { adw::ColorScheme::ForceDark } else { adw::ColorScheme::ForceLight });
    ui.floor.queue_draw();
    ui.floor_gl.queue_render();
}

/// The night setting turned: night, day.
fn turn_night_mode(ui: &Ui) {
    let next = if night_mode() == "night" { "day" } else { "night" };
    let _ = std::fs::create_dir_all(night_file().parent().unwrap_or(std::path::Path::new(".")));
    let _ = std::fs::write(night_file(), format!("{next}\n"));
    show_night(ui);
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum FloorButton {
    Menu,
    Replay,
    Close,
    /// Night and day: a grey square by day, a light one by night; pressed,
    /// the squares turn over to their other side, from it across the table.
    Night,
}

/// In the word's row at the page's right (the wallpaper turned in the
/// settings).
const FLOOR_BUTTONS: [Option<FloorButton>; BUTTONS] = [Some(FloorButton::Menu), Some(FloorButton::Replay), Some(FloorButton::Close), Some(FloorButton::Night)];
const BUTTONS: usize = 4;

/// The buttons' hover and press, eased.
#[derive(Default)]
struct Buttons {
    hover: Option<usize>,
    lift: [f32; BUTTONS],
    pressed: Option<(usize, std::time::Instant)>,
}

impl Buttons {
    /// A frame on (`dt` s): each square toward its lift - up a little under
    /// the pointer, down a moment as pressed. Whether any moved.
    fn step(&mut self, dt: f32) -> bool {
        let k = 1.0 - (-dt / 0.06).exp();
        let mut moved = false;
        for i in 0..BUTTONS {
            let pressed = self.pressed.is_some_and(|(p, at)| p == i && at.elapsed().as_secs_f32() < 0.12);
            let to = if pressed { -0.3 } else if self.hover == Some(i) { 0.12 } else { 0.0 };
            if (to - self.lift[i]).abs() > 0.001 {
                self.lift[i] += (to - self.lift[i]) * k;
                moved = true;
            } else if self.lift[i] != to {
                self.lift[i] = to;
                moved = true;
            }
        }
        moved || self.pressed.is_some_and(|(_, at)| at.elapsed().as_secs_f32() < 0.2)
    }
}

/// The button under a point of the page: one of the buttons' boxes (its
/// top or a side seen).
fn button_at(fv: &FloorView, p: (f64, f64)) -> Option<usize> {
    use gtk::graphene;
    let m = fv.matrix?;
    if fv.word <= 0.0 {
        return None;
    }
    let side = square() * fv.k;
    let p3 = |x: f32, y: f32, z: f32| {
        let v = m.transform_vec4(&graphene::Vec4::new(x, y, z, 1.0));
        ((v.x() / v.w() + fv.off.0) as f64, (v.y() / v.w() + fv.off.1) as f64)
    };
    (0..FLOOR_BUTTONS.len()).rev().find(|&i| {
        if FLOOR_BUTTONS[i].is_none() || fv.button_in[i] <= 0.5 {
            return false;
        }
        let x0 = fv.buttons_at.0 + i as f32 * side;
        let (_, faces, top) = box_shape(&p3, (x0, fv.buttons_at.1), side, fv.table, fv.button_lift[i].max(0.0));
        inside(&top, p) || faces.iter().any(|(q, _)| inside(q, p))
    })
}

/// The buttons: their squares (raised a little under the pointer, as low
/// boxes), their signs on them as the letters are.
fn draw_buttons(fv: &FloorView, cr: &gtk::cairo::Context) {
    use gtk::graphene;
    let Some(m) = fv.matrix else { return };
    if fv.word <= 0.0 {
        return;
    }
    let side = square() * fv.k;
    let p3 = |x: f32, y: f32, z: f32| {
        let v = m.transform_vec4(&graphene::Vec4::new(x, y, z, 1.0));
        ((v.x() / v.w() + fv.off.0) as f64, (v.y() / v.w() + fv.off.1) as f64)
    };
    let path = |q: &[(f64, f64)]| {
        cr.new_path();
        cr.move_to(q[0].0, q[0].1);
        for p in &q[1..] {
            cr.line_to(p.0, p.1);
        }
        cr.close_path();
    };
    // Farthest from the eye's line first: a box nearer it covers its
    // neighbour's side (drawn left to right, a box's side lay over the top
    // of the one before).
    let mut order: Vec<usize> = (0..FLOOR_BUTTONS.len()).collect();
    let off_eye = |i: usize| (fv.buttons_at.0 + (i as f32 + 0.5) * side - fv.eye_x).abs();
    order.sort_by(|a, b| off_eye(*b).partial_cmp(&off_eye(*a)).unwrap());
    for i in order {
        let Some(b) = &FLOOR_BUTTONS[i] else { continue };
        if fv.button_in[i] <= 0.0 {
            continue;
        }
        let _ = cr.push_group();
        let x0 = fv.buttons_at.0 + i as f32 * side;
        let h = fv.button_lift[i].max(0.0);
        let (_, faces, top) = box_shape(&p3, (x0, fv.buttons_at.1), side, fv.table, h);
        if h > 0.001 {
            for (q, light) in &faces {
                path(q);
                let (r, g, b) = face_rgb(*light);
                cr.set_source_rgb(r, g, b);
                let _ = cr.fill_preserve();
                ink(cr, 0.14);
                cr.set_line_width(1.6);
                let _ = cr.stroke();
            }
            path(&top);
            let (r, g, b) = face_rgb(1.0);
            cr.set_source_rgb(r, g, b * 1.005);
            let _ = cr.fill_preserve();
            ink(cr, 0.14);
            let _ = cr.stroke();
        }
        // Lying on the table: the sign alone, in its square of the grid.
        let strong = if fv.hover_button == Some(i) { 0.9 } else { 0.5 };
        draw_night_at(fv, (fv.buttons_at.0 + (i as f32 + 0.5) * square() * fv.k, fv.buttons_at.1 + 0.5 * square() * fv.k));
        draw_sign(cr, *b, top[0], top[1], top[3], strong);
        let _ = cr.pop_group_to_source();
        let _ = cr.paint_with_alpha((fv.word * fv.button_in[i] * fv.part_alpha[3]).min(1.0) as f64);
    }
}

/// A button's sign in the square whose top left, top right and bottom left
/// corners are `a`, `b`, `d` on the page (drawn in GLYPH units).
fn draw_sign(cr: &gtk::cairo::Context, button: FloorButton, a: (f64, f64), b: (f64, f64), d: (f64, f64), strength: f64) {
    let g = GLYPH;
    cr.save().ok();
    cr.transform(gtk::cairo::Matrix::new((b.0 - a.0) / g, (b.1 - a.1) / g, (d.0 - a.0) / g, (d.1 - a.1) / g, a.0, a.1));
    let c = letter_rgba((0.16, 0.16, 0.18, strength));
    cr.set_source_rgba(c.0, c.1, c.2, c.3);
    cr.set_line_width(4.0);
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    match button {
        FloorButton::Menu => {
            for y in [36.0, 50.0, 64.0] {
                cr.move_to(32.0, y);
                cr.line_to(68.0, y);
            }
            let _ = cr.stroke();
        }
        FloorButton::Replay => {
            // Round and back to where it began.
            let (r, from, to) = (17.0, -1.2, 4.3);
            cr.arc(50.0, 50.0, r, from, to);
            let _ = cr.stroke();
            let (ex, ey) = (50.0 + r * to.cos(), 50.0 + r * to.sin());
            cr.move_to(ex + 8.0, ey - 1.0);
            cr.line_to(ex, ey);
            cr.line_to(ex - 1.0, ey - 9.0);
            let _ = cr.stroke();
        }
        FloorButton::Close => {
            cr.move_to(37.0, 37.0);
            cr.line_to(63.0, 63.0);
            cr.move_to(63.0, 37.0);
            cr.line_to(37.0, 63.0);
            let _ = cr.stroke();
        }
        FloorButton::Night => {
            let g = if draw_night() { 0.86 } else { 0.55 };
            cr.set_source_rgba(g, g, g + 0.01, strength);
            cr.rectangle(35.0, 35.0, 30.0, 30.0);
            let _ = cr.fill();
        }
    }
    cr.restore().ok();
}

/// One layer of the plug's housing (as the halves' edges are drawn: its
/// silhouette layer on layer): `t` 0 the bottom, darker, to 1; the top one
/// lit - a band of light along it and a bevel round its rim.
fn duo_cable_plug(t: f64, top: bool) -> gdk::Paintable {
    use gtk::graphene;
    let k = DUO_PX_PER_MM;
    let (w, h) = (CABLE_ROOM.0 * k, CABLE_ROOM.1 * k);
    let snap = gtk::Snapshot::new();
    let cr = snap.append_cairo(&graphene::Rect::new(0.0, 0.0, w as f32, h as f32));
    cr.scale(k, k);
    let (pw, pl) = CABLE_PLUG;
    let (x, y, r) = (CABLE_PAD, CABLE_PAD, 2.4);
    let shape = |cr: &gtk::cairo::Context, inset: f64| {
        let (x, y, ww, hh, r) = (x + inset, y + inset, pw - 2.0 * inset, pl - 2.0 * inset, (r - inset).max(0.5));
        cr.new_sub_path();
        cr.arc(x + ww - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
        cr.arc(x + ww - r, y + hh - r, r, 0.0, std::f64::consts::FRAC_PI_2);
        cr.arc(x + r, y + hh - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
        cr.arc(x + r, y + r, r, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
        cr.close_path();
    };
    shape(&cr, 0.0);
    // The strain relief, narrower, after it (the cord goes on from there:
    // cable.rs).
    cr.rectangle(x + (pw - 5.0) / 2.0, y + pl - 0.5, 5.0, CABLE_RELIEF + 0.5);
    if top {
        let g = gtk::cairo::LinearGradient::new(x, y, x + pw, y + pl * 0.4);
        g.add_color_stop_rgb(0.0, 0.86, 0.865, 0.87);
        g.add_color_stop_rgb(0.4, 0.99, 0.99, 0.985);
        g.add_color_stop_rgb(1.0, 0.84, 0.845, 0.85);
        let _ = cr.set_source(&g);
        let _ = cr.fill();
        // The bevel: a soft grey rim inside the edge.
        shape(&cr, 0.6);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.08);
        cr.set_line_width(0.5);
        let _ = cr.stroke();
        // Where it goes into the port: the opening's shadow across its end.
        cr.save().ok();
        shape(&cr, 0.0);
        cr.clip();
        let g = gtk::cairo::LinearGradient::new(0.0, y, 0.0, y + 2.2);
        g.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.55);
        g.add_color_stop_rgba(0.35, 0.0, 0.0, 0.0, 0.25);
        g.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
        let _ = cr.set_source(&g);
        cr.rectangle(x, y, pw, 2.2);
        let _ = cr.fill();
        cr.restore().ok();
    } else {
        // The sides: white plastic in its own shade, darker toward the table.
        let c = 0.66 + 0.18 * t;
        cr.set_source_rgb(c, c + 0.003, c + 0.008);
        let _ = cr.fill();
        // The opening's shadow at the end, on the sides as well.
        cr.save().ok();
        shape(&cr, 0.0);
        cr.clip();
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.35);
        cr.rectangle(x, y, pw, 1.0);
        let _ = cr.fill();
        cr.restore().ok();
    }
    drop(cr);
    flatten(&snap, w as f32, h as f32)
}

/// The light on a half's screen: a soft band across it, corner to corner.
fn duo_glare(i: usize, bw: i32, bh: i32) -> gdk::Paintable {
    use gtk::{graphene, gsk};
    let k = DUO_PX_PER_MM as f32;
    let mid = (bw / 2) as f32;
    let x0 = if i == 0 { 0.0 } else { mid };
    let sx = [DUO_SCREEN_X.0, DUO_SCREEN_X.1][i] as f32 * k - x0;
    let (pw, ph) = (DUO_PANEL.0 as f32 * k, DUO_PANEL.1 as f32 * k);
    let rect = graphene::Rect::new(sx, DUO_SCREEN_TOP as f32 * k, pw, ph);
    let snap = gtk::Snapshot::new();
    snap.translate(&graphene::Point::new(DUO_PAD, DUO_PAD));
    let white = |a: f32, at: f32| gsk::ColorStop::new(at, gdk::RGBA::new(1.0, 1.0, 1.0, a));
    snap.append_linear_gradient(
        &rect,
        &graphene::Point::new(rect.x(), rect.y()),
        &graphene::Point::new(rect.x() + rect.width(), rect.y() + rect.height()),
        &[white(0.0, 0.0), white(0.0, 0.28), white(0.55, 0.42), white(0.12, 0.5), white(0.0, 0.62), white(0.0, 1.0)],
    );
    flatten(&snap, mid + 2.0 * DUO_PAD, bh as f32 + 2.0 * DUO_PAD)
}

/// A half's back: frosted glacier glass, lighter toward the spine and the
/// top, with a thin brighter rim - the body's silhouette as its mask.
fn duo_back(body: Option<&gdk::Texture>, i: usize, bw: i32, bh: i32) -> gdk::Paintable {
    use gtk::{graphene, gsk};
    let mid = (bw / 2) as f32;
    let h = bh as f32;
    let x0 = if i == 0 { 0.0 } else { mid };
    let rect = graphene::Rect::new(0.0, 0.0, mid, h);
    let snap = gtk::Snapshot::new();
    snap.translate(&graphene::Point::new(DUO_PAD, DUO_PAD));
    snap.push_mask(gsk::MaskMode::Alpha);
    snap.push_clip(&rect);
    if let Some(body) = body {
        snap.append_texture(body, &graphene::Rect::new(-x0, 0.0, bw as f32, h));
    }
    snap.pop();
    snap.pop();
    let stop = |at: f32, r: f32, g: f32, b: f32| gsk::ColorStop::new(at, gdk::RGBA::new(r, g, b, 1.0));
    // Toward the spine (the right edge for the left half, the left for the right).
    let (from, to) = if i == 0 { (graphene::Point::new(0.0, h), graphene::Point::new(mid, 0.0)) } else { (graphene::Point::new(mid, h), graphene::Point::new(0.0, 0.0)) };
    snap.append_linear_gradient(&rect, &from, &to, &[stop(0.0, 0.70, 0.71, 0.67), stop(0.6, 0.79, 0.80, 0.76), stop(1.0, 0.86, 0.87, 0.83)]);
    snap.pop();
    flatten(&snap, mid + 2.0 * DUO_PAD, h + 2.0 * DUO_PAD)
}

/// A half's silhouette in one colour (its shade, its shadow -
/// blurred by `blur`), with `pad` of transparent room round it.
fn duo_silhouette(body: Option<&gdk::Texture>, i: usize, bw: i32, bh: i32, pad: f32, rgb: [f32; 3], blur: f32) -> gdk::Paintable {
    use gtk::graphene;
    let mid = (bw / 2) as f32;
    let x0 = if i == 0 { 0.0 } else { mid };
    let snap = gtk::Snapshot::new();
    snap.translate(&graphene::Point::new(pad, pad));
    if blur > 0.0 {
        snap.push_blur(blur as f64);
    }
    // Colour from the offset, alpha kept.
    let mut m = [0.0f32; 16];
    m[15] = 1.0;
    snap.push_color_matrix(&graphene::Matrix::from_float(m), &graphene::Vec4::new(rgb[0], rgb[1], rgb[2], 0.0));
    snap.push_clip(&graphene::Rect::new(0.0, 0.0, mid, bh as f32));
    if let Some(body) = body {
        snap.append_texture(body, &graphene::Rect::new(-x0, 0.0, bw as f32, bh as f32));
    }
    snap.pop();
    snap.pop();
    if blur > 0.0 {
        snap.pop();
    }
    flatten(&snap, mid + 2.0 * pad, bh as f32 + 2.0 * pad)
}

/// The Duo as it lies on a table, seen from a little above, tipped as it is
/// held: the right half flat, the left one raised about the spine by the
/// fold (180 flat, 90 standing like a laptop's lid, 0 closed over the right,
/// 360 folded back under it). Each half shows its front or its glacier back
/// as it faces the viewer or not, the nearer drawn over the farther; the
/// raised one darkens as it turns from the light; the shadow lies under the
/// right half, fading as the phone leaves the table.
fn show_fold(ui: &Ui, angle: f64) {
    static FRAMES: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let timed = *FRAMES.get_or_init(|| std::env::var_os("ITEMGRID_FRAMES").is_some());
    let started = std::time::Instant::now();
    show_fold_now(ui, angle);
    if timed {
        trace(format_args!("show_fold in {:.2} ms", started.elapsed().as_secs_f64() * 1000.0));
    }
}

fn show_fold_now(ui: &Ui, angle: f64) {
    use gtk::{graphene, gsk};
    const TILT: f32 = 50.0;
    // The Duo is placed below in its own view (at rest the table's); the
    // table's eye, worked out after it (the wheel's lens, the eye over it,
    // the wallpaper's), may be another: what the Duo's parts are set to is
    // kept and turned into the table's view at the end (`duo_fix`) - so it
    // lies on the table whatever the eye does (it hung in the air).
    let placed: RefCell<Vec<(gtk::Widget, gsk::Transform)>> = RefCell::new(Vec::new());
    let put = |w: &gtk::Widget, t: gsk::Transform| placed.borrow_mut().push((w.clone(), t));
    let duo_fix: std::cell::Cell<Option<graphene::Matrix>> = std::cell::Cell::new(None);
    let gl_proj: Option<(gsk::Transform, gsk::Transform)>;
    let cord_screen: Option<gsk::Transform>;
    let lift = (180.0 - angle).clamp(-180.0, 180.0) as f32;
    let (mid, h) = ui.duo_size;
    let room = h * DUO_ROOM as f32;
    let width = ui.duo.width().max(1) as f32;
    let [pitch, roll] = ui.tilt.get().0;
    // ITEMGRID_TILT=pitch,roll: held so, whatever the phone says (to picture
    // it).
    let [pitch, roll] = std::env::var("ITEMGRID_TILT").ok().and_then(|v| v.split_once(',').and_then(|(a, b)| Some([a.trim().parse().ok()?, b.trim().parse().ok()?]))).unwrap_or([pitch, roll]);
    // How the phone is turned in the table's frame: by duo-motion's
    // quaternion when it comes (the whole turn, the yaw on the table too),
    // else by the gravity's pitch and roll. The quaternion's frame (the
    // right half's: y up its panel) to the drawing's (y down): S R S, S
    // flipping y.
    let orient = ui.orient.get();
    let yref = ui.yaw_ref.get().unwrap_or(0.0);
    let turn = move |t: gsk::Transform| -> gsk::Transform {
        // The reference taken off about the world's vertical: (c,0,0,s) * q.
        let q = orient.2.then(|| {
            let q = orient.0;
            let (c, s) = ((-yref / 2.0).cos(), (-yref / 2.0).sin());
            [c * q[0] - s * q[3], c * q[1] - s * q[2], c * q[2] + s * q[1], c * q[3] + s * q[0]]
        });
        match q {
            Some([w, x, y, z]) => {
                let (w, x, y, z) = (w as f32, x as f32, y as f32, z as f32);
                let rm = [
                    [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
                    [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
                    [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
                ];
                let flip = |i: usize| if i == 1 { -1.0 } else { 1.0 };
                // graphene's matrices act on rows: its [r][c] the column
                // matrix's [c][r].
                let mut f = [0.0f32; 16];
                for rr in 0..3 {
                    for cc in 0..3 {
                        f[rr * 4 + cc] = rm[cc][rr] * flip(rr) * flip(cc);
                    }
                }
                f[15] = 1.0;
                t.matrix(&graphene::Matrix::from_float(f))
            }
            None => t.rotate_3d(-pitch as f32, &graphene::Vec3::x_axis()).rotate_3d(-roll as f32, &graphene::Vec3::y_axis()),
        }
    };
    // Held and tipped, the phone is over the table, not into it: raised
    // until its lowest corner is at the table (worked out below, once the
    // fold's pose is known).
    let held = std::cell::Cell::new(0.0f32);
    // The phone in the view, about its middle at `at`: tipped back on the
    // table, then as it is held - with the perspective for drawing, without
    // it for which way a half faces and how near it is.
    let view = |at: (f32, f32), persp: bool| {
        let t = gsk::Transform::new().translate(&graphene::Point::new(at.0, at.1));
        let t = if persp { t.perspective(3.2 * h) } else { t };
        let [yaw, more] = ui.orbit.get().0;
        let t = t
            .rotate_3d((TILT + more).clamp(5.0, 85.0), &graphene::Vec3::x_axis())
            .rotate_3d(yaw, &graphene::Vec3::z_axis())
            .translate_3d(&graphene::Point3D::new(0.0, 0.0, held.get()));
        turn(t).translate(&graphene::Point::new(-mid, -h / 2.0))
    };
    // Each half's plane: the right flat beside the spine, the left turned
    // about it; its screen at `z` 0, its back at -DUO_THICK; then the
    // transparent room round its pictures.
    // As a phone lies on a table: to 180 the right half flat and the left
    // raised; folded back, a tent - both halves leaning, the hinge up, the
    // outer edges on the table - until it tips onto its side (TOPPLE) and
    // comes to lie back to back. Whatever the fold nothing goes under the
    // table: the phone raised until its lowest point touches it (the half
    // folded back went through the table before).
    const TOPPLE: f32 = 150.0;
    let back = (-lift).max(0.0);
    let rock_size = if back <= TOPPLE { back / 2.0 } else { TOPPLE / 2.0 * (178.0 - back).max(0.0) / (178.0 - TOPPLE) };
    // Folding toward the screens it turns about their plane, back to back
    // about the backs' (the halves meet face to face, or back to back).
    let axis = if lift >= 0.0 { 0.0 } else { -DUO_THICK };
    // A half in the hinge's frame (the hinge at x 0, the half toward +x):
    // the whole phone rocked about the hinge and raised, the left half
    // turned by the fold.
    // Folded, the halves come in to the spine (the Duo's hinge has an axis
    // for each half: shut, their inner edges meet over it, the spine all
    // but hidden in their rounded edges; flat, the gap between them is
    // where it lies).
    let tuck = (lift.abs() / 180.0).clamp(0.0, 1.0) * (1.85 - 0.25) * DUO_PX_PER_MM as f32;
    let local = |t: gsk::Transform, i: usize, rock: f32, raise: f32| {
        let t = t.translate_3d(&graphene::Point3D::new(0.0, 0.0, raise)).rotate_3d(rock, &graphene::Vec3::y_axis());
        if i == 0 {
            t.translate_3d(&graphene::Point3D::new(0.0, 0.0, axis))
                .rotate_3d(lift, &graphene::Vec3::y_axis())
                .translate_3d(&graphene::Point3D::new(-mid + tuck, 0.0, -axis))
        } else {
            t.translate(&graphene::Point::new(-tuck, 0.0))
        }
    };
    let lowest = |rock: f32| {
        let mut z = f32::MAX;
        for i in 0..2 {
            let m = local(gsk::Transform::new(), i, rock, 0.0).to_matrix();
            for x in [0.0, mid] {
                for zz in [0.0, -DUO_THICK] {
                    z = z.min(m.transform_point3d(&graphene::Point3D::new(x, 0.0, zz)).z());
                }
            }
        }
        z
    };
    // The way that lowers the right half's outer edge (the tent's way).
    let rock = if local(gsk::Transform::new(), 1, rock_size, 0.0).to_matrix().transform_point3d(&graphene::Point3D::new(mid, 0.0, 0.0)).z() <= 0.0 { rock_size } else { -rock_size };
    let raise = (-DUO_THICK - lowest(rock)).max(0.0);
    {
        let tipped = turn(gsk::Transform::new())
            .translate(&graphene::Point::new(-mid, -h / 2.0))
            .translate(&graphene::Point::new(mid, 0.0));
        let mut low = -DUO_THICK;
        for i in 0..2 {
            let m = local(tipped.clone(), i, rock, raise).to_matrix();
            for x in [0.0, mid] {
                for y in [0.0, h] {
                    for z in [0.0, -DUO_THICK] {
                        low = low.min(m.transform_point3d(&graphene::Point3D::new(x, y, z)).z());
                    }
                }
            }
            // The plug in its port too, with its strain relief: tipped
            // toward the viewer it went under the table, and the cord with
            // it (out of sight).
            if i == 1 && ui.cable.is_visible() {
                let kk = DUO_PX_PER_MM as f32;
                let tip = h + ((CABLE_PLUG.1 + CABLE_RELIEF - CABLE_IN) as f32) * kk;
                for x in [CABLE_PORT_X - CABLE_PLUG.0 / 2.0, CABLE_PORT_X + CABLE_PLUG.0 / 2.0] {
                    for z in [-DUO_THICK / 2.0 - CABLE_PLUG_T * kk / 2.0, -DUO_THICK / 2.0 + CABLE_PLUG_T * kk / 2.0] {
                        low = low.min(m.transform_point3d(&graphene::Point3D::new(x as f32 * kk, tip, z)).z());
                    }
                }
            }
        }
        // Tipped, it is in the hand: higher over the table the more it is
        // tipped (the sensors tell the tilt, not the height) - so the cord
        // is seen hanging from it, not lying under it.
        // Flat on the table either way up (closed, either half may be under):
        // lying, not held.
        let lying = ((pitch as f32).to_radians().cos() * (roll as f32).to_radians().cos()).abs().clamp(0.0, 1.0);
        held.set(-DUO_THICK - low + 35.0 * DUO_PX_PER_MM as f32 * (1.0 - lying));
    }
    let place = |at: (f32, f32), i: usize, z: f32, persp: bool| {
        let t = view(at, persp).translate(&graphene::Point::new(mid, 0.0));
        local(t, i, rock, raise).translate_3d(&graphene::Point3D::new(-DUO_PAD, -DUO_PAD, z))
    };
    // In the middle of the room: where the phone is seen now, centred. The
    // room's middle is its holder's: in a narrower window the holder is
    // narrower than the room, which keeps to its left edge (the Duo was
    // drawn off to the right).
    let quad = graphene::Rect::new(0.0, 0.0, mid + 2.0 * DUO_PAD, h + 2.0 * DUO_PAD);
    let holder = ui.duo.parent().map_or(width, |p| p.width().max(1) as f32);
    let first = (holder.min(width) / 2.0, room / 2.0);
    // Where it is drawn in its own view: where the table's sheet has it (as
    // the frame before saw it - the table's eye turns it there exactly,
    // `duo_fix`; this only keeps it in its GL room), else the room's middle.
    let _ = (quad, room);
    let at = ui.duo_drawn_at.get().unwrap_or(first);
    let duo_at = at;
    let mut depth = [0.0f32; 2];
    for (i, half) in ui.halves.iter().enumerate() {
        let m = place(at, i, 0.0, false).to_matrix();
        let p = |x: f32, y: f32, z: f32| m.transform_point3d(&graphene::Point3D::new(x, y, z));
        let (o, n) = (p(0.0, 0.0, 0.0), p(0.0, 0.0, 1.0));
        let facing = n.z() - o.z() > 0.0;
        depth[i] = p(DUO_PAD + mid / 2.0, DUO_PAD + h / 2.0, -DUO_THICK / 2.0).z();
        let front = place(at, i, 0.0, true);
        put(&half.front.upcast_ref(), front.clone());
        put(&half.shade.upcast_ref(), front.clone());
        put(&half.back.upcast_ref(), place(at, i, -DUO_THICK, true));
        let layers = half.edge.len().max(2) as f32 - 1.0;
        for (k, e) in half.edge.iter().enumerate() {
            put(e.upcast_ref(), place(at, i, -DUO_THICK * k as f32 / layers, true));
        }
        half.front.set_visible(facing);
        half.shade.set_visible(facing);
        half.glare.set_visible(facing);
        half.back.set_visible(!facing);
        put(&half.glare.upcast_ref(), front.clone());
        // The light from above, a little left and in front: the glare as
        // the screen mirrors it toward the viewer.
        let normal = graphene::Vec3::new(n.x() - o.x(), n.y() - o.y(), n.z() - o.z()).normalize();
        let halfway = graphene::Vec3::new(-0.25, -0.55, 1.75).normalize();
        half.glare.set_opacity((normal.dot(&halfway).max(0.0).powi(12) as f64) * 0.9);
        // Its layers far to near: the screen's side last when it faces the
        // viewer, the back's last when it does not.
        let mut stack: Vec<&gtk::Picture> = Vec::new();
        if facing {
            stack.push(&half.back);
            stack.extend(half.edge.iter().rev());
            stack.extend([&half.front, &half.glare, &half.shade]);
        } else {
            stack.extend([&half.front, &half.glare, &half.shade]);
            stack.extend(half.edge.iter());
            stack.push(&half.back);
        }
        *half.order.borrow_mut() = stack.into_iter().cloned().collect();
    }
    // The light: the raised half darker the more it turns.
    ui.halves[0].shade.set_opacity(((lift.abs().min(90.0) as f64).to_radians().sin() * 0.35).min(0.35));
    ui.halves[1].shade.set_opacity(0.0);
    // Nearer over farther: the shadows first; the spine among the halves by
    // its depth - under a half nearer than it (a raised half's back hid it
    // only so), over the rest.
    let vm = view(at, false).to_matrix();
    // The hinge between the halves' inner edges, at the middle of their
    // thickness: flat, in line with them; closed, at the seam of the stack
    // (it was kept at the right half's middle, half a phone off).
    let edge_mid = |i: usize, x: f32| local(gsk::Transform::new(), i, rock, raise).to_matrix().transform_point3d(&graphene::Point3D::new(x, 0.0, -DUO_THICK / 2.0));
    let (er, el) = (edge_mid(1, 0.0), edge_mid(0, mid));
    let spine_c = ((er.x() + el.x()) / 2.0, (er.z() + el.z()) / 2.0);
    let spine_depth = vm.transform_point3d(&graphene::Point3D::new(mid + spine_c.0, h / 2.0, spine_c.1)).z();
    let order: [usize; 2] = if depth[0] <= depth[1] { [0, 1] } else { [1, 0] };
    for h in &ui.halves {
        h.floor.insert_before(&ui.duo, ui.duo.first_child().as_ref());
    }
    let mut spine_placed = false;
    for i in order {
        if !spine_placed && depth[i] > spine_depth + 1.0 {
            ui.spine.insert_before(&ui.duo, None::<&gtk::Widget>);
            spine_placed = true;
        }
        for w in ui.halves[i].order.borrow().iter() {
            w.insert_before(&ui.duo, None::<&gtk::Widget>);
        }
    }
    if !spine_placed {
        ui.spine.insert_before(&ui.duo, None::<&gtk::Widget>);
    }
    // The hinge, a cylinder along the spine at the halves' middle depth,
    // its strip turned to face the viewer.
    let (dx, dz) = (vm.transform_vec3(&graphene::Vec3::x_axis()), vm.transform_vec3(&graphene::Vec3::z_axis()));
    let face = dx.z().atan2(dz.z()).to_degrees();
    let hw = ui.spine.width().max(1) as f32;
    let hinge = view(at, true)
        .translate_3d(&graphene::Point3D::new(mid + spine_c.0, 0.0, spine_c.1))
        .rotate_3d(face, &graphene::Vec3::y_axis())
        .translate(&graphene::Point::new(-hw / 2.0, 0.0));
    put(&ui.spine.upcast_ref(), hinge.clone());
    // The cable from the right half's bottom edge, in its plane, under the
    // halves (the plug goes into the port), after the shadows; a leaning
    // half (the tent) holds it in the air: faded there.
    let k = DUO_PX_PER_MM as f32;
    // The plug: fixed to the right half, its housing's end a little inside
    // the bottom edge (in the port).
    let plug_frame = |t: gsk::Transform| {
        local(t.translate(&graphene::Point::new(mid, 0.0)), 1, rock, raise).translate_3d(&graphene::Point3D::new(
            ((CABLE_PORT_X - CABLE_PLUG.0 / 2.0 - CABLE_PAD) as f32) * k,
            h - ((CABLE_PAD + CABLE_IN) as f32) * k,
            -DUO_THICK / 2.0,
        ))
    };
    let cable_at = |z: f32| plug_frame(view(at, true)).translate_3d(&graphene::Point3D::new(0.0, 0.0, z));
    // The cord from where the strain relief ends, in the table's frame (the
    // view without the phone's own turn): there it hangs (cable.rs).
    let [yaw, more] = ui.orbit.get().0;
    let table_view = gsk::Transform::new()
        .translate(&graphene::Point::new(at.0, at.1))
        .perspective(3.2 * h)
        .rotate_3d((TILT + more).clamp(5.0, 85.0), &graphene::Vec3::x_axis())
        .rotate_3d(yaw, &graphene::Vec3::z_axis());
    let in_table = turn(gsk::Transform::new().translate_3d(&graphene::Point3D::new(0.0, 0.0, held.get())))
        .translate(&graphene::Point::new(-mid, -h / 2.0));
    let pm = plug_frame(in_table.clone()).to_matrix();
    let table = -DUO_THICK;
    // The hole the cord goes down: one of the floor's squares, ahead and to
    // the right of the phone.
    let step = square() * k;
    // Right in front of the plug, a few centimetres toward the viewer: the
    // squares are laid so that one is centred on the port (FloorView's
    // shift).
    let shift = floor_shift(k);
    let hj = ((h / 2.0 + 48.0 * k - shift.1) / step).floor();
    let hx = CABLE_PORT_X as f32 * k;
    // The square the port's line goes through.
    let hx0 = shift.0 + ((hx - shift.0) / step).floor() * step;
    let hole = [hx0, shift.1 + hj * step, hx0 + step, shift.1 + (hj + 1.0) * step];
    let depth = HOLE_DEPTH as f32 * k;
    let at_mm = |x: f64, y: f64| {
        let p = pm.transform_point3d(&graphene::Point3D::new(x as f32 * k, y as f32 * k, 0.0));
        [p.x(), p.y(), p.z()]
    };
    let (pw, pl) = CABLE_PLUG;
    let (x0, y0) = (CABLE_PAD, CABLE_PAD);
    let start = at_mm(x0 + pw / 2.0, y0 + pl + CABLE_RELIEF);
    let ahead = at_mm(x0 + pw / 2.0, y0 + pl + CABLE_RELIEF + 1.0);
    let d = [ahead[0] - start[0], ahead[1] - start[1], ahead[2] - start[2]];
    let dl = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-4);
    {
        let mut rope = ui.rope.borrow_mut();
        rope.k = k;
        rope.table = table;
        rope.start = start;
        rope.dir = [d[0] / dl, d[1] / dl, d[2] / dl];
        rope.hole = Some((hole, depth));
        rope.end = [(hole[0] + hole[2]) / 2.0, (hole[1] + hole[3]) / 2.0, table - depth + 1.7 * k];
        rope.plug = [at_mm(x0, y0), at_mm(x0 + pw, y0), at_mm(x0 + pw, y0 + pl + CABLE_RELIEF), at_mm(x0, y0 + pl + CABLE_RELIEF)];
        rope.screen = Some(table_view.to_matrix());
        cord_screen = Some(table_view.clone());
    }
    ui.cable.queue_draw();
    // The Duo in 3D: each half's place (the pictures' frame), and the
    // perspective to the GL area's clip space (it begins `past` before the
    // duo's drawing, as the cord's does).
    {
        let past = ui.rope.borrow().gl_past;
        let (w, hh) = (ui.gl3d.width_request() as f32, ui.gl3d.height_request() as f32);
        let ndc = gsk::Transform::new()
            .translate_3d(&graphene::Point3D::new(-1.0, 1.0, 0.0))
            // Depth over a wide range: near the eye (the phone held up and
            // tipped toward the viewer) it went past the near plane, cut off.
            .scale_3d(2.0 / w, -2.0 / hh, -1.0 / 6000.0)
            .translate(&graphene::Point::new(past, ui.rope.borrow().gl_past_top));
        let persp = gsk::Transform::new()
            .translate(&graphene::Point::new(at.0, at.1))
            .perspective(3.2 * h)
            .translate(&graphene::Point::new(-at.0, -at.1));
        let mut sc = ui.scene3d.borrow_mut();
        sc.proj = Some(ndc.clone().transform(Some(&persp)).to_matrix());
        gl_proj = Some((ndc, persp));
        sc.eye = [at.0, at.1, 3.2 * h];
        for i in 0..2 {
            sc.halves[i] = Some(local(view(at, false).translate(&graphene::Point::new(mid, 0.0)), i, rock, raise).to_matrix());
        }
        // The spine: between the halves' inner edges, turned half the fold.
        sc.spine = Some(
            view(at, false)
                .translate_3d(&graphene::Point3D::new(mid + spine_c.0, 0.0, spine_c.1))
                .rotate_3d(rock + lift / 2.0, &graphene::Vec3::y_axis())
                .to_matrix(),
        );
    }
    ui.gl3d.queue_render();
    // The pictures of the halves and the hinge give way to it.
    for half in &ui.halves {
        for w in std::iter::once(&half.back).chain(&half.edge).chain([&half.front, &half.glare, &half.shade]) {
            w.set_visible(false);
        }
    }
    ui.spine.set_visible(false);
    // The floor: the table as the phone at rest sees it (no turn by the
    // pointer, no tilt in the hand), where the duo's drawing is on the page.
    {
        // At the start the eye comes down from straight above. While the
        // cubes stand the room's middle is the view's (where the Duo would
        // be centred is its own pose's - unseen then); toward the Duo's as
        // it is seen.
        // The word stays in the page's upper left (START_AT, a part of its
        // width and height): the cubes stand on the table where the view,
        // once the eye is down, shows that place - and as it comes down the
        // view moves so that they stay there. The Duo is in the middle.
        const START_AT: (f32, f32) = (0.25, 0.25);
        let off_page = ui.duo.compute_point(&ui.floor, &graphene::Point::new(0.0, 0.0)).map(|p| (p.x(), p.y())).unwrap_or((0.0, 0.0));
        // The frame is laid out at its own size (scene::REF) and shown
        // scaled into the page: all
        // below in the frame's px (`page`: its size; `off` the Duo's room's
        // corner in them), the eye scaled into the page's at the end.
        let room_rect = [0.0, 0.0, ui.floor.width().max(1) as f32, ui.floor.height().max(1) as f32];
        // As the wallpaper, a screen's frame (16:9).
        let wall_e = ui.layout.get().steps[scene::WALLPAPER].trans.ease.at(ui.wallpaper_mix.get());
        let frame_size = (scene::REF.0 + (scene::REF_WALL.0 - scene::REF.0) * wall_e, scene::REF.1 + (scene::REF_WALL.1 - scene::REF.1) * wall_e);
        let frame_s = (room_rect[2] / frame_size.0).min(room_rect[3] / frame_size.1).max(0.05);
        let frame_at = (room_rect[0] + (room_rect[2] - frame_size.0 * frame_s) / 2.0, room_rect[1] + (room_rect[3] - frame_size.1 * frame_s) / 2.0);
        let off = ((off_page.0 - frame_at.0) / frame_s, (off_page.1 - frame_at.1) / frame_s);
        let (page_w, page_h) = frame_size;
        let intro = ui.intro.borrow();
        let eye = intro.eye();
        // The start chosen (intro::Start): some leave the eye where it
        // stays from the first frame - the start's steps go by, the eye
        // does not.
        let kind = intro.kind;
        let cam_eye = if kind.still_eye() { 1.0 } else { eye };
        let front = kind == intro::Start::Wave;
        // As the wallpaper the eye nearly straight above (the word and the
        // buttons laid for that view).
        let w_mix = ui.wallpaper_mix.get();
        let tilt_rest = TILT * (1.0 - 0.8 * w_mix * w_mix * (3.0 - 2.0 * w_mix));
        let looking = |e: f32| gsk::Transform::new().perspective(3.2 * h).rotate_3d(tilt_rest * e, &graphene::Vec3::x_axis()).to_matrix();
        let shown_at = |m: &graphene::Matrix, p: (f32, f32)| {
            let v = m.transform_vec4(&graphene::Vec4::new(p.0, p.1, -DUO_THICK, 1.0));
            (v.x() / v.w(), v.y() / v.w())
        };
        // The eye's middle: the frame's as the 1000×800 window had it (the
        // Duo's room's middle there).
        let middle = (FRAME_MIDDLE.0 * page_w / scene::REF.0 - off.0, FRAME_MIDDLE.1 * page_h / scene::REF.1 - off.1);
        if std::env::var_os("ITEMGRID_SHEET").is_some() {
            let b = ui.floor.compute_bounds(&ui.window).map(|b| (b.x(), b.y(), b.width(), b.height()));
            let hp = ui.floor.parent().and_then(|p| p.compute_bounds(&ui.window)).map(|b| (b.x(), b.y(), b.width(), b.height()));
            let v = ui.toasts.compute_bounds(&ui.window).map(|b| (b.x(), b.y(), b.width(), b.height()));
            let sv = ui.shown.compute_bounds(&ui.window).map(|b| (b.x(), b.y(), b.width(), b.height()));
            let st = ui.stage.compute_bounds(&ui.window).map(|b| (b.x(), b.y(), b.width(), b.height()));
            let hd = ui.shown.first_child().and_then(|c| c.compute_bounds(&ui.window)).map(|b| (b.x(), b.y(), b.width(), b.height()));
            trace(format_args!("bounds: window {}x{} floor {b:?} home {hp:?} toasts {v:?} view {sv:?} stage {st:?} view.first {hd:?} first {:?}", ui.window.width(), ui.window.height(), ui.shown.first_child().map(|c| c.type_().name().to_string())));
        }
        if std::env::var_os("ITEMGRID_SHEET").is_some() {
            trace(format_args!("frame: room middle on the page {:?} page {}x{} frame at {frame_at:?} scale {frame_s}", (holder.min(width) / 2.0 + off_page.0, room / 2.0 + off_page.1), ui.floor.width(), ui.floor.height()));
        }
        // Where the eye at rest is on the page (the room's middle before the
        // Duo is seen, the Duo's place after), and how near: the word's place
        // as that eye shows it.
        // The sheet: the eye at rest on the room's middle, the word in the
        // page's upper left - fixed once there, whatever comes on the table
        // after (the Duo, its words): they are laid on it.
        let middle_seen = middle;
        let want = (page_w * START_AT.0 - off.0 - middle.0, page_h * START_AT.1 - off.1 - middle.1);
        // The table's point shown there (Newton's way, a few steps).
        let down = looking(1.0);
        let mut p = want;
        for _ in 0..20 {
            let f = shown_at(&down, p);
            let (fx, fy) = (shown_at(&down, (p.0 + 1.0, p.1)), shown_at(&down, (p.0, p.1 + 1.0)));
            let j = [[fx.0 - f.0, fy.0 - f.0], [fx.1 - f.1, fy.1 - f.1]];
            let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
            if det.abs() < 1e-6 {
                break;
            }
            let e = (f.0 - want.0, f.1 - want.1);
            p = (p.0 - (j[1][1] * e.0 - j[0][1] * e.1) / det, p.1 - (-j[1][0] * e.0 + j[0][0] * e.1) / det);
        }
        // There on the table, then where the table was dragged - not kept to
        // whole squares: the squares are laid from the word (floor_shift),
        // so its letters are in them wherever it is (kept to them it jumped
        // a square at a time as the page's size changed).
        let moved = grid_at();
        let cubes_at = (p.0 + moved.0 * k, p.1 + moved.1 * k);

        // On the card's way into the wallpaper the buttons (kept to whole
        // squares, counted from the page's right) go along smoothly, kept
        // to them only at its first and last quarter.
        let kept = ui.wall_t.get().map_or(1.0, |t| {
            let s = |x: f32| {
                let x = x.clamp(0.0, 1.0);
                x * x * (3.0 - 2.0 * x)
            };
            if t < 0.5 { 1.0 - s(t / 0.25) } else { s((t - 0.75) / 0.25) }
        });
        let squared = |snapped: (f32, f32), free: (f32, f32)| (free.0 + (snapped.0 - free.0) * kept, free.1 + (snapped.1 - free.1) * kept);
        // The saver's page is another size: the word kept where it was on
        // the table (worked out anew it went far off, past the squares).
        let cubes_at = if ui.saver.get().is_some() { ui.cubes_rest.get() } else { ui.cubes_rest.replace(cubes_at); cubes_at };
        // Where the word comes by itself (the sheet's corner), and where the
        // layout moves it.
        let layout = ui.layout.get();
        // The step the frame is at: each step's places and eye, one into the
        // next as the start and what comes after go on (scene.rs).
        let away = intro.timing[scene::MENU].eased(ui.duo_away.get());
        let back_now = intro.timing[scene::SECTION].eased(ui.page_back.get());
        let marks = {
            let raw = ui.marks_raw.get();
            let t = &intro.timing;
            scene::Marks {
                search: t[scene::SEARCH].eased(raw[0]),
                not_found: t[scene::NOT_FOUND].eased(raw[1]),
                phone: intro.duo(),
                open: t[scene::PHONE_OPEN].eased(raw[2]),
                cable: t[scene::CABLE].eased(raw[3]),
                menu: away,
                section: back_now,
                wall: wall_e,
            }
        };
        let (step, step_now) = layout.at([intro.grid(), cam_eye, intro.button(0)], marks);
        let (place, shot) = (step.place, step.shot);
        ui.step_now.set(step_now);
        let word_home = cubes_at;
        let cubes_at = (word_home.0 + place.word.0 * square() * k, word_home.1 + place.word.1 * square() * k);
        for (i, v) in [cubes_at.0, cubes_at.1].into_iter().enumerate() {
            CUBES_AT[i].store(v.to_bits(), std::sync::atomic::Ordering::Relaxed);
        }
        // The camera: the table's point it looks at, where that is on the
        // page, its tilt and how near - eased between straight above the
        // word, where the Duo is seen from, and near the credit (under the
        // word, a letter a quarter square).
        #[derive(Clone, Copy)]
        struct Eye {
            look: (f32, f32),
            on: (f32, f32),
            tilt: f32,
            near: f32,
            /// How far the eye is (in the Duo's heights): nearer, the far
            /// smaller against the near.
            far: f32,
        }
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let mix = |a: Eye, b: Eye, t: f32| Eye {
            look: (lerp(a.look.0, b.look.0, t), lerp(a.look.1, b.look.1, t)),
            on: (lerp(a.on.0, b.on.0, t), lerp(a.on.1, b.on.1, t)),
            tilt: lerp(a.tilt, b.tilt, t),
            near: lerp(a.near, b.near, t),
            far: lerp(a.far, b.far, t),
        };
        let matrix = |e: Eye| {
            // Nearness as a lens's (the picture scaled about its point),
            // not the eye brought nearer: scaled before the perspective the
            // far table went behind the eye and was cut off.
            gsk::Transform::new()
                .translate(&graphene::Point::new(e.on.0, e.on.1))
                .scale(e.near, e.near)
                .perspective(e.far * h)
                .rotate_3d(e.tilt, &graphene::Vec3::x_axis())
                .translate_3d(&graphene::Point3D::new(-e.look.0, -e.look.1, 0.0))
                .to_matrix()
        };
        let on_page = |m: &graphene::Matrix, p: (f32, f32)| {
            let v = m.transform_vec4(&graphene::Vec4::new(p.0, p.1, -DUO_THICK, 1.0));
            (v.x() / v.w(), v.y() / v.w())
        };
        let rest = Eye { look: (0.0, 0.0), on: middle_seen, tilt: tilt_rest, near: 1.0, far: 3.2 };
        // Where the word is once the eye is down (its cubes in the
        // table's squares, a little off the page's point): kept there all
        // the way, so that nothing moves as the eye stops.
        let word_on = on_page(&matrix(rest), cubes_at);
        let above = Eye { look: cubes_at, on: word_on, tilt: 0.0, near: 1.0, far: 3.2 };
        let cur = square() * k;
        // The row under the word (a letter a square, from its left).
        let (left, under) = on_squares(k, (cubes_at.0 - WORD_HALF * cur, cubes_at.1 + 1.5 * cur));
        let rest_m = matrix(rest);
        // The credit under the word, in its column, two rows down (with it
        // wherever the layout puts the word); moved by the layout from there.
        let credit_at = {
            let p = on_squares(k, (cubes_at.0 - WORD_HALF * cur, cubes_at.1 + 2.0 * cur));
            (p.0 + place.credit.0 * cur, p.1 + place.credit.1 * cur)
        };
        // The eye: from straight above the word down to where the Duo is
        // seen from - and there it stays (the camera's home): what comes
        // after comes on the table, not by the eye (a section drawn back
        // below, the word's corner kept).
        let mut e = match kind {
            // The table far (the lens drawn back), drawn in to the word.
            intro::Start::Zoom => mix(Eye { tilt: tilt_rest, near: 0.3, ..above }, rest, eye),
            _ => mix(above, rest, cam_eye),
        };
        // The saver: the eye drifting slowly over the table, round the word.
        let drift = ui.saver_mix.get();
        // The page's size against the usual window's: the saver's scene as
        // large as its screen.
        let page_scale = (ui.floor.width() as f32 / 1000.0).max(ui.floor.height() as f32 / 800.0).max(1.0);
        // The saver's nearness by the page's smaller side (by the larger the
        // word went past a tall screen's edges).
        // (The frame is scaled to the page already.)
        let page_fit = 1.0;
        if drift > 0.0 {
            let t = ui.saver.get().map_or(0.0, |s| s.elapsed().as_secs_f32()) + 40.0;
            // Round the word, in the page's middle.
            let page_mid = (page_w * 0.5 - off.0, page_h * 0.5 - off.1);
            let wander = Eye {
                look: (cubes_at.0 + 1.5 * cur * (t * 0.031).sin(), cubes_at.1 + 1.0 * cur + 1.5 * cur * (t * 0.023).cos()),
                on: page_mid,
                tilt: TILT + 8.0 * (t * 0.041).sin(),
                near: page_fit * 1.5 * (0.9 + 0.12 * (t * 0.027).sin()),
                far: 3.2,
            };
            let d = drift * drift * (3.0 - 2.0 * drift);
            e = mix(e, wander, d);
        }
        // The buttons in the word's row, at the page's right (as the usual
        // view shows it): the last in the square seen a square or so in
        // from the edge.
        let buttons_home = {
            let cubes_at = word_home;
            let m = rest_m;
            let row = cubes_at.1;
            // As far in from the right as the word is from the left (the
            // whole view moved half a square left after, both with it).
            let half = {
                let a = on_page(&m, (cubes_at.0, cubes_at.1));
                let b = on_page(&m, (cubes_at.0 + cur, cubes_at.1));
                (b.0 - a.0).abs() / 2.0
            };
            let word_left = on_page(&m, (cubes_at.0 - WORD_HALF * cur, cubes_at.1 - 0.5 * cur)).0 + off.0 - half;
            let want = page_w - word_left - off.0 + half;
            // Along the row to that point of the page (Newton's way).
            let mut x = cubes_at.0 + 15.0 * cur;
            for _ in 0..12 {
                let f = on_page(&m, (x, row)).0 - want;
                let d = on_page(&m, (x + 1.0, row)).0 - on_page(&m, (x, row)).0;
                if d.abs() < 1e-4 {
                    break;
                }
                x -= f / d;
            }
            let end = squared(on_squares(k, (x, cubes_at.1 - 0.5 * cur)), (x, cubes_at.1 - 0.5 * cur));
            (end.0 - FLOOR_BUTTONS.len() as f32 * cur, end.1)
        };
        let buttons_at = (buttons_home.0 + place.buttons.0 * cur, buttons_home.1 + place.buttons.1 * cur);
        // The sheet: from the word's left edge to the buttons' right, from
        // the word's row down to near the page's bottom (in whole squares).
        // What comes after is laid on it: the Duo right of its words, its
        // top two rows under the word's; its words at its left, in the
        // word's column.
        // (Where the word and the buttons come by themselves: moved by the
        // layout, the sheet stays.)
        let sheet = {
            let top = word_home.1 - 0.5 * cur;
            let (left, right) = (word_home.0 - WORD_HALF * cur, buttons_home.0 + FLOOR_BUTTONS.len() as f32 * cur);
            // The table's row at the page's bottom (Newton's way, down the
            // sheet's middle).
            let want_y = page_h * 0.9 - off.1;
            let mut y = top + 4.0 * cur;
            for _ in 0..12 {
                let f = on_page(&rest_m, ((left + right) / 2.0, y)).1 - want_y;
                let d = on_page(&rest_m, ((left + right) / 2.0, y + 1.0)).1 - on_page(&rest_m, ((left + right) / 2.0, y)).1;
                if d.abs() < 1e-4 {
                    break;
                }
                y -= f / d;
            }
            let bottom = top + ((y - top) / cur).floor().max(4.0) * cur;
            [left, top, right, bottom]
        };
        // The Duo as big as the layout makes it.
        let (mid_px, duo_h) = (ui.duo_size.0 * place.duo_scale, ui.duo_size.1 * place.duo_scale);
        // Where the layout puts it (its top left open flat, in the sheet's
        // squares); by default in the middle of what is left of the sheet
        // right of its words' column (twelve letters and a square clear).
        let duo_corner = place.duo.unwrap_or_else(|| ((((13.0 * cur + sheet[2] - sheet[0]) / 2.0 - mid_px) / cur).round(), 4.0));
        let duo_on_sheet = (sheet[0] + duo_corner.0 * cur + mid_px, sheet[1] + duo_corner.1 * cur + duo_h / 2.0);
        ui.duo_on_sheet.set(Some(duo_on_sheet));
        // The parts that can be moved (E): where each lies on the table.
        let mut edit_boxes: Vec<(&'static str, [f32; 4])> = vec![
            ("word", [cubes_at.0 - WORD_HALF * cur, cubes_at.1 - 0.5 * cur, cubes_at.0 + WORD_HALF * cur, cubes_at.1 + 0.5 * cur]),
            ("buttons", [buttons_at.0, buttons_at.1, buttons_at.0 + FLOOR_BUTTONS.len() as f32 * cur, buttons_at.1 + cur]),
            ("words", [sheet[0] + place.words.0 * cur, sheet[1] + place.words.1 * cur, sheet[0] + (place.words.0 + 12.0) * cur, sheet[1] + (place.words.1 + 3.0) * cur]),
            ("duo", [sheet[0] + duo_corner.0 * cur, sheet[1] + duo_corner.1 * cur, sheet[0] + duo_corner.0 * cur + 2.0 * mid_px, sheet[1] + duo_corner.1 * cur + duo_h]),
            ("credit", [credit_at.0, credit_at.1, credit_at.0 + 8.0 * cur, credit_at.1 + cur]),
        ];
        if std::env::var_os("ITEMGRID_SHEET").is_some() {
            trace(format_args!("sheet {sheet:?} duo {duo_on_sheet:?} mid {mid_px} h {duo_h} cur {cur} word {cubes_at:?} buttons {buttons_at:?} off {off:?} D on page {:?} origin on page {:?} duo_at {duo_at:?}", on_page(&rest_m, duo_on_sheet), on_page(&rest_m, (0.0, 0.0))));
        }
        if (ui.wall_t.get().is_some() || w_mix > 0.0) && std::env::var_os("ITEMGRID_FRAMES").is_some() {
            trace(format_args!(
                "wall laid: t {:?} mix {w_mix:.3} kept {kept:.2} floor {}x{} off {off:?} word {cubes_at:?} buttons {buttons_at:?} on page {:?}",
                ui.wall_t.get(),
                ui.floor.width(),
                ui.floor.height(),
                (on_page(&rest_m, cubes_at), on_page(&rest_m, buttons_at))
            ));
        }
        // The wheel's lens on top (not the saver's). Drawn back, the row of
        // the word and the buttons comes to the page's middle, the eye rises to straight above and
        // the perspective all but goes: the cubes go down into the table,
        // letters in its squares again (as the start begins).
        // The steps' eyes, one into the next as the start and what comes
        // after go on (each as the layout has it).
        // The step's lens and how far its eye has risen toward straight
        // above (each the step's own; the wheel's lens still raises it as
        // it draws back).
        let lens = (ui.zoom.get().0 * shot.zoom).powf(1.0 - ui.saver_mix.get());
        let up = ((1.0 - ui.zoom.get().0) / 0.6).clamp(0.0, 1.0);
        let wheel_flat = up * up * (3.0 - 2.0 * up);
        let flat = (1.0 - (1.0 - shot.top.clamp(0.0, 1.0)) * (1.0 - wheel_flat)) * (1.0 - ui.saver_mix.get());
        let mid = (page_w * 0.5 - off.0, page_h * 0.5 - off.1);
        // The row from the word's start to the buttons' end in the middle
        // (the word lies in the table now). Where they come by themselves:
        // moved by the layout (E), the eye stays.
        let row_mid = ((word_home.0 - WORD_HALF * cur + buttons_home.0 + FLOOR_BUTTONS.len() as f32 * cur) / 2.0, word_home.1);
        // The whole view half a square to the left (the buttons as far in
        // from the right as the word from the left: both margins alike).
        let half = {
            let a = on_page(&rest_m, (cubes_at.0, cubes_at.1));
            let b = on_page(&rest_m, (cubes_at.0 + cur, cubes_at.1));
            (b.0 - a.0).abs() / 2.0
        };
        let kept_s = 1.0 - ui.saver_mix.get();
        let turn = shot.turn * kept_s;
        let kept_k = 1.0 - ui.saver_mix.get();
        // The step's own on an eye: its lens, drawn back as far as it has
        // risen toward straight above (flat), the half square, tipped
        // (not as the saver drifts), moved over the table.
        let adjust = |mut e: Eye| -> Eye {
            e.near *= lens;
            if flat > 0.0 {
                e.look = (e.look.0 + (row_mid.0 - e.look.0) * flat, e.look.1 + (row_mid.1 - e.look.1) * flat);
                e.on = (e.on.0 + (mid.0 - e.on.0) * flat, e.on.1 + (mid.1 - e.on.1) * flat);
                e.tilt *= 1.0 - flat;
                e.far += (40.0 - e.far) * flat;
            }
            e.on.0 -= half * (1.0 - flat);
            e.tilt = (e.tilt + shot.tilt * kept_s).clamp(0.0, 85.0);
            e.look = (e.look.0 + shot.pan.0 * cur * kept_k, e.look.1 + shot.pan.1 * cur * kept_k);
            e
        };
        e = adjust(e);
        // Coming down, the word carried in a straight line from where the
        // logo stands (the page's upper left, START_AT) to its own place
        // (where the eye at rest, with the step's own on it, shows it):
        // one movement with the eye's, pinned last - the adjustments
        // above bent it (the word went down a little, then up).
        if cam_eye < 1.0 {
            let end_on = on_page(&matrix(adjust(rest)), cubes_at);
            let start_on = match kind {
                intro::Start::Flight => (page_w * START_AT.0 - off.0, page_h * START_AT.1 - off.1),
                // From the page's middle into the corner.
                intro::Start::Corner => (page_w * 0.5 - off.0, page_h * 0.42 - off.1),
                // (Tilt, zoom: the word at its place all along.)
                _ => end_on,
            };
            let pin = (start_on.0 + (end_on.0 - start_on.0) * eye, start_on.1 + (end_on.1 - start_on.1) * eye);
            let shown = on_page(&matrix(e), cubes_at);
            if std::env::var_os("ITEMGRID_EYE").is_some() {
                let r = adjust(rest);
                trace(format_args!("pin eye {eye:.3} start {start_on:?} end {end_on:?} shown {shown:?} e.on {:?} r.on {:?} e.look {:?} r.look {:?} e.near {:.3} r.near {:.3} e.tilt {:.1} r.tilt {:.1}", e.on, r.on, e.look, r.look, e.near, r.near, e.tilt, r.tilt));
            }
            e.on = (e.on.0 + pin.0 - shown.0, e.on.1 + pin.1 - shown.1);
        }
        // A section open: the eye drawn back only as far as the word, the
        // menu and the section need to be seen whole - about the word's
        // corner, which stays where it is on the page (the anchor); closed,
        // back the same way.
        let back = intro.timing[scene::SECTION].eased(ui.page_back.get());
        if back > 0.0 {
            let seen = |e: &Eye| {
                gsk::Transform::new()
                    .translate(&graphene::Point::new(e.on.0, e.on.1))
                    .scale(e.near, e.near)
                    .perspective(e.far * h)
                    .rotate_3d(e.tilt, &graphene::Vec3::x_axis())
                    .rotate_3d(turn, &graphene::Vec3::z_axis())
                    .translate_3d(&graphene::Point3D::new(-e.look.0, -e.look.1, 0.0))
                    .to_matrix()
            };
            let (pc, pr) = ui.page_dims.get().unwrap_or((20.0, 10.0));
            let menu_rows = ui.board.borrow().as_ref().map_or(8, |b| b.lines.len()) as f32;
            let menu_cols = ui.board.borrow().as_ref().map_or(9, |b| b.width().max(8)) as f32;
            let top = cubes_at.1 - 0.5 * cur;
            let right = (left + (place.menu.0 + menu_cols + 2.0 + pc) * cur).max(buttons_at.0 + FLOOR_BUTTONS.len() as f32 * cur);
            let bottom = under + (place.menu.1 + menu_rows.max(pr)) * cur;
            let m = seen(&e);
            let anchor = on_page(&m, (left, top));
            let corners = [(left, top), (right, top), (left, bottom), (right, bottom)].map(|p| on_page(&m, p));
            let far_x = corners.iter().map(|c| c.0).fold(f32::MIN, f32::max);
            let far_y = corners.iter().map(|c| c.1).fold(f32::MIN, f32::max);
            // As far back as the page's right and bottom (a margin) need.
            // (Little: the section's words are laid narrow instead.)
            let fit = ((page_w * 0.97 - off.0 - anchor.0) / (far_x - anchor.0).max(1.0)).min((page_h * 0.95 - off.1 - anchor.1) / (far_y - anchor.1).max(1.0)).clamp(0.85, 1.0);
            let f = 1.0 + (fit - 1.0) * back;
            e.on = (anchor.0 + (e.on.0 - anchor.0) * f, anchor.1 + (e.on.1 - anchor.1) * f);
            e.near *= f;
        }
        // From the frame's px into the page's (the Duo's room's): scaled.
        e.on = (e.on.0 * frame_s, e.on.1 * frame_s);
        e.near *= frame_s;
        let off = off_page;
        let at = e.on;
        let eye_x = e.look.0;
        let reach = 520.0 * k * page_scale / e.near.max(0.3);
        let grid_mid = e.look;
        if std::env::var_os("ITEMGRID_EYE").is_some() {
            let m = matrix(e);
            trace(format_args!("eye {eye:.3} tilt {:.1} near {:.3} word {:?} origin {:?} on {:?} look {:?}", e.tilt, e.near, on_page(&m, cubes_at), on_page(&m, (0.0, 0.0)), e.on, e.look));
        }
        // (Turned round the point it looks at.)
        let rest = gsk::Transform::new()
            .translate(&graphene::Point::new(e.on.0, e.on.1))
            .scale(e.near, e.near)
            .perspective(e.far * h)
            .rotate_3d(e.tilt, &graphene::Vec3::x_axis())
            .rotate_3d(turn, &graphene::Vec3::z_axis())
            .translate_3d(&graphene::Point3D::new(-e.look.0, -e.look.1, 0.0))
            .to_matrix();
        // The Duo's view (at its rest: its middle at the table's origin,
        // seen from `at`) into this eye's.
        let duo_rest = gsk::Transform::new().translate(&graphene::Point::new(duo_at.0, duo_at.1)).perspective(3.2 * h).rotate_3d(TILT, &graphene::Vec3::x_axis()).to_matrix();
        if let Some(inv) = duo_rest.inverse() {
            let sc = place.duo_scale;
            // Its back (z -DUO_THICK) kept on the table as it is scaled.
            duo_fix.set(Some(
                gsk::Transform::new()
                    .matrix(&rest)
                    .translate_3d(&graphene::Point3D::new(duo_on_sheet.0, duo_on_sheet.1, DUO_THICK * (sc - 1.0)))
                    .scale_3d(sc, sc, sc)
                    .matrix(&inv)
                    .to_matrix(),
            ));
        }
        // Where that is in the Duo's own drawing, for the next frame (its GL
        // room follows it).
        {
            let v = rest.transform_vec4(&graphene::Vec4::new(duo_on_sheet.0, duo_on_sheet.1, -DUO_THICK, 1.0));
            if v.w() > 0.05 {
                ui.duo_drawn_at.set(Some((v.x() / v.w(), v.y() / v.w())));
            }
        }
        let (grid, word, cubes) = (intro.grid(), intro.word(), intro.cubes());
        let note = intro.note().map(|(t, a)| (t.to_owned(), a));
        let tapped = intro.tapped();
        let hover_button = ui.buttons.borrow().hover;
        let cubes: [(f32, f32); intro::WORD.len()] = std::array::from_fn(|i| (cubes[i].0, (cubes[i].1 * (1.0 - flat) * place.height[2]).max(0.0)));
        // Each part as the step has it: shown, how strong.
        let part_alpha: [f32; 6] = std::array::from_fn(|k| (place.show[k] * place.bright[k]).clamp(0.0, 2.0));
        // Each button coming in on the table as the word's cubes go down,
        // one after the other; they lie there, squares as the word's.
        let button_in: [f32; BUTTONS] = std::array::from_fn(|i| intro.button(i) * (1.0 - ui.saver_mix.get()));
        let button_lift = [place.height[3].max(0.0); BUTTONS];
        // The credit under the word, a letter a square; the note there
        // after looking (not found): what to do.
        let mut texts = Vec::new();
        // Turned over as the growing of the squares (draw_floor's) comes
        // across each.
        let grown = grid * (520.0 * k + 3.0 * cur);
        let mut tiles: Vec<board::Tile> = intro::CREDIT
            .chars()
            .enumerate()
            .filter(|(_, ch)| *ch != ' ')
            .map(|(i, ch)| {
                let c = (credit_at.0 + (i as f32 + 0.5) * cur, credit_at.1 + 0.5 * cur);
                let d = ((c.0 - cubes_at.0).powi(2) + (c.1 - cubes_at.1).powi(2)).sqrt();
                let f = intro.credit_flip(i, ch, ((grown - d) / (14.0 * cur)).clamp(0.0, 1.0));
                board::Tile { at: (credit_at.0 + i as f32 * cur, credit_at.1), flap: board::Flap { from: f.from, to: f.to, turn: f.turn }, rgba: (0.5, 0.5, 0.52, (0.8 * f.strength * part_alpha[5]).min(1.0) as f64), lift: 0.0 }
            })
            .collect();
        // The open board (the sections' menu) under the word, a row apart
        // (where the note goes: one or the other).
        let board_at = on_squares(k, (left, under));
        let board_at = (board_at.0 + place.menu.0 * cur, board_at.1 + place.menu.1 * cur);
        let menu_cols = ui.board.borrow().as_ref().map_or(9, |b| b.width().max(8));
        if let Some(b) = ui.board.borrow().as_ref() {
            edit_boxes.push(("menu", [board_at.0, board_at.1, board_at.0 + menu_cols as f32 * cur, board_at.1 + b.lines.len() as f32 * cur]));
        }
        if let Some(b) = ui.board.borrow().as_ref() {
            tiles.extend(b.tiles(board_at, cur).into_iter().map(|t| board::Tile { rgba: (t.rgba.0, t.rgba.1, t.rgba.2, (t.rgba.3 * part_alpha[4] as f64).min(1.0)), ..t }));
        }
        // A section's board beside the menu, two squares apart.
        let page_at = (board_at.0 + (menu_cols + 2) as f32 * cur, board_at.1);
        if let Some((_, b)) = ui.page.borrow().as_ref() {
            tiles.extend(b.tiles(page_at, cur).into_iter().map(|t| board::Tile { rgba: (t.rgba.0, t.rgba.1, t.rgba.2, (t.rgba.3 * part_alpha[4] as f64).min(1.0)), ..t }));
        }
        // The phone's words under it, a letter a square of the table (never
        // smaller: the words are made short instead) - its number, its
        // system, its charge, how it is linked (or, on the cable, held), and
        // what is wrong if anything is. Under what is seen of it: shut, its
        // right half.
        // A job going on (or just over): told on the table where the phone's
        // words are - what it is, its stage, how long is left; then done or
        // stopped (a minute).
        // (This window's job, else one the command line runs.)
        // (Where it was, while it fades.)
        let (mut act_at, mut act_wanted, mut act_strength) = (ui.floor_view.borrow().act_at, false, (intro.duo() * part_alpha[0]).min(1.0));
        let job_now: Option<(String, Vec<String>, Option<Option<String>>, u64)> = ui
            .state
            .borrow()
            .job
            .as_ref()
            .map(|j| (j.kind.to_owned(), j.lines.clone(), j.ended.clone(), j.took.map_or(0, |t| j.started.elapsed().as_secs().saturating_sub(t))))
            .or_else(|| {
                itemgrid_core::activity::elsewhere(60).map(|a| {
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
                    (a.job.clone(), a.lines.clone(), a.outcome(), a.ended_at.map_or(0, |e| now.saturating_sub(e)))
                })
            });
        let job_lines: Option<Vec<String>> = job_now.and_then(|(kind, jlines, ended, over)| {
            if ended.is_some() && over > 60 {
                return None;
            }
            let title = match kind.as_str() {
                "android-clean" => "to android",
                "install-stock" => "installing item",
                "update" => "updating item",
                "full-backup" => "backing up",
                k => journey::title(k),
            };
            let mut lines = wrapped(title);
            match &ended {
                None => {
                    let stages = journey::stages(&kind);
                    if !stages.is_empty() {
                        let at = journey::locate(&stages, &jlines).min(stages.len() - 1);
                        lines.extend(wrapped(stages[at].title));
                        let left = (journey::left(&stages, at, 0.0) / 60.0).ceil();
                        lines.push(if left <= 1.0 { "a minute left".to_owned() } else { format!("{left} min left") });
                    }
                    // What the hands do meanwhile: nothing - unless the
                    // phone asks (its bootloader's unlock, by its keys).
                    if jlines.last().is_some_and(|l| l.starts_with("unlocking")) {
                        lines.extend(wrapped("on the phone: unlock, with the volume keys, then power"));
                    } else {
                        lines.push("hands off the phone".to_owned());
                    }
                }
                Some(None) => lines.push("done".to_owned()),
                Some(Some(e)) => {
                    lines.push("stopped".to_owned());
                    lines.extend(wrapped(e).into_iter().take(3));
                }
            }
            Some(lines)
        });
        if let Some(lines) = &job_lines {
            // Under the word, where the note goes (room for them there; the
            // phone's words gone meanwhile).
            let (x, top) = (left, under);
            let (head, rest) = lines.split_at(1);
            texts.push(TableText { at: (x, top), cell: cur, lines: head.to_vec(), grey: 0.16, bold: false, set: 1.0, strength: 1.0 });
            texts.push(TableText { at: (x, top + cur), cell: cur, lines: rest.to_vec(), grey: 0.45, bold: false, set: 1.0, strength: 1.0 });
        } else if intro.duo() > 0.0 && ui.state.borrow().host.is_some() {
            let low = |s: glib::GString| s.to_lowercase();
            // "Surface Duo · 00001": its club number.
            let name = low(ui.name.label());
            let name = name.split_once(" · ").map_or(name.clone(), |(_, n)| format!("duo {n}"));
            // "item 0.2.1 (dev, 5 Oct) · Droidian 102": item's version.
            let system = low(ui.name_sub.label()).split(" (").next().unwrap_or("").split(" · ").next().unwrap_or("").to_owned();
            // "Battery 78% · charging · Wi-Fi": the charge, charging or not.
            let first = ui.status_lines.label().lines().next().unwrap_or("").to_lowercase();
            let parts: Vec<&str> = first.strip_prefix("battery ").unwrap_or("").split(" · ").collect();
            let charge = match parts.as_slice() {
                [c, "charging", ..] => format!("{c} charging"),
                [c, ..] if c.ends_with('%') => c.to_string(),
                _ => String::new(),
            };
            let wifi = ui.state.borrow().host.as_deref().is_some_and(|h| itemgrid_core::link::Via::of(h) == itemgrid_core::link::Via::Wifi);
            let link = if wifi { "on wi-fi".to_owned() } else { "on cable".to_owned() };
            let title = low(ui.status_title.label());
            let wrong = if title.contains("fine") { String::new() } else { title.strip_prefix("your duo is ").or(title.strip_prefix("your duo ")).unwrap_or(&title).to_owned() };
            let lines: Vec<String> = [name, system, charge, link, wrong].into_iter().filter(|l| !l.is_empty()).collect();
            // Left of it, a square clear of where its left half lies open
            // flat (opened by a click on Wi-Fi), its top at the phone's.
            let (mid, _) = ui.duo_size;
            let wide = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f32 * cur;
            // Its left edge in the word's column (the i of item: the word is
            // put there, above), its top at the phone's.
            let _ = (mid, wide);
            let (x, top) = (sheet[0] + place.words.0 * cur, sheet[1] + place.words.1 * cur);
            let rows = lines.len() as f32;
            let _ = rows;
            let (head, rest) = lines.split_at(1);
            let strength = (intro.duo() * part_alpha[0]).min(1.0);
            texts.push(TableText { at: (x, top), cell: cur, lines: head.to_vec(), grey: 0.16, bold: false, set: 1.0, strength });
            texts.push(TableText { at: (x, top + cur), cell: cur, lines: rest.to_vec(), grey: 0.45, bold: false, set: 1.0, strength });
        } else if let Some(lines) = (intro.duo() > 0.0 && ui.state.borrow().host.is_none() && !ui.shut_away.get()).then(|| away_words(ui)).flatten() {
            let (x, top) = (sheet[0] + place.words.0 * cur, sheet[1] + place.words.1 * cur);
            let (head, rest) = lines.split_at(1);
            let strength = (intro.duo() * part_alpha[0]).min(1.0);
            // Stock Android on the cable: item put on it, a row under the
            // words (eleven letters: clear of the open phone).
            act_at = (x, top + (lines.len() + 1) as f32 * cur);
            act_wanted = matches!(ui.state.borrow().place, Place::Android(_)) && stock_android(ui).is_some();
            act_strength = strength;
            texts.push(TableText { at: (x, top), cell: cur, lines: head.to_vec(), grey: 0.16, bold: false, set: 1.0, strength });
            texts.push(TableText { at: (x, top + cur), cell: cur, lines: rest.to_vec(), grey: 0.45, bold: false, set: 1.0, strength });
        } else if intro.duo() > 0.0 && ui.shut_away.get() {
            // Asleep (gone quiet shut): as it was last seen.
            if let Some(lines) = ui.last_seen.borrow().as_ref().map(LastSeen::words) {
                let (x, top) = (sheet[0] + place.words.0 * cur, sheet[1] + place.words.1 * cur);
                let (head, rest) = lines.split_at(1);
                let strength = (intro.duo() * part_alpha[0]).min(1.0);
                texts.push(TableText { at: (x, top), cell: cur, lines: head.to_vec(), grey: 0.16, bold: false, set: 1.0, strength });
                texts.push(TableText { at: (x, top + cur), cell: cur, lines: rest.to_vec(), grey: 0.45, bold: false, set: 1.0, strength });
            }
        }
        if let Some((text, strength)) = intro.note() {
            texts.push(TableText { at: (left, under), cell: cur, lines: text.lines().map(str::to_owned).collect(), grey: 0.45, bold: false, set: 1.0, strength });
        }
        // The phone's line: open while wanted (no menu, no question), else
        // put away.
        let menu_open = ui.board.borrow().as_ref().is_some_and(|b| !b.closing());
        // A question whose board went (closed with the menu): done with.
        let asked_on = ui.page.borrow().as_ref().is_some_and(|(k, b)| k == "ask" && !b.closing());
        if !asked_on && ui.asking.borrow().as_ref().is_some_and(|a| a.ready) {
            ui.asking.borrow_mut().take();
            ui.menu_for_ask.set(false);
        }
        let wanted = act_wanted && !menu_open && ui.asking.borrow().is_none();
        if wanted != ui.act.borrow().as_ref().is_some_and(|b| !b.closing()) {
            trace(format_args!("act: wanted {wanted} (stock {act_wanted}, menu {menu_open}, asking {}) at {act_at:?}", ui.asking.borrow().is_some()));
        }
        {
            let mut a = ui.act.borrow_mut();
            match (a.as_mut(), wanted) {
                (Some(b), true) if b.closing() => *a = Some(board::Board::open(vec![board::Line::new("do:install", "› install")])),
                (None, true) => *a = Some(board::Board::open(vec![board::Line::new("do:install", "› install")])),
                (Some(b), false) if !b.closing() => b.close(),
                _ => {}
            }
        }
        if let Some(b) = ui.act.borrow().as_ref() {
            tiles.extend(b.tiles(act_at, cur).into_iter().map(|t| board::Tile { rgba: (t.rgba.0, t.rgba.1, t.rgba.2, (t.rgba.3 * act_strength as f64).min(1.0)), ..t }));
        }
        // The Duo seen as the cubes go down, its name under it with it (no
        // phone: the table and the word only).
        // Put away while the menu is open (it was over the boards).
        let away = intro.timing[scene::MENU].eased(ui.duo_away.get());
        let shown = intro.duo() * (1.0 - away) * place.show[1];
        ui.duo.set_opacity(shown as f64);
        // Its cable with it (left on the table, under the menu).
        ui.cable.set_opacity(shown as f64);
        for p in &ui.cable_plug {
            p.set_opacity(shown as f64);
        }
        // Away, the clicks go through it to the boards.
        if let Some(holder) = ui.duo.parent() {
            // (Nor while its parts are moved: the Duo is dragged on the table.)
            let target = away < 0.5 && !ui.editing.get();
            if holder.can_target() != target {
                holder.set_can_target(target);
            }
        }
        // Not seen: its GL area not drawn (it was each frame regardless; the
        // frames go on, ticking on the Duo's room).
        ui.gl3d.set_visible(shown > 0.0);
        for l in [&ui.name, &ui.name_sub, &ui.battery] {
            l.set_opacity(intro.duo() as f64);
        }
        drop(intro);
        // The waves of the squares turning over (the night button): how far
        // each has come, at one speed; one across the table, night (or day)
        // has come: the colours follow and the setting is kept.
        // The table's point under the page's bottom middle (the last frame's
        // view): the band too near the eye takes its colour, and the window
        // its colours, as a wave gets there.
        let near_at = {
            let fv = ui.floor_view.borrow();
            table_under(&fv, (ui.floor.width() as f64 / 2.0, ui.floor.height() as f64 - 1.0)).unwrap_or(fv.near_at)
        };
        let waves: Vec<(f32, (f32, f32))> = {
            let (reach, step) = {
                let fv = ui.floor_view.borrow();
                (fv.reach, square() * fv.k)
            };
            let mut ws = ui.night_waves.borrow_mut();
            let mut done = 0;
            ws.retain(|(since, _, _)| {
                let over = since.elapsed().as_secs_f32() >= NIGHT_FLIP_S;
                if over {
                    done += 1;
                }
                !over
            });
            for _ in 0..done {
                set_night_now(ui, !night());
            }
            if done > 0 {
                let _ = std::fs::create_dir_all(night_file().parent().unwrap_or(std::path::Path::new(".")));
                let _ = std::fs::write(night_file(), if night() { "night\n" } else { "day\n" });
            }
            // The window's colours as a wave reaches the page's bottom: the
            // waves still on their way past there, counted from what is set.
            let mut css = night();
            for (since, from, reached) in ws.iter_mut() {
                let how_far = (since.elapsed().as_secs_f32() / NIGHT_FLIP_S).min(1.0);
                if wave_passed(reach, step, how_far, *from, near_at) {
                    *reached = true;
                }
                if *reached {
                    css = !css;
                }
            }
            set_night_css(ui, css);
            ws.iter().map(|(since, from, _)| ((since.elapsed().as_secs_f32() / NIGHT_FLIP_S).min(1.0), *from)).collect()
        };
        let mut fv = ui.floor_view.borrow_mut();
        let changed = fv.off != off || fv.at != at || fv.matrix != Some(rest) || fv.hole.is_some() != (ui.cable.is_visible() && shown > 0.5) || (fv.grid, fv.word, fv.cubes, fv.eye, fv.cubes_at) != (grid, word, cubes, eye, cubes_at) || fv.note != note || fv.tapped != tapped || fv.texts != texts || fv.tiles != tiles || fv.board_at != board_at || fv.page_at != page_at || fv.act_at != act_at || fv.waves != waves || fv.near_at != near_at || fv.front != front || fv.eye_x != eye_x || fv.hover_button != hover_button || fv.buttons_at != buttons_at || fv.button_lift != button_lift || fv.button_in != button_in || fv.reach != reach || fv.grid_mid != grid_mid || fv.sheet != Some(sheet) || fv.part_alpha != part_alpha;
        // The hole only with the cable going down it.
        *fv = FloorView { matrix: Some(rest), off, at, k, table: -DUO_THICK, hole: (ui.cable.is_visible() && shown > 0.5).then_some(([hole[0] * place.duo_scale + duo_on_sheet.0, hole[1] * place.duo_scale + duo_on_sheet.1, hole[2] * place.duo_scale + duo_on_sheet.0, hole[3] * place.duo_scale + duo_on_sheet.1], depth)), sheet: Some(sheet), part_alpha, grid, word, cubes, eye, cubes_at, note, tapped, texts, tiles, board_at, page_at, act_at, edit_boxes, duo_corner, waves, near_at, size: (ui.floor.width() as f32, ui.floor.height() as f32), front, eye_x, buttons_at, button_lift, button_in, hover_button, reach, grid_mid };
        if changed {
            ui.floor.queue_draw();
            ui.floor_gl.queue_render();
        }
    }
    // The cord under the halves (past the plug it is outside them), over
    // the shadows; the plug over the right half's layers - its bottom
    // edge's face too: it goes into it - and under a half nearer than it.
    ui.cable.insert_after(&ui.duo, Some(&ui.halves[0].floor));
    ui.gl3d.insert_after(&ui.duo, Some(&ui.cable));
    let mut after: gtk::Widget = ui.gl3d.clone().upcast();
    let n = ui.cable_plug.len().max(2) as f32 - 1.0;
    for (i, p) in ui.cable_plug.iter().enumerate() {
        let z = (i as f32 / n - 0.5) * CABLE_PLUG_T * k;
        put(p.upcast_ref(), cable_at(z));
        p.insert_after(&ui.duo, Some(&after));
        after = p.clone().upcast();
    }
    // The shadows on the table: the right half's under it; the left's under
    // it while it lies there, narrowing toward the spine as it rises, gone
    // when it folds under.
    let lying = (pitch.to_radians().cos() * roll.to_radians().cos()).abs().clamp(0.0, 1.0);
    // Each half's shadow as wide as the half seen from above, from the
    // spine out (the halves turn about it: in the tent the outer edges come
    // in toward it).
    let right_c = rock.to_radians().cos().abs().max(0.08);
    let right = view(at, true)
        .translate_3d(&graphene::Point3D::new(mid, 0.0, -DUO_THICK))
        .scale(right_c, 1.0)
        .translate(&graphene::Point::new(-DUO_FLOOR_PAD + 4.0, -DUO_FLOOR_PAD + 10.0));
    put(&ui.halves[1].floor.upcast_ref(), right.clone());
    ui.halves[1].floor.set_opacity(0.55 * lying * lying * (0.4 + 0.6 * right_c as f64));
    let rise = lift.max(0.0).min(90.0).to_radians();
    let narrow = view(at, true)
        .translate_3d(&graphene::Point3D::new(mid, 0.0, -DUO_THICK))
        .scale(rise.cos().max(0.08), 1.0)
        .translate(&graphene::Point::new(-mid - DUO_FLOOR_PAD + 4.0, -DUO_FLOOR_PAD + 10.0));
    if back > 0.0 {
        // Folded back: the left half leaning from its outer edge, till it
        // goes under the right one.
        let tilt = (rock + if rock >= 0.0 { lift } else { -lift }).to_radians();
        let c = tilt.cos();
        let from_edge = view(at, true)
            .translate_3d(&graphene::Point3D::new(mid, 0.0, -DUO_THICK))
            .scale(c.abs().max(0.08), 1.0)
            .translate(&graphene::Point::new(-mid - DUO_FLOOR_PAD + 4.0, -DUO_FLOOR_PAD + 10.0));
        put(&ui.halves[0].floor.upcast_ref(), from_edge.clone());
        ui.halves[0].floor.set_visible(c > 0.05);
        ui.halves[0].floor.set_opacity(0.55 * lying * lying * (0.4 + 0.6 * c.max(0.0) as f64));
    } else {
        put(&ui.halves[0].floor.upcast_ref(), narrow.clone());
        ui.halves[0].floor.set_visible(lift > -5.0 && lift < 120.0);
        ui.halves[0].floor.set_opacity(0.55 * lying * lying * (1.0 - 0.75 * rise.sin() as f64));
    }
    // The Duo's parts in the table's view (see `placed`).
    let fix = duo_fix.get().map(|m| gsk::Transform::new().matrix(&m));
    for (w, t) in placed.into_inner() {
        let t = match &fix {
            Some(fx) => fx.clone().transform(Some(&t)),
            None => t,
        };
        ui.duo.set_child_transform(&w, Some(&t));
    }
    if let Some(fx) = &fix {
        if let Some((ndc, persp)) = gl_proj {
            ui.scene3d.borrow_mut().proj = Some(ndc.transform(Some(fx)).transform(Some(&persp)).to_matrix());
            ui.gl3d.queue_render();
        }
        if let Some(cord) = cord_screen {
            ui.rope.borrow_mut().screen = Some(fx.clone().transform(Some(&cord)).to_matrix());
            ui.cable.queue_draw();
        }
    }
}

fn fill(ui: &Rc<Ui>, s: &status::Status, link: &str) {
    // Running Droidian: its swirl on the drawn Duo's back (for the logo).
    let droidian = s.os.to_lowercase().contains("droidian");
    if ui.scene3d.borrow().sticker != droidian {
        ui.scene3d.borrow_mut().sticker = droidian;
        ui.gl3d.queue_render();
    }
    // The cable not drawn (the owner, 2026-10-06): the phone's words say
    // it is on it.
    ui.cable.set_visible(false);
    for p in &ui.cable_plug {
        p.set_visible(false);
    }
    // Over Wi-Fi drawn shut, lying on the table, still: it comes to life on
    // the cable.
    if link != "cable" {
        ui.shut_away.set(true);
        ui.orient.set((ui.orient.get().0, ui.orient.get().1, false));
        ui.motion_on.set(false);
        fold_to(ui, if ui.wifi_open.get() { 180.0 } else { 0.0 });
        tilt_to(ui, [0.0, 0.0, 1.0]);
        ui.pose_name.borrow_mut().clear();
        ui.pose.set_label("On Wi-Fi · plug in the cable to see it move");
    } else {
        // Seen open by a look after it was shut (followed again:
        // follow_hinge).
        if s.hinge.is_some_and(|a| a > 20.0) && ui.lid_shut_at.get().is_some() {
            ui.lid_shut_at.set(None);
            ui.shut_away.set(false);
            if ui.pose_name.borrow().as_str() == "closed" {
                ui.pose_name.borrow_mut().clear();
            }
        }
        if ui.shut_away.get() && ui.lid_shut_at.get().is_none() {
            // Come on the cable from Wi-Fi (drawn shut there): itself again.
            ui.shut_away.set(false);
        }
        if let Some(a) = s.hinge {
            fold_to(ui, if ui.pose_name.borrow().as_str() == "closed" { 0.0 } else { a });
        }
    }
    // The club's number, if this computer knows it.
    *ui.serial.borrow_mut() = s.serial.clone();
    match itemgrid_core::club::known(&s.serial) {
        Some(d) => ui.name.set_label(&format!("Surface Duo · {}", d.number)),
        None => ui.name.set_label("Surface Duo"),
    }
    ui.join.set_visible(false);
    // Its number: asked for quietly if not known yet, written onto it if
    // not there.
    let host = ui.state.borrow().host.clone();
    // Once per boot of the phone: its number looked for again after a
    // reinstall or a restart (the boot to two minutes: when it started).
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let boot = now.saturating_sub(s.uptime_s) / 120;
    claim_quietly_as(ui, &format!("{}@{boot}", s.serial), &s.serial, host);
    let problems: Vec<String> = s.warnings().into_iter().chain(s.failed.iter().map(|u| format!("{u} failed"))).collect();
    let dev = developer_mode();
    ui.banner.set_title(&problems.join(" · "));
    ui.banner.set_revealed(dev && !problems.is_empty());
    // The simple page: only what its owner can do something about (a failed
    // background unit - fstrim on the loop rootfs, a suspend the cable kept
    // busy - is for Developer Mode).
    simple_status(ui, s, &s.warnings(), link);
    // A job that ended well and asked for the PIN: done with once the phone
    // is unlocked (the card said "enter the PIN" after it was).
    if s.locked == Some(false) && s.item_running {
        let mut st = ui.state.borrow_mut();
        if st.job.as_ref().is_some_and(|j| j.ended == Some(None)) {
            st.job = None;
        }
        if let Some(a) = itemgrid_core::activity::elsewhere(15 * 60).filter(|a| a.ended_at.is_some() && a.outcome() == Some(None)) {
            st.dismissed = a.ended_at;
        }
    }
    sections::fill(&ui.sections, s, itemgrid_core::club::known(&s.serial).map(|d| d.number), link, developer_mode());
    {
        let mut f = ui.section_words.borrow_mut();
        f.insert("about", sections::about_rows(s, itemgrid_core::club::known(&s.serial).map(|d| d.number), link));
        f.insert("storage", sections::storage_rows(s));
        f.insert("updates", sections::updates_rows(s));
    }

    let charge = s.battery.map(|b| format!("{b}%")).unwrap_or_else(|| "?".into());
    let bolt = if s.battery_status == "Charging" { "⚡ " } else { "" };
    ui.battery.set_label(&format!("{bolt}Battery {charge} · {}", s.battery_status.to_lowercase()));
    ui.software.set_label(&format!("item {} · built {} · {}", s.item, s.item_built, if s.item_running { "running" } else { "not running" }));

    while let Some(child) = ui.facts.first_child() {
        ui.facts.remove(&child);
    }
    let mut facts = vec![
        ("System", s.os.clone()),
        ("Kernel", s.kernel.clone()),
        ("Up", format!("{} h {} min", s.uptime_s / 3600, s.uptime_s / 60 % 60)),
        ("Port", s.port.clone()),
        ("sensorfw", s.sensorfw.clone()),
    ];
    if let Some(t) = s.battery_temp {
        facts.push(("Battery", format!("{t:.0} °C")));
    }
    if let Some(t) = s.cpu_temp {
        facts.push(("CPU", format!("{t:.0} °C")));
    }
    for (i, (name, value)) in facts.into_iter().enumerate() {
        ui.facts.attach(&gtk::Label::builder().label(name).xalign(1.0).css_classes(["fact-name"]).build(), 0, i as i32, 1, 1);
        ui.facts.attach(&gtk::Label::builder().label(&value).xalign(0.0).selectable(true).build(), 1, i as i32, 1, 1);
    }
    // As it is now, kept: gone quiet after this, shut (on Wi-Fi it is
    // drawn shut; on the cable, the lid closed), it is asleep - after a
    // restart too.
    let name = ui.name.label().to_lowercase();
    let seen = LastSeen {
        name: name.split_once(" · ").map_or("duo".to_owned(), |(_, n)| format!("duo {n}")),
        battery: s.battery,
        at: glib::DateTime::now_local().map_or(0, |t| t.to_unix()),
        shut: link != "cable" || ui.lid_shut_at.get().is_some(),
        droidian,
    };
    seen.save();
    *ui.last_seen.borrow_mut() = Some(seen);
}

/// How the phone was last seen (~/.local/share/itemgrid/last-seen): its
/// name on the table, its charge, when (unix seconds), whether shut (gone
/// quiet so, asleep), whether it ran Droidian.
#[derive(Clone, Debug, PartialEq)]
struct LastSeen {
    name: String,
    battery: Option<u32>,
    at: i64,
    shut: bool,
    droidian: bool,
}

/// Asleep longer than this, it is only not seen (taken away, run down).
const ASLEEP_FOR_S: i64 = 24 * 3600;

impl LastSeen {
    fn path() -> std::path::PathBuf {
        glib::user_data_dir().join("itemgrid/last-seen")
    }

    fn save(&self) {
        let text = format!("name={}\nbattery={}\nat={}\nshut={}\ndroidian={}\n", self.name, self.battery.map_or(String::new(), |b| b.to_string()), self.at, self.shut as u8, self.droidian as u8);
        let _ = std::fs::create_dir_all(Self::path().parent().unwrap_or(std::path::Path::new(".")));
        let _ = std::fs::write(Self::path(), text);
    }

    fn read() -> Option<LastSeen> {
        let text = std::fs::read_to_string(Self::path()).ok()?;
        let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(&format!("{k}=")).map(str::to_owned));
        Some(LastSeen { name: get("name")?, battery: get("battery").and_then(|b| b.parse().ok()), at: get("at")?.parse().ok()?, shut: get("shut")? == "1", droidian: get("droidian").is_some_and(|d| d == "1") })
    }

    /// Shut when last seen, and not too long ago.
    fn asleep(&self) -> bool {
        self.shut && glib::DateTime::now_local().map_or(0, |t| t.to_unix()) - self.at < ASLEEP_FOR_S
    }

    /// Its words on the table: its name, asleep since when (today the time,
    /// else the day), its charge.
    fn words(&self) -> Vec<String> {
        let when = glib::DateTime::from_unix_local(self.at).ok();
        let today = glib::DateTime::now_local().ok().map(|n| (n.year(), n.day_of_year()));
        let since = when.and_then(|w| if Some((w.year(), w.day_of_year())) == today { w.format("%H:%M").ok() } else { w.format("%-d %b").ok() }).map(|s| s.to_lowercase()).unwrap_or_default();
        [self.name.clone(), format!("asleep {since}").trim().to_owned(), self.battery.map_or(String::new(), |b| format!("{b}%"))].into_iter().filter(|l| !l.is_empty()).collect()
    }
}

/// The storage bar's parts counted again, and the bar and its legend shown.
fn count_storage(ui: &Rc<Ui>) {
    let Some(host) = ui.state.borrow().host.clone() else { return };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let Ok(Ok(parts)) = gio::spawn_blocking(move || itemgrid_core::storage::read(&host)).await else { return };
        while let Some(child) = ui.legend.first_child() {
            ui.legend.remove(&child);
        }
        for (i, (name, kib)) in parts.list().into_iter().enumerate() {
            let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            dot.add_css_class("storage-legend-dot");
            dot.set_valign(gtk::Align::Center);
            if let Some((r, g, b)) = PART_COLOURS.get(i) {
                let css = gtk::CssProvider::new();
                css.load_from_string(&format!("box {{ background: rgb({}, {}, {}); }}", (r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8));
                #[allow(deprecated)]
                dot.style_context().add_provider(&css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
            } else {
                dot.add_css_class("dot-free");
            }
            ui.legend.append(&dot);
            ui.legend.append(&gtk::Label::builder().label(name).build());
            ui.legend.append(&gtk::Label::builder().label(status::size_words(kib)).css_classes(["dim-label"]).margin_end(14).build());
        }
        if let Some((_, kib)) = parts.list().into_iter().find(|(n, _)| *n == "Free") {
            ui.free_label.set_label(&format!("{} free", status::size_words(kib)));
        }
        sections::fill_parts(&ui.sections, &parts, &PART_COLOURS);
        *ui.parts.borrow_mut() = Some(parts);
        ui.storage.queue_draw();
    });
}

/// The backups on this computer under Backups: newest first, a star to keep
/// one for good, its folder; the device data asks to be copied elsewhere
/// until it is.
fn show_backups(ui: &Rc<Ui>) {
    use itemgrid_core::backup::{self, Kind};
    let pkgs = itemgrid_core::stock::packages();
    ui.stock_line.set_label(&match pkgs.last() {
        Some(p) => format!("Microsoft's Android {} (security patch {}) is on this computer.", p.build, p.security_patch),
        None => "Microsoft's package is not on this computer yet.".to_owned(),
    });
    let all = backup::list(None);
    // On Repair's Developer page and under Updates & Backups.
    for list in [&ui.backups, &ui.sections.backups] {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    list.set_visible(!all.is_empty());
    // The device data's one, and the newest few of the rest.
    let device = all.iter().find(|b| b.manifest.kind == Kind::Device).cloned();
    let rest: Vec<_> = all.iter().filter(|b| b.manifest.kind != Kind::Device).take(6).cloned().collect();
    for b in device.into_iter().chain(rest) {
        let when = b.manifest.created.get(..16).unwrap_or(&b.manifest.created).to_owned();
        let row = adw::ActionRow::builder()
            .title(b.manifest.kind.words())
            .subtitle(format!("{when} · {} · item {}", status::size_words(b.size() / 1024), b.manifest.item))
            .build();
        if b.manifest.kind == Kind::Device {
            let elsewhere = gtk::CheckButton::with_label("Copied elsewhere");
            elsewhere.set_active(b.manifest.off_computer);
            elsewhere.set_tooltip_text(Some("This exists only on the phone and here: keep a copy on a USB drive or in a cloud too"));
            if !b.manifest.off_computer {
                row.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
                row.set_subtitle(&format!("{when} · only here and on the phone - copy it elsewhere"));
            }
            let held = RefCell::new(b.clone());
            let ui2 = ui.clone();
            elsewhere.connect_toggled(move |c| {
                let _ = held.borrow_mut().set_off_computer(c.is_active());
                show_backups(&ui2);
            });
            row.add_suffix(&elsewhere);
        } else {
            let star = gtk::ToggleButton::builder().icon_name(if b.manifest.keep { "starred-symbolic" } else { "non-starred-symbolic" }).active(b.manifest.keep).valign(gtk::Align::Center).css_classes(["flat"]).build();
            star.set_tooltip_text(Some("Keep for good: never removed to make room"));
            let held = RefCell::new(b.clone());
            star.connect_toggled(move |t| {
                let _ = held.borrow_mut().set_keep(t.is_active());
                t.set_icon_name(if t.is_active() { "starred-symbolic" } else { "non-starred-symbolic" });
            });
            row.add_suffix(&star);
        }
        let open = gtk::Button::builder().icon_name("folder-open-symbolic").valign(gtk::Align::Center).css_classes(["flat"]).tooltip_text("Show in Files").build();
        let dir = b.dir.clone();
        open.connect_clicked(move |_| {
            let _ = gio::AppInfo::launch_default_for_uri(&gio::File::for_path(&dir).uri(), gio::AppLaunchContext::NONE);
        });
        row.add_suffix(&open);
        list.append(&row);
    }
    }
}

/// The slots and the RAM boot gate, read off the main thread.
fn show_slots(ui: &Rc<Ui>) {
    let Some(host) = ui.state.borrow().host.clone() else { return };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let read = gio::spawn_blocking(move || {
            let slots = itemgrid_core::slots::read(&host)?;
            let gate = itemgrid_core::backup::serial(&host).map(|s| itemgrid_core::flash::gate(&s)).ok();
            Ok::<_, String>((slots, gate))
        })
        .await
        .unwrap_or_else(|_| Err("the work stopped".into()));
        while let Some(child) = ui.slots.first_child() {
            ui.slots.remove(&child);
        }
        let (slots, gate) = match read {
            Ok(r) => r,
            Err(e) => {
                ui.slots.append(&adw::ActionRow::builder().title("The slots were not read").subtitle(e).build());
                return;
            }
        };
        for s in &slots {
            let mut state = Vec::new();
            if s.active {
                state.push("active".to_owned());
            }
            state.push(if s.successful { "booted fine".into() } else { "never booted".into() });
            if s.unbootable {
                state.push("marked unbootable".into());
            }
            state.push(format!("{} tries left", s.retries));
            let what = s.image.clone().unwrap_or_else(|| "an image not known here".into());
            let kernel = if s.kernel.is_empty() { String::new() } else { format!(" · Linux {}", s.kernel) };
            let row = adw::ActionRow::builder()
                .title(format!("Slot {} · {}", s.name.to_ascii_uppercase(), state.join(", ")))
                .subtitle(format!("{what}{kernel}"))
                .build();
            let icon = if s.active { "emblem-ok-symbolic" } else if s.unbootable { "dialog-warning-symbolic" } else { "media-record-symbolic" };
            row.add_prefix(&gtk::Image::from_icon_name(icon));
            ui.slots.append(&row);
        }
        if let Some(g) = gate {
            let max = itemgrid_core::flash::MAX_UNCONFIRMED;
            let row = adw::ActionRow::builder()
                .title(format!("RAM boots: {} of {max} unconfirmed", g.unconfirmed))
                .subtitle(if g.open() { "The gate is open: an image can be tried from RAM." } else { "The gate is closed: a good boot must be confirmed, or the counter reset on purpose." })
                .build();
            row.add_prefix(&gtk::Image::from_icon_name(if g.open() { "changes-allow-symbolic" } else { "changes-prevent-symbolic" }));
            ui.slots.append(&row);
        }
    });
}

/// The bottom bar on General only, with the phone there.
fn bottom_shown(ui: &Ui) {
    let general = ui.tabs.visible_child_name().as_deref() == Some("general");
    // The status is said on the table now (show_fold): the bar stays hidden.
    let _ = general;
    ui.bottom.set_visible(false);
}

/// The live view on while the window is in front on General with the phone
/// there, off otherwise.
fn live_sync(ui: &Rc<Ui>) {
    let host = ui.state.borrow().host.clone();
    // Kept while the window is behind others: each start and stop of it
    // made item read the screen anew (item crashed in the GPU driver there
    // on 2026-10-05, since fixed).
    // On the cable in the simple window too (the crash is fixed: item drew
    // the mirror's frame by a scaled blit); over Wi-Fi only in Developer
    // Mode - ten frames a second are some 8 MB/s of the phone's radio.
    let cable = host.as_deref().is_some_and(|h| itemgrid_core::link::Via::of(h) == itemgrid_core::link::Via::Cable);
    let wanted = (developer_mode() || cable) && host.is_some() && !ui.state.borrow().busy && ui.tabs.visible_child_name().as_deref() == Some("general");
    if !wanted {
        if let Some(stop) = ui.live.borrow_mut().take() {
            stop.stop();
        }
        ui.live_badge.set_visible(false);
        return;
    }
    let resting = ui.live_failed.borrow().is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(30));
    if ui.live.borrow().is_some() || resting {
        return;
    }
    // ssh starts at once; only the reading waits, off the main thread.
    let (mut live, stop) = match itemgrid_core::live::Live::start(&host.expect("wanted")) {
        Ok(started) => started,
        Err(_) => {
            *ui.live_failed.borrow_mut() = Some(std::time::Instant::now());
            return;
        }
    };
    *ui.live.borrow_mut() = Some(stop.clone());
    let (tx, rx) = async_channel::bounded::<itemgrid_core::live::Frame>(2);
    let work = gio::spawn_blocking(move || loop {
        let Ok(frame) = live.next() else { return };
        if tx.send_blocking(frame).is_err() {
            return;
        }
    });
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let mut frames = 0u64;
        while let Ok(frame) = rx.recv().await {
            for (picture, pixels) in ui.screens.iter().zip(frame.panels) {
                let texture = gdk::MemoryTexture::new(frame.width as i32, frame.height as i32, gdk::MemoryFormat::R8g8b8a8, &glib::Bytes::from_owned(pixels), frame.width * 4);
                picture.set_paintable(Some(&texture));
            }
            frames += 1;
            if frames == 1 {
                ui.live_badge.set_visible(developer_mode());
            }
        }
        let _ = work.await;
        // This view's end; a newer one may have started meanwhile.
        let mine = ui.live.borrow().as_ref().is_some_and(|s| s.same(&stop));
        if mine {
            ui.live.borrow_mut().take();
            ui.live_badge.set_visible(false);
            // Ended by itself (no mirror in this item, item gone): rest.
            *ui.live_failed.borrow_mut() = Some(std::time::Instant::now());
            if frames == 0 {
                ui.toasts.add_toast(adw::Toast::new("No live view: this item has no mirror yet - update item"));
            }
        }
    });
}

/// The phone's screens onto the Duo drawn here; saved too if `save`.
fn take_screens(ui: &Rc<Ui>, save: bool) {
    let Some(host) = ui.state.borrow().host.clone() else { return };
    let ui = ui.clone();
    let began = std::time::Instant::now();
    glib::spawn_future_local(async move {
        let got = gio::spawn_blocking(move || -> Result<([Vec<u8>; 2], (usize, usize), Option<std::path::PathBuf>), String> {
            // For the drawn Duo a third of each side is plenty (made small on
            // the phone: over Wi-Fi the whole frame took ~3 s); kept, whole.
            if !save {
                let (rgba, w, h) = screenshot::take_small(&host, 3)?;
                if w == screenshot::SCREEN_W {
                    return Ok((screenshot::panels(&rgba), screenshot::PANEL_SIZE, None));
                }
                let (panels, pw) = screenshot::panels_small(&rgba, w, h);
                return Ok((panels, (pw, h), None));
            }
            let rgba = screenshot::take(&host)?;
            let saved = if save {
                let png = screenshot::png(&rgba, false)?;
                let dir = shots_dir();
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let stamp = glib::DateTime::now_local().and_then(|d| d.format("%Y-%m-%d-%H%M%S")).map(|s| s.to_string()).unwrap_or_default();
                let path = dir.join(format!("{stamp}.png"));
                std::fs::write(&path, png).map_err(|e| e.to_string())?;
                Some(path)
            } else {
                None
            };
            Ok((screenshot::panels(&rgba), screenshot::PANEL_SIZE, saved))
        })
        .await
        .unwrap_or_else(|_| Err("the work stopped".into()));
        match got {
            Ok((panels, (w, h), saved)) => {
                trace(format_args!("screens: {w}x{h} a panel in {:.2} s", began.elapsed().as_secs_f64()));
                for (picture, pixels) in ui.screens.iter().zip(panels) {
                    let texture = gdk::MemoryTexture::new(w as i32, h as i32, gdk::MemoryFormat::R8g8b8a8, &glib::Bytes::from_owned(pixels), w * 4);
                    picture.set_paintable(Some(&texture));
                }
                if let Some(path) = saved {
                    ui.toasts.add_toast(adw::Toast::new(&format!("Saved {}", path.file_name().and_then(|n| n.to_str()).unwrap_or(""))));
                }
            }
            // A dark screen gives no frame: said only when asked for.
            Err(e) if save => ui.toasts.add_toast(adw::Toast::new(&e)),
            Err(_) => {}
        }
    });
}

#[derive(Clone)]
enum Job {
    Update,
    Reboot,
    Backup,
    FullBackup,
    RamBoot(std::path::PathBuf),
    Restore(Box<itemgrid_core::restore::Plan>),
    RecoveryExit(String),
    LeaveFastboot(String),
    AndroidGo(Box<itemgrid_core::android::Plan>),
    /// Back to stock Android for good (its boot on the phone again), the
    /// owner's files copied first if asked.
    AndroidClean(Box<itemgrid_core::android::CleanPlan>, bool),
    /// item put on a phone coming from stock Android (its serial).
    InstallStock(Box<itemgrid_core::install::Release>, String),
    AndroidStart(String),
    AndroidBack(String),
    /// Microsoft's package from a link, then its boot chain taken out.
    StockDownload(String, String),
    /// A release image put on the phone, userdata made anew.
    Install(Box<itemgrid_core::install::Release>, itemgrid_core::install::Mode),
}

impl Job {
    /// Its kind, for the card's stages and the record.
    fn kind(&self) -> &'static str {
        match self {
            Job::Update => "update",
            Job::Reboot => "reboot",
            Job::Backup => "backup",
            Job::FullBackup => "full-backup",
            Job::RamBoot(_) => "ramboot",
            Job::Restore(_) => "restore",
            Job::RecoveryExit(_) | Job::LeaveFastboot(_) => "recovery-exit",
            Job::AndroidGo(_) => "android-go",
            Job::AndroidClean(..) => "android-clean",
            Job::InstallStock(..) => "install-stock",
            Job::AndroidStart(_) => "android-start",
            Job::AndroidBack(_) => "android-back",
            Job::StockDownload(..) => "stock-download",
            Job::Install(_, m) => match m {
                itemgrid_core::install::Mode::Erase => "install",
                itemgrid_core::install::Mode::KeepFiles => "install-keep",
                itemgrid_core::install::Mode::FullCopy => "install-full",
            },
        }
    }
}

/// A boot chain to put back: the backup and the slot chosen, the plan made
/// off the main thread (reading only), then asked.
fn choose_restore(ui: &Rc<Ui>) {
    let serial = ui.serial.borrow().clone();
    let backups: Vec<itemgrid_core::backup::Backup> = itemgrid_core::backup::list(Some(&serial)).into_iter().filter(|b| b.manifest.kind == itemgrid_core::backup::Kind::Boot).collect();
    if backups.is_empty() {
        stopped(ui, "No boot-chain backup of this phone yet: Back Up Now makes one.");
        return;
    }
    let names: Vec<String> = backups
        .iter()
        .map(|b| format!("{} · item {} · slot {} then", b.manifest.created.get(..16).unwrap_or(&b.manifest.created), b.manifest.item, b.manifest.slot.trim_start_matches('_').to_uppercase()))
        .collect();
    let backup_pick = gtk::DropDown::from_strings(&names.iter().map(String::as_str).collect::<Vec<_>>());
    let slot_pick = gtk::DropDown::from_strings(&["Slot A", "Slot B"]);
    let fields = gtk::Box::new(gtk::Orientation::Vertical, 8);
    fields.append(&gtk::Label::builder().label("Backup").xalign(0.0).css_classes(["dim-label"]).build());
    fields.append(&backup_pick);
    fields.append(&gtk::Label::builder().label("Slot").xalign(0.0).css_classes(["dim-label"]).margin_top(6).build());
    fields.append(&slot_pick);
    let dialog = adw::AlertDialog::new(Some("Restore a boot chain"), Some("boot, dtbo and vbmeta of one slot, from a backup. The other slot is not touched. Next shows what would be written - nothing is written yet."));
    dialog.set_extra_child(Some(&fields));
    dialog.add_responses(&[("cancel", "Cancel"), ("next", "Next")]);
    dialog.set_default_response(Some("next"));
    dialog.set_close_response("cancel");
    let ui2 = ui.clone();
    dialog.connect_response(None, move |_, response| {
        if response != "next" {
            return;
        }
        let Some(host) = ui2.state.borrow().host.clone() else { return };
        let backup = backups[backup_pick.selected() as usize].clone();
        let slot = if slot_pick.selected() == 0 { 'a' } else { 'b' };
        let ui3 = ui2.clone();
        glib::spawn_future_local(async move {
            let planned = gio::spawn_blocking(move || itemgrid_core::restore::plan(&host, &backup, slot, false)).await.unwrap_or_else(|_| Err("the work stopped".into()));
            let plan = match planned {
                Ok(p) => p,
                Err(e) => {
                    stopped(&ui3, &e);
                    return;
                }
            };
            let lines: Vec<String> = plan
                .parts
                .iter()
                .map(|p| format!("{}: {}", p.partition, if p.differs { "differs - to be written" } else { "the same - left alone" }))
                .collect();
            let in_use = if plan.slot == plan.active_slot { "the slot in use: the phone restarts into it after" } else { "the spare slot: written and checked, not booted" };
            if plan.writes().is_empty() {
                stopped(&ui3, &format!("Slot {} already holds this backup - nothing to write.\n\n{}", plan.slot.to_ascii_uppercase(), lines.join("\n")));
                return;
            }
            let body = format!(
                "Slot {} - {in_use}.\n\n{}\n\nA changed boot is first booted from RAM; the chain as it is now is backed up; each partition is written and read back.",
                plan.slot.to_ascii_uppercase(),
                lines.join("\n")
            );
            let confirm = adw::AlertDialog::new(Some("Restore this boot chain?"), Some(&body));
            confirm.add_responses(&[("cancel", "Cancel"), ("go", "Restore")]);
            confirm.set_response_appearance("go", adw::ResponseAppearance::Destructive);
            confirm.set_default_response(Some("cancel"));
            confirm.set_close_response("cancel");
            let ui4 = ui3.clone();
            confirm.connect_response(None, move |_, response| {
                if response == "go" {
                    run_job(&ui4, Job::Restore(Box::new(plan.clone())));
                }
            });
            confirm.present(Some(&ui3.window));
        });
    });
    dialog.present(Some(&ui.window));
}

/// An image chosen to be tried from RAM: the checks that change nothing
/// first, off the main thread; then what they found and the plan, asked.
fn choose_ram_image(ui: &Rc<Ui>) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Boot images"));
    filter.add_pattern("*.img");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let chooser = gtk::FileDialog::builder().title("Choose a boot image to try from RAM").filters(&filters).modal(true).build();
    if let Some(out) = itemgrid_core::flash::port_tree().map(|t| t.join("out")) {
        chooser.set_initial_folder(Some(&gio::File::for_path(out)));
    }
    let ui = ui.clone();
    chooser.open(Some(&ui.window.clone()), gio::Cancellable::NONE, move |picked| {
        let Some(path) = picked.ok().and_then(|f| f.path()) else { return };
        let Some(host) = ui.state.borrow().host.clone() else { return };
        let ui = ui.clone();
        glib::spawn_future_local(async move {
            let p = path.clone();
            let checked = gio::spawn_blocking(move || {
                let (img, serial, slot) = itemgrid_core::ramboot::preflight(&host, &p)?;
                let gate = itemgrid_core::flash::gate(&serial);
                Ok::<_, String>((img, slot, gate))
            })
            .await
            .unwrap_or_else(|_| Err("the work stopped".into()));
            let (img, slot, gate) = match checked {
                Ok(c) => c,
                Err(e) => {
                    stopped(&ui, &e);
                    return;
                }
            };
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("the image").to_owned();
            let body = format!(
                "{name}\nheader v2, ARM64 kernel, DTB, Android {}, sha {}…\n\nThe phone booted from slot {}; RAM boots {} of {} unconfirmed; battery fine.\n\n\
                 In order, each a stop if it fails:\n\
                 1. the parking brake armed in misc, read back\n\
                 2. the phone into the bootloader (about a minute)\n\
                 3. in fastboot: the same phone, unlocked, the same slot, its health\n\
                 4. misc erased, the brake flashed again\n\
                 5. the attempt counted, the image booted from RAM - nothing flashed\n\
                 6. Linux awaited, the boot confirmed. Not back: stop - never the same image again.",
                img.os_version,
                &img.sha256[..16],
                slot.to_ascii_uppercase(),
                gate.unconfirmed,
                itemgrid_core::flash::MAX_UNCONFIRMED
            );
            let dialog = adw::AlertDialog::new(Some("Boot this image from RAM?"), Some(&body));
            dialog.add_responses(&[("cancel", "Cancel"), ("go", "Boot from RAM")]);
            dialog.set_response_appearance("go", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");
            let ui2 = ui.clone();
            dialog.connect_response(None, move |_, response| {
                if response == "go" {
                    run_job(&ui2, Job::RamBoot(path.clone()));
                }
            });
            dialog.present(Some(&ui.window));
        });
    });
}

/// The phone's club number, quietly (once a run per serial): claimed if
/// this computer does not know it, and written onto the phone (on Linux)
/// if it is not there. Nothing said if it cannot be had now: next run.
/// The parts as moved (E) kept in their file, said so (and in the trace).
fn keep_moved(ui: &Ui) -> String {
    let path = scene::moved_path();
    let text = ui.layout.get().write();
    let kept = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&path, text));
    let said = match kept {
        Ok(()) => format!("kept: {}", path.display()),
        Err(e) => format!("not kept: {e}"),
    };
    trace(format_args!("moved: {said}"));
    said
}

fn claim_quietly(ui: &Rc<Ui>, serial: &str, host: Option<String>) {
    claim_quietly_as(ui, serial, serial, host)
}

/// The same, once per `key` (the serial, or the serial and the phone's boot:
/// a phone installed again while this window was open gets its number
/// written onto it again - it was written once a run, 2026-10-07).
fn claim_quietly_as(ui: &Rc<Ui>, key: &str, serial: &str, host: Option<String>) {
    if serial.is_empty() || ui.claimed.borrow().iter().any(|s| s == key) {
        return;
    }
    ui.claimed.borrow_mut().push(key.to_owned());
    let (ui, serial) = (ui.clone(), serial.to_owned());
    glib::spawn_future_local(async move {
        let s = serial.clone();
        let got = gio::spawn_blocking(move || {
            let d = itemgrid_core::club::claim(&s)?;
            if let Some(h) = host {
                let _ = itemgrid_core::club::put_on_phone(&h, &d);
            }
            Ok::<_, String>(d)
        })
        .await
        .unwrap_or_else(|_| Err("the work stopped".into()));
        match got {
            Ok(d) => {
                trace(format_args!("club: {}", d.number));
                if *ui.serial.borrow() == serial {
                    ui.name.set_label(&format!("Surface Duo · {}", d.number));
                }
                show_fold(&ui, ui.fold.get().0);
            }
            Err(e) => trace(format_args!("club: not now ({e})")),
        }
    });
}

/// A stop, said so it cannot be missed.
/// The OpenRouter key asked for (pasted), checked with OpenRouter from
/// here, then put into the phone's keyring; nothing of it kept here.
fn set_agent_key(ui: &Rc<Ui>, key_row: &adw::ActionRow, key_forget: &gtk::Button) {
    let Some(host) = ui.state.borrow().host.clone() else {
        stopped(ui, "Your Duo is not here: plug it in or bring it onto the same Wi-Fi to set its key.");
        return;
    };
    let dialog = adw::AlertDialog::new(Some("OpenRouter key"), Some("Paste your key (sk-or-…) from openrouter.ai/keys. item/grid checks it with OpenRouter, then puts it into the phone's keyring; it is not kept on this computer."));
    let entry = gtk::PasswordEntry::builder().show_peek_icon(true).placeholder_text("sk-or-…").build();
    dialog.set_extra_child(Some(&entry));
    dialog.add_responses(&[("cancel", "Cancel"), ("save", "Check and Save")]);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));
    dialog.set_close_response("cancel");
    let (ui2, key_row, key_forget) = (ui.clone(), key_row.clone(), key_forget.clone());
    dialog.connect_response(None, move |_, response| {
        if response != "save" {
            return;
        }
        let key = entry.text().to_string();
        entry.set_text("");
        let (ui, key_row, key_forget, host) = (ui2.clone(), key_row.clone(), key_forget.clone(), host.clone());
        key_row.set_subtitle("Checking the key with OpenRouter…");
        glib::spawn_future_local(async move {
            let done = gio::spawn_blocking(move || {
                let info = itemgrid_core::agent::check(&key)?;
                itemgrid_core::agent::store(&host, &key)?;
                let last = itemgrid_core::agent::stored(&host)?.unwrap_or_default();
                Ok::<_, String>((info, last))
            })
            .await;
            match done {
                Ok(Ok((info, last))) => {
                    key_row.set_subtitle(&format!("Set on the phone: …{last} · {}", info.words()));
                    key_forget.set_visible(true);
                    ui.toasts.add_toast(adw::Toast::new("Key checked and saved on the phone"));
                }
                Ok(Err(e)) => {
                    key_row.set_subtitle(&format!("Not saved: {e}"));
                    stopped(&ui, &format!("The key was not saved: {e}"));
                }
                Err(_) => {}
            }
        });
    });
    dialog.present(Some(&ui.window));
}

fn stopped(ui: &Ui, why: &str) {
    let dialog = adw::AlertDialog::new(Some("Stopped"), Some(why));
    dialog.add_response("ok", "OK");
    dialog.present(Some(&ui.window));
}

/// Asks before an action that takes the phone away for a minute.
fn ask(ui: &Rc<Ui>, heading: &str, body: &str, yes: &str, job: Job) {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.add_responses(&[("cancel", "Cancel"), ("go", yes)]);
    dialog.set_response_appearance("go", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("cancel"));
    let ui2 = ui.clone();
    dialog.connect_response(None, move |_, response| {
        if response == "go" {
            run_job(&ui2, job.clone());
        }
    });
    dialog.present(Some(&ui.window));
}

/// A job run off the main thread, told on the card as it goes; recorded for
/// another item/grid too.
fn run_job(ui: &Rc<Ui>, job: Job) {
    let host = ui.state.borrow().host.clone();
    let needs_linux = matches!(job, Job::Update | Job::Reboot | Job::Backup | Job::FullBackup | Job::RamBoot(_) | Job::Restore(_) | Job::AndroidGo(_) | Job::AndroidClean(..) | Job::Install(..));
    if needs_linux && host.is_none() {
        stopped(ui, "The phone is not in Linux just now.");
        return;
    }
    // Where Linux will answer, for the jobs that end there.
    let host = host.or_else(|| itemgrid_core::phone::hosts().into_iter().next()).unwrap_or_default();
    let kind = job.kind();
    {
        let mut st = ui.state.borrow_mut();
        st.busy = true;
        st.job = Some(OwnJob { kind, lines: Vec::new(), started: std::time::Instant::now(), ended: None, took: None });
    }
    if let Some(stop) = ui.live.borrow_mut().take() {
        stop.stop();
    }
    ui.actions.set_sensitive(false);
    ui.mode_buttons.set_sensitive(false);
    tell(ui);
    let (tx, rx) = async_channel::unbounded::<String>();
    let work = gio::spawn_blocking(move || {
        itemgrid_core::activity::begin(kind);
        let mut say = |words: String| {
            itemgrid_core::activity::line(&words);
            let _ = tx.send_blocking(words);
        };
        let result = match job {
            Job::Update => itemgrid_core::update::update(&host, true, &mut |step| say(step.words().to_owned())),
            Job::Reboot => itemgrid_core::phone::reboot(&host, &mut |b| say(b.words().to_owned())),
            Job::Restore(plan) => itemgrid_core::restore::restore(&host, &plan, &mut say),
            Job::RamBoot(path) => itemgrid_core::ramboot::ram_boot(&host, &path, itemgrid_core::ramboot::Expect::of(&path), &mut say),
            Job::FullBackup => itemgrid_core::full::take(&host, &mut say).map(|_| ()),
            Job::RecoveryExit(serial) => itemgrid_core::ramboot::leave_recovery(&host, &serial, &mut say),
            Job::LeaveFastboot(serial) => itemgrid_core::ramboot::leave_fastboot(&host, &serial, &mut say),
            Job::AndroidGo(plan) => {
                let word = itemgrid_core::backup::serial(&host).map(|s| itemgrid_core::android::confirm_word(&s)).unwrap_or_default();
                // The number was typed in the window already; the losses shown.
                itemgrid_core::android::go(&host, &plan, &word, true, &mut say)
            }
            Job::AndroidClean(plan, copy_first) => (|| {
                let word = itemgrid_core::backup::serial(&host).map(|s| itemgrid_core::android::confirm_word(&s))?;
                if copy_first {
                    say("copying your files to this computer".into());
                    itemgrid_core::backup::take(&host, itemgrid_core::backup::Kind::Quick, &mut say)?;
                }
                // The number was typed on the table already.
                itemgrid_core::android::go_clean(&host, &plan, &word, &mut say)
            })(),
            Job::InstallStock(release, serial) => {
                // The number was typed on the table already.
                itemgrid_core::install::from_stock(&serial, &release, &itemgrid_core::android::confirm_word(&serial), &mut say)
            }
            Job::AndroidStart(serial) => itemgrid_core::android::start(&host, &serial, &mut say),
            Job::AndroidBack(serial) => itemgrid_core::android::back(&host, &serial, false, &mut say),
            Job::Install(release, mode) => {
                let word = itemgrid_core::backup::serial(&host).map(|s| itemgrid_core::android::confirm_word(&s)).unwrap_or_default();
                // The number was typed in the window already.
                itemgrid_core::install::erase_and_install(&host, &release, mode, &word, &mut say)
            }
            Job::StockDownload(url, label) => (|| {
                say(format!("downloading {label}"));
                let pkg = itemgrid_core::stock::download(&url, &mut |done, whole| say(format!("  downloaded: {} of {} MB", done >> 20, whole >> 20)))?;
                itemgrid_core::stock::boot_chain(&pkg, &mut say)?;
                Ok(())
            })(),
            Job::Backup => (|| {
                use itemgrid_core::backup::{self, Kind};
                // The device data once; the boot chain and home each time.
                let serial = backup::serial(&host)?;
                let mut kinds = Vec::new();
                if !backup::has_device_data(&serial) {
                    kinds.push(Kind::Device);
                }
                kinds.extend([Kind::Boot, Kind::Quick]);
                for kind in kinds {
                    backup::take(&host, kind, &mut say)?;
                }
                Ok(())
            })(),
        };
        itemgrid_core::activity::end(&result);
        result
    });
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        while let Ok(line) = rx.recv().await {
            if let Some(job) = ui.state.borrow_mut().job.as_mut() {
                job.lines.push(line);
            }
            tell(&ui);
        }
        let result = work.await.unwrap_or_else(|_| Err("the work stopped".into()));
        {
            let mut st = ui.state.borrow_mut();
            st.busy = false;
            st.pictured = false;
            // The phone may be anywhere now: looked for afresh, not "gone".
            st.last_seen = Some(std::time::Instant::now());
            if let Some(job) = st.job.as_mut() {
                job.ended = Some(result.err());
                job.took = Some(job.started.elapsed().as_secs());
            }
            // This window's own record is not "elsewhere".
            st.dismissed = itemgrid_core::activity::elsewhere(u64::MAX).and_then(|a| a.ended_at).or(st.dismissed);
        }
        tell(&ui);
        ui.actions.set_sensitive(true);
        ui.mode_buttons.set_sensitive(true);
        show_backups(&ui);
        show_slots(&ui);
        look(&ui);
    });
}

/// Microsoft's package for this Duo: Microsoft's page in a window of its own
/// (its sign-in kept for next time); once signed in, item/grid asks for the
/// Duo by its serial, takes the link from the answer, and downloads it as a
/// job on the card.
fn get_android_from_microsoft(ui: &Rc<Ui>) {
    use webkit::prelude::*;
    let serial = {
        let s = ui.serial.borrow().clone();
        if s.is_empty() { ui.state.borrow().place.serial().unwrap_or_default().to_owned() } else { s }
    };
    if serial.is_empty() {
        stopped(ui, "item/grid needs the phone connected to know its serial number.");
        return;
    }
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let session = webkit::NetworkSession::new(
        home.join(".local/share/itemgrid/web").to_str(),
        home.join(".cache/itemgrid/web").to_str(),
    );
    if let Some(cookies) = session.cookie_manager() {
        cookies.set_persistent_storage(home.join(".local/share/itemgrid/web/cookies.sqlite").to_str().unwrap_or_default(), webkit::CookiePersistentStorage::Sqlite);
    }
    let web = webkit::WebView::builder().network_session(&session).vexpand(true).hexpand(true).build();
    let note = gtk::Label::builder()
        .label("item/grid fetches Android with Microsoft's own page. Sign in with your Microsoft account once - item/grid does the rest and remembers the sign-in.")
        .wrap(true)
        .xalign(0.0)
        .margin_start(16)
        .margin_end(16)
        .margin_top(10)
        .margin_bottom(10)
        .css_classes(["dim-label"])
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&note);
    content.append(&web);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&content));
    let dialog = adw::Dialog::builder().title("Android from Microsoft").content_width(960).content_height(720).child(&view).build();
    let asked = Rc::new(std::cell::Cell::new(false));
    web.connect_load_changed({
        let ui = ui.clone();
        let dialog = dialog.clone();
        let note = note.clone();
        let serial = serial.clone();
        move |web, event| {
            if event != webkit::LoadEvent::Finished || asked.get() {
                return;
            }
            let on_page = web.uri().is_some_and(|u| u.starts_with(itemgrid_core::stock::RECOVERY_PAGE));
            if !on_page {
                note.set_label("Sign in with your Microsoft account. item/grid goes on by itself after that.");
                return;
            }
            // Signed in, the page has the form: ask for this Duo with it.
            let body = r#"
                const input = document.querySelector('input[name="ProductSerial"]');
                if (!input || !input.form) return "sign-in";
                const data = new FormData(input.form);
                data.set("ProductName", "Surface Duo");
                data.set("ProductSerial", serial);
                const answer = await fetch(location.pathname, { method: "POST", body: data, credentials: "same-origin" });
                return await answer.text();
            "#;
            let args = glib::VariantDict::new(None);
            args.insert("serial", &serial);
            let (ui, dialog, note, asked) = (ui.clone(), dialog.clone(), note.clone(), asked.clone());
            note.set_label("Asking Microsoft for this Duo's package…");
            web.call_async_javascript_function(body, Some(&args.end()), None, None, gio::Cancellable::NONE, move |result| {
                let text = result.ok().map(|v| v.to_str().to_string()).unwrap_or_default();
                if text == "sign-in" || text.is_empty() {
                    note.set_label("Sign in with your Microsoft account (Sign In on the page). item/grid goes on by itself after that.");
                    return;
                }
                match itemgrid_core::stock::link_in(&text) {
                    Some((url, label)) => {
                        asked.set(true);
                        dialog.close();
                        run_job(&ui, Job::StockDownload(url, label));
                    }
                    None => note.set_label("Microsoft offered no Android package for this serial number. Is it a Surface Duo (1st gen)?"),
                }
            });
        }
    });
    web.connect_decide_policy(|_, decision, kind| {
        use webkit::prelude::*;
        if !matches!(kind, webkit::PolicyDecisionType::NavigationAction | webkit::PolicyDecisionType::NewWindowAction) {
            return false;
        }
        let uri = decision
            .downcast_ref::<webkit::NavigationPolicyDecision>()
            .and_then(|d| d.navigation_action())
            .and_then(|mut a| a.request())
            .and_then(|r| r.uri())
            .map(|u| u.to_string())
            .unwrap_or_default();
        if microsoft_only(&uri) {
            false
        } else {
            decision.ignore();
            true
        }
    });
    web.load_uri(itemgrid_core::stock::RECOVERY_PAGE);
    dialog.present(Some(&ui.window));
}

/// The Microsoft window goes only where signing in and the support page
/// need it.
fn microsoft_only(uri: &str) -> bool {
    let Some(rest) = uri.strip_prefix("https://") else { return uri == "about:blank" };
    let host = rest.split(['/', '?', '#', ':']).next().unwrap_or("").to_ascii_lowercase();
    ["microsoft.com", "live.com", "microsoftonline.com", "msauth.net", "msftauth.net", "msidentity.com", "office.com", "aka.ms"]
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

/// Erase and install: the newest release image, what goes and what stays
/// told plainly, the phone's number typed before anything is erased.
fn erase_and_install(ui: &Rc<Ui>) {
    let Some(host) = ui.state.borrow().host.clone() else { return };
    let Some(release) = itemgrid_core::install::releases().pop() else {
        stopped(ui, "No release image on this computer yet (the port's tools/build-release-image.sh makes one).");
        return;
    };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let h = host.clone();
        let read = gio::spawn_blocking(move || {
            let serial = itemgrid_core::backup::serial(&h)?;
            let fresh = itemgrid_core::android::fresh_full(&h, &serial)?.is_some();
            Ok::<_, String>((itemgrid_core::android::confirm_word(&serial), fresh))
        })
        .await
        .unwrap_or_else(|_| Err("the work stopped".into()));
        let (word, fresh) = match read {
            Ok(r) => r,
            Err(e) => {
                stopped(&ui, &e);
                return;
            }
        };
        let _ = fresh;
        let body = format!(
            "{} - item {}, the port {}.\n\n\
             The recovery starts, the way in is tested (nothing is erased if it fails), the phone's data partition is made anew and the new system written in checked parts; its first start grows it to fill the phone. The device data, the boot chain and the unlocked bootloader are not touched.\n\n\
             Type {word} - this Duo's number - to go on.",
            release.name, release.item, release.adaptation,
        );
        let erase = gtk::CheckButton::builder().label("Erase everything - about 10 minutes").active(true).build();
        let keep = gtk::CheckButton::builder().label("Keep my files, Wi-Fi networks and PIN - about 15 minutes").group(&erase).build();
        let full = gtk::CheckButton::builder().label("Keep a full copy of this system on the computer - about 35 minutes").group(&erase).build();
        let choices = gtk::Box::new(gtk::Orientation::Vertical, 6);
        choices.append(&erase);
        choices.append(&keep);
        choices.append(&full);
        let entry = gtk::Entry::builder().placeholder_text(word.as_str()).input_purpose(gtk::InputPurpose::Digits).build();
        let dialog = adw::AlertDialog::new(Some("Erase and install item?"), Some(&body));
        let extra = gtk::Box::new(gtk::Orientation::Vertical, 12);
        extra.append(&choices);
        extra.append(&entry);
        dialog.set_extra_child(Some(&extra));
        dialog.add_responses(&[("cancel", "Cancel"), ("go", "Erase and Install")]);
        dialog.set_response_appearance("go", adw::ResponseAppearance::Destructive);
        dialog.set_response_enabled("go", false);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        entry.connect_changed({
            let dialog = dialog.clone();
            let word = word.clone();
            move |e| dialog.set_response_enabled("go", e.text().trim() == word)
        });
        let ui2 = ui.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "go" {
                let mode = if keep.is_active() {
                    itemgrid_core::install::Mode::KeepFiles
                } else if full.is_active() {
                    itemgrid_core::install::Mode::FullCopy
                } else {
                    itemgrid_core::install::Mode::Erase
                };
                run_job(&ui2, Job::Install(Box::new(release.clone()), mode));
            }
        });
        dialog.present(Some(&ui.window));
    });
}

/// Return to Android: the plan read off the main thread, then told plainly -
/// what happens, what it needs, what is lost - and the phone's number typed
/// before anything is erased.
fn return_to_android(ui: &Rc<Ui>) {
    let Some(host) = ui.state.borrow().host.clone() else { return };
    let toast = adw::Toast::builder().title("Checking what the return needs…").timeout(3).build();
    ui.toasts.add_toast(toast);
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let h = host.clone();
        let read = gio::spawn_blocking(move || {
            let plan = itemgrid_core::android::plan(&h)?;
            let serial = itemgrid_core::backup::serial(&h)?;
            Ok::<_, String>((plan, itemgrid_core::android::confirm_word(&serial)))
        })
        .await
        .unwrap_or_else(|_| Err("the work stopped".into()));
        let (plan, word) = match read {
            Ok(r) => r,
            Err(e) => {
                stopped(&ui, &e);
                return;
            }
        };
        if !plan.stops.is_empty() {
            stopped(&ui, &format!("Android cannot come back just now:\n\n• {}", plan.stops.join("\n• ")));
            return;
        }
        let build = plan.kernel.as_ref().map(|(_, b)| b.fingerprint.split('/').nth(3).unwrap_or(&b.fingerprint).to_owned()).unwrap_or_default();
        let backup_line = if plan.full_fresh { "1. The whole-system backup taken a moment ago is checked (a few minutes)." } else { "1. Everything is backed up to this computer (about 20 minutes)." };
        let mut body = format!(
            "Stock Android {build} comes back for a while. In order, each a stop if it fails:\n\n\
             {backup_line}\n\
             2. The recovery starts, and the way back is tested - nothing is erased if it fails.\n\
             3. Linux's data on the phone is erased (about 8 minutes).\n\
             4. Android starts from the computer's memory and opens its welcome screens.\n\n\
             About {} minutes in all; keep the cable in. Back to Linux, here in item/grid, puts everything back.",
            if plan.full_fresh { 15 } else { 35 }
        );
        if !plan.losses.is_empty() {
            let lost: Vec<String> = plan.losses.iter().map(|(n, b)| format!("{n} ({})", status::size_words(b / 1024))).collect();
            body.push_str(&format!("\n\nNot in any backup, and lost: {}.", lost.join(", ")));
        }
        body.push_str(&format!("\n\nType {word} - this Duo's number - to go on."));
        let entry = gtk::Entry::builder().placeholder_text(word.as_str()).input_purpose(gtk::InputPurpose::Digits).build();
        let dialog = adw::AlertDialog::new(Some("Return to Android?"), Some(&body));
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", "Cancel"), ("go", "Erase and Return")]);
        dialog.set_response_appearance("go", adw::ResponseAppearance::Destructive);
        dialog.set_response_enabled("go", false);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        entry.connect_changed({
            let dialog = dialog.clone();
            let word = word.clone();
            move |e| dialog.set_response_enabled("go", e.text().trim() == word)
        });
        let ui2 = ui.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "go" {
                run_job(&ui2, Job::AndroidGo(Box::new(plan.clone())));
            }
        });
        dialog.present(Some(&ui.window));
    });
}
/// The Logs tab: a part, a search, this boot or the one before; read through
/// the window's phone once it is known (`owner`).
fn logs_view(owner: Rc<RefCell<Option<Rc<Ui>>>>) -> gtk::Box {
    let parts = ["All", "item", "sensorfw", "kernel", "posture", "pen"];
    let part = gtk::DropDown::from_strings(&parts);
    let search = gtk::SearchEntry::builder().placeholder_text("Search the journal").hexpand(true).build();
    let previous = gtk::ToggleButton::with_label("Previous boot");
    let reload = gtk::Button::from_icon_name("view-refresh-symbolic");
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bar.set_margin_top(12);
    bar.set_margin_bottom(8);
    bar.set_margin_start(16);
    bar.set_margin_end(16);
    bar.append(&part);
    bar.append(&search);
    bar.append(&previous);
    bar.append(&reload);
    let text = gtk::TextView::builder().editable(false).monospace(true).wrap_mode(gtk::WrapMode::WordChar).left_margin(16).right_margin(16).top_margin(8).bottom_margin(8).build();
    let scroll = gtk::ScrolledWindow::builder().child(&text).vexpand(true).build();
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.append(&bar);
    page.append(&scroll);

    let load = Rc::new({
        let (part, search, previous) = (part.clone(), search.clone(), previous.clone());
        move || {
            let Some(host) = owner.borrow().as_ref().and_then(|ui| ui.state.borrow().host.clone()) else { return };
            let q = itemgrid_core::logs::Query {
                boot: if previous.is_active() { -1 } else { 0 },
                only: match part.selected() {
                    0 => None,
                    i => Some(parts[i as usize].to_owned()),
                },
                grep: Some(search.text().to_string()).filter(|s| !s.is_empty()),
                lines: Some(500),
                ..Default::default()
            };
            let (text, scroll) = (text.clone(), scroll.clone());
            glib::spawn_future_local(async move {
                let out = gio::spawn_blocking(move || itemgrid_core::phone::run(&host, &q.script())).await.unwrap_or_else(|_| Err("the work stopped".into()));
                let body = match out {
                    Ok(t) if t.trim().is_empty() => "Nothing here.".to_owned(),
                    Ok(t) => t,
                    Err(e) => e,
                };
                text.buffer().set_text(&body);
                // The newest at the bottom, in view.
                glib::idle_add_local_once(move || {
                    let adj = scroll.vadjustment();
                    adj.set_value(adj.upper());
                });
            });
        }
    });
    part.connect_selected_notify({
        let load = load.clone();
        move |_| load()
    });
    previous.connect_toggled({
        let load = load.clone();
        move |_| load()
    });
    search.connect_activate({
        let load = load.clone();
        move |_| load()
    });
    reload.connect_clicked({
        let load = load.clone();
        move |_| load()
    });
    // Read when the tab is first shown.
    page.connect_map(move |_| load());
    page
}
