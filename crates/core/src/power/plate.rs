//! Flat-plate power: one ideal panel, no self-shadowing. The baseline that
//! the full cell/string model (later slices) must reproduce in the limit of
//! a single unshaded panel at constant efficiency.

use glam::DVec3;

use crate::env::{EnvConfig, Sunlight, sunlight};
use crate::trajectory::Epoch;

/// A single flat panel fixed to the body.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatPlate {
    /// Unit outward normal of the cell side, body frame.
    pub normal_b: DVec3,
    pub area_m2: f64,
    /// Sunlight-to-electrical conversion efficiency, 0–1.
    pub efficiency: f64,
}

/// Plate output at one instant.
#[derive(Debug, Clone, PartialEq)]
pub struct PlateSample {
    pub sun: Sunlight,
    /// Cosine of the Sun incidence angle on the plate (negative: Sun behind).
    pub cos_incidence: f64,
    pub power_w: f64,
}

impl FlatPlate {
    /// `normal_b` is normalised here.
    pub fn new(normal_b: DVec3, area_m2: f64, efficiency: f64) -> Self {
        Self {
            normal_b: normal_b.normalize(),
            area_m2,
            efficiency,
        }
    }

    /// P = S₀ (1 AU / r)² · A · η · max(0, cos θ) · visible fraction.
    pub fn sample(&self, epoch: &Epoch, env: &EnvConfig) -> PlateSample {
        let sun = sunlight(epoch, env);
        let cos_incidence = self.normal_b.dot(sun.sun_dir_b);
        let power_w = sun.irradiance_w_m2
            * self.area_m2
            * self.efficiency
            * cos_incidence.max(0.0)
            * sun.visible;
        PlateSample {
            sun,
            cos_incidence,
            power_w,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AU_M;
    use glam::DQuat;

    fn epoch(sun_b: DVec3) -> Epoch {
        Epoch {
            t: 0.0,
            sun_b,
            bodies: Vec::new(),
            q_bi: DQuat::IDENTITY,
        }
    }

    #[test]
    fn analytic_plate_is_exact() {
        let env = EnvConfig::default();
        let plate = FlatPlate::new(DVec3::Z, 2.0, 0.3);
        for (deg, r_au) in [(0.0, 1.0), (60.0, 1.0), (45.0, 0.983), (89.0, 1.017)] {
            let th = f64::to_radians(deg);
            let sun = DVec3::new(th.sin(), 0.0, th.cos()) * r_au * AU_M;
            let got = plate.sample(&epoch(sun), &env).power_w;
            let want = 1361.0 / (r_au * r_au) * 2.0 * 0.3 * th.cos();
            assert!((got - want).abs() <= 1e-9 * want, "{deg}°: {got} vs {want}");
        }
        // Exact closed form at 60°, 1 AU.
        let sun = DVec3::new(3f64.sqrt() / 2.0, 0.0, 0.5) * AU_M;
        let p = plate.sample(&epoch(sun), &env).power_w;
        assert!((p - 408.3).abs() < 1e-9, "{p}");
    }

    #[test]
    fn sun_behind_gives_zero() {
        let plate = FlatPlate::new(DVec3::Z, 1.0, 0.3);
        let s = plate.sample(&epoch(-DVec3::Z * AU_M), &EnvConfig::default());
        assert_eq!(s.power_w, 0.0);
        assert!(s.cos_incidence < 0.0);
    }
}
