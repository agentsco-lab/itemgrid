//! Cradle's window: what the phone is doing, and the actions on it, over
//! cradle-core - the same actions and safety rules as the command line.
//! Everything that waits on the phone runs off the main thread
//! (gio::spawn_blocking); the window only shows.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use cradle_core::{status, Mode, Seen};
use gtk::{gio, glib};

const APP_ID: &str = "lab.agentsco.Cradle";
const REFRESH_S: u32 = 5;

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build);
    app.run()
}

/// What the window knows between refreshes.
#[derive(Default)]
struct State {
    host: Option<String>,
    /// An update or a reboot under way: no refreshes meanwhile.
    busy: bool,
}

struct Ui {
    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    mode_row: adw::ActionRow,
    mode_icon: gtk::Image,
    actions: gtk::Box,
    progress: adw::PreferencesGroup,
    progress_rows: RefCell<Vec<adw::ActionRow>>,
    details: gtk::Box,
    state: RefCell<State>,
}

/// Disks' bars by how much room is left, not GTK's levels (more is better
/// there: a nearly empty disk came out red).
const CSS: &str = "
levelbar.room-ok block.filled { background-color: @success_color; }
levelbar.room-low block.filled { background-color: @warning_color; }
levelbar.room-out block.filled { background-color: @error_color; }
";

fn build(app: &adw::Application) {
    let css = gtk::CssProvider::new();
    css.load_from_string(CSS);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
    let window = adw::ApplicationWindow::builder().application(app).title("Cradle").default_width(760).default_height(860).build();
    let header = adw::HeaderBar::new();
    let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh.set_tooltip_text(Some("Look again"));
    header.pack_end(&refresh);

    let page = gtk::Box::new(gtk::Orientation::Vertical, 24);
    page.set_margin_top(24);
    page.set_margin_bottom(24);
    page.set_margin_start(16);
    page.set_margin_end(16);

    // The phone's mode, at the top.
    let mode_group = adw::PreferencesGroup::new();
    let mode_row = adw::ActionRow::builder().title("Looking for the phone…").build();
    let mode_icon = gtk::Image::from_icon_name("phone-symbolic");
    mode_icon.set_pixel_size(32);
    mode_row.add_prefix(&mode_icon);
    mode_group.add(&mode_row);
    page.append(&mode_group);

    // The actions.
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    actions.set_halign(gtk::Align::Center);
    let button = |label: &str, icon: &str| {
        let content = adw::ButtonContent::builder().label(label).icon_name(icon).build();
        let b = gtk::Button::builder().child(&content).build();
        b.add_css_class("pill");
        b
    };
    let update = button("Update item", "software-update-available-symbolic");
    update.add_css_class("suggested-action");
    let reboot = button("Reboot", "system-reboot-symbolic");
    let shot = button("Screenshot", "camera-photo-symbolic");
    let logs = button("Logs", "text-x-generic-symbolic");
    for b in [&update, &reboot, &shot, &logs] {
        actions.append(b);
    }
    actions.set_sensitive(false);
    page.append(&actions);

    // An update's or a reboot's steps, shown while one runs.
    let progress = adw::PreferencesGroup::builder().title("Progress").build();
    progress.set_visible(false);
    page.append(&progress);

    // The phone's state, rebuilt at each look.
    let details = gtk::Box::new(gtk::Orientation::Vertical, 24);
    page.append(&details);

    let clamp = adw::Clamp::builder().maximum_size(720).child(&page).build();
    let scroll = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(&clamp).build();
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&scroll));
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&toasts));
    window.set_content(Some(&view));

    let ui = Rc::new(Ui { window: window.clone(), toasts, mode_row, mode_icon, actions, progress, progress_rows: RefCell::default(), details, state: RefCell::default() });

    refresh.connect_clicked({
        let ui = ui.clone();
        move |_| look(&ui)
    });
    update.connect_clicked({
        let ui = ui.clone();
        move |_| ask(&ui, "Update item?", "item is built and installed, and the phone reboots. Enter the PIN when it is back.", "Update", Job::Update)
    });
    reboot.connect_clicked({
        let ui = ui.clone();
        move |_| ask(&ui, "Reboot the phone?", "Enter the PIN when it is back.", "Reboot", Job::Reboot)
    });
    shot.connect_clicked({
        let ui = ui.clone();
        move |_| screenshot(&ui)
    });
    logs.connect_clicked({
        let ui = ui.clone();
        move |_| logs_dialog(&ui)
    });

    look(&ui);
    glib::timeout_add_seconds_local(REFRESH_S, {
        let ui = ui.clone();
        move || {
            if !ui.state.borrow().busy {
                look(&ui);
            }
            glib::ControlFlow::Continue
        }
    });
    window.present();
}

/// Looks at the phone again, off the main thread, and shows what it found.
fn look(ui: &Rc<Ui>) {
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let found = gio::spawn_blocking(|| {
            let seen = cradle_core::detect();
            let status = if seen.mode == Mode::Linux { Some(status::read(&seen.via)) } else { None };
            (seen, status)
        })
        .await;
        let Ok((seen, status)) = found else { return };
        if ui.state.borrow().busy {
            return;
        }
        show(&ui, &seen, status);
    });
}

fn show(ui: &Ui, seen: &Seen, status: Option<Result<status::Status, String>>) {
    let title = match seen.mode {
        Mode::Gone => "Phone not seen".to_owned(),
        mode if seen.via.is_empty() => mode.name().to_owned(),
        mode => format!("{} · {}", mode.name(), seen.via),
    };
    ui.mode_row.set_title(&title);
    ui.mode_row.set_subtitle(seen.mode.means());
    ui.mode_icon.set_icon_name(Some(match seen.mode {
        Mode::Linux => "phone-symbolic",
        Mode::Gone => "network-offline-symbolic",
        _ => "drive-harddisk-solidstate-symbolic",
    }));
    let linux = seen.mode == Mode::Linux;
    ui.state.borrow_mut().host = linux.then(|| seen.via.clone());
    ui.actions.set_sensitive(linux);

    while let Some(child) = ui.details.first_child() {
        ui.details.remove(&child);
    }
    match status {
        Some(Ok(s)) => details(&ui.details, &s),
        Some(Err(e)) => {
            let group = adw::PreferencesGroup::new();
            group.add(&row("Could not read the phone", &e));
            ui.details.append(&group);
        }
        None => {}
    }
}

fn row(title: &str, value: &str) -> adw::ActionRow {
    let r = adw::ActionRow::builder().title(title).subtitle(value).build();
    r.add_css_class("property");
    r
}

/// A row with a bar: how full, and the words.
fn level_row(title: &str, value: &str, fraction: f64) -> adw::ActionRow {
    let r = row(title, value);
    let bar = gtk::LevelBar::builder().min_value(0.0).max_value(1.0).value(fraction.clamp(0.0, 1.0)).valign(gtk::Align::Center).width_request(160).build();
    r.add_suffix(&bar);
    r
}

/// The level bar in a row made by `level_row`.
fn find_bar(row: &adw::ActionRow) -> Option<gtk::LevelBar> {
    let mut stack: Vec<gtk::Widget> = vec![row.clone().upcast()];
    while let Some(w) = stack.pop() {
        if let Ok(bar) = w.clone().downcast::<gtk::LevelBar>() {
            return Some(bar);
        }
        let mut child = w.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            stack.push(c);
        }
    }
    None
}

fn details(into: &gtk::Box, s: &status::Status) {
    let problems: Vec<String> = s.warnings().into_iter().chain(s.failed.iter().map(|u| format!("{u} failed"))).collect();
    if !problems.is_empty() {
        let group = adw::PreferencesGroup::builder().title("Worth a look").build();
        for p in &problems {
            let r = adw::ActionRow::builder().title(p).build();
            r.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
            group.add(&r);
        }
        into.append(&group);
    }

    let item = adw::PreferencesGroup::builder().title("item").build();
    item.add(&row("Version", &s.item));
    item.add(&row("Built", &s.item_built));
    item.add(&row("State", if s.item_running { "running" } else { "not running" }));
    into.append(&item);

    let power = adw::PreferencesGroup::builder().title("Power and heat").build();
    let battery = s.battery.map(|b| format!("{b}%, {}", s.battery_status.to_lowercase())).unwrap_or_else(|| "unknown".into());
    power.add(&level_row("Battery", &battery, s.battery.unwrap_or(0) as f64 / 100.0));
    if let Some(t) = s.battery_temp {
        power.add(&row("Battery temperature", &format!("{t:.0} °C")));
    }
    if let Some(t) = s.cpu_temp {
        power.add(&row("CPU temperature", &format!("{t:.0} °C")));
    }
    into.append(&power);

    let storage = adw::PreferencesGroup::builder().title("Storage").build();
    for (mount, size, free) in &s.disks {
        let used = if *size > 0 { 1.0 - *free as f64 / *size as f64 } else { 0.0 };
        let mut words = format!("{} free of {}", status::size_words(*free), status::size_words(*size));
        // /userdata is filled by the rootfs image on purpose; what is left
        // is the Android container's room (status.rs's warning).
        let room = if mount == "/userdata" {
            words.push_str(" · the rootfs image fills it");
            match *free {
                f if f < 512 * 1024 => "room-out",
                f if f < 1024 * 1024 => "room-low",
                _ => "room-ok",
            }
        } else {
            match used {
                u if u > 0.95 => "room-out",
                u if u > 0.85 => "room-low",
                _ => "room-ok",
            }
        };
        let r = level_row(mount, &words, used);
        if let Some(bar) = r.last_child().and_then(|_| find_bar(&r)) {
            for name in ["low", "high", "full"] {
                bar.remove_offset_value(Some(name));
            }
            bar.add_css_class(room);
        }
        storage.add(&r);
    }
    into.append(&storage);

    let system = adw::PreferencesGroup::builder().title("System").build();
    system.add(&row("Operating system", &s.os));
    system.add(&row("Kernel", &s.kernel));
    system.add(&row("Up", &format!("{} h {} min", s.uptime_s / 3600, s.uptime_s / 60 % 60)));
    system.add(&row("Port", &s.port));
    system.add(&row("sensorfw", &s.sensorfw));
    into.append(&system);
}

#[derive(Clone, Copy)]
enum Job {
    Update,
    Reboot,
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
            run_job(&ui2, job);
        }
    });
    dialog.present(Some(&ui.window));
}

/// An update or a reboot, its steps shown as they come.
fn run_job(ui: &Rc<Ui>, job: Job) {
    let Some(host) = ui.state.borrow().host.clone() else { return };
    ui.state.borrow_mut().busy = true;
    ui.actions.set_sensitive(false);
    for r in ui.progress_rows.borrow_mut().drain(..) {
        ui.progress.remove(&r);
    }
    ui.progress.set_visible(true);
    let (tx, rx) = async_channel::unbounded::<String>();
    let started = std::time::Instant::now();
    let work = gio::spawn_blocking(move || {
        let say = |words: &str| {
            let _ = tx.send_blocking(format!("{:.0} s · {words}", started.elapsed().as_secs_f64()));
        };
        match job {
            Job::Update => cradle_core::update::update(&host, true, &mut |step| say(step.words())),
            Job::Reboot => cradle_core::phone::reboot(&host, &mut |b| say(b.words())),
        }
    });
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        while let Ok(line) = rx.recv().await {
            let r = adw::ActionRow::builder().title(&line).build();
            r.add_prefix(&gtk::Image::from_icon_name("emblem-ok-symbolic"));
            ui.progress.add(&r);
            ui.progress_rows.borrow_mut().push(r);
        }
        let result = work.await.unwrap_or_else(|_| Err("the work stopped".into()));
        ui.state.borrow_mut().busy = false;
        let toast = match &result {
            Ok(()) => adw::Toast::new("Done: enter the PIN on the phone"),
            Err(e) => adw::Toast::new(e),
        };
        toast.set_timeout(6);
        ui.toasts.add_toast(toast);
        look(&ui);
    });
}

/// A screenshot, saved in ~/cradle-shots and shown.
fn screenshot(ui: &Rc<Ui>) {
    let Some(host) = ui.state.borrow().host.clone() else { return };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let saved = gio::spawn_blocking(move || -> Result<std::path::PathBuf, String> {
            let png = cradle_core::screenshot::take(&host).and_then(|rgba| cradle_core::screenshot::png(&rgba, false))?;
            let dir = std::path::Path::new(&std::env::var("HOME").unwrap_or_default()).join("cradle-shots");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let stamp = glib::DateTime::now_local().and_then(|d| d.format("%Y-%m-%d-%H%M%S")).map(|s| s.to_string()).unwrap_or_default();
            let path = dir.join(format!("{stamp}.png"));
            std::fs::write(&path, png).map_err(|e| e.to_string())?;
            Ok(path)
        })
        .await
        .unwrap_or_else(|_| Err("the work stopped".into()));
        match saved {
            Ok(path) => {
                let picture = gtk::Picture::for_filename(&path);
                picture.set_content_fit(gtk::ContentFit::Contain);
                let header = adw::HeaderBar::new();
                let open = gtk::Button::with_label("Open folder");
                let folder = path.parent().map(|p| p.to_path_buf());
                open.connect_clicked(move |_| {
                    if let Some(f) = &folder {
                        let _ = gio::AppInfo::launch_default_for_uri(&gio::File::for_path(f).uri(), gio::AppLaunchContext::NONE);
                    }
                });
                header.pack_start(&open);
                let view = adw::ToolbarView::new();
                view.add_top_bar(&header);
                view.set_content(Some(&picture));
                let dialog = adw::Dialog::builder().title(path.file_name().and_then(|n| n.to_str()).unwrap_or("Screenshot")).content_width(1000).content_height(720).child(&view).build();
                dialog.present(Some(&ui.window));
            }
            Err(e) => ui.toasts.add_toast(adw::Toast::new(&e)),
        }
    });
}

/// The phone's journal: a part, a pattern, this boot or the one before.
fn logs_dialog(ui: &Rc<Ui>) {
    let Some(host) = ui.state.borrow().host.clone() else { return };
    let parts = ["all", "item", "sensorfw", "kernel", "posture", "pen"];
    let part = gtk::DropDown::from_strings(&parts);
    let search = gtk::SearchEntry::builder().placeholder_text("Search").build();
    let previous = gtk::ToggleButton::with_label("Previous boot");
    let header = adw::HeaderBar::new();
    header.pack_start(&part);
    header.pack_start(&previous);
    header.set_title_widget(Some(&search));
    let text = gtk::TextView::builder().editable(false).monospace(true).wrap_mode(gtk::WrapMode::WordChar).left_margin(12).right_margin(12).top_margin(8).bottom_margin(8).build();
    let scroll = gtk::ScrolledWindow::builder().child(&text).vexpand(true).build();
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&scroll));
    let dialog = adw::Dialog::builder().title("Logs").content_width(1000).content_height(720).child(&view).build();

    let load = Rc::new({
        let (part, search, previous, text, scroll) = (part.clone(), search.clone(), previous.clone(), text.clone(), scroll.clone());
        move || {
            let q = cradle_core::logs::Query {
                boot: if previous.is_active() { -1 } else { 0 },
                only: match part.selected() {
                    0 => None,
                    i => Some(parts[i as usize].to_owned()),
                },
                grep: Some(search.text().to_string()).filter(|s| !s.is_empty()),
                lines: Some(500),
                ..Default::default()
            };
            let host = host.clone();
            let (text, scroll) = (text.clone(), scroll.clone());
            glib::spawn_future_local(async move {
                let out = gio::spawn_blocking(move || cradle_core::phone::run(&host, &q.script())).await.unwrap_or_else(|_| Err("the work stopped".into()));
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
    load();
    dialog.present(Some(&ui.window));
}
