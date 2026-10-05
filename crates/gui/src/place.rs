//! The window where it was last: its place, size and whether maximized,
//! kept on closing (~/.config/itemgrid/window) and given back on opening - the
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
    query_pointer: unsafe extern "C" fn(Display, Window, *mut Window, *mut Window, *mut i32, *mut i32, *mut i32, *mut i32, *mut u32) -> i32,
    flush: unsafe extern "C" fn(Display) -> i32,
    intern_atom: unsafe extern "C" fn(Display, *const std::ffi::c_char, i32) -> std::ffi::c_ulong,
    change_property: unsafe extern "C" fn(Display, Window, std::ffi::c_ulong, std::ffi::c_ulong, i32, i32, *const u8, i32) -> i32,
    move_resize: unsafe extern "C" fn(Display, Window, i32, i32, u32, u32) -> i32,
    unmap: unsafe extern "C" fn(Display, Window) -> i32,
    map: unsafe extern "C" fn(Display, Window) -> i32,
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
            query_pointer: std::mem::transmute(get(b"XQueryPointer\0")?),
            flush: std::mem::transmute(get(b"XFlush\0")?),
            intern_atom: std::mem::transmute(get(b"XInternAtom\0")?),
            change_property: std::mem::transmute(get(b"XChangeProperty\0")?),
            move_resize: std::mem::transmute(get(b"XMoveResizeWindow\0")?),
            unmap: std::mem::transmute(get(b"XUnmapWindow\0")?),
            map: std::mem::transmute(get(b"XMapWindow\0")?),
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
    gtk::glib::user_config_dir().join("itemgrid/window")
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
    // Not a window tried from outside (itemgrid-mcp): the owner's place kept.
    if std::env::var("ITEMGRID_CONTROL").ok().as_deref() == Some("1") {
        return;
    }
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

/// Where the pointer is on the screen (X11 only).
pub fn pointer(window: &adw::ApplicationWindow) -> Option<(i32, i32)> {
    let (x11, dpy, _) = handle(window)?;
    let (mut root_ret, mut child) = (0, 0);
    let (mut rx, mut ry, mut wx, mut wy, mut mask) = (0, 0, 0, 0, 0u32);
    let ok = unsafe { (x11.query_pointer)(dpy, (x11.root)(dpy), &mut root_ret, &mut child, &mut rx, &mut ry, &mut wx, &mut wy, &mut mask) };
    (ok != 0).then_some((rx, ry))
}

/// The window as a desktop's (`on`: under every window, the wallpaper of
/// the monitor at `rect`, x y width height) or an ordinary one again (at
/// `rect` if given). The window manager reads a window's type as it is
/// mapped: unmapped, typed, mapped again. X11 only: whether it was done.
pub fn desktop(window: &adw::ApplicationWindow, on: bool, rect: Option<(i32, i32, i32, i32)>) -> bool {
    let Some((x11, dpy, id)) = handle(window) else { return false };
    unsafe {
        let atom = |name: &str| {
            let c = std::ffi::CString::new(name).unwrap();
            (x11.intern_atom)(dpy, c.as_ptr(), 0)
        };
        let kind = atom("_NET_WM_WINDOW_TYPE");
        let value = atom(if on { "_NET_WM_WINDOW_TYPE_DESKTOP" } else { "_NET_WM_WINDOW_TYPE_NORMAL" });
        (x11.unmap)(dpy, id);
        (x11.flush)(dpy);
        // XA_ATOM (4), 32-bit, replace (0).
        (x11.change_property)(dpy, id, kind, 4, 32, 0, &value as *const std::ffi::c_ulong as *const u8, 1);
        (x11.map)(dpy, id);
        if let Some((x, y, w, h)) = rect {
            (x11.move_resize)(dpy, id, x, y, w.max(1) as u32, h.max(1) as u32);
        }
        (x11.flush)(dpy);
    }
    true
}

/// The window's X place and size now (its content's, with the shadow round
/// it), to give back later.
pub fn frame(window: &adw::ApplicationWindow) -> Option<(i32, i32, i32, i32)> {
    let (x11, dpy, id) = handle(window)?;
    let (mut x, mut y, mut child) = (0, 0, 0);
    if unsafe { (x11.translate)(dpy, id, (x11.root)(dpy), 0, 0, &mut x, &mut y, &mut child) } == 0 {
        return None;
    }
    let surface = window.surface()?;
    Some((x, y, surface.width(), surface.height()))
}
