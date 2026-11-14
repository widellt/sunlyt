//! Flat-plate runs.

use rayon::prelude::*;

use super::RunSummary;
use crate::env::EnvConfig;
use crate::power::{FlatPlate, PlateSample};
use crate::trajectory::StateTable;

/// Flat-plate results at every table row, in time order.
#[derive(Debug, Clone)]
pub struct PlateRun {
    pub t: Vec<f64>,
    pub samples: Vec<PlateSample>,
}

impl PlateRun {
    /// Evaluates `plate` at every row of `table`, in parallel.
    pub fn compute(table: &StateTable, plate: &FlatPlate, env: &EnvConfig) -> Self {
        let samples: Vec<PlateSample> = (0..table.len())
            .into_par_iter()
            .map(|i| plate.sample(&table.row(i), env))
            .collect();
        Self {
            t: table.times().to_vec(),
            samples,
        }
    }

    /// Same as [`compute`](Self::compute) on one thread; for benchmarks
    /// and for checking that parallel and serial runs agree bit for bit.
    pub fn compute_serial(table: &StateTable, plate: &FlatPlate, env: &EnvConfig) -> Self {
        Self {
            t: table.times().to_vec(),
            samples: (0..table.len())
                .map(|i| plate.sample(&table.row(i), env))
                .collect(),
        }
    }

    pub fn power_w(&self) -> impl Iterator<Item = f64> + '_ {
        self.samples.iter().map(|s| s.power_w)
    }

    pub fn summary(&self, bodies: usize) -> RunSummary {
        let p: Vec<f64> = self.power_w().collect();
        RunSummary::new(
            &self.t,
            &p,
            self.samples.iter().map(|s| s.sun.occulted.as_slice()),
            bodies,
        )
    }
}

/// Plate output at any time `t` (interpolated), or `None` outside the table.
pub fn plate_at(
    table: &StateTable,
    plate: &FlatPlate,
    env: &EnvConfig,
    t: f64,
) -> Option<PlateSample> {
    table.sample(t).map(|e| plate.sample(&e, env))
}
