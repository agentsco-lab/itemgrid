//! For looking at the window from outside (gridbay-mcp, crates/mcp): with
//! GRIDBAY_CONTROL=1, a socket of the owner's alone
//! ($XDG_RUNTIME_DIR/gridbay-control.sock) taking a request a line, as JSON,
//! and answering a line:
//!
//!   {"cmd":"shot"}    the window drawn into a picture
//!                     -> {"path", "width", "height"}
//!   {"cmd":"state"}   where the window is on the screen (its content's
//!                     top left, for input there), the start, the floor,
//!                     the drawn Duo, the phone
//!   {"cmd":"replay"}  the start again
//!   {"cmd":"renderer"} GTK's renderer for the window
//!
//! Input is not made here: gridbay-mcp moves the pointer itself (xdotool),
//! so the window's own gestures are what is tried.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

pub fn socket_path() -> std::path::PathBuf {
    glib::user_runtime_dir().join("gridbay-control.sock")
}

/// Started when GRIDBAY_CONTROL=1; `answer` runs on the main thread.
pub fn start(answer: impl Fn(&serde_json::Value) -> serde_json::Value + 'static) {
    if std::env::var("GRIDBAY_CONTROL").ok().as_deref() != Some("1") {
        return;
    }
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = UnixListener::bind(&path) else {
        eprintln!("gridbay: no control socket at {}", path.display());
        return;
    };
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    let (tx, rx) = async_channel::unbounded::<(serde_json::Value, async_channel::Sender<serde_json::Value>)>();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut out = match stream.try_clone() {
                    Ok(s) => s,
                    Err(_) => return,
                };
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { break };
                    let request = serde_json::from_str(&line).unwrap_or(serde_json::Value::Null);
                    let (back, reply) = async_channel::bounded(1);
                    if tx.send_blocking((request, back)).is_err() {
                        return;
                    }
                    let Ok(value) = reply.recv_blocking() else { return };
                    if writeln!(out, "{value}").is_err() {
                        break;
                    }
                }
            });
        }
    });
    let answer = Rc::new(answer);
    glib::spawn_future_local(async move {
        while let Ok((request, back)) = rx.recv().await {
            let _ = back.send(answer(&request)).await;
        }
    });
}

/// The window drawn into a PNG (as it is on the screen, its own size).
pub fn shot(window: &impl IsA<gtk::Window>) -> serde_json::Value {
    let window = window.as_ref();
    let paintable = gtk::WidgetPaintable::new(Some(window));
    let (w, h) = (window.width() as f64, window.height() as f64);
    let snap = gtk::Snapshot::new();
    paintable.snapshot(&snap, w, h);
    let path = glib::user_runtime_dir().join("gridbay-shot.png");
    let texture: Option<gdk::Texture> = snap.to_node().zip(window.native()).and_then(|(node, native)| native.renderer().map(|r| r.render_texture(&node, None)));
    match texture.map(|t| t.save_to_png(&path)) {
        Some(Ok(())) => serde_json::json!({ "path": path, "width": w, "height": h }),
        Some(Err(e)) => serde_json::json!({ "error": e.to_string() }),
        None => serde_json::json!({ "error": "not drawn" }),
    }
}
