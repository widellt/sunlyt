//! Eclipses: how much of the solar disk each body hides, and how much
//! stays visible when several bodies cover it at once.
//!
//! Each body is reduced to a [`Limb`], a circle on the sky. One limb is
//! handled analytically ([`disk_occultation`]). When two or more limbs touch
//! the disk together, any part of the disk they both cover is counted once
//! ([`visible_fraction`]).

use std::f64::consts::{FRAC_PI_2, PI};
use std::sync::OnceLock;

use glam::{DMat3, DVec3};

use crate::math::AngleExt;
use crate::trajectory::BodyState;

/// A body's outline on the sky as seen from the spacecraft.
///
/// An oblate body is handled exactly: stretching space along the pole by
/// a/b turns the spheroid into a sphere of radius a, and a sight line misses
/// the spheroid exactly when its stretched image misses the sphere. So a
/// limb is a circle in its own (possibly stretched) sky.
#[derive(Debug, Clone, PartialEq)]
pub struct Limb {
    /// Polar stretch for an oblate body; `None` for a sphere.
    stretch: Option<DMat3>,
    /// Unit direction to the body centre, in the stretched sky.
    centre: DVec3,
    /// Angular radius of the limb, radians (π/2 for a horizon).
    radius: f64,
}

impl Limb {
    /// A circular limb of angular radius `radius` centred on `centre_dir`.
    pub fn circle(centre_dir: DVec3, radius: f64) -> Self {
        Self {
            stretch: None,
            centre: centre_dir.normalize(),
            radius,
        }
    }

    /// The limb of `body`.
    ///
    /// `landed`: the horizon is the tangent plane at the spacecraft. Below
    /// the reference surface the same horizon is used, so the result never
    /// jumps to a full eclipse because of terrain the model does not know
    /// about.
    pub fn of(body: &BodyState, landed: bool) -> Self {
        let a = body.equatorial_radius_m;
        let stretch = (!body.is_sphere()).then(|| {
            let k = a / body.polar_radius_m;
            let p = body.pole_b;
            DMat3::IDENTITY + (k - 1.0) * DMat3::from_cols(p * p.x, p * p.y, p * p.z)
        });
        let centre = stretch.map_or(body.vec_b, |s| s * body.vec_b);
        let dist = centre.length();
        let radius = if landed || dist <= a {
            FRAC_PI_2
        } else {
            (a / dist).asin()
        };
        Self {
            stretch,
            centre: centre / dist,
            radius,
        }
    }

    fn to_sky(&self, dir: DVec3) -> DVec3 {
        self.stretch.map_or(dir, |s| s * dir)
    }

    /// Fraction of the solar disk (centre `sun_dir`, angular radius
    /// `sun_ang_r`) hidden by this limb alone. The disk keeps its true
    /// angular radius in a stretched sky; the stretch distorts it by
    /// < 0.34 %, a sub-second shift in penumbra timing.
    pub fn occultation(&self, sun_dir: DVec3, sun_ang_r: f64) -> f64 {
        let c = self.centre.angle_exact(self.to_sky(sun_dir));
        disk_occultation(sun_ang_r, self.radius, c)
    }

    /// Whether the sight line `dir` ends on the body.
    pub fn hides(&self, dir: DVec3) -> bool {
        self.to_sky(dir).angle_exact(self.centre) < self.radius
    }
}

/// Visible fraction of the solar disk given every body's limb and its
/// individual occulted fraction (`occulted[i]` from `limbs[i]`).
///
/// With at most one body touching the disk this is exact: `1 − f`. When
/// several touch it, the union is `Σ fᵢ − (excess)`, where the excess is
/// the part of the disk hidden more than once, weighted by the extra count.
/// The excess is integrated over [`DISK_SAMPLES`] equal-area points; it is
/// exactly zero when the limbs cover separate parts of the disk.
pub fn visible_fraction(limbs: &[Limb], occulted: &[f64], sun_dir: DVec3, sun_ang_r: f64) -> f64 {
    debug_assert_eq!(limbs.len(), occulted.len());
    if occulted.iter().any(|&f| f >= 1.0) {
        return 0.0;
    }
    let sum: f64 = occulted.iter().sum();
    let touching: Vec<&Limb> = limbs
        .iter()
        .zip(occulted)
        .filter(|(_, f)| **f > 0.0)
        .map(|(l, _)| l)
        .collect();
    let excess = if touching.len() < 2 {
        0.0
    } else {
        overlap_excess(&touching, sun_dir, sun_ang_r)
    };
    (1.0 - (sum - excess)).clamp(0.0, 1.0)
}

/// Number of equal-area points used to integrate overlap between limbs.
/// Error ≈ 1/√N of the overlapping area; 4096 gives ≲ 2e-3 of the disk.
pub const DISK_SAMPLES: usize = 4096;

/// Sunflower (Vogel) points filling the unit disk with equal area each.
fn disk_points() -> &'static [(f64, f64)] {
    static POINTS: OnceLock<Vec<(f64, f64)>> = OnceLock::new();
    POINTS.get_or_init(|| {
        let golden = PI * (3.0 - 5f64.sqrt());
        (0..DISK_SAMPLES)
            .map(|k| {
                let r = ((k as f64 + 0.5) / DISK_SAMPLES as f64).sqrt();
                let th = k as f64 * golden;
                (r * th.cos(), r * th.sin())
            })
            .collect()
    })
}

/// Mean over the disk of max(0, (number of limbs hiding the point) − 1).
fn overlap_excess(limbs: &[&Limb], sun_dir: DVec3, sun_ang_r: f64) -> f64 {
    let (e1, e2) = sun_dir.any_orthonormal_pair();
    let t = sun_ang_r.tan();
    let mut extra = 0usize;
    for &(x, y) in disk_points() {
        // Gnomonic offset: exact cap of angular radius sun_ang_r.
        let v = (sun_dir + (e1 * x + e2 * y) * t).normalize();
        let k = limbs.iter().filter(|l| l.hides(v)).count();
        extra += k.saturating_sub(1);
    }
    extra as f64 / DISK_SAMPLES as f64
}

/// Fraction of a small disk of angular radius `a` hidden behind a spherical
/// cap of angular radius `b` whose centre lies `c` away (all radians).
///
/// The cap edge is a small circle of geodesic curvature cot b. It is
/// replaced by a plane circle with the same edge position and curvature
/// (radius tan b), which is exact to second order across the tiny solar
/// disk; for b → 90° (a horizon) the edge is a straight line.
pub fn disk_occultation(a: f64, b: f64, c: f64) -> f64 {
    let d = c - b; // signed distance of the disk centre outside the edge
    if d >= a {
        return 0.0;
    }
    if d <= -a && b >= a {
        return 1.0;
    }
    let rho = b.tan();
    if rho.is_nan() || rho >= 1e4 * a {
        return segment_fraction(a, d);
    }
    circle_overlap(a, rho, rho + d) / (PI * a * a)
}

/// Area of the part of a disk of radius `a` lying beyond a straight edge
/// whose distance from the disk centre is `d` (negative: centre is hidden),
/// as a fraction of the disk.
fn segment_fraction(a: f64, d: f64) -> f64 {
    let h = (d / a).clamp(-1.0, 1.0);
    (h.acos() - h * (1.0 - h * h).sqrt()) / PI
}

/// Intersection area of plane disks of radii `a`, `b` with centres `c`
/// apart.
fn circle_overlap(a: f64, b: f64, c: f64) -> f64 {
    if c >= a + b {
        return 0.0;
    }
    if c <= (a - b).abs() {
        let r = a.min(b);
        return PI * r * r;
    }
    let ca = ((c * c + a * a - b * b) / (2.0 * c * a)).clamp(-1.0, 1.0);
    let cb = ((c * c + b * b - a * a) / (2.0 * c * b)).clamp(-1.0, 1.0);
    let k = ((-c + a + b) * (c + a - b) * (c - a + b) * (c + a + b)).max(0.0);
    a * a * ca.acos() + b * b * cb.acos() - 0.5 * k.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::SUN_RADIUS_M;
    use crate::math::AU_M;
    use glam::DQuat;

    fn body_occultation(body: &BodyState, sun_b: DVec3, ang: f64, landed: bool) -> f64 {
        Limb::of(body, landed).occultation(sun_b.normalize(), ang)
    }

    const TOL: f64 = 1e-12;

    #[test]
    fn disk_limits() {
        let a = 0.00465;
        assert_eq!(disk_occultation(a, 0.1, 0.2), 0.0); // clear
        assert_eq!(disk_occultation(a, 0.1, 0.05), 1.0); // umbra
        // Annular: small occulter centred on the disk. Plane answer b²/a²
        // is 0.25; the sky curvature adds ~1e-6.
        let f = disk_occultation(a, a / 2.0, 0.0);
        assert!((f - 0.25).abs() < 1e-5, "{f}");
        // Centre exactly on a horizon hides half.
        assert!((disk_occultation(a, FRAC_PI_2, FRAC_PI_2) - 0.5).abs() < TOL);
        // Monotonic through ingress.
        let mut last = 0.0;
        for i in 0..=200 {
            let c = 0.1 + a - 2.0 * a * i as f64 / 200.0;
            let f = disk_occultation(a, 0.1, c);
            assert!(f >= last - 1e-12 && (0.0..=1.0).contains(&f));
            last = f;
        }
        assert_eq!(last, 1.0);
    }

    #[test]
    fn equal_disks_half_spacing_matches_closed_form() {
        // Two unit circles one radius apart overlap by 2π/3 − √3/2.
        let want = (2.0 * PI / 3.0 - 3f64.sqrt() / 2.0) / PI;
        let got = circle_overlap(1.0, 1.0, 1.0) / PI;
        assert!((got - want).abs() < TOL);
    }

    /// Brute-force area on the unit sphere: fraction of a small cap (the
    /// Sun) inside a big cap (the body), by sampling the Sun's disk.
    fn spherical_reference(a: f64, b: f64, c: f64) -> f64 {
        let body = DVec3::Z;
        let sun = DVec3::new(c.sin(), 0.0, c.cos());
        let (e1, e2) = (DVec3::new(c.cos(), 0.0, -c.sin()), DVec3::Y);
        let n = 600;
        let (mut hit, mut all) = (0u64, 0u64);
        for i in 0..n {
            for j in 0..n {
                let (x, y) = (
                    (2.0 * i as f64 + 1.0) / n as f64 - 1.0,
                    (2.0 * j as f64 + 1.0) / n as f64 - 1.0,
                );
                if x * x + y * y > 1.0 {
                    continue;
                }
                // Gnomonic offsets: exact cap of angular radius a.
                let v = (sun + (e1 * x + e2 * y) * a.tan()).normalize();
                all += 1;
                if v.angle_exact(body) < b {
                    hit += 1;
                }
            }
        }
        hit as f64 / all as f64
    }

    #[test]
    fn curvature_correction_matches_sphere_for_large_caps() {
        let a = 0.00465;
        for b in [0.3, 1.2, 1.5] {
            for d in [-0.6 * a, 0.0, 0.5 * a] {
                let got = disk_occultation(a, b, b + d);
                let want = spherical_reference(a, b, b + d);
                // Sampling error of a 600×600 grid is ~1e-3.
                assert!((got - want).abs() < 2e-3, "b={b} d={d}: {got} vs {want}");
            }
        }
    }

    fn earth(vec_b: DVec3) -> BodyState {
        BodyState {
            index: 0,
            equatorial_radius_m: 6_378_137.0,
            polar_radius_m: 6_356_752.314_245,
            pole_b: DVec3::Z,
            vec_b,
        }
    }

    #[test]
    fn oblate_limb_is_exact_over_the_pole() {
        // Spacecraft 10,000 km from Earth in −X, level with the polar
        // radius; sunlight along +X grazes the north pole, so half the disk
        // is hidden. A sphere of the equatorial radius would hide more.
        let e = earth(DVec3::ZERO);
        let (a, b) = (e.equatorial_radius_m, e.polar_radius_m);
        let sc = DVec3::new(-1.0e7, 0.0, b);
        let body = earth(-sc);
        let sun_b = DVec3::X * AU_M;
        let ang = (SUN_RADIUS_M / AU_M).asin();
        let f = body_occultation(&body, sun_b, ang, false);
        assert!((f - 0.5).abs() < 0.005, "spheroid {f}");
        let sphere = BodyState {
            polar_radius_m: a,
            ..body
        };
        let fs = body_occultation(&sphere, sun_b, ang, false);
        assert!(fs > 0.6, "sphere {fs}");
    }

    #[test]
    fn oblate_follows_pole_in_body_frame() {
        // Same geometry, spacecraft rotated 90° about Y: pole is now body −X.
        let q = DQuat::from_rotation_y(FRAC_PI_2);
        let b = 6_356_752.314_245;
        let sc = DVec3::new(-1.0e7, 0.0, b);
        let mut body = earth(q * -sc);
        body.pole_b = q * DVec3::Z;
        let ang = (SUN_RADIUS_M / AU_M).asin();
        let f = body_occultation(&body, q * DVec3::X * AU_M, ang, false);
        assert!((f - 0.5).abs() < 0.005, "{f}");
    }

    #[test]
    fn landed_horizon_uses_local_tangent_plane() {
        let moon = |vec_b| BodyState {
            index: 0,
            equatorial_radius_m: 1_737_400.0,
            polar_radius_m: 1_737_400.0,
            pole_b: DVec3::Z,
            vec_b,
        };
        // 15 km above the reference sphere; Sun 2° below the local horizon.
        let body = moon(-DVec3::Z * 1_752_400.0);
        let sun = DVec3::new(2f64.to_radians().cos(), 0.0, -2f64.to_radians().sin()) * AU_M;
        let ang = (SUN_RADIUS_M / AU_M).asin();
        // Orbit model: the sphere is 7.5° below horizontal, so the Sun shows.
        assert_eq!(body_occultation(&body, sun, ang, false), 0.0);
        // Landed: the ground at the spacecraft hides it.
        assert_eq!(body_occultation(&body, sun, ang, true), 1.0);
        // Sun centred on the horizon: half.
        let f = body_occultation(&body, DVec3::X * AU_M, ang, true);
        assert!((f - 0.5).abs() < 1e-9);
    }

    // ── Several bodies on the disk at once ─────────────────────────────

    const SUN: f64 = 0.004_65; // angular radius at 1 AU

    /// A limb of radius `b` whose edge sits `d` outside the Sun centre
    /// (negative: covers the centre), approaching from azimuth `az`.
    fn limb_at(b: f64, d: f64, az: f64) -> Limb {
        let c = b + d;
        let dir = DVec3::new(c.sin() * az.cos(), c.sin() * az.sin(), c.cos());
        Limb::circle(dir, b)
    }

    fn visible(limbs: &[Limb]) -> f64 {
        let f: Vec<f64> = limbs.iter().map(|l| l.occultation(DVec3::Z, SUN)).collect();
        visible_fraction(limbs, &f, DVec3::Z, SUN)
    }

    /// Brute-force union: fraction of a dense grid on the disk that no limb
    /// hides. Independent of the inclusion–exclusion used in the code.
    fn visible_reference(limbs: &[Limb]) -> f64 {
        let n = 500;
        let (mut lit, mut all) = (0u64, 0u64);
        for i in 0..n {
            for j in 0..n {
                let x = (2.0 * i as f64 + 1.0) / n as f64 - 1.0;
                let y = (2.0 * j as f64 + 1.0) / n as f64 - 1.0;
                if x * x + y * y > 1.0 {
                    continue;
                }
                let v = DVec3::new(x * SUN.tan(), y * SUN.tan(), 1.0).normalize();
                all += 1;
                lit += limbs.iter().all(|l| !l.hides(v)) as u64;
            }
        }
        lit as f64 / all as f64
    }

    #[test]
    fn separate_parts_add_exactly() {
        // Two bodies nibbling opposite edges: nothing is covered twice, so
        // the visible part is exactly 1 − f₁ − f₂.
        let l = [limb_at(0.1, 0.5 * SUN, 0.0), limb_at(0.3, 0.4 * SUN, PI)];
        let f1 = l[0].occultation(DVec3::Z, SUN);
        let f2 = l[1].occultation(DVec3::Z, SUN);
        assert!(f1 > 0.0 && f2 > 0.0);
        assert_eq!(visible(&l), 1.0 - (f1 + f2));
    }

    #[test]
    fn same_part_covered_twice_counts_once() {
        let one = limb_at(0.1, -0.3 * SUN, 0.0);
        let f = one.occultation(DVec3::Z, SUN);
        let v = visible(&[one.clone(), one.clone()]);
        assert!((v - (1.0 - f)).abs() < 2e-3, "{v} vs {}", 1.0 - f);
        // The naive sum would have hidden 2f.
        assert!(v > 1.0 - 2.0 * f + 0.1);
    }

    #[test]
    fn body_inside_anothers_shadow_adds_nothing() {
        // A small body in front of the Sun centre (annular on its own),
        // wholly inside a big limb that also covers the centre.
        let small = limb_at(0.001, -0.001 - 0.000_1, 0.0); // centred near Sun centre
        let big = limb_at(0.2, -0.6 * SUN, 1.0);
        let fb = big.occultation(DVec3::Z, SUN);
        let v = visible(&[small, big]);
        assert!((v - (1.0 - fb)).abs() < 2e-3, "{v} vs {}", 1.0 - fb);
    }

    #[test]
    fn partial_mutual_overlap_matches_brute_force() {
        for (d1, d2, az) in [
            (0.0, 0.0, FRAC_PI_2),
            (-0.4 * SUN, 0.2 * SUN, 1.0),
            (0.3 * SUN, -0.5 * SUN, 2.5),
            (-0.2 * SUN, -0.2 * SUN, 0.4),
        ] {
            let l = [limb_at(0.05, d1, 0.0), limb_at(1.2, d2, az)];
            let got = visible(&l);
            let want = visible_reference(&l);
            assert!((got - want).abs() < 3e-3, "{d1} {d2} {az}: {got} vs {want}");
        }
    }

    #[test]
    fn double_umbra_and_horizon_plus_body_never_go_negative() {
        let l = [limb_at(0.1, -2.0 * SUN, 0.0), limb_at(0.2, -3.0 * SUN, 1.0)];
        assert_eq!(visible(&l), 0.0);
        // A body half-hiding the Sun right on a landed horizon.
        let l = [limb_at(FRAC_PI_2, 0.0, 0.0), limb_at(0.01, 0.0, 0.0)];
        let got = visible(&l);
        assert!((got - visible_reference(&l)).abs() < 3e-3, "{got}");
    }

    #[test]
    fn union_is_bounded_by_largest_and_sum() {
        // For any pair: max(fᵢ) ≤ hidden ≤ Σ fᵢ.
        for i in 0..40 {
            let s = i as f64;
            let l = [
                limb_at(
                    0.02 + 0.01 * (s * 0.7).sin().abs(),
                    SUN * (s * 1.3).sin(),
                    s,
                ),
                limb_at(0.5, SUN * (s * 0.9).cos(), s * 2.1),
            ];
            let f: Vec<f64> = l.iter().map(|x| x.occultation(DVec3::Z, SUN)).collect();
            let hidden = 1.0 - visible(&l);
            let (mx, sum) = (f[0].max(f[1]), f[0] + f[1]);
            assert!(
                hidden >= mx - 3e-3 && hidden <= sum + 1e-12,
                "{i}: {hidden} {f:?}"
            );
        }
    }
}
