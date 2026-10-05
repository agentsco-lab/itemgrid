//! The orientation: Mahony's filter with the magnetometer (MARG) - the
//! gyroscope turns it, the accelerometer pulls it to down, the compass to
//! north; the gyroscope's offset learnt while the phone is still.
//!
//! Its world: x to magnetic north (level), y west of it, z up - a
//! right-handed frame - so the turn about the vertical is absolute and does
//! not drift. The phone's frame (the right half's, sensorfw's): x across
//! its panel, y along it to its top, z out of its screen.
//!
//! Each correction is heeded only while it is believable: down when the
//! phone is not being swung (the accelerometer's size near 1 g), north when
//! the field is as it was (its size and dip near those learnt - a computer,
//! a speaker, a steel desk bend it; then the gyroscope alone carries the
//! turn).

pub type V3 = [f64; 3];

fn norm(v: V3) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

pub struct Ahrs {
    /// (w, x, y, z): the phone's frame to the world's.
    pub q: [f64; 4],
    integral: V3,
    /// The gyroscope's offset (rad/s), learnt while still.
    pub bias: V3,
    pub started: bool,
    /// The field's size and dip (rad) as learnt while believed.
    field_size: Option<f64>,
    field_dip: Option<f64>,
    /// Whether the last field was heeded.
    pub north_trusted: bool,
}

/// Gains: down's and north's pull (proportional), and the integral's.
const KP_DOWN: f64 = 2.0;
const KP_NORTH: f64 = 1.0;
const KI: f64 = 0.02;

impl Default for Ahrs {
    fn default() -> Ahrs {
        Ahrs { q: [1.0, 0.0, 0.0, 0.0], integral: [0.0; 3], bias: [0.0; 3], started: false, field_size: None, field_dip: None, north_trusted: false }
    }
}

impl Ahrs {
    /// The phone's axes in the world (the rotation's columns): x, y, z.
    pub fn axes(&self) -> [V3; 3] {
        let [w, x, y, z] = self.q;
        [
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z), 2.0 * (x * z - w * y)],
            [2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)],
            [2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 1.0 - 2.0 * (x * x + y * y)],
        ]
    }

    /// A vector in the phone's frame, in the world's.
    pub fn to_world(&self, v: V3) -> V3 {
        let [ax, ay, az] = self.axes();
        [0, 1, 2].map(|i| ax[i] * v[0] + ay[i] * v[1] + az[i] * v[2])
    }

    /// Started from down and, if it is there, north: at once where they say.
    pub fn start(&mut self, a: V3, m: Option<V3>) {
        let n = norm(a).max(1e-9);
        let (ax, ay, az) = (a[0] / n, a[1] / n, a[2] / n);
        let roll = ay.atan2(az);
        let pitch = (-ax).atan2((ay * ay + az * az).sqrt());
        let (cr, sr, cp, sp) = ((roll / 2.0).cos(), (roll / 2.0).sin(), (pitch / 2.0).cos(), (pitch / 2.0).sin());
        self.q = [cr * cp, sr * cp, cr * sp, -sr * sp];
        if let Some(m) = m {
            // The level field's bearing, turned away: north along x.
            let mw = self.to_world(m);
            let yaw = mw[1].atan2(mw[0]);
            let (c, s) = ((-yaw / 2.0).cos(), (-yaw / 2.0).sin());
            let q = self.q;
            self.q = [c * q[0] - s * q[3], c * q[1] - s * q[2], c * q[2] + s * q[1], c * q[3] + s * q[0]];
        }
        self.started = true;
    }

    /// Whether a field reading is believable (and the reference learnt from
    /// the believable ones).
    fn believe_field(&mut self, m: V3, down: V3) -> bool {
        let size = norm(m);
        if size < 1e-6 {
            return false;
        }
        // The dip: the field's angle below level, from down (in the phone's
        // frame, both).
        let dn = norm(down).max(1e-9);
        let along = (m[0] * down[0] + m[1] * down[1] + m[2] * down[2]) / (size * dn);
        let dip = along.clamp(-1.0, 1.0).asin();
        let ok = match (self.field_size, self.field_dip) {
            (Some(s), Some(d)) => (size / s - 1.0).abs() < 0.15 && (dip - d).abs() < 0.17,
            _ => true,
        };
        if ok {
            self.field_size = Some(self.field_size.map_or(size, |s| s + (size - s) * 0.01));
            self.field_dip = Some(self.field_dip.map_or(dip, |d| d + (dip - d) * 0.01));
        }
        ok
    }

    /// A step of `dt` s: turning at `w` (rad/s, the offset not yet taken
    /// off), down `a` (any unit), the field `m` if one came; `still`: the
    /// phone at rest (the offset learnt from `w`).
    pub fn step(&mut self, w: V3, a: V3, m: Option<V3>, dt: f64, still: bool) {
        if still {
            self.bias = [0, 1, 2].map(|i| self.bias[i] + (w[i] - self.bias[i]) * 0.05);
        }
        let mut w = [0, 1, 2].map(|i| w[i] - self.bias[i]);
        // At rest what is left is noise: no turn.
        if still && norm(w) < 0.006 {
            w = [0.0; 3];
        }
        let [q0, q1, q2, q3] = self.q;
        let mut e = [0.0; 3];
        // Down: where the world's z is in the phone's frame, against the
        // accelerometer; heeded as its size nears 1 g.
        let an = norm(a);
        if an > 1e-6 {
            let trust = (1.0 - (an - 1.0).abs() * 4.0).clamp(0.0, 1.0);
            let (ax, ay, az) = (a[0] / an, a[1] / an, a[2] / an);
            let vx = 2.0 * (q1 * q3 - q0 * q2);
            let vy = 2.0 * (q0 * q1 + q2 * q3);
            let vz = q0 * q0 - q1 * q1 - q2 * q2 + q3 * q3;
            let ed = [ay * vz - az * vy, az * vx - ax * vz, ax * vy - ay * vx];
            for i in 0..3 {
                e[i] += KP_DOWN * trust * ed[i];
            }
        }
        // North: the field's level part along the world's x.
        self.north_trusted = false;
        if let Some(m) = m {
            if self.believe_field(m, a) {
                self.north_trusted = true;
                let mn = norm(m);
                let (mx, my, mz) = (m[0] / mn, m[1] / mn, m[2] / mn);
                let hx = 2.0 * (mx * (0.5 - q2 * q2 - q3 * q3) + my * (q1 * q2 - q0 * q3) + mz * (q1 * q3 + q0 * q2));
                let hy = 2.0 * (mx * (q1 * q2 + q0 * q3) + my * (0.5 - q1 * q1 - q3 * q3) + mz * (q2 * q3 - q0 * q1));
                let bx = (hx * hx + hy * hy).sqrt();
                let bz = 2.0 * (mx * (q1 * q3 - q0 * q2) + my * (q2 * q3 + q0 * q1) + mz * (0.5 - q1 * q1 - q2 * q2));
                let wx = 2.0 * (bx * (0.5 - q2 * q2 - q3 * q3) + bz * (q1 * q3 - q0 * q2));
                let wy = 2.0 * (bx * (q1 * q2 - q0 * q3) + bz * (q0 * q1 + q2 * q3));
                let wz = 2.0 * (bx * (q0 * q2 + q1 * q3) + bz * (0.5 - q1 * q1 - q2 * q2));
                let em = [my * wz - mz * wy, mz * wx - mx * wz, mx * wy - my * wx];
                for i in 0..3 {
                    e[i] += KP_NORTH * em[i];
                }
            }
        }
        for i in 0..3 {
            self.integral[i] += KI * e[i] * dt;
            w[i] += e[i] + self.integral[i];
        }
        let h = 0.5 * dt;
        let dq = [
            -q1 * w[0] - q2 * w[1] - q3 * w[2],
            q0 * w[0] + q2 * w[2] - q3 * w[1],
            q0 * w[1] - q1 * w[2] + q3 * w[0],
            q0 * w[2] + q1 * w[1] - q2 * w[0],
        ];
        let mut q = [q0 + dq[0] * h, q1 + dq[1] * h, q2 + dq[2] * h, q3 + dq[3] * h];
        let l = q.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-12);
        for v in &mut q {
            *v /= l;
        }
        self.q = q;
    }

    /// The turn about the vertical (rad): where the phone's top points,
    /// level, from north (or, lying face up/down, the same for its top).
    #[cfg(test)]
    pub fn yaw(&self) -> f64 {
        let [w, x, y, z] = self.q;
        (2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The field at a mid-latitude: 50 uT, dipping 60 degrees (x north).
    fn field_world() -> V3 {
        let dip = 60f64.to_radians();
        [50.0 * dip.cos(), 0.0, -50.0 * dip.sin()]
    }

    /// The world's down and field as a phone turned `yaw` about the
    /// vertical, lying face up, reads them.
    fn readings(yaw: f64) -> (V3, V3) {
        let f = field_world();
        let (c, s) = (yaw.cos(), yaw.sin());
        // World to phone: turned by -yaw about z.
        let m = [c * f[0] + s * f[1], -s * f[0] + c * f[1], f[2]];
        ([0.0, 0.0, 1.0], m)
    }

    fn settle(a: &mut Ahrs, yaw: f64, bias: V3, seconds: f64) {
        let (acc, m) = readings(yaw);
        for _ in 0..(seconds * 100.0) as usize {
            a.step(bias, acc, Some(m), 0.01, true);
        }
    }

    #[test]
    fn finds_north_lying_still() {
        let mut a = Ahrs::default();
        let (acc, m) = readings(0.7);
        a.start(acc, Some(m));
        settle(&mut a, 0.7, [0.0; 3], 2.0);
        // Turned 0.7 rad from north (counter-clockwise seen from above).
        assert!((a.yaw() - 0.7).abs() < 0.02, "yaw {}", a.yaw());
    }

    #[test]
    fn no_drift_with_a_gyro_offset() {
        let mut a = Ahrs::default();
        let (acc, m) = readings(0.0);
        a.start(acc, Some(m));
        // An offset of 0.5 deg/s about z: lying still for a minute.
        settle(&mut a, 0.0, [0.0, 0.0, 0.5f64.to_radians()], 60.0);
        assert!(a.yaw().abs() < 0.02, "drifted to {}", a.yaw());
        assert!((a.bias[2] - 0.5f64.to_radians()).abs() < 0.002, "offset {}", a.bias[2]);
    }

    #[test]
    fn follows_a_turn() {
        let mut a = Ahrs::default();
        let (acc, m) = readings(0.0);
        a.start(acc, Some(m));
        // A quarter turn in a second, counter-clockwise.
        let rate = std::f64::consts::FRAC_PI_2;
        for i in 0..100 {
            let yaw = rate * (i + 1) as f64 / 100.0;
            let (acc, m) = readings(yaw);
            a.step([0.0, 0.0, rate], acc, Some(m), 0.01, false);
        }
        assert!((a.yaw() - std::f64::consts::FRAC_PI_2).abs() < 0.05, "yaw {}", a.yaw());
    }

    #[test]
    fn a_bent_field_is_not_heeded() {
        let mut a = Ahrs::default();
        let (acc, m) = readings(0.0);
        a.start(acc, Some(m));
        settle(&mut a, 0.0, [0.0; 3], 5.0);
        // A steel desk: the field twice as strong and pointing elsewhere.
        let (_, bent) = readings(1.0);
        for _ in 0..300 {
            a.step([0.0; 3], acc, Some(bent.map(|v| v * 2.0)), 0.01, true);
        }
        assert!(!a.north_trusted);
        assert!(a.yaw().abs() < 0.02, "pulled to {}", a.yaw());
    }

    /// Held up to be looked at: its top raised 60 degrees, its screen
    /// facing `bearing` from north - the screen's way found from down and
    /// the field alone.
    #[test]
    fn finds_the_screens_way_held_up() {
        let bearing = 2.0f64;
        let t = 60f64.to_radians();
        // The truth: tipped about x by the rise, then turned about z.
        let truth = {
            let mut a = Ahrs::default();
            let (cr, sr) = ((t / 2.0).cos(), (t / 2.0).sin());
            a.q = [cr, sr, 0.0, 0.0];
            // Its screen faces -y after the tip: turned so it faces `bearing`.
            let yaw = bearing + std::f64::consts::FRAC_PI_2;
            let (c, s) = ((yaw / 2.0).cos(), (yaw / 2.0).sin());
            let q = a.q;
            a.q = [c * q[0] - s * q[3], c * q[1] - s * q[2], c * q[2] + s * q[1], c * q[3] + s * q[0]];
            a
        };
        // World to phone: the axes' dot products.
        let ax = truth.axes();
        let into = |v: V3| [0, 1, 2].map(|i| ax[i][0] * v[0] + ax[i][1] * v[1] + ax[i][2] * v[2]);
        let (acc, m) = (into([0.0, 0.0, 1.0]), into(field_world()));
        let mut a = Ahrs::default();
        a.start(acc, Some(m));
        for _ in 0..200 {
            a.step([0.0; 3], acc, Some(m), 0.01, true);
        }
        let screen = a.axes()[2];
        let got = screen[1].atan2(screen[0]);
        assert!((got - bearing).abs() < 0.03, "screen faces {got}, not {bearing}");
        assert!((truth.axes()[2][1].atan2(truth.axes()[2][0]) - bearing).abs() < 1e-9);
    }

    #[test]
    fn tilt_is_found_from_down() {
        let mut a = Ahrs::default();
        // Its top raised 40 degrees: down along -y and -z of... in the
        // phone, gravity's reaction (up) is (0, sin, cos).
        let t = 40f64.to_radians();
        a.start([0.0, t.sin(), t.cos()], None);
        let ay = a.axes()[1];
        assert!((ay[2] - t.sin()).abs() < 0.01, "top's rise {}", ay[2]);
    }
}
