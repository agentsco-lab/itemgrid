//! The USB-C cable's cord, alive: a rope of small links (Verlet) that hangs
//! from the plug, swings as the phone is turned and comes to rest on the
//! table, sliding there with some friction; its far end lies on the table
//! off to the right, toward the computer.
//!
//! Worked out in the table's frame (the phone's middle at the origin, the
//! table level at `table`, up +z - gravity -z; the phone turns in it as it
//! is held), drawn through the view's matrix with its perspective: a tube
//! (dark at its sides, a highlight toward the light) over its shadow on the
//! table, which fades as the cord rises. The plug is the window's (layers
//! fixed to the right half); this starts where the plug's strain relief
//! ends, stiff there as a real one is.

use gtk::graphene;

type V = [f32; 3];

fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V, k: f32) -> V {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn len(a: V) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// Links in the cord.
const LINKS: usize = 30;
/// Constraint passes a step.
const PASSES: usize = 30;

#[derive(Default)]
pub struct Rope {
    pts: Vec<V>,
    prev: Vec<V>,
    seg: f32,
    /// Where the cord leaves the plug and which way, and its far end on the
    /// table; the table's level; px a mm.
    pub start: V,
    pub dir: V,
    pub end: V,
    pub table: f32,
    pub k: f32,
    /// The plug's outline (its top face's corners), for its shadow.
    pub plug: [V; 4],
    /// The table's frame to the drawing's (with the perspective).
    pub screen: Option<graphene::Matrix>,
    /// Where the drawing's origin is in the duo's (it reaches past it).
    pub offset: (f32, f32),
    /// Steps since anything moved: at rest, not worked out again.
    quiet: u32,
    last_start: V,
}

impl Rope {
    fn radius(&self) -> f32 {
        1.7 * self.k
    }

    fn lay(&mut self) {
        let d = len(sub(self.end, self.start));
        // A little slack, as a cable lies from a phone to a computer: more
        // crumples on the table.
        // As much as the bends take (out of the plug toward the viewer, then
        // round toward the computer), hardly more.
        let total = d * 1.12;
        self.seg = total / (LINKS - 1) as f32;
        // Laid as a smooth curve: out of the plug its way, then to the far
        // end (a quadratic through a point ahead of the plug).
        let c1 = add(self.start, mul(self.dir, d * 0.4));
        let c2 = sub(self.end, [d * 0.4, 0.0, 0.0]);
        self.pts = (0..LINKS)
            .map(|i| {
                let t = i as f32 / (LINKS - 1) as f32;
                let u = 1.0 - t;
                let p = add(add(mul(self.start, u * u * u), mul(c1, 3.0 * u * u * t)), add(mul(c2, 3.0 * u * t * t), mul(self.end, t * t * t)));
                [p[0], p[1], p[2].max(self.table + self.radius())]
            })
            .collect();
        self.prev = self.pts.clone();
        self.quiet = 0;
    }

    /// One step of `dt` seconds; whether anything moved.
    pub fn step(&mut self, dt: f32) -> bool {
        if self.k <= 0.0 {
            return false;
        }
        if self.pts.len() != LINKS {
            self.lay();
        }
        let moved_start = len(sub(self.start, self.last_start)) > 0.05;
        self.last_start = self.start;
        if moved_start {
            self.quiet = 0;
        }
        if self.quiet > 90 {
            return false;
        }
        // Pulled taut further than it reaches: more of it off the desk.
        if len(sub(self.end, self.start)) > self.seg * (LINKS - 1) as f32 * 0.92 {
            self.lay();
        }
        let r = self.radius();
        let g = 9810.0 * self.k;
        let dt = dt.clamp(1.0 / 240.0, 1.0 / 30.0);
        let mut motion = 0.0;
        for i in 2..LINKS - 2 {
            let p = self.pts[i];
            let v = mul(sub(p, self.prev[i]), 0.985);
            self.prev[i] = p;
            self.pts[i] = add(add(p, v), [0.0, 0.0, -g * dt * dt]);
        }
        let pin = |s: &mut Rope| {
            s.pts[0] = s.start;
            s.pts[1] = add(s.start, mul(s.dir, s.seg));
            s.pts[LINKS - 1] = s.end;
            // It goes on off to the right, along the table.
            s.pts[LINKS - 2] = sub(s.end, [s.seg, 0.0, 0.0]);
        };
        pin(self);
        for _ in 0..PASSES {
            for i in 0..LINKS - 1 {
                let d = sub(self.pts[i + 1], self.pts[i]);
                let l = len(d).max(1e-4);
                let c = mul(d, (l - self.seg) / l);
                let (a, b) = (i >= 2 && i < LINKS - 2, i + 1 >= 2 && i + 1 < LINKS - 2);
                match (a, b) {
                    (true, true) => {
                        self.pts[i] = add(self.pts[i], mul(c, 0.5));
                        self.pts[i + 1] = sub(self.pts[i + 1], mul(c, 0.5));
                    }
                    (true, false) => self.pts[i] = add(self.pts[i], c),
                    (false, true) => self.pts[i + 1] = sub(self.pts[i + 1], c),
                    _ => {}
                }
            }
            // Stiffness: no sharper bend than a cable takes (a radius of
            // about 12 mm: links two apart nearly their whole length apart).
            let turn = self.seg / (12.0 * self.k);
            let min = 2.0 * self.seg * (turn / 2.0).cos();
            for i in 0..LINKS - 2 {
                let d = sub(self.pts[i + 2], self.pts[i]);
                let l = len(d).max(1e-4);
                if l < min {
                    let c = mul(d, (min - l) / l * 0.35);
                    if i >= 2 {
                        self.pts[i] = sub(self.pts[i], c);
                    }
                    if i + 2 < LINKS - 2 {
                        self.pts[i + 2] = add(self.pts[i + 2], c);
                    }
                }
            }
            // The table: on it, not through it; sliding there slows.
            for i in 2..LINKS - 2 {
                if self.pts[i][2] < self.table + r {
                    self.pts[i][2] = self.table + r;
                    for a in 0..2 {
                        self.prev[i][a] = self.pts[i][a] - (self.pts[i][a] - self.prev[i][a]) * 0.93;
                    }
                    self.prev[i][2] = self.pts[i][2];
                }
            }
            pin(self);
        }
        for i in 0..LINKS {
            motion += len(sub(self.pts[i], self.prev[i]));
        }
        if motion < 0.02 * LINKS as f32 {
            self.quiet += 1;
        } else {
            self.quiet = 0;
        }
        true
    }

    fn project(&self, m: &graphene::Matrix, p: V) -> (f64, f64) {
        let v = m.transform_vec4(&graphene::Vec4::new(p[0], p[1], p[2], 1.0));
        let w = if v.w().abs() < 1e-6 { 1.0 } else { v.w() };
        ((v.x() / w - self.offset.0) as f64, (v.y() / w - self.offset.1) as f64)
    }

    /// The table's surface marked in centimetre squares, as a cutting mat:
    /// thin lines in its plane under the phone, fading out from its middle.
    pub fn draw_table(&self, cr: &gtk::cairo::Context) {
        let Some(m) = self.screen else { return };
        if self.k <= 0.0 {
            return;
        }
        let step = 10.0 * self.k;
        let reach = 150.0 * self.k;
        let z = self.table;
        // Faded by the distance on the table itself (the screen's circle
        // left hard edges where the perspective shortens it).
        let alpha = |x: f32, y: f32| {
            let d = (x * x + y * y).sqrt() / reach;
            (0.085 * (1.0 - d * d).max(0.0).powf(1.5)) as f64
        };
        let n = (reach / step).ceil() as i32;
        let pieces = 24;
        cr.set_line_width(1.0);
        for i in -n..=n {
            let t = i as f32 * step;
            for along_x in [true, false] {
                for j in 0..pieces {
                    let (u0, u1) = (-reach + 2.0 * reach * j as f32 / pieces as f32, -reach + 2.0 * reach * (j + 1) as f32 / pieces as f32);
                    let (a, b) = if along_x { ([u0, t, z], [u1, t, z]) } else { ([t, u0, z], [t, u1, z]) };
                    let mid = ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0);
                    let al = alpha(mid.0, mid.1);
                    if al < 0.003 {
                        continue;
                    }
                    let (x0, y0) = self.project(&m, a);
                    let (x1, y1) = self.project(&m, b);
                    cr.set_source_rgba(0.0, 0.0, 0.0, al);
                    cr.move_to(x0, y0);
                    cr.line_to(x1, y1);
                    let _ = cr.stroke();
                }
            }
        }
    }

    /// A smooth path through projected points (Catmull-Rom as Béziers).
    fn path(&self, cr: &gtk::cairo::Context, m: &graphene::Matrix, pts: &[V], off: (f64, f64)) {
        let q: Vec<(f64, f64)> = pts.iter().map(|p| self.project(m, *p)).map(|(x, y)| (x + off.0, y + off.1)).collect();
        cr.new_path();
        cr.move_to(q[0].0, q[0].1);
        for i in 0..q.len() - 1 {
            let p0 = q[i.saturating_sub(1)];
            let (p1, p2) = (q[i], q[i + 1]);
            let p3 = q[(i + 2).min(q.len() - 1)];
            let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
            let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
            cr.curve_to(c1.0, c1.1, c2.0, c2.1, p2.0, p2.1);
        }
    }

    /// The cord drawn: its shadow on the table, then the tube; the far end
    /// fading.
    pub fn draw(&self, cr: &gtk::cairo::Context) {
        let Some(m) = self.screen else { return };
        if self.pts.len() != LINKS {
            return;
        }
        let r = self.radius();
        // Its width where it leaves the plug, in the drawing's px.
        let (x0, y0) = self.project(&m, self.pts[0]);
        let (x1, y1) = self.project(&m, add(self.pts[0], [r, 0.0, 0.0]));
        let wd = (((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt() * 2.0).max(1.5);
        // Faded along its far part: a gradient from halfway to the end.
        let fade = |cr: &gtk::cairo::Context, rgb: (f64, f64, f64), a: f64, off: (f64, f64)| {
            let (ax, ay) = self.project(&m, self.pts[LINKS / 2]);
            let (bx, by) = self.project(&m, self.pts[LINKS - 2]);
            let g = gtk::cairo::LinearGradient::new(ax + off.0, ay + off.1, bx + off.0, by + off.1);
            g.add_color_stop_rgba(0.0, rgb.0, rgb.1, rgb.2, a);
            g.add_color_stop_rgba(1.0, rgb.0, rgb.1, rgb.2, 0.0);
            let _ = cr.set_source(&g);
        };
        // The shadow: where the light from above and a little in front puts
        // it - down and right of the cord on the table, further and fainter
        // as the cord rises.
        let shadow: Vec<V> = self
            .pts
            .iter()
            .map(|p| {
                let up = (p[2] - self.table).max(0.0);
                [p[0] + 0.6 * self.k + up * 0.08, p[1] + 1.0 * self.k + up * 0.12, self.table]
            })
            .collect();
        let rise = self.pts.iter().map(|p| p[2] - self.table).fold(0.0f32, f32::max);
        let strength = 0.16 * (-(rise / (40.0 * self.k))).exp() as f64 + 0.05;
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.set_line_join(gtk::cairo::LineJoin::Round);
        // The plug's shadow.
        cr.new_path();
        for (i, c) in self.plug.iter().enumerate() {
            let up = (c[2] - self.table).max(0.0);
            let (x, y) = self.project(&m, [c[0] + 0.6 * self.k + up * 0.08, c[1] + 1.0 * self.k + up * 0.12, self.table]);
            if i == 0 {
                cr.move_to(x, y);
            } else {
                cr.line_to(x, y);
            }
        }
        cr.close_path();
        // Only near the table: high above it the plug's shadow is too soft
        // to see.
        let up = self.plug.iter().map(|c| c[2] - self.table).fold(f32::MAX, f32::min).max(0.0);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.12 * (-(up / (6.0 * self.k))).exp() as f64);
        let _ = cr.fill();
        for (w, a) in [(1.6, 0.4), (1.1, 0.7)] {
            self.path(cr, &m, &shadow, (0.0, 0.0));
            fade(cr, (0.0, 0.0, 0.0), strength * a, (0.0, 0.0));
            cr.set_line_width(wd * w);
            let _ = cr.stroke();
        }
        // The tube: white plastic - its rim in shade, its middle lit, a
        // highlight toward the light (up and left).
        for (w, c, o) in [(1.0, 0.70, 0.0), (0.78, 0.84, -0.1), (0.52, 0.93, -0.18), (0.16, 1.0, -0.26)] {
            let off = (o * wd, o * wd);
            self.path(cr, &m, &self.pts, off);
            fade(cr, (c, c + 0.004, c + 0.01), 1.0, off);
            cr.set_line_width(wd * w);
            let _ = cr.stroke();
        }
    }
}
