//! Shared numerics: constants and full-precision angle helpers.
//!
//! glam's `angle_between` and `DQuat::slerp` use an approximate `acos`
//! (~1e-8 rad error), too coarse for interpolation and shadow checks, so
//! everything in sunlyt goes through these instead.

use glam::{DQuat, DVec3};

/// Metres per astronomical unit (IAU 2012 Resolution B2).
pub const AU_M: f64 = 149_597_870_700.0;

/// Exact angle between vectors.
pub trait AngleExt {
    fn angle_exact(self, other: Self) -> f64;
}

impl AngleExt for DVec3 {
    fn angle_exact(self, other: Self) -> f64 {
        self.cross(other).length().atan2(self.dot(other))
    }
}

/// Shortest-path spherical interpolation at full f64 precision.
pub fn slerp(a: DQuat, b: DQuat, u: f64) -> DQuat {
    let b = if a.dot(b) < 0.0 { -b } else { b };
    // Angle between a and b as 4-vectors (half the rotation angle),
    // accurate even when tiny.
    let theta = 2.0 * (a - b).length().atan2((a + b).length());
    if theta < 1e-9 {
        return a.lerp(b, u).normalize();
    }
    let s = theta.sin();
    (a * (((1.0 - u) * theta).sin() / s) + b * ((u * theta).sin() / s)).normalize()
}
