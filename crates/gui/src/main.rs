//! Cradle's window, in the spirit of Finder's page for a connected iPhone.
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
//! the storage as one bar along the bottom. Over cradle-core: the same
//! actions and safety rules as the command line. Everything that waits on the
//! phone runs off the main thread (gio::spawn_blocking); the window only
//! shows.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use cradle_core::{screenshot, status, Mode};
use gtk::{gdk, gio, glib};

mod card;
mod journey;

const APP_ID: &str = "lab.agentsco.Cradle";
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
const DUO_PX_PER_MM: f64 = 2.35;
/// The drawn Duo's room, in its body's heights and widths: the raised half
/// above it, its near edge wider in perspective.
const DUO_ROOM: f64 = 1.25;
const DUO_ROOM_W: f64 = 1.3;
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
.mode-title { font-weight: 800; font-size: 1.2em; }
.status-dot { min-width: 12px; min-height: 12px; border-radius: 6px; }
.status-dot.fine { background: #33d17a; }
.status-dot.look { background: #f6d32d; }
.status-dot.busy { background: #62a0ea; }
.status-dot.away { background: #77767b; }
.status-title { font-weight: 800; font-size: 1.9em; }
.repair-row-title { font-weight: 700; }
.wordmark { font-family: Lato, Ubuntu, sans-serif; font-weight: 300; font-size: 54px; letter-spacing: 0.32em; opacity: 0.88; }
.free-label { opacity: 0.6; font-size: 0.9em; }
";

fn main() -> glib::ExitCode {
    // Ubuntu 24.04 lets no unconfined program make user namespaces, and
    // WebKit's sandbox needs them: without Cradle's AppArmor profile
    // (data/apparmor) the Microsoft window would bring the whole app down.
    // Then WebKit runs unsandboxed - and that window goes to Microsoft's
    // sign-in and support pages only (see microsoft_only).
    let restricted = std::fs::read_to_string("/proc/sys/kernel/apparmor_restrict_unprivileged_userns").is_ok_and(|v| v.trim() == "1");
    if restricted && !std::path::Path::new("/etc/apparmor.d/cradle-gui").exists() {
        std::env::set_var("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", "1");
    }
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build);
    app.run()
}

/// Where the phone is, as Cradle sees it.
#[derive(Clone, Default, PartialEq)]
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
    /// The Duo drawn: the right half (and its back) turned as the phone
    /// folds; the angle shown and the one to go to.
    duo: gtk::Fixed,
    duo_left: gtk::Picture,
    duo_right: gtk::Picture,
    duo_back: gtk::Box,
    shades: [gtk::Box; 2],
    floor: gtk::Box,
    spine: gtk::Picture,
    /// The angle the drawn Duo shows, and the one to go to.
    fold: std::cell::Cell<(f64, f64)>,
    /// How the phone is tipped from lying flat (pitch about its width, roll
    /// about its length, degrees, from its gravity): shown, and to go to.
    tilt: std::cell::Cell<([f64; 2], [f64; 2])>,
    /// The hinge followed (posture.rs): where, and its stop.
    following: RefCell<Option<(String, cradle_core::posture::Stop)>>,
    /// The simple page and its parts.
    home: gtk::Box,
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
    parts: RefCell<Option<cradle_core::storage::Parts>>,
    /// The live view running (its stop), and when it last failed - not
    /// tried again for a while (an item without a mirror).
    live: RefCell<Option<cradle_core::live::Stop>>,
    live_failed: RefCell<Option<std::time::Instant>>,
    live_badge: gtk::Label,
    state: RefCell<State>,
}

/// A phone not seen for this long is away; before that, restarting.
const GONE_AFTER_S: u64 = 90;

fn build(app: &adw::Application) {
    let css = gtk::CssProvider::new();
    css.load_from_string(&format!("{CSS}{}", card::CSS));
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
    let window = adw::ApplicationWindow::builder().application(app).title("Cradle").default_width(980).default_height(720).build();

    // The tabs, in the header as Finder has them.
    let stack = adw::ViewStack::new();
    let switcher = adw::ViewSwitcher::builder().stack(&stack).policy(adw::ViewSwitcherPolicy::Wide).visible(false).build();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&switcher));
    let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh.set_tooltip_text(Some("Look again"));
    header.pack_end(&refresh);
    refresh.set_visible(developer_mode());
    // The menu: Developer Mode.
    let menu = gio::Menu::new();
    menu.append(Some("Settings"), Some("win.settings"));
    let menu_button = gtk::MenuButton::builder().icon_name("open-menu-symbolic").menu_model(&menu).tooltip_text("Menu").build();
    header.pack_end(&menu_button);

    // General: the Duo on the left, the sections on the right.
    let general = gtk::Box::new(gtk::Orientation::Horizontal, 40);
    general.set_margin_top(32);
    general.set_margin_bottom(24);
    general.set_margin_start(40);
    general.set_margin_end(40);

    let device = gtk::Box::new(gtk::Orientation::Vertical, 14);
    device.set_valign(gtk::Align::Start);
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
    let leaf = |paintable: Option<&gdk::Texture>, w: i32| gtk::Picture::builder().content_fit(gtk::ContentFit::Fill).width_request(w).height_request(bh).can_shrink(true).paintable(paintable.map(|t| t.clone().upcast::<gdk::Paintable>()).as_ref().unwrap_or(&gdk::Paintable::new_empty(w, bh))).build();
    // The live screens: pictures held, not shown - each half drawn with its.
    let screens = [0, 1].map(|_| gtk::Picture::new());
    let bodies = [texture(include_bytes!("../data/duo-left.svg"), bw), texture(include_bytes!("../data/duo-right.svg"), bw)];
    let halves = [0, 1].map(|_| leaf(None, mid));
    for i in 0..2 {
        let body = bodies[i].clone();
        let half = halves[i].clone();
        let draw = move |screen: Option<gdk::Paintable>| half.set_paintable(Some(&duo_half(body.as_ref(), screen.as_ref(), i, bw, bh)));
        draw(None);
        screens[i].connect_paintable_notify(move |p| draw(p.paintable()));
    }
    let [duo_left, duo_right] = halves.clone();
    // Its light on the raised half; its glacier back past a right angle.
    let shade = |class: &str| gtk::Box::builder().width_request(mid).height_request(bh).css_classes(["duo-shade", class]).opacity(0.0).can_target(false).build();
    let (shade_left, shade_right) = (shade("duo-half-left"), shade("duo-half-right"));
    let duo_back = gtk::Box::builder().width_request(mid).height_request(bh).css_classes(["duo-back"]).visible(false).build();
    // Its shadow on the table, soft, under the half lying there.
    let floor = gtk::Box::builder().width_request(mid).height_request(bh).css_classes(["duo-floor"]).can_target(false).build();
    let spine = leaf(texture(include_bytes!("../data/duo-spine.svg"), bw).as_ref(), bw);
    for w in [floor.upcast_ref::<gtk::Widget>(), duo_left.upcast_ref(), shade_left.upcast_ref(), duo_back.upcast_ref(), duo_right.upcast_ref(), shade_right.upcast_ref(), spine.upcast_ref()] {
        duo.put(w, 0.0, 0.0);
    }
    let duo_sized = gtk::Overlay::builder().halign(gtk::Align::Center).build();
    duo_sized.set_child(Some(&gtk::Box::builder().width_request((bw as f64 * DUO_ROOM_W) as i32).height_request(room).build()));
    duo_sized.add_overlay(&duo);
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
    let live_badge = gtk::Label::builder().label("● LIVE").css_classes(["live-badge"]).build();
    live_badge.set_visible(false);
    device.append(&live_badge);
    let name = gtk::Label::builder().label("Surface Duo").css_classes(["title-1"]).margin_top(10).build();
    let join = gtk::Button::builder().label("Join the Club…").css_classes(["pill"]).halign(gtk::Align::Center).build();
    join.set_tooltip_text(Some("A number for this Duo in the owners' club on agentsco.uk (00001...)"));
    join.set_visible(false);
    let name_sub = gtk::Label::builder().css_classes(["dim-label"]).build();
    let battery = gtk::Label::new(None);
    device.append(&name);
    device.append(&name_sub);
    device.append(&battery);
    device.append(&join);
    general.append(&device);

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
    android.append(&body("Stock Android can come back for a while. Cradle backs everything up first and tests the way back, then clears Linux's data and starts Android. Back to Linux puts it all back from the backup."));
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
    scr.append(&body("What both panels show, on the Duo here and saved to ~/cradle-shots."));
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
    let home = gtk::Box::new(gtk::Orientation::Vertical, 22);
    let status_head = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    let status_dot = gtk::Box::builder().css_classes(["status-dot", "fine"]).valign(gtk::Align::Center).build();
    let status_title = gtk::Label::builder().label("Your Duo is fine").xalign(0.0).wrap(true).css_classes(["status-title"]).build();
    status_head.append(&status_dot);
    status_head.append(&status_title);
    let status_lines = gtk::Label::builder().xalign(0.0).wrap(true).max_width_chars(60).css_classes(["dim-label"]).margin_start(26).build();
    let status_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
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
    sections.append(&home);
    sections.append(&linux_only);

    let scroll = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(&sections).hexpand(true).build();
    // Repair & Reset: what is done once in a while, each asking first, all
    // on the cable.
    let repair = gtk::Box::new(gtk::Orientation::Vertical, 16);
    let repair_back = gtk::Button::builder().icon_name("go-previous-symbolic").css_classes(["flat", "circular"]).halign(gtk::Align::Start).tooltip_text("Back").build();
    repair.append(&repair_back);
    repair.append(&gtk::Label::builder().label("Settings").xalign(0.0).css_classes(["status-title"]).build());
    // Developer Mode: what the simple page leaves out.
    let dev_list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build();
    let dev_row = adw::SwitchRow::builder().title("Developer Mode").subtitle("Slots, images from RAM, every kind of backup, item built from your tree, the logs.").active(developer_mode()).build();
    dev_list.append(&dev_row);
    repair.append(&dev_list);
    repair.append(&gtk::Label::builder().label("Repair & Reset").xalign(0.0).css_classes(["section-title"]).margin_top(10).build());
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
    let repair_scroll = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(&repair).hexpand(true).build();
    let right = gtk::Stack::builder().transition_type(gtk::StackTransitionType::SlideLeftRight).transition_duration(250).hexpand(true).build();
    right.add_named(&scroll, Some("main"));
    right.add_named(&repair_scroll, Some("repair"));
    general.append(&right);

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
    // The start: the word, while the phone is first looked for.
    let splash = gtk::Label::builder().label("cradle").css_classes(["wordmark"]).halign(gtk::Align::Center).valign(gtk::Align::Center).build();
    pages.add_named(&splash, Some("splash"));
    pages.set_visible_child_name("splash");

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
        screens,
        duo_mode,
        duo_mode_label,
        name,
        join: join.clone(),
        serial: RefCell::default(),
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
        cable_only: [&reinstall, &restore, &try_ram, &backup_all, &back_full, &to_android, &r_reinstall, &r_restore, &r_android].into_iter().map(|b| (b.clone(), b.tooltip_text())).collect(),
        duo: duo.clone(),
        duo_left: duo_left.clone(),
        duo_right: duo_right.clone(),
        duo_back: duo_back.clone(),
        shades: [shade_left.clone(), shade_right.clone()],
        floor: floor.clone(),
        spine: spine.clone(),
        fold: std::cell::Cell::new((180.0, 180.0)),
        following: RefCell::default(),
        tilt: std::cell::Cell::new(([0.0; 2], [0.0; 2])),
        home,
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

    // Settings (the menu): Developer Mode, kept between runs, and Repair &
    // Reset.
    let settings = gio::SimpleAction::new("settings", None);
    settings.connect_activate({
        let right = right.clone();
        move |_, _| right.set_visible_child_name("repair")
    });
    window.add_action(&settings);
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
    repair_back.connect_clicked({
        let right = right.clone();
        move |_| right.set_visible_child_name("main")
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
        move |_| join_club(&ui)
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
            let newest = cradle_core::backup::list(Some(&serial)).into_iter().find(|b| b.manifest.kind == cradle_core::backup::Kind::Full);
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
            st.dismissed = cradle_core::activity::elsewhere(u64::MAX).and_then(|a| a.ended_at).or(st.dismissed);
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
    glib::timeout_add_seconds_local(REFRESH_S, {
        let ui = ui.clone();
        let mut ticks = 0u32;
        move || {
            ticks += 1;
            let (busy, elsewhere) = {
                let st = ui.state.borrow();
                (st.busy, st.elsewhere)
            };
            // Over Wi-Fi a look costs the phone's radio: every 10 s with the
            // window in front, every 30 s behind it.
            let wifi = ui.state.borrow().host.as_deref().is_some_and(|h| cradle_core::link::Via::of(h) == cradle_core::link::Via::Wifi);
            let every = if !wifi { 1 } else if ui.window.is_active() { 2 } else { 6 };
            if !busy && !elsewhere && ticks % every == 0 {
                look(&ui);
                live_sync(&ui);
                // Without the live view (an item without a mirror), a picture
                // now and then.
                let front = ui.window.is_active() && ui.tabs.visible_child_name().as_deref() == Some("general");
                if front && ui.live.borrow().is_none() && ticks % SCREENS_EVERY == 0 {
                    take_screens(&ui, false);
                }
            }
            glib::ControlFlow::Continue
        }
    });
    // The fold, eased toward the hinge's angle each frame; the angle read
    // each second while the window is in front (each 5 s behind it).
    ui.duo.add_tick_callback({
        let ui = Rc::downgrade(&ui);
        move |_, clock| {
            let Some(ui) = ui.upgrade() else { return glib::ControlFlow::Break };
            let _ = clock;
            let k = 1.0 - (-1.0f64 / 60.0 / 0.05).exp();
            let (shown, to) = ui.fold.get();
            let (tilt, tilt_to) = ui.tilt.get();
            let far = (shown - to).abs() > 0.05 || (0..2).any(|i| (tilt[i] - tilt_to[i]).abs() > 0.05);
            if far {
                let now = shown + (to - shown) * k;
                ui.fold.set((now, to));
                ui.tilt.set(([0, 1].map(|i| tilt[i] + (tilt_to[i] - tilt[i]) * k), tilt_to));
                show_fold(&ui, now);
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
            glib::ControlFlow::Continue
        }
    });
    // CRADLE_SHOT=file.png: the window drawn into a picture 4 s after the start
    // (to see it without a screen grab).
    if let Some(path) = std::env::var_os("CRADLE_SHOT") {
        let window = ui.window.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(std::env::var("CRADLE_SHOT_AFTER").ok().and_then(|v| v.parse().ok()).unwrap_or(4)), move || {
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
            ui.card.show(job.kind, &job.lines, secs, job.ended.clone(), false);
            if job.ended.is_none() {
                duo_moving(ui, ui.card.phone(job.kind, &job.lines));
            }
            return;
        }
    }
    let other = cradle_core::activity::elsewhere(15 * 60);
    let dismissed = ui.state.borrow().dismissed;
    match other {
        Some(a) if a.ended_at.is_none() || a.ended_at != dismissed => {
            let running = a.ended_at.is_none();
            ui.card.show(&a.job, &a.lines, a.seconds(), a.outcome(), true);
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
    ui.duo_mode.add_css_class("moving");
    ui.duo_mode.set_visible(true);
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
            let place = match seen.mode {
                Mode::Linux => Place::Linux(seen.via.clone()),
                Mode::Fastboot => Place::Fastboot(seen.via.clone()),
                Mode::Recovery => Place::Recovery(seen.via.clone()),
                Mode::Android => Place::Android(seen.via.clone()),
                Mode::Gone if cradle_core::android::port_without_system() => Place::NoSystem,
                Mode::Gone => cradle_core::android::on_usb_quietly().map(Place::Quiet).unwrap_or(Place::Gone),
            };
            // What can be done from there: is Android a guest (Linux's data
            // erased), is there a whole backup to come back from.
            let guest = place.serial().is_some_and(|s| cradle_core::android::guest(s).is_some());
            let status = if let Place::Linux(host) = &place { Some(status::read(host)) } else { None };
            (place, guest, status)
        })
        .await;
        let Ok((place, guest, status)) = found else { return };
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
        away_from_linux(ui, &place, guest);
        return;
    };
    ui.pages.set_visible_child_name("phone");
    let dev = developer_mode();
    ui.switcher.set_visible(dev);
    if !dev {
        ui.tabs.set_visible_child_name("general");
    }
    ui.mode.set_visible(false);
    ui.linux_only.set_visible(dev);
    ui.home.set_visible(!dev);
    ui.battery.set_visible(dev);
    ui.refresh.set_visible(dev);
    ui.legend.set_visible(dev);
    ui.free_label.set_visible(!dev);
    ui.duo_mode.set_visible(false);
    // On the cable or on Wi-Fi: what leaves Linux only on the cable.
    let cable = cradle_core::link::Via::of(host) == cradle_core::link::Via::Cable;
    ui.name_sub.set_label(&if cable { "Linux · cable".to_owned() } else { format!("Linux · Wi-Fi ({host})") });
    for (b, tip) in &ui.cable_only {
        b.set_sensitive(cable);
        b.set_tooltip_text(if cable { tip.as_deref() } else { Some("Plug in the cable: this takes the phone out of Linux, where Wi-Fi does not reach") });
    }
    ui.cable_note.set_visible(!cable);
    ui.repair_note.set_visible(!cable);
    match status {
        Some(Ok(s)) => fill(ui, &s, if cable { "cable" } else { "Wi-Fi" }),
        Some(Err(e)) => {
            // Said plainly on the simple page; the error itself for developers.
            if dev {
                ui.banner.set_title(&format!("Could not read the phone: {e}"));
                ui.banner.set_revealed(true);
            }
            say_status(ui, "look", "Your Duo is not answering", &format!("It is there, but did not answer just now. Cradle keeps trying.\n{e}"));
        }
        None => {}
    }
    bottom_shown(ui);
    // The screens and the storage, once each time the phone comes.
    if was.is_none() || !ui.state.borrow().pictured {
        ui.state.borrow_mut().pictured = true;
        take_screens(ui, false);
        count_storage(ui);
        show_backups(ui);
        show_slots(ui);
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
    if *place == Place::Gone && !seen_lately {
        ui.pages.set_visible_child_name("away");
        ui.switcher.set_visible(false);
        bottom_shown(ui);
        return;
    }
    ui.pages.set_visible_child_name("phone");
    ui.tabs.set_visible_child_name("general");
    ui.switcher.set_visible(false);
    ui.linux_only.set_visible(false);
    ui.home.set_visible(false);
    ui.mode.set_visible(true);
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
            ("applications-engineering-symbolic", "Recovery (TWRP)", "The Duo is in the recovery", "TWRP, a small repair system, runs from memory. It has no touch: Cradle drives it from here.", false)
        }
        Place::Android(s) => {
            if guest {
                button("Back to Linux…", true, Job::AndroidBack(s.clone()), Some(("Back to Linux?", BACK_BODY)));
                button("Restart Android", false, Job::AndroidStart(s.clone()), None);
            }
            ("phone-symbolic", "Android", "The Duo runs Android", if guest { "Stock Android, started by Cradle as a guest. Don't restart it from its own menu: Restart Android here does it the right way." } else { "Android runs on the phone." }, false)
        }
        Place::Quiet(_) => (
            "phone-symbolic",
            "Android",
            "The Duo runs Android - or is starting",
            "Cradle sees the phone on the cable but cannot talk to it yet. If Android is up: Settings → About phone → tap Build number seven times → System → Developer options → USB debugging, then allow this computer on the phone.",
            true,
        ),
        Place::NoSystem => (
            "dialog-information-symbolic",
            "No system",
            "The Duo started without a system",
            "Android was restarted plainly, so the phone started Linux's kernel - but Linux's data is in the backup now. Hold Power about 15 seconds until it is off, then hold Volume Down and press Power: the bootloader opens, and Cradle takes it from there.",
            false,
        ),
        _ => ("content-loading-symbolic", "Restarting…", "Waiting for the Duo", "It is restarting, or the cable came out. Cradle keeps looking.", true),
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
    ui.duo_mode.set_visible(true);
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
/// of backup and the logs (~/.config/cradle/gui).
fn developer_mode() -> bool {
    std::fs::read_to_string(gui_settings()).is_ok_and(|t| t.lines().any(|l| l.trim() == "developer=1"))
}

fn set_developer_mode(on: bool) {
    let path = gui_settings();
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = std::fs::write(path, if on { "developer=1\n" } else { "developer=0\n" });
}

fn gui_settings() -> std::path::PathBuf {
    std::path::Path::new(&std::env::var("HOME").unwrap_or_default()).join(".config/cradle/gui")
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
    use cradle_core::backup::{self, Kind};
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
    let want = {
        let st = ui.state.borrow();
        st.host.clone().filter(|_| !st.busy && !st.elsewhere)
    };
    let running = ui.following.borrow().as_ref().map(|(h, _)| h.clone());
    if running == want {
        return;
    }
    if let Some((_, stop)) = ui.following.borrow_mut().take() {
        stop.stop();
    }
    let Some(host) = want else { return };
    let Ok((mut follow, stop)) = cradle_core::posture::follow(&host) else { return };
    *ui.following.borrow_mut() = Some((host, stop.clone()));
    let (tx, rx) = async_channel::bounded::<cradle_core::posture::Reading>(16);
    gio::spawn_blocking(move || {
        while let Some(a) = follow.next() {
            if tx.send_blocking(a).is_err() {
                break;
            }
        }
    });
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        while let Ok(r) = rx.recv().await {
            match r {
                cradle_core::posture::Reading::Angle(a) => fold_to(&ui, a),
                cradle_core::posture::Reading::Gravity(g) => tilt_to(&ui, g),
            }
        }
        // It ended (the phone went, or was stopped): started again next second.
        let mine = ui.following.borrow().as_ref().is_some_and(|(_, s)| s.same(&stop));
        if mine {
            ui.following.borrow_mut().take();
        }
    });
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
    snap.to_paintable(Some(&graphene::Size::new(mid, bh as f32))).unwrap_or_else(|| gdk::Paintable::new_empty(bw / 2, bh))
}

/// The Duo as it lies on a table, seen from a little above: the right half
/// flat on the table, the left one raised about the spine by the fold (180
/// flat, 90 standing up like a laptop's lid, less folding over the right -
/// its glacier back then showing); folded back it goes under the table's
/// line a little, no more. Its shadow lies under the right half; the raised
/// half darkens as it turns from the light.
fn show_fold(ui: &Ui, angle: f64) {
    use gtk::{graphene, gsk};
    const TILT: f32 = 50.0;
    let lift = (180.0 - angle).clamp(-25.0, 178.0);
    let h = ui.duo_right.height().max(1) as f32;
    let mid = ui.duo_right.width().max(1) as f32;
    let room = h * DUO_ROOM as f32;
    let centre = ui.duo.width().max(1) as f32 / 2.0;
    // The table: the whole body tipped back about its middle, low in the
    // room; then as the phone is held (its gravity: top raised, edge raised).
    let [pitch, roll] = ui.tilt.get().0;
    let table = || {
        gsk::Transform::new()
            .translate(&graphene::Point::new(centre, room - h * 0.42))
            .perspective(3.2 * h)
            .rotate_3d(TILT, &graphene::Vec3::x_axis())
            .rotate_3d(-pitch as f32, &graphene::Vec3::x_axis())
            .rotate_3d(-roll as f32, &graphene::Vec3::y_axis())
            .translate(&graphene::Point::new(-mid, -h / 2.0))
    };
    // Lifted off the table, its shadow fades.
    let lying = (pitch.to_radians().cos() * roll.to_radians().cos()).clamp(0.0, 1.0);
    ui.floor.set_opacity(lying * lying);
    let flat = table().translate(&graphene::Point::new(mid, 0.0));
    // The left half turned about its right edge, the spine.
    let raised = table().translate(&graphene::Point::new(mid, 0.0)).rotate_3d(lift as f32, &graphene::Vec3::y_axis()).translate(&graphene::Point::new(-mid, 0.0));
    for w in [ui.duo_right.upcast_ref::<gtk::Widget>(), ui.shades[1].upcast_ref()] {
        ui.duo.set_child_transform(w, Some(&flat));
    }
    ui.duo.set_child_transform(&ui.spine, Some(&table()));
    ui.duo.set_child_transform(&ui.floor, Some(&flat.clone().translate(&graphene::Point::new(4.0, 10.0))));
    let front = lift < 90.0;
    ui.duo_left.set_visible(front);
    ui.shades[0].set_visible(front);
    ui.duo_back.set_visible(!front);
    for w in [ui.duo_left.upcast_ref::<gtk::Widget>(), ui.shades[0].upcast_ref(), ui.duo_back.upcast_ref()] {
        ui.duo.set_child_transform(w, Some(&raised));
    }
    ui.shades[1].set_opacity(0.0);
    ui.shades[0].set_opacity((lift.max(0.0).to_radians().sin() * 0.35).min(0.35));
}

fn fill(ui: &Ui, s: &status::Status, link: &str) {
    if let Some(a) = s.hinge {
        fold_to(ui, a);
    }
    // The club's number, if this computer knows it.
    *ui.serial.borrow_mut() = s.serial.clone();
    match cradle_core::club::known(&s.serial) {
        Some(d) => {
            ui.name.set_label(&format!("Surface Duo · {}", d.number));
            ui.join.set_visible(false);
        }
        None => {
            ui.name.set_label("Surface Duo");
            ui.join.set_visible(!s.serial.is_empty());
        }
    }
    let problems: Vec<String> = s.warnings().into_iter().chain(s.failed.iter().map(|u| format!("{u} failed"))).collect();
    let dev = developer_mode();
    ui.banner.set_title(&problems.join(" · "));
    ui.banner.set_revealed(dev && !problems.is_empty());
    // The simple page: only what its owner can do something about (a failed
    // background unit - fstrim on the loop rootfs, a suspend the cable kept
    // busy - is for Developer Mode).
    simple_status(ui, s, &s.warnings(), link);

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
        if let Some((_, kib)) = parts.list().into_iter().find(|(n, _)| *n == "Free") {
            ui.free_label.set_label(&format!("{} free", status::size_words(kib)));
        }
        *ui.parts.borrow_mut() = Some(parts);
        ui.storage.queue_draw();
    });
}

/// The backups on this computer under Backups: newest first, a star to keep
/// one for good, its folder; the device data asks to be copied elsewhere
/// until it is.
fn show_backups(ui: &Rc<Ui>) {
    use cradle_core::backup::{self, Kind};
    let pkgs = cradle_core::stock::packages();
    ui.stock_line.set_label(&match pkgs.last() {
        Some(p) => format!("Microsoft's Android {} (security patch {}) is on this computer.", p.build, p.security_patch),
        None => "Microsoft's package is not on this computer yet.".to_owned(),
    });
    while let Some(child) = ui.backups.first_child() {
        ui.backups.remove(&child);
    }
    let all = backup::list(None);
    ui.backups.set_visible(!all.is_empty());
    // The device data's one, and the newest few of the rest.
    let device = all.iter().find(|b| b.manifest.kind == Kind::Device).cloned();
    let rest: Vec<_> = all.into_iter().filter(|b| b.manifest.kind != Kind::Device).take(6).collect();
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
        ui.backups.append(&row);
    }
}

/// The slots and the RAM boot gate, read off the main thread.
fn show_slots(ui: &Rc<Ui>) {
    let Some(host) = ui.state.borrow().host.clone() else { return };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let read = gio::spawn_blocking(move || {
            let slots = cradle_core::slots::read(&host)?;
            let gate = cradle_core::backup::serial(&host).map(|s| cradle_core::flash::gate(&s)).ok();
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
            let max = cradle_core::flash::MAX_UNCONFIRMED;
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
    ui.bottom.set_visible(general && ui.state.borrow().host.is_some());
}

/// The live view on while the window is in front on General with the phone
/// there, off otherwise.
fn live_sync(ui: &Rc<Ui>) {
    let host = ui.state.borrow().host.clone();
    // The live view only in Developer Mode, and kept while the window is
    // behind others: on the phone each start and stop of it reads the
    // screen anew, and item crashed in the GPU driver there (2026-10-05);
    // the simple window has a picture every few seconds instead.
    let wanted = developer_mode() && host.is_some() && !ui.state.borrow().busy && ui.tabs.visible_child_name().as_deref() == Some("general");
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
    let (mut live, stop) = match cradle_core::live::Live::start(&host.expect("wanted")) {
        Ok(started) => started,
        Err(_) => {
            *ui.live_failed.borrow_mut() = Some(std::time::Instant::now());
            return;
        }
    };
    *ui.live.borrow_mut() = Some(stop.clone());
    let (tx, rx) = async_channel::bounded::<cradle_core::live::Frame>(2);
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
                ui.live_badge.set_visible(true);
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

#[derive(Clone)]
enum Job {
    Update,
    Reboot,
    Backup,
    FullBackup,
    RamBoot(std::path::PathBuf),
    Restore(Box<cradle_core::restore::Plan>),
    RecoveryExit(String),
    LeaveFastboot(String),
    AndroidGo(Box<cradle_core::android::Plan>),
    AndroidStart(String),
    AndroidBack(String),
    /// Microsoft's package from a link, then its boot chain taken out.
    StockDownload(String, String),
    /// A release image put on the phone, userdata made anew.
    Install(Box<cradle_core::install::Release>, cradle_core::install::Mode),
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
            Job::AndroidStart(_) => "android-start",
            Job::AndroidBack(_) => "android-back",
            Job::StockDownload(..) => "stock-download",
            Job::Install(_, m) => match m {
                cradle_core::install::Mode::Erase => "install",
                cradle_core::install::Mode::KeepFiles => "install-keep",
                cradle_core::install::Mode::FullCopy => "install-full",
            },
        }
    }
}

/// A boot chain to put back: the backup and the slot chosen, the plan made
/// off the main thread (reading only), then asked.
fn choose_restore(ui: &Rc<Ui>) {
    let serial = ui.serial.borrow().clone();
    let backups: Vec<cradle_core::backup::Backup> = cradle_core::backup::list(Some(&serial)).into_iter().filter(|b| b.manifest.kind == cradle_core::backup::Kind::Boot).collect();
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
            let planned = gio::spawn_blocking(move || cradle_core::restore::plan(&host, &backup, slot, false)).await.unwrap_or_else(|_| Err("the work stopped".into()));
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
    if let Some(out) = cradle_core::flash::port_tree().map(|t| t.join("out")) {
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
                let (img, serial, slot) = cradle_core::ramboot::preflight(&host, &p)?;
                let gate = cradle_core::flash::gate(&serial);
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
                cradle_core::flash::MAX_UNCONFIRMED
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

/// This Duo into the club: the token asked for first if the keyring has none.
fn join_club(ui: &Rc<Ui>) {
    if cradle_core::club::token().is_some() {
        register_now(ui);
        return;
    }
    let entry = gtk::PasswordEntry::builder().show_peek_icon(true).placeholder_text("creg_…").build();
    let dialog = adw::AlertDialog::new(
        Some("Join the owners' club"),
        Some(&format!("Paste a registry token from {} (Settings → Device registry). Cradle keeps it in your keyring; the phone's serial number never leaves this computer.", cradle_core::club::server())),
    );
    dialog.set_extra_child(Some(&entry));
    dialog.add_responses(&[("cancel", "Cancel"), ("go", "Join")]);
    dialog.set_response_appearance("go", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("go"));
    let ui2 = ui.clone();
    dialog.connect_response(None, move |_, response| {
        if response != "go" {
            return;
        }
        match cradle_core::club::set_token(&entry.text()) {
            Ok(()) => register_now(&ui2),
            Err(e) => stopped(&ui2, &e),
        }
    });
    dialog.present(Some(&ui.window));
}

fn register_now(ui: &Rc<Ui>) {
    let Some(host) = ui.state.borrow().host.clone() else { return };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        match gio::spawn_blocking(move || cradle_core::club::register(&host)).await.unwrap_or_else(|_| Err("the work stopped".into())) {
            Ok(d) => {
                ui.name.set_label(&format!("Surface Duo · {}", d.number));
                ui.join.set_visible(false);
                ui.toasts.add_toast(adw::Toast::new(&format!("This Duo is {} in the club", d.number)));
            }
            Err(e) => stopped(&ui, &e),
        }
    });
}

/// A stop, said so it cannot be missed.
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
/// another Cradle too.
fn run_job(ui: &Rc<Ui>, job: Job) {
    let host = ui.state.borrow().host.clone();
    let needs_linux = matches!(job, Job::Update | Job::Reboot | Job::Backup | Job::FullBackup | Job::RamBoot(_) | Job::Restore(_) | Job::AndroidGo(_) | Job::Install(..));
    if needs_linux && host.is_none() {
        stopped(ui, "The phone is not in Linux just now.");
        return;
    }
    // Where Linux will answer, for the jobs that end there.
    let host = host.or_else(|| cradle_core::phone::hosts().into_iter().next()).unwrap_or_default();
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
        cradle_core::activity::begin(kind);
        let mut say = |words: String| {
            cradle_core::activity::line(&words);
            let _ = tx.send_blocking(words);
        };
        let result = match job {
            Job::Update => cradle_core::update::update(&host, true, &mut |step| say(step.words().to_owned())),
            Job::Reboot => cradle_core::phone::reboot(&host, &mut |b| say(b.words().to_owned())),
            Job::Restore(plan) => cradle_core::restore::restore(&host, &plan, &mut say),
            Job::RamBoot(path) => cradle_core::ramboot::ram_boot(&host, &path, cradle_core::ramboot::Expect::of(&path), &mut say),
            Job::FullBackup => cradle_core::full::take(&host, &mut say).map(|_| ()),
            Job::RecoveryExit(serial) => cradle_core::ramboot::leave_recovery(&host, &serial, &mut say),
            Job::LeaveFastboot(serial) => cradle_core::ramboot::leave_fastboot(&host, &serial, &mut say),
            Job::AndroidGo(plan) => {
                let word = cradle_core::backup::serial(&host).map(|s| cradle_core::android::confirm_word(&s)).unwrap_or_default();
                // The number was typed in the window already; the losses shown.
                cradle_core::android::go(&host, &plan, &word, true, &mut say)
            }
            Job::AndroidStart(serial) => cradle_core::android::start(&host, &serial, &mut say),
            Job::AndroidBack(serial) => cradle_core::android::back(&host, &serial, false, &mut say),
            Job::Install(release, mode) => {
                let word = cradle_core::backup::serial(&host).map(|s| cradle_core::android::confirm_word(&s)).unwrap_or_default();
                // The number was typed in the window already.
                cradle_core::install::erase_and_install(&host, &release, mode, &word, &mut say)
            }
            Job::StockDownload(url, label) => (|| {
                say(format!("downloading {label}"));
                let pkg = cradle_core::stock::download(&url, &mut |done, whole| say(format!("  downloaded: {} of {} MB", done >> 20, whole >> 20)))?;
                cradle_core::stock::boot_chain(&pkg, &mut say)?;
                Ok(())
            })(),
            Job::Backup => (|| {
                use cradle_core::backup::{self, Kind};
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
        cradle_core::activity::end(&result);
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
            st.dismissed = cradle_core::activity::elsewhere(u64::MAX).and_then(|a| a.ended_at).or(st.dismissed);
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
/// (its sign-in kept for next time); once signed in, Cradle asks for the
/// Duo by its serial, takes the link from the answer, and downloads it as a
/// job on the card.
fn get_android_from_microsoft(ui: &Rc<Ui>) {
    use webkit::prelude::*;
    let serial = {
        let s = ui.serial.borrow().clone();
        if s.is_empty() { ui.state.borrow().place.serial().unwrap_or_default().to_owned() } else { s }
    };
    if serial.is_empty() {
        stopped(ui, "Cradle needs the phone connected to know its serial number.");
        return;
    }
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let session = webkit::NetworkSession::new(
        home.join(".local/share/cradle/web").to_str(),
        home.join(".cache/cradle/web").to_str(),
    );
    if let Some(cookies) = session.cookie_manager() {
        cookies.set_persistent_storage(home.join(".local/share/cradle/web/cookies.sqlite").to_str().unwrap_or_default(), webkit::CookiePersistentStorage::Sqlite);
    }
    let web = webkit::WebView::builder().network_session(&session).vexpand(true).hexpand(true).build();
    let note = gtk::Label::builder()
        .label("Cradle fetches Android with Microsoft's own page. Sign in with your Microsoft account once - Cradle does the rest and remembers the sign-in.")
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
            let on_page = web.uri().is_some_and(|u| u.starts_with(cradle_core::stock::RECOVERY_PAGE));
            if !on_page {
                note.set_label("Sign in with your Microsoft account. Cradle goes on by itself after that.");
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
                    note.set_label("Sign in with your Microsoft account (Sign In on the page). Cradle goes on by itself after that.");
                    return;
                }
                match cradle_core::stock::link_in(&text) {
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
    web.load_uri(cradle_core::stock::RECOVERY_PAGE);
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
    let Some(release) = cradle_core::install::releases().pop() else {
        stopped(ui, "No release image on this computer yet (the port's tools/build-release-image.sh makes one).");
        return;
    };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let h = host.clone();
        let read = gio::spawn_blocking(move || {
            let serial = cradle_core::backup::serial(&h)?;
            let fresh = cradle_core::android::fresh_full(&h, &serial)?.is_some();
            Ok::<_, String>((cradle_core::android::confirm_word(&serial), fresh))
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
                    cradle_core::install::Mode::KeepFiles
                } else if full.is_active() {
                    cradle_core::install::Mode::FullCopy
                } else {
                    cradle_core::install::Mode::Erase
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
            let plan = cradle_core::android::plan(&h)?;
            let serial = cradle_core::backup::serial(&h)?;
            Ok::<_, String>((plan, cradle_core::android::confirm_word(&serial)))
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
             About {} minutes in all; keep the cable in. Back to Linux, here in Cradle, puts everything back.",
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
