//! The layout editor (F2): the frames as they are seen, step by step, laid
//! out by hand - for now, until the release takes the owner's layout in.
//!
//! The frame is shown as large as it fits beside the editor's bar (the
//! steps, over the page's top) and panel (the step's eye, the part chosen,
//! the looks: at the right), at the frame's own 1000×800 (scene's REF)
//! scaled. It holds still on the step chosen; nothing in it acts as the
//! app does (a click on the table chooses a part, it does not look for the
//! phone); the phone's words are a sample's.
//!
//! What is changed is the draft (written as it changes: nothing lost if
//! the window goes); ok keeps it as the layout, cancel goes back to the
//! layout kept. Ctrl+Z, Ctrl+Shift+Z: a change undone, done again.

use std::cell::{Cell, RefCell};
use std::time::Instant;

use adw::prelude::*;
use gtk::gdk;

use super::{board, close_boards, intro, menu_lines, set_style, settings_lines, show_fold, square, table_under, trace, Ui, DUO_PX_PER_MM};
use crate::scene::{self, Layout, Shot, FONTS, INKS, PAPERS, PARTS, STEPS, WIDTHS};

/// The frame's own size: laid out at this, shown scaled to the page (or to
/// what the editor leaves of it).
pub const REF: (f32, f32) = (1000.0, 800.0);
/// The editor's bar and panel (px), and the room round the frame.
const BAR_H: f32 = 56.0;
const PANEL_W: f32 = 300.0;
const ROOM: f32 = 24.0;

/// What is being dragged: a part (from its place, the table's point under
/// the pointer then, where the pointer was), or the step's eye (from where
/// it was moved; the table's px a page's px then).
#[derive(Clone, Copy, Debug)]
enum Drag {
    Part { i: usize, from: (f32, f32), began: (f32, f32), at: (f64, f64) },
    Eye { step: usize, from: (f32, f32), per: (f32, f32) },
}

pub struct Editor {
    pub bar: gtk::Box,
    pub panel: gtk::Box,
    steps: Vec<gtk::ToggleButton>,
    state: gtk::Label,
    play: gtk::Button,
    undo_button: gtk::Button,
    redo_button: gtk::Button,
    ok: gtk::Button,
    cancel: gtk::Button,
    step_title: gtk::Label,
    zoom: gtk::SpinButton,
    pan: [gtk::SpinButton; 2],
    top: gtk::SpinButton,
    reset_eye: gtk::Button,
    part_title: gtk::Label,
    part_box: gtk::Box,
    part_at: [gtk::SpinButton; 2],
    size_row: gtk::Box,
    size: gtk::SpinButton,
    looks: Vec<gtk::DropDown>,
    /// The step shown (held there; none: as it goes), the part chosen and
    /// the one under the pointer.
    pub preview: Cell<Option<usize>>,
    pub selected: Cell<Option<usize>>,
    pub hover: Cell<Option<usize>>,
    drag: Cell<Option<Drag>>,
    /// The layout kept (ok keeps the draft as it; cancel goes back to it).
    saved: Cell<Layout>,
    undo: RefCell<Vec<Layout>>,
    redo: RefCell<Vec<Layout>>,
    /// The last change (its kind, when): a run of one kind (the wheel's
    /// notches, a spin held) undone at once.
    last: Cell<Option<(&'static str, Instant)>>,
    /// The panel being set from the layout (its own signals let be).
    syncing: Cell<bool>,
}

fn spin(lo: f64, hi: f64, step: f64, digits: u32) -> gtk::SpinButton {
    let s = gtk::SpinButton::with_range(lo, hi, step);
    s.set_digits(digits);
    s.set_width_chars(6);
    s
}

fn row(label: &str, widgets: &[&gtk::Widget]) -> gtk::Box {
    let r = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    r.append(&gtk::Label::builder().label(label).xalign(0.0).width_chars(9).css_classes(["dim-label"]).build());
    for w in widgets {
        r.append(*w);
    }
    r
}

fn heading(text: &str) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(0.0).css_classes(["heading"]).margin_top(10).build()
}

/// The editor's bar and panel (hidden until F2).
pub fn build() -> Editor {
    let bar = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).spacing(2).valign(gtk::Align::Start).height_request(BAR_H as i32).visible(false).css_classes(["editor-bar"]).build();
    let steps: Vec<gtk::ToggleButton> = STEPS
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let b = gtk::ToggleButton::builder().label(format!("{} {name}", i + 1)).css_classes(["flat"]).valign(gtk::Align::Center).build();
            bar.append(&b);
            b
        })
        .collect();
    let gap = || gtk::Box::builder().hexpand(true).build();
    let play = gtk::Button::builder().label("▶ play").css_classes(["flat"]).valign(gtk::Align::Center).tooltip_text("The start played from the beginning, each step's frame as laid").build();
    let undo_button = gtk::Button::builder().icon_name("edit-undo-symbolic").css_classes(["flat"]).valign(gtk::Align::Center).tooltip_text("Undo (Ctrl+Z)").build();
    let redo_button = gtk::Button::builder().icon_name("edit-redo-symbolic").css_classes(["flat"]).valign(gtk::Align::Center).tooltip_text("Redo (Ctrl+Shift+Z)").build();
    let state = gtk::Label::builder().css_classes(["dim-label"]).margin_start(8).margin_end(8).build();
    let cancel = gtk::Button::builder().label("cancel").css_classes(["flat"]).valign(gtk::Align::Center).tooltip_text("Back to the layout kept").build();
    let ok = gtk::Button::builder().label("ok").css_classes(["suggested-action"]).valign(gtk::Align::Center).tooltip_text("Keep the draft as the layout (Ctrl+S); the step marked ✓").build();
    bar.append(&play);
    bar.append(&gap());
    for w in [undo_button.upcast_ref::<gtk::Widget>(), redo_button.upcast_ref(), state.upcast_ref(), cancel.upcast_ref(), ok.upcast_ref()] {
        bar.append(w);
    }

    let panel = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(6).halign(gtk::Align::End).width_request(PANEL_W as i32).margin_top(BAR_H as i32).visible(false).css_classes(["editor-panel"]).build();
    let step_title = gtk::Label::builder().xalign(0.0).css_classes(["title-4"]).build();
    panel.append(&step_title);
    panel.append(&heading("eye"));
    let zoom = spin(0.2, 4.0, 0.05, 2);
    let pan = [spin(-200.0, 200.0, 0.5, 1), spin(-200.0, 200.0, 0.5, 1)];
    let top = spin(0.0, 1.0, 0.05, 2);
    let reset_eye = gtk::Button::builder().label("as it comes").css_classes(["flat"]).halign(gtk::Align::Start).tooltip_text("This step's eye as it comes by itself").build();
    panel.append(&row("zoom", &[zoom.upcast_ref()]));
    panel.append(&row("moved", &[pan[0].upcast_ref(), pan[1].upcast_ref()]));
    panel.append(&row("from above", &[top.upcast_ref()]));
    panel.append(&reset_eye);
    panel.append(&heading("part"));
    let part_title = gtk::Label::builder().xalign(0.0).wrap(true).css_classes(["dim-label"]).build();
    panel.append(&part_title);
    let part_at = [spin(-200.0, 200.0, 1.0, 0), spin(-200.0, 200.0, 1.0, 0)];
    let size = spin(0.3, 3.0, 0.05, 2);
    let size_row = row("size", &[size.upcast_ref()]);
    let part_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    part_box.append(&row("place", &[part_at[0].upcast_ref(), part_at[1].upcast_ref()]));
    part_box.append(&size_row);
    panel.append(&part_box);
    panel.append(&heading("looks · all steps"));
    let looks: Vec<gtk::DropDown> = [
        ("paper", PAPERS.iter().map(|p| p.0.to_owned()).collect::<Vec<_>>()),
        ("lines", INKS.iter().map(|p| p.0.to_owned()).collect()),
        ("width", WIDTHS.iter().map(|w| format!("{w}")).collect()),
        ("font", FONTS.iter().map(|f| f.1.to_owned()).collect()),
    ]
    .into_iter()
    .map(|(name, items)| {
        let items: Vec<&str> = items.iter().map(String::as_str).collect();
        let d = gtk::DropDown::from_strings(&items);
        d.set_hexpand(true);
        panel.append(&row(name, &[d.upcast_ref()]));
        d
    })
    .collect();
    let hints = gtk::Label::builder()
        .label("click a part to choose it · drag it · arrows: a square (Shift: five) · Esc: none\nwheel on the table: zoom to the pointer · on the Duo: its size · drag the table: the eye\n← →  with nothing chosen: the steps · Ctrl+Z / Ctrl+Shift+Z · Ctrl+S: ok")
        .xalign(0.0)
        .wrap(true)
        .vexpand(true)
        .valign(gtk::Align::End)
        .css_classes(["dim-label", "caption"])
        .build();
    panel.append(&hints);
    Editor {
        bar,
        panel,
        steps,
        state,
        play,
        undo_button,
        redo_button,
        ok,
        cancel,
        step_title,
        zoom,
        pan,
        top,
        reset_eye,
        part_title,
        part_box,
        part_at,
        size_row,
        size,
        looks,
        preview: Cell::new(None),
        selected: Cell::new(None),
        hover: Cell::new(None),
        drag: Cell::new(None),
        saved: Cell::new(scene::read_saved()),
        undo: RefCell::default(),
        redo: RefCell::default(),
        last: Cell::new(None),
        syncing: Cell::new(false),
    }
}

pub const CSS: &str = "
.editor-bar { background: alpha(white, 0.97); border-bottom: 1px solid alpha(black, 0.10); padding: 0 10px; }
.editor-bar button { padding: 2px 8px; min-height: 0; }
.editor-bar button:checked { background: alpha(black, 0.10); }
.editor-panel { background: alpha(white, 0.97); border-left: 1px solid alpha(black, 0.10); padding: 14px 16px; }
window.night .editor-bar, window.night .editor-panel { background: alpha(#2a2a2e, 0.97); }
";

/// The editor's controls hooked to the layout.
pub fn connect(ui: &std::rc::Rc<Ui>) {
    let ed = &ui.ed;
    let weak = |ui: &std::rc::Rc<Ui>| std::rc::Rc::downgrade(ui);
    for (i, b) in ed.steps.iter().enumerate() {
        let w = weak(ui);
        b.connect_clicked(move |_| {
            if let Some(ui) = w.upgrade() {
                if !ui.ed.syncing.get() {
                    show_step(&ui, Some(i));
                }
            }
        });
    }
    let w = weak(ui);
    ed.play.connect_clicked(move |_| {
        if let Some(ui) = w.upgrade() {
            show_step(&ui, None);
            close_boards(&ui);
            ui.intro.borrow_mut().replay();
            show_fold(&ui, ui.fold.get().0);
        }
    });
    let w = weak(ui);
    ed.undo_button.connect_clicked(move |_| {
        if let Some(ui) = w.upgrade() {
            undo(&ui, true);
        }
    });
    let w = weak(ui);
    ed.redo_button.connect_clicked(move |_| {
        if let Some(ui) = w.upgrade() {
            undo(&ui, false);
        }
    });
    let w = weak(ui);
    ed.ok.connect_clicked(move |_| {
        if let Some(ui) = w.upgrade() {
            keep(&ui);
        }
    });
    let w = weak(ui);
    ed.cancel.connect_clicked(move |_| {
        if let Some(ui) = w.upgrade() {
            back_to_kept(&ui);
        }
    });
    let w = weak(ui);
    ed.reset_eye.connect_clicked(move |_| {
        if let Some(ui) = w.upgrade() {
            change(&ui, "eye reset", |l, i| l.steps[i].shot = Shot::default());
        }
    });
    // The step's eye and the part chosen, from the panel's numbers.
    let eye_spin = |s: &gtk::SpinButton, set: fn(&mut Shot, f32)| {
        let w = weak(ui);
        s.connect_value_changed(move |s| {
            let Some(ui) = w.upgrade() else { return };
            if ui.ed.syncing.get() {
                return;
            }
            let v = s.value() as f32;
            change(&ui, "eye number", |l, i| set(&mut l.steps[i].shot, v));
        });
    };
    eye_spin(&ed.zoom, |s, v| s.zoom = v);
    eye_spin(&ed.pan[0], |s, v| s.pan.0 = v);
    eye_spin(&ed.pan[1], |s, v| s.pan.1 = v);
    eye_spin(&ed.top, |s, v| s.top = v);
    for axis in 0..2 {
        let w = weak(ui);
        ed.part_at[axis].connect_value_changed(move |s| {
            let Some(ui) = w.upgrade() else { return };
            if ui.ed.syncing.get() {
                return;
            }
            let Some(part) = ui.ed.selected.get() else { return };
            let v = s.value() as f32;
            let from = part_place(&ui, part);
            let to = if axis == 0 { (v, from.1) } else { (from.0, v) };
            change(&ui, "part number", |l, i| l.steps[i].place.set_part(part, to));
        });
    }
    let w = weak(ui);
    ed.size.connect_value_changed(move |s| {
        let Some(ui) = w.upgrade() else { return };
        if ui.ed.syncing.get() {
            return;
        }
        let v = s.value() as f32;
        change(&ui, "duo size", |l, i| l.steps[i].place.duo_scale = v);
    });
    for (k, d) in ed.looks.iter().enumerate() {
        let w = weak(ui);
        d.connect_selected_notify(move |d| {
            let Some(ui) = w.upgrade() else { return };
            if ui.ed.syncing.get() {
                return;
            }
            let at = d.selected() as usize;
            change(&ui, "looks", |l, _| match k {
                0 => l.style.paper = at.min(PAPERS.len() - 1),
                1 => l.style.ink = at.min(INKS.len() - 1),
                2 => l.style.width = at.min(WIDTHS.len() - 1),
                _ => l.style.font = at.min(FONTS.len() - 1),
            });
        });
    }
}

/// Editing on or off. On: the draft shown (else the layout kept), the
/// first step held. Off: the layout kept shown again (the draft left for
/// the next time), the app as it goes.
pub fn set_on(ui: &std::rc::Rc<Ui>, on: bool) {
    let ed = &ui.ed;
    ui.layout_edit.set(on);
    if let Some(holder) = ui.duo.parent() {
        holder.set_can_target(!on);
    }
    ed.saved.set(scene::read_saved());
    let shown = if on { scene::read_draft().unwrap_or(ed.saved.get()) } else { ed.saved.get() };
    ui.layout.set(shown);
    set_style(shown.style);
    ed.bar.set_visible(on);
    ed.panel.set_visible(on);
    ed.selected.set(None);
    ed.hover.set(None);
    trace(format_args!("layout: {}", if on { "editing" } else { "fixed" }));
    if on {
        show_step(ui, Some(ed.preview.get().unwrap_or(0)));
    } else {
        show_step(ui, None);
    }
    super::refresh_settings(ui);
    // Once the panel has its size, the frame laid beside it.
    let weak = std::rc::Rc::downgrade(ui);
    gtk::glib::idle_add_local_once(move || {
        if let Some(ui) = weak.upgrade() {
            show_fold(&ui, ui.fold.get().0);
        }
    });
}

/// Where the frame is shown on the page (its rect, the floor's px): all of
/// it, or editing what the bar and the panel leave.
pub fn frame_room(ui: &Ui) -> [f32; 4] {
    let (w, h) = (ui.floor.width().max(1) as f32, ui.floor.height().max(1) as f32);
    if ui.layout_edit.get() {
        // Left of the panel as it is laid (its words make it wider than
        // asked).
        let panel_left = ui.ed.panel.compute_point(&ui.floor, &gtk::graphene::Point::new(0.0, 0.0)).map_or(w - PANEL_W, |p| p.x()).min(w - PANEL_W);
        [ROOM, BAR_H + ROOM, (panel_left - 2.0 * ROOM).max(100.0), (h - BAR_H - 2.0 * ROOM).max(80.0)]
    } else {
        [0.0, 0.0, w, h]
    }
}

/// The step the editor sets: the one shown, else the one the frame is at.
pub fn current_step(ui: &Ui) -> usize {
    ui.ed.preview.get().unwrap_or(ui.step_now.get()).min(STEPS.len() - 1)
}

/// Whether part `i` is seen at the step shown: only those are outlined,
/// chosen and dragged.
pub fn part_shown(ui: &Ui, i: usize) -> bool {
    let step = current_step(ui);
    match i {
        0 => step >= 4,
        1 => step == 4,
        2 => true,
        3 => step >= 3,
        4 => step >= 5,
        _ => step == 1,
    }
}

/// The parts' outlines to draw: the one chosen (solid), the one under the
/// pointer or dragged (dashed) - none otherwise, the step as it is seen.
pub fn outlines(ui: &Ui) -> Option<Vec<([f32; 4], bool)>> {
    if !ui.layout_edit.get() {
        return None;
    }
    let boxes = ui.layout_boxes.get();
    let dragged = match ui.ed.drag.get() {
        Some(Drag::Part { i, .. }) => Some(i),
        _ => None,
    };
    let chosen = ui.ed.selected.get().filter(|&i| part_shown(ui, i));
    let mut v: Vec<([f32; 4], bool)> = chosen.map(|i| (boxes[i], true)).into_iter().collect();
    if let Some(i) = dragged.or(ui.ed.hover.get()).filter(|&i| part_shown(ui, i) && Some(i) != chosen) {
        v.push((boxes[i], false));
    }
    Some(v)
}

/// The sheet outlined: a guide while a part is dragged.
pub fn sheet_shown(ui: &Ui) -> bool {
    ui.layout_edit.get() && matches!(ui.ed.drag.get(), Some(Drag::Part { .. }))
}

/// The part (seen at the step) at a point of the table.
fn part_at(ui: &Ui, t: (f32, f32)) -> Option<usize> {
    let boxes = ui.layout_boxes.get();
    [1, 0, 2, 3, 4, 5].into_iter().find(|&i| part_shown(ui, i) && (boxes[i][0]..=boxes[i][2]).contains(&t.0) && (boxes[i][1]..=boxes[i][3]).contains(&t.1))
}

/// Part `i`'s place at the step (the Duo where it comes by itself: from
/// its outline).
fn part_place(ui: &Ui, i: usize) -> (f32, f32) {
    let l = ui.layout.get();
    l.steps[current_step(ui)].place.part(i).unwrap_or_else(|| {
        let cur = square() * DUO_PX_PER_MM as f32;
        let b = ui.layout_boxes.get()[i];
        let sheet = ui.floor_view.borrow().sheet.unwrap_or([0.0; 4]);
        (((b[0] - sheet[0]) / cur).round(), ((b[1] - sheet[1]) / cur).round())
    })
}

/// A change to the draft (of `kind`; `f` on the layout and the step
/// being set): undoable, written, the frame shown anew; the step not
/// marked fixed any more.
fn change(ui: &Ui, kind: &'static str, f: impl FnOnce(&mut Layout, usize)) {
    let before = ui.layout.get();
    let mut l = before;
    let i = current_step(ui);
    f(&mut l, i);
    if l == before {
        return;
    }
    // A run of one kind (the wheel, a spin held): one undo.
    let run = ui.ed.last.get().is_some_and(|(k, t)| k == kind && t.elapsed().as_secs_f32() < 0.8);
    if !run {
        ui.ed.undo.borrow_mut().push(before);
        ui.ed.redo.borrow_mut().clear();
    }
    ui.ed.last.set(Some((kind, Instant::now())));
    l.fixed[i] = false;
    set_layout(ui, l);
}

/// The draft as `l`: written, its looks taken, all shown anew.
fn set_layout(ui: &Ui, l: Layout) {
    let style_changed = l.style != ui.layout.get().style;
    ui.layout.set(l);
    scene::save_draft(&l);
    if style_changed {
        set_style(l.style);
        ui.floor.queue_draw();
        ui.floor_gl.queue_render();
    }
    show(ui);
    show_fold(ui, ui.fold.get().0);
}

fn undo(ui: &Ui, back: bool) {
    let (from, to) = if back { (&ui.ed.undo, &ui.ed.redo) } else { (&ui.ed.redo, &ui.ed.undo) };
    let Some(l) = from.borrow_mut().pop() else { return };
    to.borrow_mut().push(ui.layout.get());
    ui.ed.last.set(None);
    set_layout(ui, l);
}

/// ok: the draft kept as the layout, the step marked fixed.
fn keep(ui: &Ui) {
    let mut l = ui.layout.get();
    l.fixed[current_step(ui)] = true;
    ui.layout.set(l);
    scene::save(&l);
    scene::drop_draft();
    ui.ed.saved.set(l);
    trace(format_args!("layout kept, step {} fixed", STEPS[current_step(ui)]));
    show(ui);
}

/// cancel: back to the layout kept (undoable).
fn back_to_kept(ui: &Ui) {
    let saved = ui.ed.saved.get();
    if saved == ui.layout.get() {
        return;
    }
    ui.ed.undo.borrow_mut().push(ui.layout.get());
    ui.ed.redo.borrow_mut().clear();
    set_layout(ui, saved);
    scene::drop_draft();
    show(ui);
}

/// The bar and the panel as things are.
pub fn show(ui: &Ui) {
    let ed = &ui.ed;
    if !ui.layout_edit.get() {
        return;
    }
    ed.syncing.set(true);
    let l = ui.layout.get();
    let shown = ed.preview.get();
    for (i, b) in ed.steps.iter().enumerate() {
        if b.is_active() != (shown == Some(i)) {
            b.set_active(shown == Some(i));
        }
        b.set_label(&format!("{}{} {}", if l.fixed[i] { "✓ " } else { "" }, i + 1, STEPS[i]));
    }
    let i = current_step(ui);
    let kept = l.same(&ed.saved.get());
    ed.state.set_label(if kept { "kept" } else { "draft - ok to keep" });
    ed.ok.set_sensitive(!kept || !l.fixed[i]);
    ed.cancel.set_sensitive(!kept);
    ed.undo_button.set_sensitive(!ed.undo.borrow().is_empty());
    ed.redo_button.set_sensitive(!ed.redo.borrow().is_empty());
    ed.step_title.set_label(&format!("{} · {}{}", i + 1, STEPS[i], if l.fixed[i] { "  ✓" } else { "" }));
    let shot = l.steps[i].shot;
    ed.zoom.set_value(shot.zoom as f64);
    ed.pan[0].set_value(shot.pan.0 as f64);
    ed.pan[1].set_value(shot.pan.1 as f64);
    ed.top.set_value(shot.top as f64);
    match ed.selected.get().filter(|&p| part_shown(ui, p)) {
        Some(p) => {
            ed.part_title.set_label(&format!("{} - at this step", PARTS[p]));
            ed.part_box.set_visible(true);
            let at = part_place(ui, p);
            ed.part_at[0].set_value(at.0 as f64);
            ed.part_at[1].set_value(at.1 as f64);
            ed.size_row.set_visible(p == 1);
            ed.size.set_value(l.steps[i].place.duo_scale as f64);
        }
        None => {
            ed.part_title.set_label("none chosen - click a part in the frame");
            ed.part_box.set_visible(false);
        }
    }
    for (d, at) in ed.looks.iter().zip([l.style.paper, l.style.ink, l.style.width, l.style.font]) {
        if d.selected() != at as u32 {
            d.set_selected(at as u32);
        }
    }
    ed.syncing.set(false);
}

/// Step `i` shown and held (none: the start going as it goes again): the
/// start held at its seconds, the Duo there or not, the menu and the
/// settings open or not.
pub fn show_step(ui: &Ui, i: Option<usize>) {
    ui.ed.preview.set(i);
    {
        let mut intro = ui.intro.borrow_mut();
        intro.clear_note();
        match i {
            Some(i) => {
                intro.hold = Some(intro::Intro::STEP_S[i.min(3)]);
                let sink = if i >= 4 { 1.0 } else { 0.0 };
                intro.force_sink = Some(sink);
                intro.sink = sink;
            }
            None => {
                intro.hold = None;
                intro.force_sink = None;
            }
        }
    }
    if i.is_some() {
        hold_sample(ui);
    }
    if let Some(i) = i {
        let menu_open = ui.board.borrow().as_ref().is_some_and(|b| !b.closing());
        if i >= 5 && !menu_open {
            *ui.board.borrow_mut() = Some(board::Board::open(menu_lines(ui)));
        }
        let settings_open = ui.page.borrow().as_ref().is_some_and(|(k, b)| k == "settings" && !b.closing());
        if i == 6 && !settings_open {
            if let Some(b) = ui.board.borrow_mut().as_mut() {
                b.chosen = b.lines.iter().position(|l| l.key == "settings");
            }
            *ui.page.borrow_mut() = Some(("settings".to_owned(), board::Board::open(settings_lines(ui))));
        }
        if i < 6 {
            if let Some((_, p)) = ui.page.borrow_mut().as_mut() {
                p.close();
            }
            if let Some(b) = ui.board.borrow_mut().as_mut() {
                b.chosen = None;
            }
        }
        if i < 5 {
            close_boards(ui);
        }
    }
    trace(format_args!("step shown: {:?}", i.map(|i| STEPS[i])));
    show(ui);
    show_fold(ui, ui.fold.get().0);
}

/// The sample phone as the frames show it: on Wi-Fi, shut, lying flat
/// (whatever the phone there is does; tilt_to and fold_to wait while
/// editing).
fn hold_sample(ui: &Ui) {
    let (fold, _) = ui.fold.get();
    ui.fold.set((fold, 0.0));
    let (tilt, _) = ui.tilt.get();
    ui.tilt.set((tilt, [0.0, 0.0]));
    let o = ui.orient.get();
    ui.orient.set((o.0, o.1, false));
}

/// The wheel at `at` (the page's px): over the Duo its size, elsewhere the
/// step's lens - toward the pointer (the table's point under it kept
/// there).
pub fn wheel(ui: &Ui, at: (f64, f64), dy: f64) {
    let under = table_under(&ui.floor_view.borrow(), at);
    let over_duo = part_shown(ui, 1) && under.is_some_and(|t| {
        let b = ui.layout_boxes.get()[1];
        (b[0]..=b[2]).contains(&t.0) && (b[1]..=b[3]).contains(&t.1)
    });
    if over_duo {
        change(ui, "duo size", |l, i| l.steps[i].place.duo_scale = (l.steps[i].place.duo_scale * 1.05f32.powf(-dy as f32)).clamp(0.3, 3.0));
        return;
    }
    change(ui, "zoom", |l, i| l.steps[i].shot.zoom = (l.steps[i].shot.zoom * 1.1f32.powf(-dy as f32)).clamp(0.2, 4.0));
    // The point under the pointer brought back there.
    // (The view let go of before it is drawn anew.)
    let after = table_under(&ui.floor_view.borrow(), at);
    if let (Some(t0), Some(t1)) = (under, after) {
        let cur = square() * DUO_PX_PER_MM as f32;
        let mut l = ui.layout.get();
        let i = current_step(ui);
        l.steps[i].shot.pan.0 += (t0.0 - t1.0) / cur;
        l.steps[i].shot.pan.1 += (t0.1 - t1.1) / cur;
        set_layout(ui, l);
    }
}

/// A drag beginning at `at`: a part under it (chosen), else the step's
/// eye. Whether the editor took it.
pub fn drag_begin(ui: &Ui, at: (f64, f64)) -> bool {
    let fv = ui.floor_view.borrow();
    let Some(t) = table_under(&fv, at) else { return false };
    let (bx, by) = (table_under(&fv, (at.0 + 10.0, at.1)), table_under(&fv, (at.0, at.1 + 10.0)));
    drop(fv);
    if let Some(i) = part_at(ui, t) {
        ui.ed.selected.set(Some(i));
        ui.ed.drag.set(Some(Drag::Part { i, from: part_place(ui, i), began: t, at }));
        ui.ed.last.set(None);
        show(ui);
        show_fold(ui, ui.fold.get().0);
        return true;
    }
    if let (Some(bx), Some(by)) = (bx, by) {
        let step = current_step(ui);
        ui.ed.drag.set(Some(Drag::Eye { step, from: ui.layout.get().steps[step].shot.pan, per: ((bx.0 - t.0) / 10.0, (by.1 - t.1) / 10.0) }));
        ui.ed.last.set(None);
        return true;
    }
    false
}

/// The drag gone on by (dx, dy): the part by whole squares, or the eye.
pub fn drag_update(ui: &Ui, dx: f64, dy: f64) -> bool {
    let cur = square() * DUO_PX_PER_MM as f32;
    match ui.ed.drag.get() {
        Some(Drag::Part { i, from, began, at }) => {
            let now = table_under(&ui.floor_view.borrow(), (at.0 + dx, at.1 + dy));
            if let Some(now) = now {
                let to = ((from.0 + (now.0 - began.0) / cur).round(), (from.1 + (now.1 - began.1) / cur).round());
                change(ui, "drag", |l, s| l.steps[s].place.set_part(i, to));
            }
            true
        }
        Some(Drag::Eye { step, from, per }) => {
            let to = (from.0 - dx as f32 * per.0 / cur, from.1 - dy as f32 * per.1 / cur);
            change(ui, "drag", |l, _| l.steps[step].shot.pan = to);
            true
        }
        None => false,
    }
}

pub fn drag_end(ui: &Ui) -> bool {
    let was = ui.ed.drag.take().is_some();
    if was {
        ui.ed.last.set(None);
        show_fold(ui, ui.fold.get().0);
    }
    was
}

/// The pointer at `at`: the part under it outlined. Whether one is there.
pub fn hover(ui: &Ui, at: (f64, f64)) -> bool {
    let part = table_under(&ui.floor_view.borrow(), at).and_then(|t| part_at(ui, t));
    if ui.ed.hover.replace(part) != part {
        show_fold(ui, ui.fold.get().0);
    }
    part.is_some()
}

/// A click at `at`: the part there chosen (none: none).
pub fn click(ui: &Ui, at: (f64, f64)) {
    let part = table_under(&ui.floor_view.borrow(), at).and_then(|t| part_at(ui, t));
    ui.ed.selected.set(part);
    show(ui);
    show_fold(ui, ui.fold.get().0);
}

/// A key while editing: whether the editor took it.
pub fn key(ui: &Ui, key: gdk::Key, state: gdk::ModifierType) -> bool {
    let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
    let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
    let k = key.to_lower();
    if ctrl && k == gdk::Key::z {
        undo(ui, !shift);
        return true;
    }
    if ctrl && k == gdk::Key::y {
        undo(ui, false);
        return true;
    }
    if ctrl && k == gdk::Key::s {
        keep(ui);
        return true;
    }
    if key == gdk::Key::Escape {
        ui.ed.selected.set(None);
        show(ui);
        show_fold(ui, ui.fold.get().0);
        return true;
    }
    let by = if shift { 5.0 } else { 1.0 };
    let (dx, dy) = match key {
        gdk::Key::Left => (-by, 0.0),
        gdk::Key::Right => (by, 0.0),
        gdk::Key::Up => (0.0, -by),
        gdk::Key::Down => (0.0, by),
        gdk::Key::Page_Up => return step_by(ui, -1),
        gdk::Key::Page_Down => return step_by(ui, 1),
        _ => return false,
    };
    match ui.ed.selected.get().filter(|&p| part_shown(ui, p)) {
        Some(p) => {
            let at = part_place(ui, p);
            change(ui, "arrows", |l, i| l.steps[i].place.set_part(p, (at.0 + dx, at.1 + dy)));
            true
        }
        // Nothing chosen: ← → the steps.
        None if dx != 0.0 => step_by(ui, dx.signum() as i32),
        None => false,
    }
}

fn step_by(ui: &Ui, by: i32) -> bool {
    let i = (current_step(ui) as i32 + by).clamp(0, STEPS.len() as i32 - 1) as usize;
    show_step(ui, Some(i));
    true
}

/// Over the frame's room, outside the frame: the table dimmed, the frame
/// outlined (editing only; `frame` the floor's px).
pub fn draw_frame(cr: &gtk::cairo::Context, frame: [f32; 4], size: (f64, f64)) {
    let [x, y, w, h] = frame.map(|v| v as f64);
    cr.save().ok();
    cr.set_fill_rule(gtk::cairo::FillRule::EvenOdd);
    cr.rectangle(0.0, 0.0, size.0, size.1);
    cr.rectangle(x, y, w, h);
    cr.set_source_rgba(0.5, 0.5, 0.52, 0.18);
    let _ = cr.fill();
    cr.rectangle(x - 0.5, y - 0.5, w + 1.0, h + 1.0);
    cr.set_line_width(1.0);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.35);
    let _ = cr.stroke();
    cr.restore().ok();
}

/// The sample phone's words (the editor's frames show it, not the phone
/// there is or is not).
pub const SAMPLE_PHONE: [&str; 4] = ["duo 00001", "item 0.2.1", "82% charging", "on wi-fi"];
