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
    /// The hole in the table the far end goes down: its square (x0, y0, x1,
    /// y1) and depth.
    pub hole: Option<([f32; 4], f32)>,
    /// How far before the duo's drawing the cord's and the 3D area begin.
    pub gl_past: f32,
    /// And how far above it the 3D area begins.
    pub gl_past_top: f32,
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
        let total = d * 1.05 + 30.0 * self.k;
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

    /// The table, the hole's walls and floor, and its rounded rim: where a
    /// link may be.
    fn collide(&mut self, i: usize) {
        let r = self.radius();
        let p = self.pts[i];
        let Some(([x0, y0, x1, y1], depth)) = self.hole else {
            if p[2] < self.table + r {
                self.rest_on(i, self.table + r);
            }
            return;
        };
        let inside = p[0] > x0 && p[0] < x1 && p[1] > y0 && p[1] < y1;
        if inside {
            // Down to its floor, and within its walls below the table.
            if p[2] < self.table - depth + r {
                self.rest_on(i, self.table - depth + r);
            }
            if self.pts[i][2] < self.table {
                self.pts[i][0] = self.pts[i][0].clamp(x0 + r, x1 - r);
                self.pts[i][1] = self.pts[i][1].clamp(y0 + r, y1 - r);
            }
        } else if p[2] < self.table + r {
            self.rest_on(i, self.table + r);
        }
        // The rim, rounded (as a cylinder along each edge): the cord goes
        // over it in a curve, not through its corner.
        let rim = 1.5 * self.k + r;
        let p = self.pts[i];
        let edges = [(1, y0, x0, x1), (1, y1, x0, x1), (0, x0, y0, y1), (0, x1, y0, y1)];
        for (axis, at, lo, hi) in edges {
            let along = if axis == 1 { p[0] } else { p[1] };
            if along < lo || along > hi {
                continue;
            }
            let across = p[axis] - at;
            let up = p[2] - self.table;
            let d = (across * across + up * up).sqrt();
            if d < rim && d > 1e-4 {
                let k = rim / d;
                self.pts[i][axis] = at + across * k;
                self.pts[i][2] = self.table + up * k;
            }
        }
    }

    /// Resting on a surface at `z`: there, sliding on it slowed - and held
    /// still when it hardly slides (without that it crept on for ever).
    fn rest_on(&mut self, i: usize, z: f32) {
        self.pts[i][2] = z;
        let v = [self.pts[i][0] - self.prev[i][0], self.pts[i][1] - self.prev[i][1]];
        let slow = (v[0] * v[0] + v[1] * v[1]).sqrt() < 0.5;
        for a in 0..2 {
            self.prev[i][a] = if slow { self.pts[i][a] } else { self.pts[i][a] - v[a] * 0.8 };
        }
        self.prev[i][2] = z;
    }

    fn pin(&mut self) {
        self.pts[0] = self.start;
        self.pts[1] = add(self.start, mul(self.dir, self.seg));
        self.pts[LINKS - 1] = self.end;
        // It goes straight down into the hole.
        self.pts[LINKS - 2] = add(self.end, [0.0, 0.0, self.seg]);
    }

    fn free(i: usize) -> bool {
        (2..LINKS - 2).contains(&i)
    }

    /// One step of `dt` seconds (in fixed steps of 1/120 s); whether
    /// anything moved.
    pub fn step(&mut self, dt: f32) -> bool {
        if self.k <= 0.0 {
            return false;
        }
        if self.pts.len() != LINKS {
            self.lay();
            self.last_start = self.start;
        }
        // Woken by a real move of the plug, not the sensors' tremor (a
        // fraction of a pixel, each time the hinge or the gravity is read).
        if len(sub(self.start, self.last_start)) > 0.4 {
            self.last_start = self.start;
            self.quiet = 0;
        }
        if self.quiet > 45 {
            return false;
        }
        // Pulled taut further than it reaches: more of it comes off the
        // desk - longer, as it lies (laid out again, it jumped into the same
        // shape at each turn of the phone, as if stuck to it).
        let d = len(sub(self.end, self.start));
        let reach = d / ((LINKS - 1) as f32 * 0.9);
        if reach > self.seg {
            self.seg = reach;
        }
        // And back: what more came out than lying there takes goes back
        // into the hole, slowly (kept, it lay in a loop once the phone was
        // down again).
        let lying = (d * 1.05 + 30.0 * self.k) / (LINKS - 1) as f32;
        if self.seg > lying {
            self.seg = (self.seg * 0.995).max(lying);
            self.quiet = 0;
        }
        let g = 9810.0 * self.k;
        let h = 1.0 / 120.0;
        // Two steps a frame, always: an even count, so a link that rocks
        // between two places step by step (where the hole's floor and the
        // cord's stiffness meet) is drawn in one.
        let _ = dt;
        let before = self.pts.clone();
        for _ in 0..2 {
            for i in 2..LINKS - 2 {
                let p = self.pts[i];
                // A cable's own friction: its swings die down soon.
                let v = mul(sub(p, self.prev[i]), 0.96);
                self.prev[i] = p;
                self.pts[i] = add(add(p, v), [0.0, 0.0, -g * h * h]);
            }
            self.pin();
            for _ in 0..PASSES {
                // Its length.
                for i in 0..LINKS - 1 {
                    let d = sub(self.pts[i + 1], self.pts[i]);
                    let l = len(d).max(1e-4);
                    let c = mul(d, (l - self.seg) / l);
                    match (Self::free(i), Self::free(i + 1)) {
                        (true, true) => {
                            self.pts[i] = add(self.pts[i], mul(c, 0.5));
                            self.pts[i + 1] = sub(self.pts[i + 1], mul(c, 0.5));
                        }
                        (true, false) => self.pts[i] = add(self.pts[i], c),
                        (false, true) => self.pts[i + 1] = sub(self.pts[i + 1], c),
                        _ => {}
                    }
                }
                // Its stiffness: each link drawn toward the line between its
                // neighbours (the bend spread along it, as an elastic rod
                // bends), and nowhere sharper than a 25 mm radius.
                let turn = self.seg / (25.0 * self.k);
                let min = 2.0 * self.seg * (turn / 2.0).cos();
                for i in 1..LINKS - 1 {
                    let mid = mul(add(self.pts[i - 1], self.pts[i + 1]), 0.5);
                    let c = mul(sub(mid, self.pts[i]), 0.06);
                    if Self::free(i) {
                        self.pts[i] = add(self.pts[i], c);
                    }
                    for j in [i - 1, i + 1] {
                        if Self::free(j) {
                            self.pts[j] = sub(self.pts[j], mul(c, 0.5));
                        }
                    }
                    let d = sub(self.pts[i + 1], self.pts[i - 1]);
                    let l = len(d).max(1e-4);
                    if l < min {
                        let c = mul(d, (min - l) / l * 0.5);
                        if Self::free(i - 1) {
                            self.pts[i - 1] = sub(self.pts[i - 1], c);
                        }
                        if Self::free(i + 1) {
                            self.pts[i + 1] = add(self.pts[i + 1], c);
                        }
                    }
                }
                for i in 2..LINKS - 2 {
                    self.collide(i);
                }
                self.pin();
            }
        }
        // How far it moved over the frame (the free links: the pinned ones
        // follow the plug).
        let mut motion = 0.0f32;
        for i in 2..LINKS - 2 {
            motion = motion.max(len(sub(self.pts[i], before[i])));
        }
        if motion < 0.05 {
            self.quiet += 1;
            if self.quiet == 45 && std::env::var_os("GRIDBAY_ROPE").is_some() {
                eprintln!("rope: at rest");
            }
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
        // Plain, the far end no longer faded: it goes down the hole.
        let fade = |cr: &gtk::cairo::Context, rgb: (f64, f64, f64), a: f64, _off: (f64, f64)| {
            cr.set_source_rgba(rgb.0, rgb.1, rgb.2, a);
        };
        // Where it goes below the table: from there it is seen only through
        // the hole's opening, and in its shade.
        let below = self.pts.iter().position(|p| p[2] < self.table - 0.5).unwrap_or(LINKS);
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
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.05 * (-(up / (6.0 * self.k))).exp() as f64);
        let _ = cr.fill();
        let above = below.min(LINKS);
        for (w, a) in [(1.6, 0.4), (1.1, 0.7)] {
            if above < 2 {
                break;
            }
            self.path(cr, &m, &shadow[..above], (0.0, 0.0));
            fade(cr, (0.0, 0.0, 0.0), strength * a, (0.0, 0.0));
            cr.set_line_width(wd * w);
            let _ = cr.stroke();
        }
        // The tube: white plastic - its rim in shade, its middle lit, a
        // highlight toward the light (up and left).
        // `dark`: in the hole, from the rim (where it is as lit as above) to
        // its shade a little way down.
        let tube = |cr: &gtk::cairo::Context, pts: &[V], shade: f64, dark: Option<((f64, f64), (f64, f64))>| {
            if pts.len() < 2 {
                return;
            }
            // Its round shading in many thin layers - from the rim in shade
            // to the lit middle and a highlight toward the light (up and
            // left) - so the light turns over it smoothly (four layers
            // showed as bands).
            let layers = 14;
            for n in 0..layers {
                let t = n as f64 / (layers - 1) as f64;
                let w = 1.0 - 0.9 * t;
                let c = 0.68 + 0.32 * (t * std::f64::consts::FRAC_PI_2).sin();
                let o = -0.28 * t;
                let off = (o * wd, o * wd);
                self.path(cr, &m, pts, off);
                let c = c * shade;
                match dark {
                    Some(((ax, ay), (bx, by))) => {
                        let g = gtk::cairo::LinearGradient::new(ax + off.0, ay + off.1, bx + off.0, by + off.1);
                        g.add_color_stop_rgb(0.0, c, c + 0.004, c + 0.01);
                        g.add_color_stop_rgb(1.0, c * 0.06, c * 0.06, c * 0.065);
                        let _ = cr.set_source(&g);
                    }
                    None => fade(cr, (c, c + 0.004, c + 0.01), 1.0, off),
                }
                cr.set_line_width(wd * w);
                let _ = cr.stroke();
            }
        };
        // Where it crosses the table's level: both parts meet there, no gap.
        let mut upper: Vec<V> = self.pts[..below.min(LINKS)].to_vec();
        let mut lower: Vec<V> = Vec::new();
        if below > 0 && below < LINKS {
            let (a, b) = (self.pts[below - 1], self.pts[below]);
            let t = ((a[2] - self.table) / (a[2] - b[2]).max(1e-4)).clamp(0.0, 1.0);
            let cross = add(a, mul(sub(b, a), t));
            upper.push(cross);
            lower.push(cross);
            lower.extend_from_slice(&self.pts[below..]);
        }
        // In the hole, clipped to its opening.
        if !lower.is_empty() {
            if let Some(([x0, y0, x1, y1], _)) = self.hole {
                cr.save().ok();
                cr.new_path();
                for (i, (x, y)) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].into_iter().enumerate() {
                    let (px, py) = self.project(&m, [x, y, self.table]);
                    if i == 0 {
                        cr.move_to(px, py);
                    } else {
                        cr.line_to(px, py);
                    }
                }
                cr.close_path();
                cr.clip();
                // In the dark of the hole: dimmer the deeper (seen as it
                // goes in, not as it lies at the bottom).
                let rim = self.project(&m, lower[0]);
                let deep = self.project(&m, [lower[0][0], lower[0][1], self.table - 0.45 * self.hole.map_or(0.0, |h| h.1)]);
                tube(cr, &lower, 1.0, Some((rim, deep)));
                cr.restore().ok();
            }
        }
        tube(cr, &upper, 1.0, None);
    }
}
