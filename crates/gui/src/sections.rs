//! The sections under the Duo that show the phone itself (#169): About,
//! Battery, Storage, Wallpapers & Look, Updates & Backups. Each a page of
//! its own; what it shows read when it is opened (Battery, Look) or with
//! each look at the phone (About, Storage).

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use hythe_core::status::{self, Status};
use gtk::{gdk, gio, glib};

pub const CSS: &str = "
list.boxed-list > row > box.header { min-height: 34px; }
list.boxed-list > row > box.header > box.title { margin-top: 3px; margin-bottom: 3px; }
button.pill { padding: 4px 14px; min-height: 26px; font-weight: 500; }
.wall-tile { border-radius: 8px; }
.wall-button { padding: 0; border-radius: 8px; }
.wall-tile picture { border-radius: 8px; }
.wall-tile.on { box-shadow: 0 0 0 2px @accent_bg_color; }
.wall-remove { min-width: 22px; min-height: 22px; padding: 0; margin: 6px; background: alpha(black, 0.55); color: white; }
.swatch { min-width: 26px; min-height: 26px; padding: 0; border-radius: 13px; }
.swatch.on { box-shadow: 0 0 0 2px white, 0 0 0 4px alpha(black, 0.55); }
.chart-frame { border: 1px solid alpha(black, 0.08); border-radius: 12px; padding: 12px 14px 8px 14px; }
";

/// The sections' widgets, filled as the phone tells.
pub struct Pages {
    pub about: gtk::ListBox,
    pub battery: gtk::ListBox,
    pub chart: gtk::DrawingArea,
    pub history: Rc<RefCell<Vec<(i64, f64, String)>>>,
    pub disks: gtk::ListBox,
    pub parts: gtk::ListBox,
    pub accents: gtk::Box,
    pub walls: gtk::FlowBox,
    pub add_walls: gtk::Button,
    pub look_note: gtk::Label,
    pub updates: gtk::ListBox,
    pub update_tree: gtk::Button,
    pub backups: gtk::ListBox,
    pub back_up: gtk::Button,
}

fn title(page: &gtk::Box, text: &str) {
    page.append(&gtk::Label::builder().label(text).xalign(0.0).css_classes(["status-title"]).build());
}

fn heading(page: &gtk::Box, text: &str) {
    page.append(&gtk::Label::builder().label(text).xalign(0.0).css_classes(["section-title"]).margin_top(10).build());
}

fn note(text: &str) -> gtk::Label {
    gtk::Label::builder().label(text).wrap(true).max_width_chars(70).xalign(0.0).css_classes(["dim-label"]).build()
}

fn list() -> gtk::ListBox {
    gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build()
}

fn page() -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, 14)
}

/// A row with its value on the right, selectable.
fn fact(name: &str, value: &str) -> adw::ActionRow {
    let r = adw::ActionRow::builder().title(name).use_markup(false).build();
    r.add_suffix(&gtk::Label::builder().label(value).selectable(true).css_classes(["dim-label"]).xalign(1.0).ellipsize(gtk::pango::EllipsizeMode::Middle).width_chars(4).build());
    r
}

fn clear(l: &gtk::ListBox) {
    while let Some(c) = l.first_child() {
        l.remove(&c);
    }
}

fn waiting(l: &gtk::ListBox, text: &str) {
    clear(l);
    l.append(&adw::ActionRow::builder().title(text).css_classes(["dim-label"]).build());
}

/// The pages, by the key their section has.
pub fn build() -> (Pages, Vec<(&'static str, gtk::Box)>) {
    // About.
    let about_page = page();
    title(&about_page, "About");
    let about = list();
    waiting(&about, "Waiting for your Duo…");
    about_page.append(&about);

    // Battery.
    let battery_page = page();
    title(&battery_page, "Battery");
    let battery = list();
    waiting(&battery, "Reading the battery…");
    battery_page.append(&battery);
    heading(&battery_page, "Charge, the last two days");
    let chart = gtk::DrawingArea::builder().content_height(170).hexpand(true).build();
    let frame = gtk::Box::builder().css_classes(["chart-frame"]).build();
    frame.append(&chart);
    battery_page.append(&frame);
    let history: Rc<RefCell<Vec<(i64, f64, String)>>> = Rc::default();
    let hover: Rc<RefCell<Option<f64>>> = Rc::default();
    chart.set_draw_func({
        let (history, hover) = (history.clone(), hover.clone());
        move |area, cr, w, h| draw_chart(area, cr, w, h, &history.borrow(), *hover.borrow())
    });
    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion({
        let (hover, chart) = (hover.clone(), chart.clone());
        move |_, x, _| {
            *hover.borrow_mut() = Some(x);
            chart.queue_draw();
        }
    });
    motion.connect_leave({
        let (hover, chart) = (hover.clone(), chart.clone());
        move |_| {
            *hover.borrow_mut() = None;
            chart.queue_draw();
        }
    });
    chart.add_controller(motion);

    // Storage.
    let storage_page = page();
    title(&storage_page, "Storage");
    let disks = list();
    waiting(&disks, "Waiting for your Duo…");
    storage_page.append(&disks);
    heading(&storage_page, "The system disk, by part");
    let parts = list();
    waiting(&parts, "Counting… (a few seconds)");
    storage_page.append(&parts);

    // Wallpapers & Look.
    let look_page = page();
    title(&look_page, "Wallpapers & Look");
    heading(&look_page, "Accent");
    let accents = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    look_page.append(&accents);
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    head.set_margin_top(10);
    head.append(&gtk::Label::builder().label("Wallpapers").xalign(0.0).hexpand(true).css_classes(["section-title"]).build());
    let add_walls = gtk::Button::builder().label("Add Pictures…").css_classes(["pill"]).build();
    head.append(&add_walls);
    look_page.append(&head);
    let look_note = note("Click a picture to put it on both panels. Pictures you add here are yours on the phone; the ones item came with stay.");
    look_page.append(&look_note);
    let walls = gtk::FlowBox::builder().selection_mode(gtk::SelectionMode::None).column_spacing(10).row_spacing(10).max_children_per_line(8).min_children_per_line(2).halign(gtk::Align::Start).valign(gtk::Align::Start).build();
    look_page.append(&walls);

    // Updates & Backups.
    let updates_page = page();
    title(&updates_page, "Updates & Backups");
    let updates = list();
    waiting(&updates, "Waiting for your Duo…");
    updates_page.append(&updates);
    let update_tree = gtk::Button::builder().label("Build and Install from Your Tree…").css_classes(["pill"]).halign(gtk::Align::Start).visible(false).build();
    updates_page.append(&update_tree);
    let bhead = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    bhead.set_margin_top(10);
    bhead.append(&gtk::Label::builder().label("Backups on this computer").xalign(0.0).hexpand(true).css_classes(["section-title"]).build());
    let back_up = gtk::Button::builder().label("Back Up Now").css_classes(["pill"]).build();
    bhead.append(&back_up);
    updates_page.append(&bhead);
    updates_page.append(&note("Your home folder and settings, copied to this computer over the cable or Wi-Fi. The whole system and Android are under Repair & Reset."));
    let backups = list();
    updates_page.append(&backups);

    let pages = Pages { about, battery, chart, history, disks, parts, accents, walls, add_walls, look_note, updates, update_tree, backups, back_up };
    (pages, vec![("about", about_page), ("battery", battery_page), ("storage", storage_page), ("look", look_page), ("updates", updates_page)])
}

/// About, Storage's disks and Updates from a look at the phone.
/// About's facts, as words (the page's rows; the table's board).
pub fn about_rows(s: &Status, number: Option<String>, link: &str) -> Vec<(&'static str, String)> {
    let up = format!("{} h {} min", s.uptime_s / 3600, s.uptime_s / 60 % 60);
    let mut rows = vec![("Name", format!("Surface Duo{}", number.map(|n| format!(" · {n}")).unwrap_or_default())), ("Serial number", s.serial.clone()), ("item", format!("{} (built {})", s.item, s.item_built)), ("System", s.os.clone()), ("Kernel", s.kernel.clone()), ("Port", s.port.clone()), ("Sensors (sensorfw)", s.sensorfw.clone())];
    if let Some(f) = s.fingers {
        rows.push(("Fingers", f.to_string()));
    }
    rows.push(("Up", up));
    rows.push(("Connected", link.to_owned()));
    rows.retain(|(_, v)| !v.is_empty());
    rows
}

/// Storage's disks, as words.
pub fn storage_rows(s: &Status) -> Vec<(&'static str, String)> {
    s.disks
        .iter()
        .map(|(mount, size, free)| {
            let name = match mount.as_str() {
                "/" => "System and files",
                "/userdata" => "Data",
                _ => "Disk",
            };
            (name, format!("{} free of {}", status::size_words(*free), status::size_words(*size)))
        })
        .collect()
}

/// Updates', as words.
pub fn updates_rows(s: &Status) -> Vec<(&'static str, String)> {
    vec![("item on the phone", format!("{} (built {})", s.item, s.item_built))]
}

pub fn fill(p: &Pages, s: &Status, number: Option<String>, link: &str, developer: bool) {
    clear(&p.about);
    for (k, v) in about_rows(s, number, link) {
        p.about.append(&fact(k, &v));
    }

    clear(&p.disks);
    for (mount, size, free) in &s.disks {
        let name = match mount.as_str() {
            "/" => "System and your files",
            "/userdata" => "Data partition",
            m => m,
        };
        let r = fact(name, &format!("{} free of {}", status::size_words(*free), status::size_words(*size)));
        let used = if *size > 0 { 1.0 - *free as f64 / *size as f64 } else { 0.0 };
        let bar = gtk::LevelBar::builder().min_value(0.0).max_value(1.0).value(used).valign(gtk::Align::Center).width_request(140).build();
        // One colour whatever the fill (GTK's own offsets paint a little
        // used space red).
        for o in ["low", "high", "full"] {
            bar.remove_offset_value(Some(o));
        }
        r.add_suffix(&bar);
        p.disks.append(&r);
    }

    clear(&p.updates);
    p.updates.append(&fact("item on the phone", &format!("{} (built {})", s.item, s.item_built)));
    let r = adw::ActionRow::builder().title("New versions").subtitle("item's releases will be offered here once they are published.").use_markup(false).build();
    p.updates.append(&r);
    p.update_tree.set_visible(developer);
}

/// Storage's parts, counted.
pub fn fill_parts(p: &Pages, parts: &hythe_core::storage::Parts, colours: &[(f64, f64, f64)]) {
    clear(&p.parts);
    for (i, (name, kib)) in parts.list().into_iter().enumerate() {
        let r = fact(name, &status::size_words(kib));
        let dot = gtk::DrawingArea::builder().content_width(10).content_height(10).valign(gtk::Align::Center).build();
        let c = colours.get(i).copied();
        dot.set_draw_func(move |_, cr, w, h| {
            let (r, g, b) = c.unwrap_or((0.8, 0.8, 0.8));
            cr.set_source_rgb(r, g, b);
            cr.arc(w as f64 / 2.0, h as f64 / 2.0, 5.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        });
        r.add_prefix(&dot);
        p.parts.append(&r);
    }
}

/// The battery read from the phone, its rows and its chart.
pub fn load_battery(p: &Rc<Pages>, host: String) {
    let p = p.clone();
    glib::spawn_future_local(async move {
        let Ok(r) = gio::spawn_blocking(move || hythe_core::battery::read(&host)).await else { return };
        clear(&p.battery);
        let b = match r {
            Ok(b) => b,
            Err(e) => {
                waiting(&p.battery, &format!("Could not read the battery: {e}"));
                return;
            }
        };
        let charging = b.state == "Charging";
        let mut rows = vec![("Charge", b.percent.map(|v| format!("{v} %")).unwrap_or_default()), ("State", b.state.clone()), ("Health", b.health.clone())];
        if let (Some(f), Some(d)) = (b.full_mah, b.design_mah) {
            rows.push(("Capacity", format!("{f} mAh of {d} mAh new ({:.0} %)", f as f64 * 100.0 / d.max(1) as f64)));
        }
        if let Some(c) = b.cycles {
            rows.push(("Charge cycles", c.to_string()));
        }
        if let Some(v) = b.volts {
            rows.push(("Voltage", format!("{v:.2} V")));
        }
        if let Some(c) = b.current_ma {
            // This kernel's sign: negative while it charges.
            rows.push(("Current", format!("{:.0} mA {}", c.abs(), if charging { "in" } else { "out" })));
        }
        if let Some(t) = b.temp_c {
            rows.push(("Temperature", format!("{t:.1} °C")));
        }
        for (k, v) in rows {
            if !v.is_empty() {
                p.battery.append(&fact(k, &v));
            }
        }
        *p.history.borrow_mut() = b.history;
        p.chart.queue_draw();
    });
}

/// The charge over two days: one series, so no legend - the heading names
/// it; a thin line over a faint fill, the grid recessive (0, 50, 100 %),
/// the times along the bottom; under the pointer a rule and its reading.
fn draw_chart(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, w: i32, h: i32, points: &[(i64, f64, String)], hover: Option<f64>) {
    let (w, h) = (w as f64, h as f64);
    let ink = area.color();
    let (left, right, top, bottom) = (34.0, 8.0, 8.0, 20.0);
    let (pw, ph) = (w - left - right, h - top - bottom);
    let now = glib::DateTime::now_local().map(|d| d.to_unix()).unwrap_or(0);
    let start = now - 48 * 3600;
    let x = |t: i64| left + (t - start) as f64 / (48.0 * 3600.0) * pw;
    let y = |p: f64| top + (1.0 - p / 100.0) * ph;
    cr.select_font_face("sans-serif", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Normal);
    cr.set_font_size(10.0);
    // The grid and its labels.
    cr.set_line_width(1.0);
    for p in [0.0, 50.0, 100.0] {
        cr.set_source_rgba(ink.red() as f64, ink.green() as f64, ink.blue() as f64, 0.08);
        cr.move_to(left, y(p).round() + 0.5);
        cr.line_to(w - right, y(p).round() + 0.5);
        let _ = cr.stroke();
        cr.set_source_rgba(ink.red() as f64, ink.green() as f64, ink.blue() as f64, 0.5);
        let label = format!("{p:.0}%");
        let ext = cr.text_extents(&label).ok();
        cr.move_to(left - 6.0 - ext.map_or(0.0, |e| e.width()), y(p) + 3.5);
        let _ = cr.show_text(&label);
    }
    // Every 12 hours along the bottom, on the hour.
    let mut t = start - start.rem_euclid(12 * 3600) + 12 * 3600;
    while t <= now {
        if let Ok(label) = glib::DateTime::from_unix_local(t).and_then(|d| d.format(if d.hour() == 0 { "%a" } else { "%H:%M" })) {
            let ext = cr.text_extents(&label).ok();
            cr.set_source_rgba(ink.red() as f64, ink.green() as f64, ink.blue() as f64, 0.5);
            cr.move_to(x(t) - ext.map_or(0.0, |e| e.width()) / 2.0, h - 5.0);
            let _ = cr.show_text(&label);
        }
        t += 12 * 3600;
    }
    let shown: Vec<&(i64, f64, String)> = points.iter().filter(|(t, _, _)| *t >= start).collect();
    if shown.len() < 2 {
        cr.set_source_rgba(ink.red() as f64, ink.green() as f64, ink.blue() as f64, 0.5);
        cr.move_to(left + 8.0, top + ph / 2.0);
        let _ = cr.show_text(if points.is_empty() { "No history yet: the phone keeps it as it runs." } else { "Not enough history yet." });
        return;
    }
    let (r, g, b) = (0.21, 0.52, 0.89);
    // In runs: a gap of over three hours (the phone off) breaks the line.
    let mut runs: Vec<Vec<(i64, f64)>> = Vec::new();
    for &&(t, p, _) in &shown {
        match runs.last_mut() {
            Some(run) if t - run.last().map_or(t, |l| l.0) <= 3 * 3600 => run.push((t, p)),
            _ => runs.push(vec![(t, p)]),
        }
    }
    for run in &runs {
        cr.move_to(x(run[0].0), y(run[0].1));
        for &(t, p) in &run[1..] {
            cr.line_to(x(t), y(p));
        }
        let path = cr.copy_path().ok();
        cr.line_to(x(run[run.len() - 1].0), y(0.0));
        cr.line_to(x(run[0].0), y(0.0));
        cr.close_path();
        cr.set_source_rgba(r, g, b, 0.08);
        let _ = cr.fill();
        if let Some(path) = path {
            cr.append_path(&path);
        }
        cr.set_source_rgb(r, g, b);
        cr.set_line_width(2.0);
        cr.set_line_join(gtk::cairo::LineJoin::Round);
        let _ = cr.stroke();
    }
    // The reading under the pointer: the nearest point.
    let Some(hx) = hover.filter(|hx| *hx >= left && *hx <= w - right) else { return };
    let Some(&&(t, p, ref state)) = shown.iter().min_by(|a, b| (x(a.0) - hx).abs().total_cmp(&(x(b.0) - hx).abs())) else { return };
    cr.set_source_rgba(ink.red() as f64, ink.green() as f64, ink.blue() as f64, 0.25);
    cr.set_line_width(1.0);
    cr.move_to(x(t).round() + 0.5, top);
    cr.line_to(x(t).round() + 0.5, top + ph);
    let _ = cr.stroke();
    // The dot, ringed in the surface so it stands off the line.
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.arc(x(t), y(p), 6.0, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
    cr.set_source_rgb(r, g, b);
    cr.arc(x(t), y(p), 4.0, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
    let when = glib::DateTime::from_unix_local(t).and_then(|d| d.format("%a %H:%M")).map(|s| s.to_string()).unwrap_or_default();
    let label = format!("{when} · {p:.0}% · {state}");
    let ext = cr.text_extents(&label).ok();
    let lw = ext.map_or(0.0, |e| e.width());
    let lx = (x(t) + 8.0).min(w - right - lw).max(left);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
    cr.rectangle(lx - 4.0, top, lw + 8.0, 16.0);
    let _ = cr.fill();
    cr.set_source_rgba(ink.red() as f64, ink.green() as f64, ink.blue() as f64, 0.85);
    cr.move_to(lx, top + 12.0);
    let _ = cr.show_text(&label);
}

/// What Look's controls do, given the page and the phone.
type Toast = Rc<dyn Fn(&str)>;

/// The look read from the phone: the accents and the pictures (their small
/// versions coming one by one).
pub fn load_look(p: &Rc<Pages>, host: String, toast: Toast) {
    let p = p.clone();
    glib::spawn_future_local(async move {
        let h = host.clone();
        let Ok(r) = gio::spawn_blocking(move || hythe_core::look::read(&h)).await else { return };
        let look = match r {
            Ok(l) => l,
            Err(e) => {
                p.look_note.set_label(&format!("Could not read item's look: {e}"));
                return;
            }
        };
        show_accents(&p, &host, &look.accent, &toast);
        show_walls(&p, &host, &look, &toast);
    });
}

fn show_accents(p: &Rc<Pages>, host: &str, accent: &str, toast: &Toast) {
    while let Some(c) = p.accents.first_child() {
        p.accents.remove(&c);
    }
    let auto = gtk::Button::builder().label("From the wallpaper").css_classes(["pill"]).build();
    if accent == "auto" {
        auto.add_css_class("suggested-action");
    }
    p.accents.append(&auto);
    let mut choices: Vec<(gtk::Button, String)> = vec![(auto, "auto".to_owned())];
    for (name, hex) in hythe_core::look::PALETTE {
        let b = gtk::Button::builder().css_classes(["swatch"]).tooltip_text(name).valign(gtk::Align::Center).build();
        let css = gtk::CssProvider::new();
        css.load_from_string(&format!("button {{ background: {hex}; }}"));
        #[allow(deprecated)]
        b.style_context().add_provider(&css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        if accent.eq_ignore_ascii_case(hex) {
            b.add_css_class("on");
        }
        p.accents.append(&b);
        choices.push((b, hex.to_owned()));
    }
    for (b, value) in choices {
        let (p, host, toast) = (p.clone(), host.to_owned(), toast.clone());
        b.connect_clicked(move |_| {
            let (p, host, toast, value) = (p.clone(), host.clone(), toast.clone(), value.clone());
            glib::spawn_future_local(async move {
                let (h, v) = (host.clone(), value.clone());
                match gio::spawn_blocking(move || hythe_core::look::set_accent(&h, &v)).await {
                    Ok(Ok(())) => show_accents(&p, &host, &value, &toast),
                    Ok(Err(e)) => toast(&e),
                    Err(_) => {}
                }
            });
        });
    }
}

fn show_walls(p: &Rc<Pages>, host: &str, look: &hythe_core::look::Look, toast: &Toast) {
    p.walls.remove_all();
    let on = if look.each { String::new() } else { look.both.clone() };
    let mut pictures = Vec::new();
    for w in &look.walls {
        let tile = gtk::Overlay::builder().css_classes(["wall-tile"]).width_request(150).height_request(100).halign(gtk::Align::Start).valign(gtk::Align::Start).build();
        if w.name == on {
            tile.add_css_class("on");
        }
        let pic = gtk::Picture::builder().content_fit(gtk::ContentFit::Cover).width_request(150).height_request(100).can_shrink(true).build();
        let click = gtk::Button::builder().child(&pic).css_classes(["flat", "wall-button"]).tooltip_text(if w.credit.is_empty() { w.name.clone() } else { w.credit.clone() }).build();
        click.set_cursor_from_name(Some("pointer"));
        // Held to its width: a picture asks for its own pixels' size.
        tile.set_child(Some(&adw::Clamp::builder().maximum_size(150).tightening_threshold(150).child(&click).build()));
        {
            let (p, host, toast, name) = (p.clone(), host.to_owned(), toast.clone(), w.name.clone());
            click.connect_clicked(move |_| {
                let (p, host, toast, name) = (p.clone(), host.clone(), toast.clone(), name.clone());
                glib::spawn_future_local(async move {
                    let h = host.clone();
                    match gio::spawn_blocking(move || hythe_core::look::set_wallpaper(&h, &name)).await {
                        Ok(Ok(())) => load_look(&p, host, toast),
                        Ok(Err(e)) => toast(&e),
                        Err(_) => {}
                    }
                });
            });
        }
        if w.own {
            let remove = gtk::Button::builder().icon_name("window-close-symbolic").css_classes(["circular", "wall-remove"]).halign(gtk::Align::End).valign(gtk::Align::Start).tooltip_text("Remove from the phone").build();
            let (p, host, toast, name) = (p.clone(), host.to_owned(), toast.clone(), w.name.clone());
            remove.connect_clicked(move |_| {
                let (p, host, toast, name) = (p.clone(), host.clone(), toast.clone(), name.clone());
                glib::spawn_future_local(async move {
                    let h = host.clone();
                    match gio::spawn_blocking(move || hythe_core::look::remove_wall(&h, &name)).await {
                        Ok(Ok(())) => load_look(&p, host, toast),
                        Ok(Err(e)) => toast(&e),
                        Err(_) => {}
                    }
                });
            });
            tile.add_overlay(&remove);
        }
        p.walls.append(&tile);
        pictures.push((w.name.clone(), pic));
    }
    // The small pictures, one by one as they come over the link.
    let (tx, rx) = async_channel::unbounded::<(usize, Vec<u8>)>();
    let names: Vec<String> = pictures.iter().map(|(n, _)| n.clone()).collect();
    let h = host.to_owned();
    gio::spawn_blocking(move || {
        for (i, n) in names.iter().enumerate() {
            if let Ok(bytes) = hythe_core::look::thumb(&h, n) {
                if tx.send_blocking((i, bytes)).is_err() {
                    break;
                }
            }
        }
    });
    glib::spawn_future_local(async move {
        while let Ok((i, bytes)) = rx.recv().await {
            if let (Some((_, pic)), Ok(t)) = (pictures.get(i), gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes))) {
                pic.set_paintable(Some(&t));
            }
        }
    });
}

/// Pictures from this computer added to the phone: each made a .jpg no
/// larger than the screens want (3840 px), then copied.
pub fn add_pictures(p: &Rc<Pages>, window: &gtk::Window, host: String, toast: Toast) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Pictures"));
    filter.add_pixbuf_formats();
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder().title("Add Pictures to the Phone").filters(&filters).modal(true).build();
    let p = p.clone();
    dialog.open_multiple(Some(window), gio::Cancellable::NONE, move |r| {
        let Ok(files) = r else { return };
        let paths: Vec<std::path::PathBuf> = (0..files.n_items()).filter_map(|i| files.item(i).and_downcast::<gio::File>().and_then(|f| f.path())).collect();
        let mut made = Vec::new();
        for path in paths {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let Ok(pixbuf) = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(&path, 3840, 3840, true) else {
                toast(&format!("{} is not a picture this computer can read", path.display()));
                continue;
            };
            let tmp = std::env::temp_dir().join(format!("hythe-{}-{}.jpg", std::process::id(), made.len()));
            if pixbuf.savev(&tmp, "jpeg", &[("quality", "92")]).is_ok() {
                made.push((tmp, stem));
            }
        }
        if made.is_empty() {
            return;
        }
        p.look_note.set_label(&format!("Copying {} picture{} to the phone…", made.len(), if made.len() == 1 { "" } else { "s" }));
        let (p, toast) = (p.clone(), toast.clone());
        glib::spawn_future_local(async move {
            let h = host.clone();
            let out = gio::spawn_blocking(move || {
                let mut errors = Vec::new();
                for (tmp, stem) in &made {
                    if let Err(e) = hythe_core::look::add_wall(&h, tmp, stem, "") {
                        errors.push(e);
                    }
                    let _ = std::fs::remove_file(tmp);
                }
                errors
            })
            .await
            .unwrap_or_default();
            for e in &out {
                toast(e);
            }
            p.look_note.set_label("Click a picture to put it on both panels. Pictures you add here are yours on the phone; the ones item came with stay.");
            load_look(&p, host, toast);
        });
    });
}
