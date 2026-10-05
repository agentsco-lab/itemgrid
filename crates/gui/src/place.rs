//! The window where it was last: its place, size and whether maximized,
//! kept on closing (~/.config/hythe/window) and given back on opening - the
//! window manager put it wherever (the other monitor, the pointer's).
//!
//! GTK 4 has no way to place a window; on X11 the window is moved itself
//! (Xlib, through GTK's own connection). Elsewhere (Wayland) only its size
//! and maximized are given back.

use gtk::prelude::*;

type Display = *mut std::ffi::c_void;
type Window = std::ffi::c_ulong;

struct X11 {
    xid: unsafe extern "C" fn(*mut gtk::gdk::ffi::GdkSurface) -> Window,
    xdisplay: unsafe extern "C" fn(*mut gtk::gdk::ffi::GdkDisplay) -> Display,
    root: unsafe extern "C" fn(Display) -> Window,
    translate: unsafe extern "C" fn(Display, Window, Window, i32, i32, *mut i32, *mut i32, *mut Window) -> i32,
    moved: unsafe extern "C" fn(Display, Window, i32, i32) -> i32,
    flush: unsafe extern "C" fn(Display) -> i32,
}

/// GTK's X11 functions and Xlib's, from the process (both are loaded when
/// GTK runs on X11).
fn x11() -> Option<&'static X11> {
    static X: std::sync::OnceLock<Option<X11>> = std::sync::OnceLock::new();
    X.get_or_init(|| unsafe {
        let this = libloading::os::unix::Library::this();
        let get = |name: &[u8]| this.get::<*const ()>(name).ok().map(|s| *s);
        Some(X11 {
            xid: std::mem::transmute(get(b"gdk_x11_surface_get_xid\0")?),
            xdisplay: std::mem::transmute(get(b"gdk_x11_display_get_xdisplay\0")?),
            root: std::mem::transmute(get(b"XDefaultRootWindow\0")?),
            translate: std::mem::transmute(get(b"XTranslateCoordinates\0")?),
            moved: std::mem::transmute(get(b"XMoveWindow\0")?),
            flush: std::mem::transmute(get(b"XFlush\0")?),
        })
    })
    .as_ref()
}

/// The window's X11 display and id, when it is on X11.
fn handle(window: &impl IsA<gtk::Window>) -> Option<(&'static X11, Display, Window)> {
    let display = WidgetExt::display(window.as_ref());
    if !display.type_().name().contains("X11") {
        return None;
    }
    let surface = window.as_ref().surface()?;
    let x = x11()?;
    unsafe {
        use gtk::glib::translate::ToGlibPtr;
        let dpy = (x.xdisplay)(display.to_glib_none().0);
        let id = (x.xid)(surface.to_glib_none().0);
        Some((x, dpy, id))
    }
}

fn file() -> std::path::PathBuf {
    gtk::glib::user_config_dir().join("hythe/window")
}

/// Kept: x y width height maximized.
fn read() -> Option<(Option<(i32, i32)>, i32, i32, bool)> {
    let text = std::fs::read_to_string(file()).ok()?;
    let v: Vec<&str> = text.split_whitespace().collect();
    let num = |i: usize| v.get(i).and_then(|s| s.parse::<i32>().ok());
    let at = match (num(0), num(1)) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    };
    Some((at, num(2)?.max(400), num(3)?.max(300), v.get(4) == Some(&"1")))
}

/// Given back: the size now, the place once the window is up.
pub fn restore(window: &adw::ApplicationWindow) {
    let Some((at, w, h, max)) = read() else { return };
    window.set_default_size(w, h);
    window.connect_map(move |window| {
        if let (Some((x, y)), Some((x11, dpy, id))) = (at, handle(window)) {
            unsafe {
                (x11.moved)(dpy, id, x, y);
                (x11.flush)(dpy);
            }
        }
        if max {
            window.maximize();
        }
    });
}

/// Kept as it closes.
pub fn keep(window: &adw::ApplicationWindow) {
    window.connect_close_request(|window| {
        let max = window.is_maximized();
        let (w, h) = window.default_size();
        let at = handle(window).and_then(|(x11, dpy, id)| unsafe {
            let (mut x, mut y, mut child) = (0, 0, 0);
            ((x11.translate)(dpy, id, (x11.root)(dpy), 0, 0, &mut x, &mut y, &mut child) != 0).then_some((x, y))
        });
        let at = at.map_or(String::new(), |(x, y)| format!("{x} {y} "));
        let at = if at.is_empty() { "- - ".to_owned() } else { at };
        let _ = std::fs::create_dir_all(file().parent().unwrap_or(std::path::Path::new(".")));
        let _ = std::fs::write(file(), format!("{at}{w} {h} {}\n", max as u8));
        gtk::glib::Propagation::Proceed
    });
}

/// Where the window's content is on the screen (its top left; X11 only):
/// its X window's place, past the shadow drawn round it.
pub fn content_origin(window: &adw::ApplicationWindow) -> Option<(f64, f64)> {
    let (x11, dpy, id) = handle(window)?;
    let (mut x, mut y, mut child) = (0, 0, 0);
    if unsafe { (x11.translate)(dpy, id, (x11.root)(dpy), 0, 0, &mut x, &mut y, &mut child) } == 0 {
        return None;
    }
    let (sx, sy) = window.native()?.surface_transform();
    Some((x as f64 + sx, y as f64 + sy))
}
