//! The table's squares on the GPU: a GLArea under the floor's drawing, each
//! pixel looked up on the table through the view turned back and lit where
//! a line passes, as strong as the floor's fading says. Drawn by cairo
//! (on the CPU, in the frame's paint) the lines took 12-20 ms a frame of
//! the wheel's zoom; here next to nothing.

use std::cell::RefCell;
use std::rc::Rc;

use glow::HasContext;
use gtk::glib;
use gtk::prelude::*;

/// What the squares are drawn from (all in the page's px and the table's
/// px, as the floor's own drawing has them).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Grid {
    /// The page (less `off`, homogeneous) to the table: the view turned
    /// back (rows).
    pub inv: [[f32; 3]; 3],
    /// The view's w of a place on the table (x, y, 1): in front of the eye
    /// above 0.005 (the page bottom lies that near: below it a band was bare).
    pub w_row: [f32; 3],
    pub off: (f32, f32),
    pub step: f32,
    pub shift: (f32, f32),
    /// The fading's middle, how far it reaches.
    pub mid: (f32, f32),
    pub reach: f32,
    /// The start: the squares grown out from the cubes so far (`grid` < 1).
    pub cubes_at: (f32, f32),
    pub grown: f32,
    pub grid: f32,
    /// The lines' ink (premultiplied by `ink_k`).
    pub ink: [f32; 3],
    pub ink_k: f32,
    /// The lines' width, times the old 1.6 px.
    pub width: f32,
    /// Night coming (or going): waves from the night button across the
    /// table, each turning the squares over to their other side - how far
    /// each has come (0..1) and from where; how many; whether it is night
    /// before them; the table's fill and the lines' ink by day and night.
    pub waves: [(f32, (f32, f32)); WAVES],
    pub wave_n: i32,
    /// Which way they come (main.rs's TURNS): 0 the squares turning over
    /// in a ring, 1 ink spreading soft-edged, 2 the whole table fading, 3
    /// the far corners first (to night; the middle first to day), 4 rows
    /// turning over from the top down.
    pub turn: i32,
    pub base_night: bool,
    pub fill_day: [f32; 3],
    pub fill_night: [f32; 3],
    pub ink_day: [f32; 3],
    pub ink_night: [f32; 3],
    /// How strong the lines are drawn (times the one strength: main.rs's
    /// GRIDS), on the day's and the night's ink alike.
    pub grid_k: f32,
    /// The table's point under the page's bottom middle: what the band
    /// too near the eye to be drawn takes its colour from.
    pub near_at: (f32, f32),
    /// The page's size (px): the table, paper and lines, fades out toward
    /// its edges - see-through there, no hard edge against what is round
    /// the window (the window itself shows nothing under it: main.rs's
    /// see-through class).
    pub size: (f32, f32),
    /// The lamp under the table (lamp.rs): where it stands (table px),
    /// how far its spot reaches (px; 0: the whole table lit alike) and how
    /// strong it is (0: no light). Its light is the lines' colour glowing
    /// round each line, through the slits between the squares.
    pub lamp: (f32, f32),
    pub lamp_r: f32,
    pub lamp_k: f32,
}

/// How many waves may be on their way at once.
pub const WAVES: usize = 6;

/// Whether the GPU draws the squares (else the floor's cairo does).
pub static ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn on() -> bool {
    ON.load(std::sync::atomic::Ordering::Relaxed)
}

const VERTEX: &str = "
const vec2 corners[3] = vec2[3](vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
void main() { gl_Position = vec4(corners[gl_VertexID], 0.0, 1.0); }
";

const FRAGMENT: &str = "
uniform mat3 u_inv;
uniform vec3 u_w;
uniform vec2 u_off;
uniform float u_scale;
uniform float u_height;
uniform float u_step;
uniform vec2 u_shift;
uniform vec2 u_mid;
uniform float u_reach;
uniform vec2 u_cubes;
uniform float u_grown;
uniform float u_grid;
uniform vec4 u_ink;
uniform float u_width;
uniform int u_wave_n;
uniform int u_turn;
uniform float u_wave_p[6];
uniform vec2 u_wave_from[6];
uniform float u_base_night;
uniform vec3 u_fill_day;
uniform vec3 u_fill_night;
uniform vec4 u_ink_day;
uniform vec4 u_ink_night;
uniform vec2 u_near_at;
uniform vec2 u_size;
uniform vec2 u_lamp;
uniform float u_lamp_r;
uniform float u_lamp_k;
out vec4 color;

float line(float t, float shift) {
    // How far from the nearest line, in the page's device px.
    float d = abs(fract((t - shift) / u_step + 0.5) - 0.5) * u_step;
    float px = length(vec2(dFdx(t), dFdy(t)));
    float half_w = 0.8 * u_width * u_scale;
    return clamp(half_w + 0.5 - d / max(px, 1e-6), 0.0, 1.0);
}

void main() {
    vec2 page = vec2(gl_FragCoord.x, u_height - gl_FragCoord.y) / u_scale - u_off;
    vec3 q = u_inv * vec3(page, 1.0);
    vec2 t = q.xy / q.z;
    float a = 0.0;
    bool seen = dot(u_w, vec3(t, 1.0)) > 0.005;
    // Toward the page's edges the table goes, paper and lines: a soft rim,
    // see-through, not a cut.
    float rim = 0.09 * min(u_size.x, u_size.y);
    vec2 e = smoothstep(vec2(0.0), vec2(rim), min(page + u_off, u_size - page - u_off));
    float edge = e.x * e.y;
    if (seen) {
        float d = length(t - u_mid) / u_reach;
        float reached = u_grid >= 1.0 ? 1.0 : clamp((u_grown - length(t - u_cubes)) / (2.0 * u_step), 0.0, 1.0);
        a = 0.15 * pow(max(1.0 - d, 0.0), 1.3) * reached;
        if (a < 0.004) a = 0.0;
    }
    float lit = a > 0.0 ? max(line(t.x, u_shift.x), line(t.y, u_shift.y)) : 0.0;
    // Light under the table (u_light): through the slits between the
    // squares, a glow of the lines' colour falling off from each line
    // (a few lines' widths), with the lines as the squares fade.
    float px1 = length(vec2(dFdx(t.x), dFdy(t.x)));
    float halo = 0.0;
    // The lamp's spot: its light falls off from where it stands, gone a
    // reach or so away (a lamp reaching 0 lights the whole table alike).
    float spot = 1.0;
    if (u_lamp_r > 0.0) {
        float r = length(t - u_lamp) / u_lamp_r;
        spot = exp(-2.5 * r * r);
    }
    if (u_lamp_k > 0.0 && a > 0.0) {
        float dx = abs(fract((t.x - u_shift.x) / u_step + 0.5) - 0.5) * u_step;
        float dy = abs(fract((t.y - u_shift.y) / u_step + 0.5) - 0.5) * u_step;
        float d = min(dx, dy) / max(px1, 1e-6);
        // A bright seam, falling off within a line's width or two, and a
        // faint spill further out; as strong as the lines themselves are
        // here (a), so it fades with them.
        float w = u_width * u_scale;
        halo = (0.9 * exp(-d / (1.2 * w)) + 0.15 * exp(-d / (5.0 * w))) * a * 2.0 * u_lamp_k * spot;
    }
    vec4 ink = u_ink;
    vec4 under = vec4(u_base_night > 0.5 ? u_fill_night : u_fill_day, 1.0);
    if (u_wave_n > 0) {
        // (Too near the eye to be drawn: as the nearest row.)
        if (!seen) t = u_near_at;
        vec2 cell = floor((t - u_shift) / u_step);
        vec2 c = (cell + 0.5) * u_step + u_shift;
        float spread = 6.0 * u_step;
        float far = u_reach * 1.6 + spread;
        bool now = u_base_night > 0.5;
        bool after = now;
        if (u_turn == 0 || u_turn == 4) {
            // Each wave that has passed this square turned it over once:
            // the face shown now, and what it is turning to (the one
            // passing) - in a ring from where it began, or row by row
            // from the top (blinds).
            float face = 1.0;
            for (int i = 0; i < 6; i++) {
                if (i >= u_wave_n) break;
                float p = u_turn == 4
                    ? clamp((u_wave_p[i] * (far + 7.0 * u_step) - (c.y - u_wave_from[i].y + 4.0 * u_step)) / (3.0 * u_step), 0.0, 1.0)
                    : clamp((u_wave_p[i] * far - length(c - u_wave_from[i])) / spread, 0.0, 1.0);
                if (p > 0.5) now = !now;
                if (p > 0.0) after = !after;
                if (p > 0.0 && p < 1.0) face = abs(cos(p * 3.14159265));
            }
            // The square turned over about its middle line: its face
            // squeezed to it and grown back the other colour, the other
            // colour beside.
            float v = abs(fract((t.y - u_shift.y) / u_step) - 0.5) * 2.0;
            float fw = fwidth(v);
            float beside = smoothstep(face - fw, face + fw, v);
            vec3 fill = mix(now ? u_fill_night : u_fill_day, after ? u_fill_night : u_fill_day, beside);
            ink = (beside > 0.5 ? after : now) ? u_ink_night : u_ink_day;
            under = vec4(fill, 1.0);
        } else {
            // Soft: each wave mixes the colour under it toward the other
            // side as far as it has come here - ink spreading from where it
            // began, the whole table at once (switch), or from the far
            // corners in (to night) and the middle out (to day): the lamp.
            vec3 fill = now ? u_fill_night : u_fill_day;
            ink = now ? u_ink_night : u_ink_day;
            for (int i = 0; i < 6; i++) {
                if (i >= u_wave_n) break;
                float d = length(t - u_wave_from[i]);
                float p;
                if (u_turn == 2) p = u_wave_p[i];
                else if (u_turn == 3 && !after) p = clamp((u_wave_p[i] * (u_reach + spread) - (u_reach - d)) / spread, 0.0, 1.0);
                else if (u_turn == 3) p = clamp((u_wave_p[i] * (u_reach + spread) - d) / spread, 0.0, 1.0);
                else p = clamp((u_wave_p[i] * far - d) / spread, 0.0, 1.0);
                after = !after;
                float m = smoothstep(0.0, 1.0, p);
                fill = mix(fill, after ? u_fill_night : u_fill_day, m);
                ink = mix(ink, after ? u_ink_night : u_ink_day, m);
            }
            under = vec4(fill, 1.0);
        }
    }
    float k = min(a * ink.a * lit, 1.0);
    // The glow over the paper, the lines over both, premultiplied, gone at
    // the rim.
    float kh = min(halo * ink.a, 1.0);
    vec4 base = vec4(ink.rgb * kh, kh) + under * (1.0 - kh);
    // Under the lamp the squares' faces catch a little of its light too, so
    // the spot reads as one between the lines (not with the whole table lit).
    if (u_lamp_r > 0.0 && u_lamp_k > 0.0 && a > 0.0) {
        float kf = min(0.07 * spot * u_lamp_k * (a / 0.15), 1.0);
        base = vec4(ink.rgb * kf, kf) + base * (1.0 - kf);
    }
    color = (vec4(ink.rgb * k, k) + base * (1.0 - k)) * edge;
}
";

struct Gpu {
    gl: glow::Context,
    prog: glow::Program,
    vao: glow::VertexArray,
}

impl Gpu {
    fn new(es: bool) -> Result<Gpu, String> {
        let load = crate::duo3d::loader();
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
            gl.link_program(prog);
            if !gl.get_program_link_status(prog) {
                return Err(gl.get_program_info_log(prog));
            }
            for s in shaders {
                gl.delete_shader(s);
            }
            let vao = gl.create_vertex_array()?;
            Ok(Gpu { gl, prog, vao })
        }
    }

    fn render(&self, g: &Grid, w: i32, h: i32, scale: f32) {
        let gl = &self.gl;
        unsafe {
            gl.viewport(0, 0, w, h);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.disable(glow::BLEND);
            gl.disable(glow::DEPTH_TEST);
            gl.use_program(Some(self.prog));
            let u = |name: &str| gl.get_uniform_location(self.prog, name);
            let inv: Vec<f32> = (0..3).flat_map(|c| (0..3).map(move |r| g.inv[r][c])).collect();
            gl.uniform_matrix_3_f32_slice(u("u_inv").as_ref(), false, &inv);
            gl.uniform_3_f32(u("u_w").as_ref(), g.w_row[0], g.w_row[1], g.w_row[2]);
            gl.uniform_2_f32(u("u_off").as_ref(), g.off.0, g.off.1);
            gl.uniform_1_f32(u("u_scale").as_ref(), scale);
            gl.uniform_1_f32(u("u_height").as_ref(), h as f32);
            gl.uniform_1_f32(u("u_step").as_ref(), g.step);
            gl.uniform_2_f32(u("u_shift").as_ref(), g.shift.0, g.shift.1);
            gl.uniform_2_f32(u("u_mid").as_ref(), g.mid.0, g.mid.1);
            gl.uniform_1_f32(u("u_reach").as_ref(), g.reach);
            gl.uniform_2_f32(u("u_cubes").as_ref(), g.cubes_at.0, g.cubes_at.1);
            gl.uniform_1_f32(u("u_grown").as_ref(), g.grown);
            gl.uniform_1_f32(u("u_grid").as_ref(), g.grid);
            gl.uniform_4_f32(u("u_ink").as_ref(), g.ink[0], g.ink[1], g.ink[2], g.ink_k);
            gl.uniform_1_f32(u("u_width").as_ref(), g.width);
            gl.uniform_1_i32(u("u_wave_n").as_ref(), g.wave_n);
            gl.uniform_1_i32(u("u_turn").as_ref(), g.turn);
            let ps: Vec<f32> = g.waves.iter().map(|w| w.0).collect();
            let froms: Vec<f32> = g.waves.iter().flat_map(|w| [w.1 .0, w.1 .1]).collect();
            gl.uniform_1_f32_slice(u("u_wave_p").as_ref(), &ps);
            gl.uniform_2_f32_slice(u("u_wave_from").as_ref(), &froms);
            gl.uniform_1_f32(u("u_base_night").as_ref(), if g.base_night { 1.0 } else { 0.0 });
            gl.uniform_3_f32(u("u_fill_day").as_ref(), g.fill_day[0], g.fill_day[1], g.fill_day[2]);
            gl.uniform_3_f32(u("u_fill_night").as_ref(), g.fill_night[0], g.fill_night[1], g.fill_night[2]);
            gl.uniform_4_f32(u("u_ink_day").as_ref(), g.ink_day[0], g.ink_day[1], g.ink_day[2], g.grid_k);
            gl.uniform_4_f32(u("u_ink_night").as_ref(), g.ink_night[0], g.ink_night[1], g.ink_night[2], 1.15 * g.grid_k);
            gl.uniform_2_f32(u("u_near_at").as_ref(), g.near_at.0, g.near_at.1);
            gl.uniform_2_f32(u("u_size").as_ref(), g.size.0, g.size.1);
            gl.uniform_2_f32(u("u_lamp").as_ref(), g.lamp.0, g.lamp.1);
            gl.uniform_1_f32(u("u_lamp_r").as_ref(), g.lamp_r);
            gl.uniform_1_f32(u("u_lamp_k").as_ref(), g.lamp_k);
            gl.bind_vertex_array(Some(self.vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_vertex_array(None);
        }
    }
}

/// The GL area drawing the squares `grid()` says (None: nothing).
pub fn area(grid: impl Fn() -> Option<Grid> + 'static) -> gtk::GLArea {
    let area = gtk::GLArea::builder().has_depth_buffer(false).can_target(false).hexpand(true).vexpand(true).build();
    let gpu: Rc<RefCell<Option<Gpu>>> = Rc::default();
    area.connect_realize({
        let gpu = gpu.clone();
        move |a| {
            a.make_current();
            if let Some(e) = a.error() {
                eprintln!("itemgrid: the squares on the GPU: {e}");
                return;
            }
            let es = a.context().is_some_and(|c| c.api() == gtk::gdk::GLAPI::GLES);
            match Gpu::new(es) {
                Ok(g) => {
                    *gpu.borrow_mut() = Some(g);
                    ON.store(std::env::var_os("ITEMGRID_CAIRO_GRID").is_none(), std::sync::atomic::Ordering::Relaxed);
                }
                Err(e) => eprintln!("itemgrid: the squares on the GPU: {e}"),
            }
        }
    });
    area.connect_unrealize({
        let gpu = gpu.clone();
        move |a| {
            a.make_current();
            ON.store(false, std::sync::atomic::Ordering::Relaxed);
            gpu.borrow_mut().take();
        }
    });
    area.connect_render(move |a, _| {
        let s = a.scale_factor();
        match (gpu.borrow().as_ref(), grid()) {
            (Some(g), Some(grid)) if on() => g.render(&grid, a.width() * s, a.height() * s, s as f32),
            _ => unsafe {
                if let Some(g) = gpu.borrow().as_ref() {
                    g.gl.clear_color(0.0, 0.0, 0.0, 0.0);
                    g.gl.clear(glow::COLOR_BUFFER_BIT);
                }
            },
        }
        glib::Propagation::Stop
    });
    area
}
