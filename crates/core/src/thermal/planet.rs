//! Sunlight reflected by a planet (albedo) and its own infrared emission,
//! arriving on a flat plate.
//!
//! The planet is a sphere seen as a disk of angular radius ρ (sin ρ = R/d).
//! Irradiance on a plate with normal n is
//!
//! E = ∫_disk L(ω) · max(0, n·ω) dΩ
//!
//! with L the surface radiance in direction ω: Lambertian, so L = M/π for
//! the exitance M at the surface point the ray hits. The integral is a
//! midpoint rule over equal-solid-angle cells (rings in 1 − cos t, sectors
//! in azimuth) around the disk centre. One radiance evaluation per
//! direction is shared by every plate, so many panels cost little more
//! than one.
//!
//! Surface models:
//! - albedo: M = A · S · max(0, cos φ), φ = angle from the subsolar point;
//! - IR, `Subsolar` (airless body in radiative equilibrium, design D6):
//!   M = max((1 − A) · S · cos φ, ε σ T_night⁴). Day side re-emits what it
//!   absorbs; the night side is held at a floor temperature.
//! - IR, `Uniform` (a body with an atmosphere that spreads heat): M fixed.
//!
//! Neglected: the planet's flattening (Earth uses its equatorial radius),
//! occlusion of the planet by the spacecraft's own structure, the solar
//! flux difference between the planet and the spacecraft (≲ 1e-5 from
//! lunar orbit), and thermal inertia of the surface (the night floor
//! stands in for it).

use glam::DVec3;

use super::SIGMA;

/// How a body's surface emits in the infrared.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfaceEmission {
    /// The same exitance everywhere, W/m².
    Uniform { exitance_w_m2: f64 },
    /// Local radiative equilibrium with sunlight on the day side, and a
    /// floor temperature on the night side.
    Subsolar { emissivity: f64, night_k: f64 },
}

/// Reflective and emissive properties of a body's surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyRadiation {
    /// Bond albedo, 0–1.
    pub bond_albedo: f64,
    pub ir: SurfaceEmission,
}

impl BodyRadiation {
    /// The Moon: Bond albedo 0.12, emissivity 0.95 (Vasavada et al. 2012,
    /// JGR 117, E00H18); equatorial night-side temperature ≈ 95 K
    /// (Williams et al. 2017, Icarus 283, Diviner).
    pub fn moon() -> Self {
        Self {
            bond_albedo: 0.12,
            ir: SurfaceEmission::Subsolar {
                emissivity: 0.95,
                night_k: 95.0,
            },
        }
    }

    /// Earth, globally averaged: Bond albedo 0.29 and outgoing longwave
    /// radiation 240 W/m² (CERES EBAF; Loeb et al. 2018, J. Climate 31).
    pub fn earth() -> Self {
        Self {
            bond_albedo: 0.29,
            ir: SurfaceEmission::Uniform {
                exitance_w_m2: 240.0,
            },
        }
    }

    /// Built-in properties by body name (case-insensitive), if known.
    pub fn for_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "moon" => Some(Self::moon()),
            "earth" => Some(Self::earth()),
            _ => None,
        }
    }

    /// Albedo and IR exitance (W/m²) at a surface point whose sunward
    /// cosine is `cos_sun`, under solar irradiance `solar_w_m2`.
    #[inline]
    pub fn exitance(&self, cos_sun: f64, solar_w_m2: f64) -> (f64, f64) {
        let lit = solar_w_m2 * cos_sun.max(0.0);
        let ir = match self.ir {
            SurfaceEmission::Uniform { exitance_w_m2 } => exitance_w_m2,
            SurfaceEmission::Subsolar {
                emissivity,
                night_k,
            } => ((1.0 - self.bond_albedo) * lit).max(emissivity * SIGMA * night_k.powi(4)),
        };
        (self.bond_albedo * lit, ir)
    }
}

/// Quadrature size: rings × sectors directions over the disk. Checked
/// against the closed-form view factor to < 0.3 % (see tests).
pub const RINGS: usize = 32;
pub const SECTORS: usize = 64;

/// Adds this body's (albedo, IR) irradiance, W/m², onto each plate normal
/// in `normals` (`out[j] += …`).
///
/// - `center_b`: from the spacecraft to the body centre, body frame, m;
/// - `sun_dir_b`: unit vector to the Sun (the same from the body: the Sun
///   is ~400 lunar distances away);
/// - `solar_w_m2`: solar irradiance at the body.
///
/// Inside or on the sphere (a landed craft below the reference radius),
/// the disk is clamped to a half-space just short of 90°.
pub fn add_irradiance(
    rad: &BodyRadiation,
    center_b: DVec3,
    radius_m: f64,
    sun_dir_b: DVec3,
    solar_w_m2: f64,
    normals: &[DVec3],
    out: &mut [(f64, f64)],
) {
    let d = center_b.length();
    if d == 0.0 {
        return;
    }
    let c_hat = center_b / d;
    let sin_rho = (radius_m / d).min(1.0 - 1e-9);
    let cos_rho = (1.0 - sin_rho * sin_rho).sqrt();
    // 1 − cos ρ without cancellation for distant (small) bodies.
    let cap = sin_rho * sin_rho / (1.0 + cos_rho);
    let d_omega = std::f64::consts::TAU * cap / (RINGS * SECTORS) as f64;
    let (e1, e2) = c_hat.any_orthonormal_pair();
    // The sphere as seen in the clamped case: keep the ray geometry
    // consistent by using the effective radius of the clamped disk.
    let r_eff = d * sin_rho;
    let (sectors_cos, sectors_sin): (Vec<f64>, Vec<f64>) = (0..SECTORS)
        .map(|j| {
            let psi = std::f64::consts::TAU * (j as f64 + 0.5) / SECTORS as f64;
            (psi.cos(), psi.sin())
        })
        .unzip();
    for k in 0..RINGS {
        let u = cap * (k as f64 + 0.5) / RINGS as f64; // 1 − cos t
        let cos_t = 1.0 - u;
        let sin_t = (u * (2.0 - u)).sqrt();
        for j in 0..SECTORS {
            let w = c_hat * cos_t + (e1 * sectors_cos[j] + e2 * sectors_sin[j]) * sin_t;
            // Nearest intersection of the ray s·w with the sphere.
            let b = d * cos_t;
            let disc = (r_eff * r_eff - (d * d - b * b)).max(0.0);
            let s = b - disc.sqrt();
            let m = (w * s - center_b) / r_eff; // outward surface normal
            let (alb, ir) = rad.exitance(m.dot(sun_dir_b), solar_w_m2);
            let (l_alb, l_ir) = (alb / std::f64::consts::PI, ir / std::f64::consts::PI);
            for (n, o) in normals.iter().zip(out.iter_mut()) {
                let cos_n = n.dot(w);
                if cos_n > 0.0 {
                    let k = cos_n * d_omega;
                    o.0 += l_alb * k;
                    o.1 += l_ir * k;
                }
            }
        }
    }
}

/// View factor from a small plate to a sphere, closed form (Howell's
/// catalog, configuration B-?: "differential planar element to sphere,
/// element tilted"). `cos_beta`: cosine between the plate normal and the
/// direction to the sphere centre; `h` = d/R > 1. Used to check the
/// quadrature.
pub fn view_factor_sphere(cos_beta: f64, h: f64) -> f64 {
    let pi = std::f64::consts::PI;
    let x = (h * h - 1.0).sqrt();
    let cb = cos_beta.clamp(-1.0, 1.0);
    let sb = (1.0 - cb * cb).sqrt();
    if cb >= 1.0 / h {
        // The whole sphere is above the plate's horizon.
        return cb / (h * h);
    }
    if cb <= -1.0 / h {
        return 0.0;
    }
    // Partly below the horizon.
    let cot = cb / sb;
    0.5 - (x / (h * sb)).min(1.0).asin() / pi
        + (cb * (-x * cot).clamp(-1.0, 1.0).acos() - x * (1.0 - h * h * cb * cb).max(0.0).sqrt())
            / (pi * h * h)
}
