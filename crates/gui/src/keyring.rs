//! The phone's keys on the table, as every section is: the names in its
//! keyring on a board beside the menu (the core's keys.rs does the
//! talking), each a line to click - what it is for, and forget; a key
//! added by hand on the board itself, its name and its value typed on
//! the keyboard (or pasted), the value as dots; keys from a file and to a
//! file through the system's file chooser, the one widget here (the path
//! has to come from it); and, under "› about keys", where a key goes and
//! what is without one. Nothing of a key stays on this computer and none
//! reaches the trace: names only.

use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};

use super::{board, open_ask, set_page, trace, wrapped, wrapped_to, Ask, Entry, Ui};

const SECTION: &str = "keys";

#[derive(Default)]
pub struct Keys {
    /// The names on the phone; None until read (or while read again).
    names: Option<Result<Vec<String>, String>>,
    /// "about keys" open in place of the names.
    about: bool,
    /// What the last doing said (a key on the phone, a file written).
    said: Vec<String>,
}

/// Where a key goes and what is without one (the board under "about
/// keys"), each a paragraph wrapped to the room beside the menu.
const ABOUT: [&str; 4] = [
    "a key goes to the phone's keyring over the link; nothing of it kept here, nothing sent anywhere",
    "the phone gives it to its own service only, when you use that service",
    "without keys item works; the microphone key says it has none",
    "to a file: the keys in the clear, for a fresh install; load them there, then delete it",
];
/// The about's column, squares (the room beside the menu).
const ABOUT_WIDE: usize = 21;

/// The section's board: the names (each to click), add a key, from a
/// file, to a file, what the last doing said, and about keys - or, that
/// opened, the about in their place.
pub fn lines(ui: &Ui) -> Vec<board::Line> {
    let k = ui.keys.borrow();
    if k.about {
        let mut lines = vec![board::Line::new("more:keys", "⌄ about keys")];
        for (i, p) in ABOUT.iter().enumerate() {
            if i > 0 {
                lines.push(board::Line::new("", ""));
            }
            lines.extend(wrapped_to(p, ABOUT_WIDE).into_iter().map(|t| board::Line::new("", t)));
        }
        return lines;
    }
    let mut lines = Vec::new();
    let mut any = false;
    match &k.names {
        None => lines.push(board::Line::new("", "reading the keyring…")),
        Some(Err(e)) => lines.extend(wrapped(e).into_iter().map(|t| board::Line::new("", t))),
        Some(Ok(names)) if names.is_empty() => lines.push(board::Line::new("", "no keys on the phone")),
        Some(Ok(names)) => {
            any = true;
            lines.extend(names.iter().map(|n| board::Line::new(format!("key:{n}"), n.clone())));
        }
    }
    lines.push(board::Line::new("", ""));
    lines.push(board::Line::new("do:key-add", "› add a key"));
    lines.push(board::Line::new("do:keys-from", "› from a file"));
    if any {
        lines.push(board::Line::new("do:keys-to", "› to a file"));
    }
    if !k.said.is_empty() {
        lines.push(board::Line::new("", ""));
        lines.extend(k.said.iter().map(|t| board::Line::new("", t.clone())));
    }
    lines.push(board::Line::new("", ""));
    lines.push(board::Line::new("more:keys", "› about keys"));
    lines
}

/// The names read off the phone (its keyring, over the link), the board
/// following.
pub fn load(ui: &Rc<Ui>) {
    let Some(host) = ui.state.borrow().host.clone() else {
        ui.keys.borrow_mut().names = Some(Err("no duo yet".into()));
        return;
    };
    ui.keys.borrow_mut().names = None;
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let read = gio::spawn_blocking(move || itemgrid_core::keys::list(&host)).await.unwrap_or_else(|_| Err("the read stopped".into()));
        ui.keys.borrow_mut().names = Some(read);
        show(&ui);
    });
}

/// The board laid anew with what is known now (if it is the open one).
fn show(ui: &Rc<Ui>) {
    set_page(ui, SECTION, lines(ui));
}

/// What the last doing said, on the board under the lines to click.
fn say(ui: &Rc<Ui>, text: &str) {
    ui.keys.borrow_mut().said = wrapped(text);
    show(ui);
}

/// About keys opened, or the names back.
pub fn toggle_about(ui: &Rc<Ui>) {
    let open = {
        let mut k = ui.keys.borrow_mut();
        k.about = !k.about;
        k.about
    };
    trace(format_args!("keys: about {}", if open { "open" } else { "closed" }));
    show(ui);
}

/// The section's board back on the table beside the menu (the menu
/// opened again if it went), as a question of this section closes.
pub fn reopen(ui: &Rc<Ui>) {
    if ui.board.borrow().as_ref().is_none_or(|b| b.closing()) {
        let mut b = board::Board::open(super::menu_lines(ui));
        b.chosen = b.lines.iter().position(|l| l.key == SECTION);
        *ui.board.borrow_mut() = Some(b);
    }
    *ui.page.borrow_mut() = Some((SECTION.to_owned(), board::Board::open(lines(ui))));
    super::show_fold(ui, ui.fold.get().0);
}

/// A name's line clicked: what the key is for, and forget - a question.
pub fn clicked(ui: &Rc<Ui>, name: &str) {
    let info = match itemgrid_core::keys::about(name) {
        Some((service, purpose)) => {
            let mut v = wrapped(service);
            v.push(String::new());
            v.extend(wrapped(purpose));
            v
        }
        None => wrapped("a name item/grid does not know; the agent reads it with item-secret get"),
    };
    let mut ask = Ask::new(name, info);
    ask.verb = "forget";
    ask.back = Some(SECTION);
    let name = name.to_owned();
    ask.go = Some(Rc::new(move |ui, _, _| forget(ui, name.clone())));
    open_ask(ui, ask);
}

/// The key under `name` forgotten on the phone; said, the names read again.
fn forget(ui: &Rc<Ui>, name: String) {
    let Some(host) = ui.state.borrow().host.clone() else { return };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let n = name.clone();
        let done = gio::spawn_blocking(move || itemgrid_core::keys::forget(&host, &n)).await.unwrap_or_else(|_| Err("the work stopped".into()));
        match done {
            Ok(true) => {
                trace(format_args!("keys: {name} forgotten"));
                say(&ui, &format!("{name}: forgotten on the phone"));
            }
            Ok(false) => say(&ui, &format!("{name}: there was none")),
            Err(e) => say(&ui, &e),
        }
        load(&ui);
    });
}

/// A key by hand: its name and its value typed on the board (the value
/// as dots), onto the phone.
pub fn add(ui: &Rc<Ui>) {
    let mut ask = Ask::new("add a key", wrapped("its name as the agent asks for it (openrouter, github), and the key; into the phone's keyring over the link, nothing kept here"));
    ask.entries = vec![Entry::new("name", false), Entry::new("key", true)];
    ask.verb = "put on the phone";
    ask.back = Some(SECTION);
    ask.go = Some(Rc::new(|ui, _, typed| {
        let name = typed.first().map(|n| n.trim().to_ascii_lowercase()).unwrap_or_default();
        let value = typed.get(1).cloned().unwrap_or_default();
        set(ui, name, value);
    }));
    open_ask(ui, ask);
}

/// `value` under `name` into the phone's keyring; said, the names read
/// again. (The value goes with the thread and is dropped there.)
fn set(ui: &Rc<Ui>, name: String, value: String) {
    let Some(host) = ui.state.borrow().host.clone() else { return };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let n = name.clone();
        let done = gio::spawn_blocking(move || itemgrid_core::keys::set(&host, &n, &value)).await.unwrap_or_else(|_| Err("the work stopped".into()));
        match done {
            Ok(()) => {
                trace(format_args!("keys: {name} put on the phone"));
                say(&ui, &format!("{name}: on the phone, in its keyring"));
            }
            Err(e) => say(&ui, &e),
        }
        load(&ui);
    });
}

/// Keys from a file (keys.txt, a .env: a line each), onto the phone. The
/// file stays where it is: said so, to be deleted once they are on.
pub fn from_file(ui: &Rc<Ui>) {
    let dialog = gtk::FileDialog::builder().title("Keys from a File").modal(true).build();
    let window = ui.window.clone();
    let ui = ui.clone();
    dialog.open(Some(&window), gio::Cancellable::NONE, move |r| {
        let Ok(file) = r else { return };
        let Some(path) = file.path() else { return };
        let Some(host) = ui.state.borrow().host.clone() else { return };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                say(&ui, &format!("{}: {e}", path.display()));
                return;
            }
        };
        say(&ui, "putting them on the phone…");
        glib::spawn_future_local(async move {
            let done = gio::spawn_blocking(move || itemgrid_core::keys::set_all(&host, &text)).await.unwrap_or_else(|_| Err("the work stopped".into()));
            match done {
                Ok(names) => {
                    trace(format_args!("keys: {} from a file", names.join(", ")));
                    say(&ui, &format!("on the phone: {}. the file stays here: delete it once you are sure", names.join(", ")));
                }
                Err(e) => say(&ui, &e),
            }
            load(&ui);
        });
    });
}

/// Every key on the phone into a file the owner names (keys.txt), for a
/// fresh install or another phone; what it means asked first.
pub fn to_file(ui: &Rc<Ui>) {
    let mut ask = Ask::new("keys to a file", wrapped("every key in the clear, readable by you alone (0600). keep it where only you can read it, load it on the new phone with from a file, and delete it. nothing is sent anywhere"));
    ask.verb = "choose the file";
    ask.back = Some(SECTION);
    ask.go = Some(Rc::new(|ui, _, _| choose_and_export(ui)));
    open_ask(ui, ask);
}

fn choose_and_export(ui: &Rc<Ui>) {
    let chooser = gtk::FileDialog::builder().title("Keys to a File").initial_name("keys.txt").modal(true).build();
    let window = ui.window.clone();
    let ui = ui.clone();
    chooser.save(Some(&window), gio::Cancellable::NONE, move |r| {
        let Ok(file) = r else { return };
        let Some(path) = file.path() else { return };
        let Some(host) = ui.state.borrow().host.clone() else { return };
        say(&ui, "writing the file…");
        glib::spawn_future_local(async move {
            let p = path.clone();
            let done = gio::spawn_blocking(move || itemgrid_core::keys::export(&host, &p)).await.unwrap_or_else(|_| Err("the work stopped".into()));
            match done {
                Ok(names) => {
                    trace(format_args!("keys: {} written to a file", names.len()));
                    say(&ui, &format!("{} keys in {}: yours to keep safe, and to delete once the new phone has them", names.len(), path.display()));
                }
                Err(e) => say(&ui, &e),
            }
        });
    });
}
