//! The drawn Duo in 3D: its body and hinge as meshes, drawn with GL in a
//! GtkGLArea - a depth buffer for what is in front, lit materials, edges
//! smoothed (multisampled) - in place of pictures turned in 3D (their
//! edges were layers, their order guessed, GTK's bounds for them loose).
//!
//! The geometry is the Surface Duo 1's, in millimetres, from the drawing
//! Cradle has (data/duo-body.py: agentsco.uk's DuoBody, off a straight-on
//! photo and Microsoft's spec sheet): each half 91.6 x 145.2 mm and 4.8 thick,
//! its outer corners round (R 10), at its inner edge the notches the hinge's
//! knuckles sit in; its edges rounded all round. Each half is made in the
//! same frame as the window's pictures of it (x from the left edge of the
//! left half's, or from the spine for the right one; y down from the top;
//! z up from the screen's plane, the back at -4.8 mm), so the window's
//! transforms for the halves - the fold, the tent, the hand, the pointer's
//! turn - place them here as they did the pictures.
//!
//! The hinge: on each half a knuckle in its notch - a block rounded at its
//! spine end (radius half the thickness) - turning with it; open flat the
//! knuckles meet at the spine, closed they lie one on the other and are the
//! stack's rounded spine. Between them, along the spine, a rod for each half
//! and the dark of the gap.

use std::cell::RefCell;
use std::rc::Rc;

use glow::HasContext;
use gtk::prelude::*;
use gtk::{glib, graphene};

/// The body (mm).
const BODY_W: f32 = 186.9;
const BODY_H: f32 = 145.2;
const GAP: f32 = 3.7;
const HALF_W: f32 = (BODY_W - GAP) / 2.0;
const MID: f32 = BODY_W / 2.0;
const THICK: f32 = 4.8;
const R: f32 = 10.0;
const NOTCH_W: f32 = 3.65;
const NOTCH_D: f32 = 8.0;
const NOTCH_FILLET: f32 = 0.6;
/// The edges' rounding, all round each half.
const EDGE: f32 = 0.5;

/// Materials, as the fragment shader knows them.
const GLASS: f32 = 0.0;
const CHASSIS: f32 = 1.0;
const BACK: f32 = 2.0;
const HINGE: f32 = 3.0;
const SCREEN: f32 = 6.0;
const MIRROR: f32 = 7.0;
/// The Microsoft logo on the left half's back, mirror-polished: four
/// squares (mm) with a gap, at the back's middle.
const LOGO_SQUARE: f32 = 4.4;
const LOGO_GAP: f32 = 0.5;
/// The screens (mm): each panel's size, the left edge of each in the body,
/// and their top.
const PANEL: (f32, f32) = (86.654, 115.539);
const SCREEN_X: (f32, f32) = (4.1, 96.146);
const SCREEN_TOP: f32 = 14.831;

/// What the window gives to draw: each half's place in the view (its frame
/// to the duo drawing's px, without the perspective), and the perspective
/// to the GL area's clip space.
#[derive(Default)]
pub struct Scene {
    pub halves: [Option<graphene::Matrix>; 2],
    /// The hinge's spine: its middle, turned with half the fold.
    pub spine: Option<graphene::Matrix>,
    /// What each screen shows (the phone's pictures), and a count bumped
    /// each time one changes.
    pub screens: [Option<gtk::gdk::Texture>; 2],
    pub screens_changed: [u64; 2],
    pub proj: Option<graphene::Matrix>,
    /// Where the eye is (the perspective's), in the halves' view.
    pub eye: [f32; 3],
}

/// A mesh's vertices: position (px), normal, material.
#[derive(Default)]
struct Mesh {
    v: Vec<f32>,
}

impl Mesh {
    fn vert(&mut self, p: [f32; 3], n: [f32; 3], m: f32) {
        self.v.extend_from_slice(&[p[0], p[1], p[2], n[0], n[1], n[2], m, 0.0, 0.0]);
    }
    fn vert_uv(&mut self, p: [f32; 3], n: [f32; 3], m: f32, uv: [f32; 2]) {
        self.v.extend_from_slice(&[p[0], p[1], p[2], n[0], n[1], n[2], m, uv[0], uv[1]]);
    }
    fn tri(&mut self, a: ([f32; 3], [f32; 3]), b: ([f32; 3], [f32; 3]), c: ([f32; 3], [f32; 3]), m: f32) {
        self.vert(a.0, a.1, m);
        self.vert(b.0, b.1, m);
        self.vert(c.0, c.1, m);
    }
    fn quad(&mut self, a: ([f32; 3], [f32; 3]), b: ([f32; 3], [f32; 3]), c: ([f32; 3], [f32; 3]), d: ([f32; 3], [f32; 3]), m: f32) {
        self.tri(a, b, c, m);
        self.tri(a, c, d, m);
    }
}

type P2 = [f32; 2];

fn quad_bezier(out: &mut Vec<P2>, a: P2, c: P2, b: P2, n: usize) {
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let u = 1.0 - t;
        out.push([u * u * a[0] + 2.0 * u * t * c[0] + t * t * b[0], u * u * a[1] + 2.0 * u * t * c[1] + t * t * b[1]]);
    }
}

fn arc(out: &mut Vec<P2>, c: P2, r: f32, from: f32, to: f32, n: usize) {
    for i in 1..=n {
        let a = from + (to - from) * i as f32 / n as f32;
        out.push([c[0] + r * a.cos(), c[1] + r * a.sin()]);
    }
}

/// The left half's outline (mm, its picture's frame), as duo-body.py's
/// half_path: the outer corners round, the inner edge's notches.
fn left_outline() -> Vec<P2> {
    use std::f32::consts::PI;
    let (w, h, nw, nd, f) = (HALF_W, BODY_H, NOTCH_W, NOTCH_D, NOTCH_FILLET);
    let x = w - nw;
    let mut o = vec![[R, 0.0], [x - f, 0.0]];
    quad_bezier(&mut o, [x - f, 0.0], [x, 0.0], [x, f], 4);
    o.push([x, nd - f]);
    quad_bezier(&mut o, [x, nd - f], [x, nd], [x + f, nd], 4);
    o.push([w, nd]);
    o.push([w, h - nd]);
    o.push([x + f, h - nd]);
    quad_bezier(&mut o, [x + f, h - nd], [x, h - nd], [x, h - nd + f], 4);
    o.push([x, h - f]);
    quad_bezier(&mut o, [x, h - f], [x, h], [x - f, h], 4);
    o.push([R, h]);
    arc(&mut o, [R, h - R], R, PI / 2.0, PI, 14);
    o.push([0.0, R]);
    arc(&mut o, [R, R], R, PI, 1.5 * PI, 14);
    o.pop(); // back at the start
    o
}

fn area2(p: &[P2]) -> f32 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (u, v) = (p[i], p[(i + 1) % p.len()]);
        a += u[0] * v[1] - v[0] * u[1];
    }
    a / 2.0
}

/// Ear clipping: the outline's triangles (it is simple, partly concave).
fn triangulate(p: &[P2]) -> Vec<[usize; 3]> {
    let ccw = area2(p) > 0.0;
    let cross = |a: P2, b: P2, c: P2| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let inside = |a: P2, b: P2, c: P2, q: P2| {
        let (d1, d2, d3) = (cross(a, b, q), cross(b, c, q), cross(c, a, q));
        let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        !(neg && pos)
    };
    let mut idx: Vec<usize> = (0..p.len()).collect();
    let mut out = Vec::new();
    let mut guard = 0;
    while idx.len() > 3 && guard < 100_000 {
        guard += 1;
        let n = idx.len();
        let mut cut = None;
        for i in 0..n {
            let (a, b, c) = (idx[(i + n - 1) % n], idx[i], idx[(i + 1) % n]);
            let cv = cross(p[a], p[b], p[c]);
            let convex = if ccw { cv > 1e-6 } else { cv < -1e-6 };
            if !convex {
                continue;
            }
            if idx.iter().any(|&j| j != a && j != b && j != c && inside(p[a], p[b], p[c], p[j])) {
                continue;
            }
            out.push([a, b, c]);
            cut = Some(i);
            break;
        }
        match cut {
            Some(i) => {
                idx.remove(i);
            }
            None => break,
        }
    }
    if idx.len() == 3 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    out
}

/// Each vertex's way out (unit), its miter (to offset the outline by a
/// distance keeping its edges parallel), and whether the outline turns
/// sharply there (its sides then shaded flat on each side).
fn vertex_frames(p: &[P2]) -> Vec<(P2, P2, bool, P2, P2)> {
    let ccw = area2(p) > 0.0;
    let n = p.len();
    let edge_out = |a: P2, b: P2| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = (dx * dx + dy * dy).sqrt().max(1e-6);
        if ccw { [dy / l, -dx / l] } else { [-dy / l, dx / l] }
    };
    (0..n)
        .map(|i| {
            let (a, b, c) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n]);
            let (e0, e1) = (edge_out(a, b), edge_out(b, c));
            let s = [e0[0] + e1[0], e0[1] + e1[1]];
            let l = (s[0] * s[0] + s[1] * s[1]).sqrt().max(1e-6);
            let avg = [s[0] / l, s[1] / l];
            let cos = (avg[0] * e0[0] + avg[1] * e0[1]).max(0.3);
            let miter = [avg[0] / cos, avg[1] / cos];
            let sharp = e0[0] * e1[0] + e0[1] * e1[1] < 0.8;
            (avg, miter, sharp, e0, e1)
        })
        .collect()
}

/// A half's body: its outline extruded through its thickness with rounded
/// edges, its glass on top, its back below.
fn body(mesh: &mut Mesh, outline: &[P2], k: f32) {
    let frames = vertex_frames(outline);
    let n = outline.len();
    // The profile through the thickness: (inset, z, normal's out, up).
    let mut rings: Vec<(f32, f32, f32, f32)> = Vec::new();
    let steps = 5;
    for s in 0..=steps {
        let a = std::f32::consts::FRAC_PI_2 * (1.0 - s as f32 / steps as f32);
        rings.push((EDGE - EDGE * a.cos(), -THICK + EDGE - EDGE * a.sin(), a.cos(), -a.sin()));
    }
    for s in 0..=steps {
        let a = std::f32::consts::FRAC_PI_2 * s as f32 / steps as f32;
        rings.push((EDGE - EDGE * a.cos(), -EDGE + EDGE * a.sin(), a.cos(), a.sin()));
    }
    let at = |i: usize, r: (f32, f32, f32, f32), out: P2| {
        let (_, miter, _, _, _) = frames[i];
        let p = outline[i];
        (
            [(p[0] - miter[0] * r.0) * k, (p[1] - miter[1] * r.0) * k, r.1 * k],
            [out[0] * r.2, out[1] * r.2, r.3],
        )
    };
    for i in 0..n {
        let j = (i + 1) % n;
        let (avg_i, _, sharp_i, _, e1_i) = frames[i];
        let (avg_j, _, sharp_j, e0_j, _) = frames[j];
        let ni = if sharp_i { e1_i } else { avg_i };
        let nj = if sharp_j { e0_j } else { avg_j };
        for r in 0..rings.len() - 1 {
            let (r0, r1) = (rings[r], rings[r + 1]);
            mesh.quad(at(i, r0, ni), at(j, r0, nj), at(j, r1, nj), at(i, r1, ni), CHASSIS);
        }
    }
    // The faces: the glass (z 0) and the back (z -THICK), the outline inset
    // by the edges' rounding.
    let inset: Vec<P2> = (0..n).map(|i| [outline[i][0] - frames[i].1[0] * EDGE, outline[i][1] - frames[i].1[1] * EDGE]).collect();
    for t in triangulate(&inset) {
        let v = |i: usize, z: f32, nz: f32| ([inset[i][0] * k, inset[i][1] * k, z * k], [0.0, 0.0, nz]);
        mesh.tri(v(t[0], 0.0, 1.0), v(t[1], 0.0, 1.0), v(t[2], 0.0, 1.0), GLASS);
        mesh.tri(v(t[0], -THICK, -1.0), v(t[1], -THICK, -1.0), v(t[2], -THICK, -1.0), BACK);
    }
}




/// The logo on the left half's back: four squares, a hair out of the glass.
fn logo(mesh: &mut Mesh, k: f32) {
    let (cx, cy) = (HALF_W / 2.0, BODY_H / 2.0);
    let z = (-THICK - 0.03) * k;
    let n = [0.0, 0.0, -1.0];
    let s = LOGO_SQUARE;
    let g = LOGO_GAP / 2.0;
    for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let (x0, y0) = (cx + if dx < 0.0 { -g - s } else { g }, cy + if dy < 0.0 { -g - s } else { g });
        let c = |x: f32, y: f32| ([x * k, y * k, z], n);
        mesh.quad(c(x0, y0), c(x0 + s, y0), c(x0 + s, y0 + s), c(x0, y0 + s), MIRROR);
    }
}

/// The hinge's spine, whole: one metal bar the length of the phone that
/// both halves hang on (the Duo's halves come off a central spine; two
/// geared hinges and two rods are inside it). Its section a rounded bar
/// twice the phone's thickness long, about its middle: turned with half the
/// fold, it lies across the gap open flat (just under the glass, seen
/// between the screens) and stands as the stack's round spine closed.
fn spine(k: f32) -> Mesh {
    let mut m = Mesh::default();
    let (long, thick) = (2.0 * THICK, THICK - 0.4);
    let r = thick / 2.0;
    // The section, round: from the right end's top round to its bottom,
    // then the left end's.
    let mut sec: Vec<(P2, P2)> = Vec::new();
    let n = 14;
    for (cx, from) in [(long / 2.0 - r, std::f32::consts::FRAC_PI_2), (-long / 2.0 + r, -std::f32::consts::FRAC_PI_2)] {
        for i in 0..=n {
            // Clockwise round each end (x right, z up): right end from top
            // down, left end from bottom up.
            let t = from - std::f32::consts::PI * i as f32 / n as f32;
            sec.push(([cx + r * t.cos(), r * t.sin()], [t.cos(), t.sin()]));
        }
    }
    let (y0, y1) = (0.3, BODY_H - 0.3);
    let p = |q: P2, y: f32| [q[0] * k, y * k, q[1] * k];
    let nor = |q: P2| [q[0], 0.0, q[1]];
    for i in 0..sec.len() {
        let ((a, na), (b, nb)) = (sec[i], sec[(i + 1) % sec.len()]);
        // The straight runs between the ends shade flat.
        let (na, nb) = if (a[1] - b[1]).abs() < 1e-3 && (a[0] - b[0]).abs() > 1e-3 { ([0.0, a[1].signum()], [0.0, a[1].signum()]) } else { (na, nb) };
        m.quad((p(a, y0), nor(na)), (p(b, y0), nor(nb)), (p(b, y1), nor(nb)), (p(a, y1), nor(na)), HINGE);
    }
    for (y, ny) in [(y0, -1.0), (y1, 1.0)] {
        for i in 0..sec.len() {
            let (a, b) = (sec[i].0, sec[(i + 1) % sec.len()].0);
            m.tri((p([0.0, 0.0], y), [0.0, ny, 0.0]), (p(a, y), [0.0, ny, 0.0]), (p(b, y), [0.0, ny, 0.0]), HINGE);
        }
    }
    m
}

/// Half `i`'s screen: its panel on the glass, a hair over it, its picture
/// mapped corner to corner.
fn screen(i: usize, k: f32) -> Mesh {
    let mut m = Mesh::default();
    let x0 = if i == 0 { SCREEN_X.0 } else { SCREEN_X.1 - MID };
    let (x1, y0, y1, z) = (x0 + PANEL.0, SCREEN_TOP, SCREEN_TOP + PANEL.1, 0.02);
    let n = [0.0, 0.0, 1.0];
    let c = |x: f32, y: f32| [x * k, y * k, z * k];
    for (p, uv) in [(c(x0, y0), [0.0, 0.0]), (c(x1, y0), [1.0, 0.0]), (c(x1, y1), [1.0, 1.0]), (c(x0, y0), [0.0, 0.0]), (c(x1, y1), [1.0, 1.0]), (c(x0, y1), [0.0, 1.0])] {
        m.vert_uv(p, n, SCREEN, uv);
    }
    m
}

/// Half `i` (0 left, 1 right) whole, in its picture's frame, in px (`k` px
/// a mm).
fn half(i: usize, k: f32) -> Mesh {
    let mut m = Mesh::default();
    let left = left_outline();
    // The right half the left one mirrored, in its own frame (from the
    // spine).
    let outline: Vec<P2> = if i == 0 { left } else { left.iter().map(|p| [MID - p[0], p[1]]).collect() };
    body(&mut m, &outline, k);
    if i == 0 {
        logo(&mut m, k);
    }
    m
}

const VERTEX: &str = r#"
in vec3 a_pos;
in vec3 a_nor;
in float a_mat;
in vec2 a_uv;
uniform mat4 u_mv;
uniform mat4 u_p;
out vec3 v_pos;
out vec3 v_nor;
out float v_mat;
out vec2 v_uv;
void main() {
    vec4 p = u_mv * vec4(a_pos, 1.0);
    v_pos = p.xyz;
    v_nor = mat3(u_mv) * a_nor;
    v_mat = a_mat;
    v_uv = a_uv;
    gl_Position = u_p * p;
}
"#;

/// Lit as the window's light is (above, a little left and in front; y is
/// down): a soft diffuse with a sky above, a highlight by the material.
const FRAGMENT: &str = r#"
in vec3 v_pos;
in vec3 v_nor;
in float v_mat;
in vec2 v_uv;
uniform vec3 u_eye;
uniform sampler2D u_tex;
out vec4 o;
void main() {
    vec3 n = normalize(v_nor);
    vec3 l = normalize(vec3(-0.3, -0.75, 0.85));
    vec3 v = normalize(u_eye - v_pos);
    int m = int(v_mat + 0.5);
    vec3 base; float ks; float sh; float amb;
    if (m == 0) { base = vec3(0.035, 0.036, 0.04); ks = 0.55; sh = 90.0; amb = 0.6; }        // glass
    else if (m == 1) { base = vec3(0.79, 0.80, 0.77); ks = 0.22; sh = 24.0; amb = 0.45; }    // chassis
    else if (m == 2) { base = vec3(0.83, 0.845, 0.81); ks = 0.12; sh = 12.0; amb = 0.5; }    // back: frosted
    else if (m == 3) { base = vec3(0.62, 0.61, 0.57); ks = 0.55; sh = 40.0; amb = 0.4; }     // hinge
    else if (m == 4) { base = vec3(0.25, 0.25, 0.22); ks = 0.6; sh = 50.0; amb = 0.4; }      // rods
    else { base = vec3(0.02, 0.02, 0.022); ks = 0.05; sh = 8.0; amb = 0.5; }                 // the gap
    float diff = max(dot(n, l), 0.0);
    float sky = 0.5 + 0.5 * (-n.y);
    vec3 h = normalize(l + v);
    float spec = pow(max(dot(n, h), 0.0), sh) * ks;
    vec3 c = base * (amb * mix(0.55, 1.0, sky) + 0.6 * diff) + vec3(spec);
    // Glossy glass mirrors the room: a bright sky above fading to the floor,
    // stronger toward grazing (Fresnel); the frosted back a softer sheen.
    vec3 r = reflect(-v, n);
    float up = clamp(-r.y * 0.5 + 0.5, 0.0, 1.0);
    vec3 room = mix(vec3(0.05), vec3(0.95), smoothstep(0.35, 0.95, up));
    float fres = pow(1.0 - max(dot(n, v), 0.0), 3.0);
    if (m == 7) {
        // Mirror-polished metal: the room as it is mirrored, a hard highlight.
        c = mix(vec3(0.3, 0.3, 0.31), room, 0.85) + vec3(pow(max(dot(n, h), 0.0), 200.0) * 1.5);
    } else if (m == 6) {
        // A screen: its own light, under the glass's reflection.
        c = texture(u_tex, v_uv).rgb * 0.96 + room * mix(0.03, 0.45, fres);
    } else if (m == 0) {
        c += room * mix(0.08, 0.6, fres);
    } else if (m == 2) {
        c = mix(c, room, 0.06 + 0.25 * fres);
    }
    o = vec4(c, 1.0);
}
"#;

struct Gpu {
    gl: glow::Context,
    prog: glow::Program,
    halves: [(glow::VertexArray, glow::Buffer, i32); 2],
    screens: [(glow::VertexArray, glow::Buffer, i32); 2],
    spine: (glow::VertexArray, glow::Buffer, i32),
    /// The screens' pictures on the GPU, and which change each is.
    textures: [Option<glow::Texture>; 2],
    uploaded: [u64; 2],
    /// The multisampled target, and its size.
    msaa: Option<(glow::Framebuffer, glow::Renderbuffer, glow::Renderbuffer, i32, i32)>,
}

/// GL's functions, from EGL (GTK draws through it) or else GLX.
fn loader() -> impl Fn(&str) -> *const std::ffi::c_void {
    type GetProc = unsafe extern "C" fn(*const std::ffi::c_char) -> *const std::ffi::c_void;
    let lib: &'static libloading::Library = Box::leak(Box::new(unsafe {
        libloading::Library::new("libEGL.so.1").or_else(|_| libloading::Library::new("libGL.so.1")).expect("libEGL or libGL")
    }));
    let get: GetProc = unsafe {
        lib.get::<GetProc>(b"eglGetProcAddress\0").or_else(|_| lib.get::<GetProc>(b"glXGetProcAddressARB\0")).map(|s| *s).expect("a GL loader")
    };
    move |name: &str| {
        let c = std::ffi::CString::new(name).unwrap_or_default();
        unsafe { get(c.as_ptr()) }
    }
}

impl Gpu {
    fn new(es: bool, k: f32) -> Result<Gpu, String> {
        let load = loader();
        let gl = unsafe { glow::Context::from_loader_function(|s| load(s)) };
        let head = if es { "#version 300 es\nprecision highp float;\n" } else { "#version 330 core\n" };
        unsafe {
            let prog = gl.create_program()?;
            let mut shaders = Vec::new();
            for (kind, src) in [(glow::VERTEX_SHADER, VERTEX), (glow::FRAGMENT_SHADER, FRAGMENT)] {
                let s = gl.create_shader(kind)?;
                gl.shader_source(s, &format!("{head}{src}"));
                gl.compile_shader(s);
                if !gl.get_shader_compile_status(s) {
                    return Err(gl.get_shader_info_log(s));
                }
                gl.attach_shader(prog, s);
                shaders.push(s);
            }
            gl.bind_attrib_location(prog, 0, "a_pos");
            gl.bind_attrib_location(prog, 1, "a_nor");
            gl.bind_attrib_location(prog, 2, "a_mat");
            gl.bind_attrib_location(prog, 3, "a_uv");
            gl.link_program(prog);
            if !gl.get_program_link_status(prog) {
                return Err(gl.get_program_info_log(prog));
            }
            for s in shaders {
                gl.delete_shader(s);
            }
            let upload = |m: &Mesh| -> Result<(glow::VertexArray, glow::Buffer, i32), String> {
                let vao = gl.create_vertex_array()?;
                let vbo = gl.create_buffer()?;
                gl.bind_vertex_array(Some(vao));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
                let bytes: &[u8] = std::slice::from_raw_parts(m.v.as_ptr() as *const u8, m.v.len() * 4);
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
                let stride = 9 * 4;
                gl.enable_vertex_attrib_array(0);
                gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
                gl.enable_vertex_attrib_array(1);
                gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, stride, 12);
                gl.enable_vertex_attrib_array(2);
                gl.vertex_attrib_pointer_f32(2, 1, glow::FLOAT, false, stride, 24);
                gl.enable_vertex_attrib_array(3);
                gl.vertex_attrib_pointer_f32(3, 2, glow::FLOAT, false, stride, 28);
                gl.bind_vertex_array(None);
                Ok((vao, vbo, (m.v.len() / 9) as i32))
            };
            let halves = [upload(&half(0, k))?, upload(&half(1, k))?];
            let screens = [upload(&screen(0, k))?, upload(&screen(1, k))?];
            let spine = upload(&spine(k))?;
            Ok(Gpu { gl, prog, halves, screens, spine, textures: [None, None], uploaded: [0, 0], msaa: None })
        }
    }

    fn render(&mut self, scene: &Scene, w: i32, h: i32) {
        let Some(proj) = scene.proj else { return };
        let gl = &self.gl;
        unsafe {
            // GTK's framebuffer, to resolve into.
            let target = std::num::NonZeroU32::new(gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING) as u32).map(glow::NativeFramebuffer);
            // The multisampled target, made again when the size changes.
            if self.msaa.map(|m| (m.3, m.4)) != Some((w, h)) {
                if let Some((f, c, d, _, _)) = self.msaa.take() {
                    gl.delete_framebuffer(f);
                    gl.delete_renderbuffer(c);
                    gl.delete_renderbuffer(d);
                }
                let samples = gl.get_parameter_i32(glow::MAX_SAMPLES).clamp(1, 8);
                let (Ok(f), Ok(c), Ok(d)) = (gl.create_framebuffer(), gl.create_renderbuffer(), gl.create_renderbuffer()) else { return };
                gl.bind_renderbuffer(glow::RENDERBUFFER, Some(c));
                gl.renderbuffer_storage_multisample(glow::RENDERBUFFER, samples, glow::RGBA8, w, h);
                gl.bind_renderbuffer(glow::RENDERBUFFER, Some(d));
                gl.renderbuffer_storage_multisample(glow::RENDERBUFFER, samples, glow::DEPTH_COMPONENT24, w, h);
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(f));
                gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::RENDERBUFFER, Some(c));
                gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::DEPTH_ATTACHMENT, glow::RENDERBUFFER, Some(d));
                self.msaa = Some((f, c, d, w, h));
            }
            let Some((fb, _, _, _, _)) = self.msaa else { return };
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
            gl.viewport(0, 0, w, h);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear_depth_f32(1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LESS);
            gl.disable(glow::CULL_FACE);
            gl.use_program(Some(self.prog));
            let u_mv = gl.get_uniform_location(self.prog, "u_mv");
            let u_p = gl.get_uniform_location(self.prog, "u_p");
            gl.uniform_matrix_4_f32_slice(u_p.as_ref(), false, &proj.to_float());
            let u_eye = gl.get_uniform_location(self.prog, "u_eye");
            gl.uniform_3_f32(u_eye.as_ref(), scene.eye[0], scene.eye[1], scene.eye[2]);
            for (i, (vao, _, count)) in self.halves.iter().enumerate() {
                let Some(mv) = scene.halves[i] else { continue };
                gl.uniform_matrix_4_f32_slice(u_mv.as_ref(), false, &mv.to_float());
                gl.bind_vertex_array(Some(*vao));
                gl.draw_arrays(glow::TRIANGLES, 0, *count);
            }
            if let Some(mv) = scene.spine {
                gl.uniform_matrix_4_f32_slice(u_mv.as_ref(), false, &mv.to_float());
                gl.bind_vertex_array(Some(self.spine.0));
                gl.draw_arrays(glow::TRIANGLES, 0, self.spine.2);
            }
            // The screens, over their glass: their pictures uploaded when
            // they changed (mipmapped: they are drawn far smaller).
            for i in 0..2 {
                if self.uploaded[i] != scene.screens_changed[i] {
                    self.uploaded[i] = scene.screens_changed[i];
                    if let Some(t) = self.textures[i].take() {
                        gl.delete_texture(t);
                    }
                    if let Some(tex) = &scene.screens[i] {
                        let mut d = gtk::gdk::TextureDownloader::new(tex);
                        d.set_format(gtk::gdk::MemoryFormat::R8g8b8a8Premultiplied);
                        let (bytes, stride) = d.download_bytes();
                        if let Ok(t) = gl.create_texture() {
                            gl.bind_texture(glow::TEXTURE_2D, Some(t));
                            gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, (stride / 4) as i32);
                            gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, tex.width(), tex.height(), 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(Some(&bytes)));
                            gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
                            gl.generate_mipmap(glow::TEXTURE_2D);
                            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR_MIPMAP_LINEAR as i32);
                            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
                            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
                            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
                            self.textures[i] = Some(t);
                        }
                    }
                }
                let (Some(t), Some(mv)) = (self.textures[i], scene.halves[i]) else { continue };
                gl.active_texture(glow::TEXTURE0);
                gl.bind_texture(glow::TEXTURE_2D, Some(t));
                gl.uniform_1_i32(gl.get_uniform_location(self.prog, "u_tex").as_ref(), 0);
                gl.uniform_matrix_4_f32_slice(u_mv.as_ref(), false, &mv.to_float());
                gl.enable(glow::POLYGON_OFFSET_FILL);
                gl.polygon_offset(-1.0, -4.0);
                let (vao, _, count) = self.screens[i];
                gl.bind_vertex_array(Some(vao));
                gl.draw_arrays(glow::TRIANGLES, 0, count);
                gl.disable(glow::POLYGON_OFFSET_FILL);
            }
            gl.bind_vertex_array(None);
            // Onto GTK's framebuffer, resolved.
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(fb));
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, target);
            gl.blit_framebuffer(0, 0, w, h, 0, 0, w, h, glow::COLOR_BUFFER_BIT, glow::NEAREST);
            gl.bind_framebuffer(glow::FRAMEBUFFER, target);
        }
    }
}

/// The GL area drawing `scene`, `w` x `h` px, `k` px a mm.
pub fn area(scene: Rc<RefCell<Scene>>, w: i32, h: i32, k: f32) -> gtk::GLArea {
    let area = gtk::GLArea::builder().has_depth_buffer(false).can_target(false).width_request(w).height_request(h).build();
    let gpu: Rc<RefCell<Option<Gpu>>> = Rc::default();
    area.connect_realize({
        let gpu = gpu.clone();
        move |a| {
            a.make_current();
            if let Some(e) = a.error() {
                eprintln!("cradle: 3D: {e}");
                return;
            }
            let es = a.context().is_some_and(|c| c.api() == gtk::gdk::GLAPI::GLES);
            match Gpu::new(es, k) {
                Ok(g) => *gpu.borrow_mut() = Some(g),
                Err(e) => eprintln!("cradle: 3D: {e}"),
            }
        }
    });
    area.connect_unrealize({
        let gpu = gpu.clone();
        move |a| {
            a.make_current();
            gpu.borrow_mut().take();
        }
    });
    area.connect_render(move |a, _| {
        if let Some(g) = gpu.borrow_mut().as_mut() {
            let s = a.scale_factor();
            g.render(&scene.borrow(), a.width() * s, a.height() * s);
        }
        glib::Propagation::Stop
    });
    area
}
