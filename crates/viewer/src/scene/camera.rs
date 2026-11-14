//! Orbit camera on critically damped springs: motion keeps momentum, can be
//! interrupted at any time and never overshoots (viewer-design skill).
//! Body frame, +Z up.

use glam::{Mat4, Vec3};

/// Spring rate while the user drags or scrolls, rad/s: settles in ~0.12 s.
const FOLLOW_OMEGA: f32 = 40.0;
/// Spring rate of preset glides: ~99 % of the way in [`GLIDE_S`].
const GLIDE_OMEGA: f32 = 6.6 / crate::ui::motion::GLIDE_S;
const PITCH_LIMIT: f32 = 1.55;

#[derive(Debug, Clone, Copy)]
struct Spring {
    x: f32,
    v: f32,
    target: f32,
}

impl Spring {
    fn new(x: f32) -> Self {
        Self {
            x,
            v: 0.0,
            target: x,
        }
    }

    /// Exact critically damped step toward `target`.
    fn step(&mut self, omega: f32, dt: f32) {
        let d = self.x - self.target;
        let e = (-omega * dt).exp();
        let c = self.v + omega * d;
        self.x = self.target + (d + c * dt) * e;
        self.v = (self.v - omega * c * dt) * e;
        if (self.x - self.target).abs() < 1e-5 && self.v.abs() < 1e-4 {
            self.snap();
        }
    }

    fn snap(&mut self) {
        self.x = self.target;
        self.v = 0.0;
    }

    fn moving(&self) -> bool {
        self.x != self.target || self.v != 0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Sun,
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
}

impl Preset {
    pub const ALL: [Preset; 7] = [
        Preset::Sun,
        Preset::PosX,
        Preset::NegX,
        Preset::PosY,
        Preset::NegY,
        Preset::PosZ,
        Preset::NegZ,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Sun => "Sun",
            Preset::PosX => "+X",
            Preset::NegX => "−X",
            Preset::PosY => "+Y",
            Preset::NegY => "−Y",
            Preset::PosZ => "+Z",
            Preset::NegZ => "−Z",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Camera {
    pub centre: Vec3,
    /// Scene bounding radius, m.
    pub radius: f32,
    yaw: Spring,
    pitch: Spring,
    /// ln(distance / radius).
    log_dist: Spring,
    omega: f32,
}

const HOME: (f32, f32, f32) = (-2.4, 0.45, 1.2);

impl Camera {
    pub fn new(centre: Vec3, radius: f32) -> Self {
        Self {
            centre,
            radius: radius.max(1e-3),
            yaw: Spring::new(HOME.0),
            pitch: Spring::new(HOME.1),
            log_dist: Spring::new(HOME.2),
            omega: FOLLOW_OMEGA,
        }
    }

    /// Drag by `dx`, `dy` pixels.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.omega = FOLLOW_OMEGA;
        self.yaw.target -= dx * 0.008;
        self.pitch.target = (self.pitch.target + dy * 0.008).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// Scroll by `amount` points (positive: closer).
    pub fn zoom(&mut self, amount: f32) {
        self.omega = FOLLOW_OMEGA;
        self.log_dist.target = (self.log_dist.target - amount * 0.0025).clamp(0.2, 6.0);
    }

    /// Looks from direction `dir` (body frame, toward the camera). Glides,
    /// or jumps with `reduced_motion`.
    pub fn look_from(&mut self, dir: Vec3, reduced_motion: bool) {
        let d = dir.normalize_or(Vec3::X);
        let yaw = d.y.atan2(d.x);
        // Shortest way round from the current yaw.
        let tau = std::f32::consts::TAU;
        let k = ((self.yaw.x - yaw) / tau).round();
        self.yaw.target = yaw + k * tau;
        self.pitch.target = d.z.clamp(-1.0, 1.0).asin().clamp(-PITCH_LIMIT, PITCH_LIMIT);
        self.omega = GLIDE_OMEGA;
        if reduced_motion {
            self.snap();
        }
    }

    pub fn preset(&mut self, p: Preset, sun_dir: Vec3, reduced_motion: bool) {
        let dir = match p {
            Preset::Sun => sun_dir,
            Preset::PosX => Vec3::X,
            Preset::NegX => -Vec3::X,
            Preset::PosY => Vec3::Y,
            Preset::NegY => -Vec3::Y,
            Preset::PosZ => Vec3::Z,
            Preset::NegZ => -Vec3::Z,
        };
        self.look_from(dir, reduced_motion);
    }

    /// Back to the home view, instantly (double-click).
    pub fn reset(&mut self) {
        self.yaw.target = HOME.0;
        self.pitch.target = HOME.1;
        self.log_dist.target = HOME.2;
        self.snap();
    }

    fn snap(&mut self) {
        self.yaw.snap();
        self.pitch.snap();
        self.log_dist.snap();
    }

    /// Advances the springs; true while still moving.
    pub fn update(&mut self, dt: f32) -> bool {
        let dt = dt.min(0.05);
        for s in [&mut self.yaw, &mut self.pitch, &mut self.log_dist] {
            s.step(self.omega, dt);
        }
        self.moving()
    }

    pub fn moving(&self) -> bool {
        self.yaw.moving() || self.pitch.moving() || self.log_dist.moving()
    }

    pub fn distance(&self) -> f32 {
        self.radius * self.log_dist.x.exp()
    }

    pub fn eye(&self) -> Vec3 {
        let (y, p) = (self.yaw.x, self.pitch.x);
        self.centre + Vec3::new(p.cos() * y.cos(), p.cos() * y.sin(), p.sin()) * self.distance()
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let d = self.distance();
        let view = Mat4::look_at_rh(self.eye(), self.centre, Vec3::Z);
        let proj = Mat4::perspective_rh(
            35f32.to_radians(),
            aspect.max(1e-3),
            (d - 3.0 * self.radius).max(d * 0.01),
            d + 3.0 * self.radius,
        );
        proj * view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_settles_without_overshoot() {
        let mut s = Spring::new(0.0);
        s.target = 1.0;
        let mut max: f32 = 0.0;
        let mut t = 0.0;
        while s.moving() && t < 2.0 {
            s.step(GLIDE_OMEGA, 1.0 / 60.0);
            max = max.max(s.x);
            t += 1.0 / 60.0;
        }
        assert!(max <= 1.0 + 1e-6, "overshoot {max}");
        assert!(t < 0.6, "settled after {t} s");
    }

    #[test]
    fn preset_takes_the_short_way_round() {
        let mut c = Camera::new(Vec3::ZERO, 1.0);
        c.yaw = Spring::new(3.0);
        c.look_from(Vec3::new(-1.0, -0.01, 0.0), true);
        assert!((c.yaw.x - 3.0).abs() < 0.3, "yaw {}", c.yaw.x);
    }
}
