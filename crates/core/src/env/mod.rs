//! Solar environment at the spacecraft: irradiance and eclipses.
//!
//! The result, [`Sunlight`], is what every power model consumes. It holds
//! the unoccluded irradiance and the visible fraction of the solar disk;
//! the disk fraction dims every cell equally, independently of the
//! spacecraft's own shadows (which slice 3 multiplies in per cell).

pub mod irradiance;
pub mod occultation;

use glam::DVec3;

pub use irradiance::{SOLAR_CONSTANT_W_M2, SUN_RADIUS_M, irradiance};
pub use occultation::{Limb, disk_occultation, visible_fraction};

use crate::trajectory::Epoch;

/// Environment settings shared by every power model.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvConfig {
    /// Irradiance at 1 AU, W/m².
    pub solar_constant_w_m2: f64,
    pub sun_radius_m: f64,
    /// Landed on a body from a given time on. The horizon then follows the
    /// local tangent plane at the spacecraft rather than the reference
    /// surface, which matters when terrain sits well above it.
    pub surface: Option<Surface>,
}

impl Default for EnvConfig {
    fn default() -> Self {
        Self {
            solar_constant_w_m2: SOLAR_CONSTANT_W_M2,
            sun_radius_m: SUN_RADIUS_M,
            surface: None,
        }
    }
}

/// The spacecraft rests on body `body` (index into the bodies) from `from_t`
/// seconds after the table epoch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Surface {
    pub body: usize,
    pub from_t: f64,
}

/// Sunlight at one instant.
#[derive(Debug, Clone, PartialEq)]
pub struct Sunlight {
    /// Spacecraft–Sun distance, metres.
    pub sun_dist_m: f64,
    /// Unoccluded irradiance at the spacecraft, W/m².
    pub irradiance_w_m2: f64,
    /// Unit Sun direction, body frame.
    pub sun_dir_b: DVec3,
    /// Fraction of the solar disk hidden by each body on its own (same
    /// order as [`Epoch::bodies`]). These can overlap; use `visible`.
    pub occulted: Vec<f64>,
    /// Fraction of the solar disk visible, 0–1. Where two bodies cover the
    /// same part of the disk it is counted once.
    pub visible: f64,
}

impl Sunlight {
    /// Index of the body hiding the largest part of the disk, if any.
    pub fn main_occulter(&self) -> Option<usize> {
        self.occulted
            .iter()
            .enumerate()
            .filter(|(_, f)| **f > 0.0)
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
    }
}

/// Irradiance and eclipse state at `epoch`.
pub fn sunlight(epoch: &Epoch, cfg: &EnvConfig) -> Sunlight {
    let sun_dist = epoch.sun_b.length();
    let sun_dir_b = epoch.sun_b / sun_dist;
    let sun_ang_r = (cfg.sun_radius_m / sun_dist).asin();
    let limbs: Vec<Limb> = epoch
        .bodies
        .iter()
        .map(|b| {
            let landed = cfg
                .surface
                .is_some_and(|s| s.body == b.index && epoch.t >= s.from_t);
            Limb::of(b, landed)
        })
        .collect();
    let occulted: Vec<f64> = limbs
        .iter()
        .map(|l| l.occultation(sun_dir_b, sun_ang_r))
        .collect();
    let visible = visible_fraction(&limbs, &occulted, sun_dir_b, sun_ang_r);
    Sunlight {
        sun_dist_m: sun_dist,
        irradiance_w_m2: irradiance(sun_dist, cfg.solar_constant_w_m2),
        sun_dir_b,
        occulted,
        visible,
    }
}
