//! Mission runs: evaluate a power model at every table row, and summarise.
//!
//! Rows are independent (no state carries between steps until the thermal
//! model in slice 5), so they run in parallel with rayon; serial versions
//! exist to check that parallel results are identical. The viewer uses the
//! `*_at(t)` functions for one interpolated instant, on the same code path.

pub mod array;
pub mod plate;

pub use array::{ArrayRun, ECLIPSE_SAMPLE_S, array_at};
pub use plate::{PlateRun, plate_at};

/// Summary statistics of a run.
#[derive(Debug, Clone, PartialEq)]
pub struct RunSummary {
    pub rows: usize,
    /// Rows where each body hides the whole disk.
    pub umbra: Vec<usize>,
    /// Rows where each body hides part of the disk.
    pub penumbra: Vec<usize>,
    pub mean_w: f64,
    pub min_w: f64,
    pub max_w: f64,
    pub energy_wh: f64,
}

impl RunSummary {
    /// From per-row times, total power and per-body occulted fractions.
    pub fn new<'a>(
        t: &[f64],
        power_w: &[f64],
        occulted: impl Iterator<Item = &'a [f64]>,
        bodies: usize,
    ) -> Self {
        let mut s = Self {
            rows: power_w.len(),
            umbra: vec![0; bodies],
            penumbra: vec![0; bodies],
            mean_w: 0.0,
            min_w: f64::INFINITY,
            max_w: 0.0,
            energy_wh: energy_wh(t, power_w),
        };
        for occ in occulted {
            for (k, &f) in occ.iter().enumerate() {
                s.umbra[k] += (f >= 1.0) as usize;
                s.penumbra[k] += (f > 0.0 && f < 1.0) as usize;
            }
        }
        for &p in power_w {
            s.mean_w += p;
            s.min_w = s.min_w.min(p);
            s.max_w = s.max_w.max(p);
        }
        if s.rows > 0 {
            s.mean_w /= s.rows as f64;
        }
        s
    }
}

/// Trapezoidal energy, Wh.
pub fn energy_wh(t: &[f64], power_w: &[f64]) -> f64 {
    t.windows(2)
        .zip(power_w.windows(2))
        .map(|(t, p)| 0.5 * (p[0] + p[1]) * (t[1] - t[0]))
        .sum::<f64>()
        / 3600.0
}
