//! Solar constants and the inverse-square law.

use crate::math::AU_M;

/// Nominal solar radius (IAU 2015 Resolution B3), metres.
pub const SUN_RADIUS_M: f64 = 695_700_000.0;

/// Total solar irradiance at 1 AU (Kopp & Lean 2011), W/m².
pub const SOLAR_CONSTANT_W_M2: f64 = 1361.0;

/// Irradiance at distance `sun_dist_m` from the Sun, W/m² (inverse square).
pub fn irradiance(sun_dist_m: f64, solar_constant_w_m2: f64) -> f64 {
    let r = sun_dist_m / AU_M;
    solar_constant_w_m2 / (r * r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_square() {
        assert_eq!(irradiance(AU_M, 1361.0), 1361.0);
        assert!((irradiance(2.0 * AU_M, 1361.0) - 1361.0 / 4.0).abs() < 1e-12);
    }
}
