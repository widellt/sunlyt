//! Synthetic trajectories for examples, docs and tests: a circular lunar
//! orbit written in the `stk-body-vectors` CSV layout. Nothing here comes
//! from mission data, so the output can be committed and shared.
//!
//! Geometry (J2000-like axes, Moon-centred): the Sun is 1 AU along +X and
//! the Earth 384,400 km along +Y (quarter phase, so the Earth never
//! eclipses the Sun). The orbit plane makes the angle `beta` with the Sun
//! line: β = 0 gives the longest eclipse every orbit; |β| above about
//! asin(R_moon / r) gives none.

use std::io::{self, Write};

use chrono::{NaiveDateTime, TimeDelta};
use glam::{DMat3, DQuat, DVec3};

const MOON_R_KM: f64 = 1737.4;
const MOON_MU_KM3_S2: f64 = 4902.8;
const AU_KM: f64 = 149_597_870.7;
const EARTH_KM: f64 = 384_400.0;

/// How the spacecraft is pointed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SynthAttitude {
    /// Body axes = J2000 axes.
    Inertial,
    /// Body axis `axis` (unit, body frame) points at the Sun; the roll is
    /// fixed by keeping a second body axis in the plane of the Sun line and
    /// the orbit normal.
    SunPointing { axis: DVec3 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SynthOrbit {
    pub start: NaiveDateTime,
    pub duration_s: f64,
    /// Row spacing, s: a whole number of minutes (the STK time format has
    /// no seconds).
    pub step_s: f64,
    pub altitude_km: f64,
    pub beta_deg: f64,
    pub attitude: SynthAttitude,
}

#[derive(Debug, thiserror::Error)]
pub enum SynthError {
    #[error("step must be a positive whole number of minutes, got {0} s")]
    Step(f64),
    #[error("altitude must be above 0 km, got {0}")]
    Altitude(f64),
    #[error("duration must be positive, got {0} s")]
    Duration(f64),
    #[error("the Sun-pointing axis must be non-zero")]
    Axis,
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl SynthOrbit {
    /// Orbital period, s.
    pub fn period_s(&self) -> f64 {
        let r = MOON_R_KM + self.altitude_km;
        std::f64::consts::TAU * (r.powi(3) / MOON_MU_KM3_S2).sqrt()
    }

    /// Writes the CSV (header plus one row per step, both ends included).
    pub fn write_csv<W: Write>(&self, w: W) -> Result<usize, SynthError> {
        if !(self.step_s > 0.0 && (self.step_s / 60.0).fract() == 0.0) {
            return Err(SynthError::Step(self.step_s));
        }
        if self.altitude_km.is_nan() || self.altitude_km <= 0.0 {
            return Err(SynthError::Altitude(self.altitude_km));
        }
        if self.duration_s.is_nan() || self.duration_s <= 0.0 {
            return Err(SynthError::Duration(self.duration_s));
        }
        if let SynthAttitude::SunPointing { axis } = self.attitude
            && axis.length() == 0.0
        {
            return Err(SynthError::Axis);
        }
        let mut w = io::BufWriter::new(w);
        writeln!(
            w,
            "Time (UTCG),EarthJ2000-x (km),EarthJ2000-y (km),EarthJ2000-z (km),\
             J2000-qx,J2000-qy,J2000-qz,J2000-qs,\
             EarthBdyX (km),EarthBdyY (km),EarthBdyZ (km),\
             MoonBdyX (km),MoonBdyY (km),MoonBdyZ (km),\
             SunBdyX (km),SunBdyY (km),SunBdyZ (km)"
        )?;
        let r = MOON_R_KM + self.altitude_km;
        let n = std::f64::consts::TAU / self.period_s();
        let b = self.beta_deg.to_radians();
        // In-plane basis: `a` is the in-plane direction closest to the Sun.
        let a = DVec3::new(b.cos(), 0.0, b.sin());
        let c = DVec3::Y;
        let normal = a.cross(c);
        let sun = DVec3::X * AU_KM;
        let earth = DVec3::Y * EARTH_KM;
        let rows = (self.duration_s / self.step_s).floor() as usize + 1;
        for k in 0..rows {
            let t = k as f64 * self.step_s;
            let sc = (a * (n * t).cos() + c * (n * t).sin()) * r;
            let q = match self.attitude {
                SynthAttitude::Inertial => DQuat::IDENTITY,
                SynthAttitude::SunPointing { axis } => sun_pointing(axis, sun - sc, normal),
            };
            // Body vectors: v_b = R(q)ᵀ v_J2000 (q maps body → J2000).
            let inv = q.conjugate();
            let (e, m, s) = (inv * (earth - sc), inv * -sc, inv * (sun - sc));
            let pos = sc - earth;
            let time = (self.start + TimeDelta::seconds(t as i64)).format("%-m/%-d/%Y %-H:%M");
            writeln!(
                w,
                "{time},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                pos.x,
                pos.y,
                pos.z,
                q.x,
                q.y,
                q.z,
                q.w,
                e.x,
                e.y,
                e.z,
                m.x,
                m.y,
                m.z,
                s.x,
                s.y,
                s.z
            )?;
        }
        w.flush()?;
        Ok(rows)
    }
}

/// Rotation (body → J2000) taking body `axis` onto `to_sun` and a second
/// body axis into the plane of `to_sun` and `normal`.
fn sun_pointing(axis: DVec3, to_sun: DVec3, normal: DVec3) -> DQuat {
    let b1 = axis.normalize();
    // Second body axis: whichever of X, Z is least aligned with b1.
    let helper = if b1.z.abs() < 0.9 { DVec3::Z } else { DVec3::X };
    let b2 = (helper - b1 * helper.dot(b1)).normalize();
    let t1 = to_sun.normalize();
    let mut t2 = normal - t1 * normal.dot(t1);
    if t2.length() < 1e-9 {
        t2 = t1.any_orthonormal_vector();
    }
    let t2 = t2.normalize();
    let body = DMat3::from_cols(b1, b2, b1.cross(b2));
    let target = DMat3::from_cols(t1, t2, t1.cross(t2));
    DQuat::from_mat3(&(target * body.transpose())).normalize()
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::env::{EnvConfig, sunlight};
    use crate::trajectory::{CsvMapping, StateTable};

    fn orbit(attitude: SynthAttitude, beta_deg: f64) -> SynthOrbit {
        SynthOrbit {
            start: NaiveDate::from_ymd_opt(2030, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            duration_s: 4.0 * 3600.0,
            step_s: 60.0,
            altitude_km: 100.0,
            beta_deg,
            attitude,
        }
    }

    fn load(o: &SynthOrbit) -> StateTable {
        let mut buf = Vec::new();
        o.write_csv(&mut buf).unwrap();
        StateTable::from_csv_reader(buf.as_slice(), &CsvMapping::stk_body_vectors()).unwrap()
    }

    #[test]
    fn loads_with_consistent_attitude_and_points_at_the_sun() {
        let axis = DVec3::Z;
        let o = orbit(SynthAttitude::SunPointing { axis }, 0.0);
        let tb = load(&o);
        assert_eq!(tb.len(), 241);
        // The quaternion reproduces the Earth vector from the position.
        let res = tb.attitude_residual().unwrap();
        assert!(res.max_m < 1.0, "attitude residual {} m", res.max_m);
        for i in 0..tb.len() {
            let s = tb.row(i).sun_b.normalize();
            assert!((s - axis).length() < 1e-9, "row {i}: Sun at {s}");
        }
    }

    #[test]
    fn eclipse_length_matches_cylinder_geometry() {
        let o = orbit(SynthAttitude::Inertial, 0.0);
        let tb = load(&o);
        assert!(tb.attitude_residual().unwrap().max_m < 1.0);
        let env = EnvConfig::default();
        let dark = (0..tb.len())
            .filter(|&i| sunlight(&tb.row(i), &env).visible == 0.0)
            .count() as f64;
        // Umbra fraction of a β = 0 circular orbit ≈ asin(R/r)/π (shadow
        // cylinder; the cone narrows it by well under a minute here).
        let r = MOON_R_KM + o.altitude_km;
        let expected = (MOON_R_KM / r).asin() / std::f64::consts::PI * tb.len() as f64;
        assert!(
            (dark - expected).abs() <= 6.0,
            "{dark} dark rows, expected ≈ {expected:.1}"
        );
        // β above asin(R/r) = 71° at 100 km: no eclipse at all.
        let tb = load(&orbit(SynthAttitude::Inertial, 80.0));
        assert!((0..tb.len()).all(|i| sunlight(&tb.row(i), &env).visible == 1.0));
    }

    #[test]
    fn rejects_bad_steps() {
        let mut o = orbit(SynthAttitude::Inertial, 0.0);
        o.step_s = 30.0;
        assert!(matches!(o.write_csv(Vec::new()), Err(SynthError::Step(_))));
    }
}
