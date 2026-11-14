//! Trajectory input: a time-tagged table of Sun / body vectors and attitude,
//! plus interpolation between rows (design D7, D10).

use std::fs::File;
use std::io::Read;
use std::path::Path;

use chrono::{NaiveDateTime, TimeDelta};
use glam::{DQuat, DVec3};

use super::mapping::{CsvMapping, QuatDirection, QuatOrder};
use crate::math::slerp;

#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("opening {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("CSV: {0}")]
    Csv(#[from] csv::Error),
    #[error("column {0:?} not found in the CSV header")]
    MissingColumn(String),
    #[error("line {line}: cannot parse {column:?} value {value:?} as a number")]
    Number {
        line: usize,
        column: String,
        value: String,
    },
    #[error("line {line}: time {value:?} does not match format {format:?}")]
    Time {
        line: usize,
        value: String,
        format: String,
    },
    #[error("line {line}: time does not increase")]
    NonMonotonic { line: usize },
    #[error("line {line}: quaternion norm {norm} is not 1")]
    BadQuaternion { line: usize, norm: f64 },
    #[error("body {0:?}: radii must be positive with polar ≤ equatorial, and the pole non-zero")]
    BadShape(String),
    #[error("position center {0:?} is not one of the listed bodies")]
    UnknownCenter(String),
    #[error("the CSV has no data rows")]
    Empty,
}

/// Per-body time series. Vectors are spacecraft→body, metres.
#[derive(Debug, Clone)]
struct BodyTrack {
    name: String,
    equatorial_radius_m: f64,
    polar_radius_m: f64,
    /// Unit spin axis, inertial frame.
    pole_i: DVec3,
    vec_b: Vec<DVec3>,
    /// Same vectors in the inertial frame.
    vec_i: Vec<DVec3>,
}

impl BodyTrack {
    fn state(&self, index: usize, vec_b: DVec3, q_bi: DQuat) -> BodyState {
        BodyState {
            index,
            equatorial_radius_m: self.equatorial_radius_m,
            polar_radius_m: self.polar_radius_m,
            pole_b: q_bi.conjugate() * self.pole_i,
            vec_b,
        }
    }
}

/// One body at one instant. The body is an oblate spheroid (a sphere when
/// the two radii are equal) whose symmetry axis is `pole_b`.
#[derive(Debug, Clone, PartialEq)]
pub struct BodyState {
    /// Index into [`StateTable::body_names`].
    pub index: usize,
    pub equatorial_radius_m: f64,
    pub polar_radius_m: f64,
    /// Unit spin axis, spacecraft body frame.
    pub pole_b: DVec3,
    /// Spacecraft→body-centre vector, body frame, metres.
    pub vec_b: DVec3,
}

impl BodyState {
    pub fn is_sphere(&self) -> bool {
        self.equatorial_radius_m == self.polar_radius_m
    }

    /// Radius of the surface in the direction of the spacecraft (geocentric).
    pub fn surface_radius_m(&self) -> f64 {
        let (a, b) = (self.equatorial_radius_m, self.polar_radius_m);
        if a == b {
            return a;
        }
        let up = -self.vec_b.normalize();
        let sin_lat = up.dot(self.pole_b).clamp(-1.0, 1.0);
        let cos_lat = (1.0 - sin_lat * sin_lat).sqrt();
        a * b / ((b * cos_lat).powi(2) + (a * sin_lat).powi(2)).sqrt()
    }

    /// Height above the surface along the line to the centre. Within ~1 m
    /// of geodetic altitude beyond a few thousand km; ~21 km at most near
    /// Earth's surface at mid-latitudes.
    pub fn altitude_m(&self) -> f64 {
        self.vec_b.length() - self.surface_radius_m()
    }
}

/// Geometry at one instant.
#[derive(Debug, Clone, PartialEq)]
pub struct Epoch {
    /// Seconds since [`StateTable::epoch`].
    pub t: f64,
    /// Spacecraft→Sun vector, body frame, metres.
    pub sun_b: DVec3,
    pub bodies: Vec<BodyState>,
    /// Body→inertial rotation (`v_i = q_bi * v_b`).
    pub q_bi: DQuat,
}

/// Attitude-consistency residual: how far the body-frame vector of the
/// position's centre body is from `R · (−position)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Residual {
    pub max_m: f64,
    pub rms_m: f64,
    /// Largest error relative to the distance, dimensionless.
    pub max_rel: f64,
    pub worst_row: usize,
}

/// Trajectory table in canonical form: SI units, body-frame vectors, and the
/// body→inertial attitude.
#[derive(Debug, Clone)]
pub struct StateTable {
    epoch: NaiveDateTime,
    t: Vec<f64>,
    sun_b: Vec<DVec3>,
    sun_i: Vec<DVec3>,
    bodies: Vec<BodyTrack>,
    q_bi: Vec<DQuat>,
    pos_i: Option<(usize, Vec<DVec3>)>,
}

impl StateTable {
    pub fn from_csv_path(path: &Path, map: &CsvMapping) -> Result<Self, InputError> {
        let file = File::open(path).map_err(|source| InputError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_csv_reader(file, map)
    }

    pub fn from_csv_reader<R: Read>(reader: R, map: &CsvMapping) -> Result<Self, InputError> {
        let mut rdr = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_reader(reader);
        let headers = rdr.headers()?.clone();
        let col = |name: &String| {
            headers
                .iter()
                .position(|h| h == name)
                .ok_or_else(|| InputError::MissingColumn(name.clone()))
        };
        let col3 = |names: &[String; 3]| -> Result<[usize; 3], InputError> {
            Ok([col(&names[0])?, col(&names[1])?, col(&names[2])?])
        };

        let time_c = col(&map.time.column)?;
        let sun_c = col3(&map.sun_body)?;
        let body_c = map
            .bodies
            .iter()
            .map(|b| col3(&b.columns))
            .collect::<Result<Vec<_>, _>>()?;
        let a = &map.attitude;
        let att_c = [
            col(&a.columns[0])?,
            col(&a.columns[1])?,
            col(&a.columns[2])?,
            col(&a.columns[3])?,
        ];
        let pos_c = match &map.position {
            Some(p) => {
                let center = map
                    .bodies
                    .iter()
                    .position(|b| b.name == p.center)
                    .ok_or_else(|| InputError::UnknownCenter(p.center.clone()))?;
                Some((center, col3(&p.columns)?))
            }
            None => None,
        };

        let unit = map.length_unit_m;
        let mut times = Vec::new();
        let mut sun_b = Vec::new();
        let mut bodies = map
            .bodies
            .iter()
            .map(|b| {
                let equatorial = b.radius_km;
                let polar = b.polar_radius_km.unwrap_or(equatorial);
                let pole = DVec3::from_array(b.pole_inertial);
                let valid = equatorial > 0.0
                    && polar > 0.0
                    && polar <= equatorial
                    && pole.length() > 0.0
                    && pole.is_finite();
                if !valid {
                    return Err(InputError::BadShape(b.name.clone()));
                }
                Ok(BodyTrack {
                    name: b.name.clone(),
                    equatorial_radius_m: equatorial * 1000.0,
                    polar_radius_m: polar * 1000.0,
                    pole_i: pole.normalize(),
                    vec_b: Vec::new(),
                    vec_i: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut quats = Vec::new();
        let mut pos = Vec::new();

        for (k, rec) in rdr.records().enumerate() {
            let rec = rec?;
            let line = k + 2;
            let num = |c: usize| -> Result<f64, InputError> {
                rec[c].parse::<f64>().map_err(|_| InputError::Number {
                    line,
                    column: headers[c].to_string(),
                    value: rec[c].to_string(),
                })
            };
            let vec3 = |c: [usize; 3]| -> Result<DVec3, InputError> {
                Ok(DVec3::new(num(c[0])?, num(c[1])?, num(c[2])?) * unit)
            };

            let ts =
                NaiveDateTime::parse_from_str(&rec[time_c], &map.time.format).map_err(|_| {
                    InputError::Time {
                        line,
                        value: rec[time_c].to_string(),
                        format: map.time.format.clone(),
                    }
                })?;
            times.push(ts);
            if times.len() > 1 && ts <= times[times.len() - 2] {
                return Err(InputError::NonMonotonic { line });
            }
            sun_b.push(vec3(sun_c)?);
            for (track, c) in bodies.iter_mut().zip(&body_c) {
                track.vec_b.push(vec3(*c)?);
            }
            let c = att_c;
            let v = [num(c[0])?, num(c[1])?, num(c[2])?, num(c[3])?];
            let q = match a.order {
                QuatOrder::ScalarLast => DQuat::from_xyzw(v[0], v[1], v[2], v[3]),
                QuatOrder::ScalarFirst => DQuat::from_xyzw(v[1], v[2], v[3], v[0]),
            };
            let norm = q.length();
            if !norm.is_finite() || (norm - 1.0).abs() > 1e-3 {
                return Err(InputError::BadQuaternion { line, norm });
            }
            let q = q.normalize();
            // InertialToBody: v_b = R(q)ᵀ v_i  ⇒  v_i = R(q) v_b  ⇒  q_bi = q.
            quats.push(match a.direction {
                QuatDirection::InertialToBody => q,
                QuatDirection::BodyToInertial => q.conjugate(),
            });
            if let Some((_, c)) = pos_c {
                pos.push(vec3(c)?);
            }
        }

        let Some(&epoch) = times.first() else {
            return Err(InputError::Empty);
        };
        let t = times.iter().map(|ts| seconds(*ts - epoch)).collect();
        let q_bi = quats;
        let sun_i = rotate_all(&q_bi, &sun_b);
        for track in &mut bodies {
            track.vec_i = rotate_all(&q_bi, &track.vec_b);
        }

        Ok(Self {
            epoch,
            t,
            sun_b,
            sun_i,
            bodies,
            q_bi,
            pos_i: pos_c.map(|(center, _)| (center, pos)),
        })
    }

    /// UTC time of `t = 0`.
    pub fn epoch(&self) -> NaiveDateTime {
        self.epoch
    }

    /// UTC time of `t` seconds after the epoch (millisecond resolution).
    pub fn datetime(&self, t: f64) -> NaiveDateTime {
        self.epoch + TimeDelta::milliseconds((t * 1000.0).round() as i64)
    }

    pub fn len(&self) -> usize {
        self.t.len()
    }

    pub fn is_empty(&self) -> bool {
        self.t.is_empty()
    }

    /// Row times, seconds since the epoch.
    pub fn times(&self) -> &[f64] {
        &self.t
    }

    pub fn duration(&self) -> f64 {
        self.t.last().copied().unwrap_or(0.0)
    }

    pub fn body_names(&self) -> impl Iterator<Item = &str> {
        self.bodies.iter().map(|b| b.name.as_str())
    }

    pub fn body_index(&self, name: &str) -> Option<usize> {
        self.bodies.iter().position(|b| b.name == name)
    }

    /// Exact values of row `i`.
    pub fn row(&self, i: usize) -> Epoch {
        let q = self.q_bi[i];
        Epoch {
            t: self.t[i],
            sun_b: self.sun_b[i],
            bodies: self
                .bodies
                .iter()
                .enumerate()
                .map(|(index, b)| b.state(index, b.vec_b[i], q))
                .collect(),
            q_bi: q,
        }
    }

    /// Geometry at `t` seconds after the epoch, or `None` outside the table.
    ///
    /// Body vectors are rotated to inertial, interpolated linearly there
    /// (they move slowly), the attitude is slerped, and the result is
    /// rotated back into the body frame.
    pub fn sample(&self, t: f64) -> Option<Epoch> {
        let n = self.t.len();
        if n == 0 || !(t >= self.t[0] && t <= self.t[n - 1]) {
            return None;
        }
        let k = self.t.partition_point(|&x| x <= t);
        let i = k.saturating_sub(1);
        if i + 1 >= n || t == self.t[i] {
            return Some(self.row(i.min(n - 1)));
        }
        let u = (t - self.t[i]) / (self.t[i + 1] - self.t[i]);

        let qt = slerp(self.q_bi[i], self.q_bi[i + 1], u);
        let inv = qt.conjugate();
        let back = |v: &[DVec3]| inv * v[i].lerp(v[i + 1], u);
        Some(Epoch {
            t,
            sun_b: back(&self.sun_i),
            bodies: self
                .bodies
                .iter()
                .enumerate()
                .map(|(index, b)| b.state(index, back(&b.vec_i), qt))
                .collect(),
            q_bi: qt,
        })
    }

    /// Cross-checks the attitude convention: the centre body's body-frame
    /// vector should equal `q_bi⁻¹ · (−position)`. `None` without position
    /// columns.
    pub fn attitude_residual(&self) -> Option<Residual> {
        let q = &self.q_bi;
        let (center, pos) = self.pos_i.as_ref()?;
        let body = &self.bodies[*center];
        let mut res = Residual {
            max_m: 0.0,
            rms_m: 0.0,
            max_rel: 0.0,
            worst_row: 0,
        };
        let mut sum_sq = 0.0;
        for i in 0..self.t.len() {
            let predicted = q[i].conjugate() * -pos[i];
            let err = (predicted - body.vec_b[i]).length();
            sum_sq += err * err;
            if err > res.max_m {
                res.max_m = err;
                res.worst_row = i;
            }
            res.max_rel = res.max_rel.max(err / pos[i].length().max(1.0));
        }
        res.rms_m = (sum_sq / self.t.len() as f64).sqrt();
        Some(res)
    }

    /// A new table holding only the given rows (in the given order, which
    /// must be increasing). Used for leave-one-out interpolation checks.
    pub fn subset(&self, rows: &[usize]) -> Self {
        let pick = |v: &[DVec3]| -> Vec<DVec3> { rows.iter().map(|&i| v[i]).collect() };
        Self {
            epoch: self.epoch,
            t: rows.iter().map(|&i| self.t[i]).collect(),
            sun_b: pick(&self.sun_b),
            sun_i: pick(&self.sun_i),
            bodies: self
                .bodies
                .iter()
                .map(|b| BodyTrack {
                    vec_b: pick(&b.vec_b),
                    vec_i: pick(&b.vec_i),
                    ..b.clone()
                })
                .collect(),
            q_bi: rows.iter().map(|&i| self.q_bi[i]).collect(),
            pos_i: self.pos_i.as_ref().map(|(c, p)| (*c, pick(p))),
        }
    }
}

fn seconds(d: TimeDelta) -> f64 {
    d.num_milliseconds() as f64 / 1000.0
}

fn rotate_all(q: &[DQuat], v: &[DVec3]) -> Vec<DVec3> {
    q.iter().zip(v).map(|(q, v)| *q * *v).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AngleExt;

    /// Synthetic STK-style CSV: spacecraft spinning about body +X at `omega`
    /// rad/s, Sun fixed in inertial space, Moon on a circular orbit around
    /// the spacecraft (enough to exercise the maths).
    fn synthetic_csv(rows: usize, omega: f64) -> String {
        let mut s = String::from(
            "Time (UTCG),EarthJ2000-x (km),EarthJ2000-y (km),EarthJ2000-z (km),\
             J2000-qx,J2000-qy,J2000-qz,J2000-qs,\
             EarthBdyX (km),EarthBdyY (km),EarthBdyZ (km),\
             MoonBdyX (km),MoonBdyY (km),MoonBdyZ (km),\
             SunBdyX (km),SunBdyY (km),SunBdyZ (km)\n",
        );
        let sun_i = DVec3::new(0.0, 1.0e8, 1.0e8);
        let pos = DVec3::new(-370_000.0, 50_000.0, 1_000.0);
        for k in 0..rows {
            let t = k as f64 * 60.0;
            let q = DQuat::from_rotation_x(omega * t); // body→inertial
            let moon_i = DVec3::new(2000.0 * (1e-3 * t).cos(), 2000.0 * (1e-3 * t).sin(), 0.0);
            let to_b = |v: DVec3| q.conjugate() * v;
            let (e, m, su) = (to_b(-pos), to_b(moon_i), to_b(sun_i));
            let mins = 9 * 60 + 7 + k;
            s.push_str(&format!(
                "1/{}/2030 {}:{:02},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                5 + mins / 1440,
                (mins % 1440) / 60,
                mins % 60,
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
                su.x,
                su.y,
                su.z,
            ));
        }
        s
    }

    fn load(rows: usize, omega: f64) -> StateTable {
        StateTable::from_csv_reader(
            synthetic_csv(rows, omega).as_bytes(),
            &CsvMapping::stk_body_vectors(),
        )
        .unwrap()
    }

    #[test]
    fn parses_unpadded_stk_time() {
        let table = load(3, 0.0);
        assert_eq!(table.epoch().to_string(), "2030-01-05 09:07:00");
        assert_eq!(table.times(), &[0.0, 60.0, 120.0]);
        assert_eq!(table.datetime(90.0).to_string(), "2030-01-05 09:08:30");
    }

    #[test]
    fn converts_km_to_m() {
        let table = load(1, 0.0);
        let sun = table.row(0).sun_b.length();
        assert!((sun - 2f64.sqrt() * 1e11).abs() < 1.0, "{sun}");
        let row = table.row(0);
        let (earth, moon) = (&row.bodies[0], &row.bodies[1]);
        assert_eq!(moon.equatorial_radius_m, 1_737_400.0);
        assert!(moon.is_sphere());
        assert_eq!(earth.equatorial_radius_m, 6_378_137.0);
        assert!((earth.polar_radius_m - 6_356_752.314245).abs() < 1e-6);
        assert!(!earth.is_sphere());
    }

    #[test]
    fn earth_pole_follows_attitude() {
        // Body rolls about +X, so J2000 +Z sweeps around the body Y–Z plane.
        let omega = 6f64.to_radians() / 60.0;
        let table = load(10, omega);
        for t in [0.0, 30.0, 95.5, 300.0] {
            let pole = table.sample(t).unwrap().bodies[0].pole_b;
            let truth = DQuat::from_rotation_x(omega * t).conjugate() * DVec3::Z;
            assert!(pole.angle_exact(truth) < 1e-12, "t={t}");
        }
    }

    #[test]
    fn rejects_bad_shape() {
        let mut map = CsvMapping::stk_body_vectors();
        map.bodies[0].polar_radius_km = Some(7000.0); // prolate: not supported
        assert!(matches!(
            StateTable::from_csv_reader(synthetic_csv(2, 0.0).as_bytes(), &map),
            Err(InputError::BadShape(name)) if name == "Earth"
        ));
    }

    #[test]
    fn attitude_residual_is_zero_for_consistent_data() {
        let r = load(20, 0.1).attitude_residual().unwrap();
        assert!(r.max_rel < 1e-12, "{r:?}");
    }

    #[test]
    fn wrong_quaternion_convention_is_detected() {
        let mut map = CsvMapping::stk_body_vectors();
        map.attitude.direction = QuatDirection::BodyToInertial;
        let table = StateTable::from_csv_reader(synthetic_csv(20, 0.01).as_bytes(), &map).unwrap();
        assert!(table.attitude_residual().unwrap().max_rel > 0.1);
    }

    #[test]
    fn interpolation_follows_spin_exactly() {
        // 6°/min roll: naive body-frame lerp would be badly wrong mid-step.
        let omega = 6f64.to_radians() / 60.0;
        let table = load(30, omega);
        let sun_i = DVec3::new(0.0, 1.0e8, 1.0e8) * 1000.0;
        for k in 0..290 {
            let t = k as f64 * 6.0 + 0.5;
            let e = table.sample(t).unwrap();
            let truth = DQuat::from_rotation_x(omega * t).conjugate() * sun_i;
            let ang = e.sun_b.angle_exact(truth);
            assert!(ang < 1e-9, "t={t} err={ang} rad");
        }
    }

    #[test]
    fn slerp_takes_short_path_across_sign_flip() {
        // A quaternion and its negation are the same rotation; the CSV may
        // flip sign between rows. Interpolating must not swing the long way.
        let mut csv = synthetic_csv(2, 0.0);
        let lines: Vec<&str> = csv.lines().collect();
        let mut f: Vec<String> = lines[2].split(',').map(String::from).collect();
        for c in &mut f[4..8] {
            let v: f64 = c.parse().unwrap();
            *c = (-v).to_string();
        }
        csv = format!("{}\n{}\n{}\n", lines[0], lines[1], f.join(","));
        let table =
            StateTable::from_csv_reader(csv.as_bytes(), &CsvMapping::stk_body_vectors()).unwrap();
        let mid = table.sample(30.0).unwrap();
        assert!(mid.sun_b.angle_exact(table.row(0).sun_b) < 1e-9);
    }

    #[test]
    fn sample_outside_range_is_none_and_nodes_are_exact() {
        let table = load(5, 0.05);
        assert!(table.sample(-1.0).is_none());
        assert!(table.sample(241.0).is_none());
        assert_eq!(table.sample(120.0).unwrap(), table.row(2));
        assert_eq!(table.sample(240.0).unwrap(), table.row(4));
    }

    #[test]
    fn subset_keeps_rows() {
        let table = load(10, 0.05);
        let sub = table.subset(&[0, 2, 4]);
        assert_eq!(sub.len(), 3);
        assert_eq!(sub.row(1), table.row(2));
    }

    #[test]
    fn rejects_bad_input() {
        let map = CsvMapping::stk_body_vectors();
        let csv = synthetic_csv(3, 0.0);
        let missing = csv.replacen("MoonBdyX (km)", "Nope", 1);
        assert!(matches!(
            StateTable::from_csv_reader(missing.as_bytes(), &map),
            Err(InputError::MissingColumn(_))
        ));
        let lines: Vec<&str> = csv.lines().collect();
        let backwards = format!("{}\n{}\n{}\n", lines[0], lines[2], lines[1]);
        assert!(matches!(
            StateTable::from_csv_reader(backwards.as_bytes(), &map),
            Err(InputError::NonMonotonic { line: 3 })
        ));
        let header_only = format!("{}\n", lines[0]);
        assert!(matches!(
            StateTable::from_csv_reader(header_only.as_bytes(), &map),
            Err(InputError::Empty)
        ));
    }
}
