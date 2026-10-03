//! Cradle's window, in the spirit of Finder's page for a connected iPhone:
//! the Duo on the left - its two panels showing what is on them - with its
//! name, mode and battery; on the right Software, Backups, Screen and System;
//! the storage as one bar along the bottom. Over cradle-core: the same
//! actions and safety rules as the command line. Everything that waits on the
//! phone runs off the main thread (gio::spawn_blocking); the window only
//! shows.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use cradle_core::{screenshot, status, Mode, Seen};
use gtk::{gdk, gio, glib};

const APP_ID: &str = "lab.agentsco.Cradle";
const REFRESH_S: u32 = 5;
/// The Duo drawn on the left: a panel's size (logical px).
const PANEL_W: i32 = 186;
const PANEL_H: i32 = 248;
/// The storage bar's parts' colours, in the order of `Parts::list`.
const PART_COLOURS: [(f64, f64, f64); 4] = [(0.21, 0.52, 0.89), (0.20, 0.82, 0.48), (1.0, 0.47, 0.0), (0.57, 0.25, 0.67)];

const CSS: &str = "
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
.section-title { font-weight: 700; font-size: 1.15em; }
.fact-name { opacity: 0.55; }
.storage-legend-dot { min-width: 10px; min-height: 10px; border-radius: 5px; }
.dot-free { background: alpha(currentColor, 0.18); }
.bottom-bar { padding: 14px 24px 16px 24px; }
.duo-floor {
  min-height: 26px;
  margin: -6px 10px 0 10px;
  background: radial-gradient(ellipse at center, alpha(black, 0.55) 0%, alpha(black, 0.0) 70%);
}
";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build);
    app.run()
}

/// What the window knows between looks.
#[derive(Default)]
struct State {
    host: Option<String>,
    /// An update or a reboot under way: no looks meanwhile.
    busy: bool,
    /// The phone's screens were taken once since it came: not again on each
    /// look (a frame takes seconds).
    pictured: bool,
}

struct Ui {
    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    banner: adw::Banner,
    /// The phone's page, or the page asking for it.
    pages: gtk::Stack,
    switcher: adw::ViewSwitcher,
    away: adw::StatusPage,
    screens: [gtk::Picture; 2],
    name_sub: gtk::Label,
    battery: gtk::Label,
    software: gtk::Label,
    actions: gtk::Box,
    progress: gtk::ListBox,
    facts: gtk::Grid,
    storage: gtk::DrawingArea,
    legend: gtk::Box,
    bottom: gtk::Box,
    tabs: adw::ViewStack,
    /// The system disk by part, for the bar; counted when the phone comes.
    parts: RefCell<Option<cradle_core::storage::Parts>>,
    state: RefCell<State>,
}

fn build(app: &adw::Application) {
    let css = gtk::CssProvider::new();
    css.load_from_string(CSS);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
    let window = adw::ApplicationWindow::builder().application(app).title("Cradle").default_width(980).default_height(700).build();

    // The tabs, in the header as Finder has them.
    let stack = adw::ViewStack::new();
    let switcher = adw::ViewSwitcher::builder().stack(&stack).policy(adw::ViewSwitcherPolicy::Wide).build();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&switcher));
    let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh.set_tooltip_text(Some("Look again"));
    header.pack_end(&refresh);

    // General: the Duo on the left, the sections on the right.
    let general = gtk::Box::new(gtk::Orientation::Horizontal, 40);
    general.set_margin_top(32);
    general.set_margin_bottom(24);
    general.set_margin_start(40);
    general.set_margin_end(40);

    let device = gtk::Box::new(gtk::Orientation::Vertical, 14);
    device.set_valign(gtk::Align::Start);
    let duo = gtk::Box::new(gtk::Orientation::Horizontal, 3);
    duo.set_halign(gtk::Align::Center);
    let screens = [0, 1].map(|_| {
        let picture = gtk::Picture::builder().content_fit(gtk::ContentFit::Cover).width_request(PANEL_W).height_request(PANEL_H).build();
        picture.add_css_class("duo-screen");
        picture.set_overflow(gtk::Overflow::Hidden);
        picture
    });
    for (i, screen) in screens.iter().enumerate() {
        if i == 1 {
            let hinge = gtk::Box::new(gtk::Orientation::Vertical, 0);
            hinge.add_css_class("duo-hinge");
            duo.append(&hinge);
        }
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        panel.add_css_class("duo-panel");
        panel.append(screen);
        duo.append(&panel);
    }
    device.append(&duo);
    // Its shadow on the table.
    let floor = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    floor.add_css_class("duo-floor");
    device.append(&floor);
    let name = gtk::Label::builder().label("Surface Duo").css_classes(["title-1"]).margin_top(10).build();
    let name_sub = gtk::Label::builder().css_classes(["dim-label"]).build();
    let battery = gtk::Label::new(None);
    device.append(&name);
    device.append(&name_sub);
    device.append(&battery);
    general.append(&device);

    let sections = gtk::Box::new(gtk::Orientation::Vertical, 22);
    sections.set_hexpand(true);
    let section = |title: &str| {
        let b = gtk::Box::new(gtk::Orientation::Vertical, 8);
        b.append(&gtk::Label::builder().label(title).css_classes(["section-title"]).xalign(0.0).build());
        b
    };
    let body = |text: &str| gtk::Label::builder().label(text).wrap(true).xalign(0.0).css_classes(["dim-label"]).build();
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
    soft_row.append(&update);
    soft_row.append(&reboot);
    soft.append(&soft_row);
    let progress = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).margin_top(6).build();
    progress.set_visible(false);
    soft.append(&progress);
    actions.append(&soft);

    // Backups: with flashing, the next stage.
    let back = section("Backups");
    back.append(&body("Back up the phone's system to this computer, and put it back when something goes wrong. Coming with flashing: a RAM boot first, one change per boot."));
    let back_row = row();
    let backup = pill("Back Up Now");
    let restore = pill("Restore Backup…");
    for b in [&backup, &restore] {
        b.set_sensitive(false);
        b.set_tooltip_text(Some("Coming with flashing"));
        back_row.append(b);
    }
    back.append(&back_row);
    actions.append(&back);

    // Screen.
    let scr = section("Screen");
    scr.append(&body("What both panels show, on the Duo here and saved to ~/cradle-shots."));
    let scr_row = row();
    let shot = pill("Take Screenshot");
    let folder = pill("Open Folder");
    scr_row.append(&shot);
    scr_row.append(&folder);
    scr.append(&scr_row);
    actions.append(&scr);
    sections.append(&actions);

    // System: a few facts.
    let sys = section("System");
    let facts = gtk::Grid::builder().row_spacing(6).column_spacing(18).build();
    sys.append(&facts);
    sections.append(&sys);

    let scroll = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(&sections).hexpand(true).build();
    general.append(&scroll);

    // The storage along the bottom.
    let storage = gtk::DrawingArea::builder().content_height(18).hexpand(true).build();
    let legend = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let bottom = gtk::Box::new(gtk::Orientation::Vertical, 8);
    bottom.add_css_class("bottom-bar");
    bottom.append(&storage);
    bottom.append(&legend);

    general.set_vexpand(true);
    stack.add_titled_with_icon(&general, Some("general"), "General", "phone-symbolic");

    // Logs.
    let owner: Rc<RefCell<Option<Rc<Ui>>>> = Rc::default();
    let logs_page = logs_view(owner.clone());
    stack.add_titled_with_icon(&logs_page, Some("logs"), "Logs", "text-x-generic-symbolic");

    // The page asking for the phone, when it is not there.
    let away = adw::StatusPage::builder().icon_name("phone-symbolic").title("Connect your Surface Duo").description("Plug it in with a USB cable. Cradle sees it when Linux is up, in fastboot, or in recovery.").build();

    let pages = gtk::Stack::new();
    pages.add_named(&stack, Some("phone"));
    pages.add_named(&away, Some("away"));
    pages.set_visible_child_name("away");

    let banner = adw::Banner::new("");
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&banner);
    content.append(&pages);
    pages.set_vexpand(true);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&content));
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&toasts));
    // The storage bar along the window's bottom, on General only.
    view.add_bottom_bar(&bottom);
    bottom.set_visible(false);
    window.set_content(Some(&view));

    let ui = Rc::new(Ui {
        window: window.clone(),
        toasts,
        banner,
        pages,
        switcher,
        away,
        screens,
        name_sub,
        battery,
        software,
        actions,
        progress,
        facts,
        storage: storage.clone(),
        legend,
        bottom,
        tabs: stack.clone(),
        parts: RefCell::default(),
        state: RefCell::default(),
    });
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
    shot.connect_clicked({
        let ui = ui.clone();
        move |_| take_screens(&ui, true)
    });
    folder.connect_clicked(|_| {
        let dir = shots_dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = gio::AppInfo::launch_default_for_uri(&gio::File::for_path(&dir).uri(), gio::AppLaunchContext::NONE);
    });

    stack.connect_visible_child_notify({
        let ui = Rc::downgrade(&ui);
        move |_| {
            if let Some(ui) = ui.upgrade() {
                bottom_shown(&ui);
            }
        }
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
    std::path::Path::new(&std::env::var("HOME").unwrap_or_default()).join("cradle-shots")
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

fn show(ui: &Rc<Ui>, seen: &Seen, status: Option<Result<status::Status, String>>) {
    let linux = seen.mode == Mode::Linux;
    let was = ui.state.borrow().host.clone();
    ui.state.borrow_mut().host = linux.then(|| seen.via.clone());
    if !linux {
        ui.state.borrow_mut().pictured = false;
        ui.away.set_title(match seen.mode {
            Mode::Gone => "Connect your Surface Duo",
            Mode::Fastboot => "The Duo is in fastboot",
            Mode::Recovery => "The Duo is in recovery",
            _ => "The Duo runs Android",
        });
        let how = if seen.mode == Mode::Gone {
            "Plug it in with a USB cable. Cradle sees it when Linux is up, in fastboot, or in recovery.".to_owned()
        } else {
            format!("{} - {}.", seen.via, seen.mode.means())
        };
        ui.away.set_description(Some(&how));
        ui.pages.set_visible_child_name("away");
        ui.switcher.set_visible(false);
        ui.banner.set_revealed(false);
        bottom_shown(ui);
        return;
    }
    ui.pages.set_visible_child_name("phone");
    ui.switcher.set_visible(true);
    ui.name_sub.set_label(&format!("Linux · {}", seen.via));
    match status {
        Some(Ok(s)) => fill(ui, &s),
        Some(Err(e)) => {
            ui.banner.set_title(&format!("Could not read the phone: {e}"));
            ui.banner.set_revealed(true);
        }
        None => {}
    }
    bottom_shown(ui);
    // The screens and the storage, once each time the phone comes.
    if was.is_none() || !ui.state.borrow().pictured {
        ui.state.borrow_mut().pictured = true;
        take_screens(ui, false);
        count_storage(ui);
    }
}

fn fill(ui: &Ui, s: &status::Status) {
    let problems: Vec<String> = s.warnings().into_iter().chain(s.failed.iter().map(|u| format!("{u} failed"))).collect();
    ui.banner.set_title(&problems.join(" · "));
    ui.banner.set_revealed(!problems.is_empty());

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

}

/// The storage bar's parts counted again, and the bar and its legend shown.
fn count_storage(ui: &Rc<Ui>) {
    let Some(host) = ui.state.borrow().host.clone() else { return };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let Ok(Ok(parts)) = gio::spawn_blocking(move || cradle_core::storage::read(&host)).await else { return };
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
        *ui.parts.borrow_mut() = Some(parts);
        ui.storage.queue_draw();
    });
}

/// The bottom bar on General only, with the phone there.
fn bottom_shown(ui: &Ui) {
    let general = ui.tabs.visible_child_name().as_deref() == Some("general");
    ui.bottom.set_visible(general && ui.state.borrow().host.is_some());
}

/// The phone's screens onto the Duo drawn here; saved too if `save`.
fn take_screens(ui: &Rc<Ui>, save: bool) {
    let Some(host) = ui.state.borrow().host.clone() else { return };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let got = gio::spawn_blocking(move || -> Result<(Vec<u8>, Option<std::path::PathBuf>), String> {
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
            Ok((rgba, saved))
        })
        .await
        .unwrap_or_else(|_| Err("the work stopped".into()));
        match got {
            Ok((rgba, saved)) => {
                let (w, h) = screenshot::PANEL_SIZE;
                for (picture, pixels) in ui.screens.iter().zip(screenshot::panels(&rgba)) {
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

/// An update or a reboot, its steps shown under Software as they come.
fn run_job(ui: &Rc<Ui>, job: Job) {
    let Some(host) = ui.state.borrow().host.clone() else { return };
    ui.state.borrow_mut().busy = true;
    ui.actions.set_sensitive(false);
    while let Some(child) = ui.progress.first_child() {
        ui.progress.remove(&child);
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
        // Each step: a spinner while it runs, a tick once the next comes.
        let mut last: Option<(adw::ActionRow, gtk::Spinner)> = None;
        while let Ok(line) = rx.recv().await {
            if let Some((prev, spin)) = last.take() {
                spin.set_visible(false);
                prev.add_prefix(&gtk::Image::from_icon_name("emblem-ok-symbolic"));
            }
            let r = adw::ActionRow::builder().title(&line).build();
            let spin = gtk::Spinner::builder().spinning(true).build();
            r.add_prefix(&spin);
            ui.progress.append(&r);
            last = Some((r, spin));
        }
        let result = work.await.unwrap_or_else(|_| Err("the work stopped".into()));
        if let Some((prev, spin)) = last {
            spin.set_visible(false);
            prev.add_prefix(&gtk::Image::from_icon_name(if result.is_ok() { "emblem-ok-symbolic" } else { "dialog-error-symbolic" }));
        }
        ui.state.borrow_mut().busy = false;
        ui.state.borrow_mut().pictured = false;
        ui.actions.set_sensitive(true);
        let toast = match &result {
            Ok(()) => adw::Toast::new("Done: enter the PIN on the phone"),
            Err(e) => adw::Toast::new(e),
        };
        toast.set_timeout(6);
        ui.toasts.add_toast(toast);
        look(&ui);
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
    reload.connect_clicked({
        let load = load.clone();
        move |_| load()
    });
    // Read when the tab is first shown.
    page.connect_map(move |_| load());
    page
}
