//! Umbra and penumbra intervals for the timeline and plots.
//!
//! Rows are 60 s apart but lunar penumbra lasts ~10 s, so row values alone
//! would miss most of it. Every interval whose ends differ, or that sits in
//! partial shadow, is resampled every [`BAND_SAMPLE_S`] from the
//! interpolated trajectory (sunlight only, no rays: cheap).

use rayon::prelude::*;
use sunlyt_core::env::sunlight;
use sunlyt_core::{ArrayRun, EnvConfig, StateTable};

/// Resolution of band edges, s.
pub const BAND_SAMPLE_S: f64 = 0.5;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bands {
    /// Whole solar disk hidden, as `(start, end)` seconds after the epoch.
    pub umbra: Vec<(f64, f64)>,
    /// Part of the disk hidden.
    pub penumbra: Vec<(f64, f64)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shade {
    Sun,
    Penumbra,
    Umbra,
}

fn shade(visible: f64) -> Shade {
    if visible <= 0.0 {
        Shade::Umbra
    } else if visible < 1.0 {
        Shade::Penumbra
    } else {
        Shade::Sun
    }
}

impl Bands {
    pub fn compute(table: &StateTable, env: &EnvConfig, run: &ArrayRun) -> Self {
        let t = &run.t;
        let v: Vec<f64> = run.samples.iter().map(|s| s.sun.visible).collect();
        let pts: Vec<(f64, f64)> = (0..t.len())
            .into_par_iter()
            .flat_map_iter(|k| {
                let mut out = vec![(t[k], v[k])];
                if let Some(&t1) = t.get(k + 1) {
                    let steady = v[k] == v[k + 1] && (v[k] == 0.0 || v[k] == 1.0);
                    if !steady {
                        let m = ((t1 - t[k]) / BAND_SAMPLE_S).ceil().max(1.0) as usize;
                        out.extend((1..m).filter_map(|j| {
                            let tj = t[k] + (t1 - t[k]) * j as f64 / m as f64;
                            table.sample(tj).map(|e| (tj, sunlight(&e, env).visible))
                        }));
                    }
                }
                out
            })
            .collect();
        Self::from_points(&pts)
    }

    /// Runs of equal shade in time-ordered `(t, visible)` points. Each run
    /// extends halfway to its neighbouring points.
    fn from_points(pts: &[(f64, f64)]) -> Self {
        let mut bands = Self::default();
        let mut a = 0;
        while a < pts.len() {
            let s = shade(pts[a].1);
            let mut b = a;
            while b + 1 < pts.len() && shade(pts[b + 1].1) == s {
                b += 1;
            }
            let start = if a == 0 {
                pts[a].0
            } else {
                0.5 * (pts[a - 1].0 + pts[a].0)
            };
            let end = match pts.get(b + 1) {
                Some(n) => 0.5 * (pts[b].0 + n.0),
                None => pts[b].0,
            };
            match s {
                Shade::Umbra => bands.umbra.push((start, end)),
                Shade::Penumbra => bands.penumbra.push((start, end)),
                Shade::Sun => {}
            }
            a = b + 1;
        }
        bands
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_split_at_midpoints() {
        let pts = [
            (0.0, 1.0),
            (1.0, 1.0),
            (2.0, 0.5),
            (3.0, 0.0),
            (4.0, 0.0),
            (5.0, 0.3),
            (6.0, 1.0),
        ];
        let b = Bands::from_points(&pts);
        assert_eq!(b.penumbra, vec![(1.5, 2.5), (4.5, 5.5)]);
        assert_eq!(b.umbra, vec![(2.5, 4.5)]);
    }
}
