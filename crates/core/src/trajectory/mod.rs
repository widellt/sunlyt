//! Trajectory input: CSV column mapping, the time-tagged state table, and
//! interpolation between rows (design D7, D10).

pub mod mapping;
pub mod synth;
pub mod table;

pub use mapping::CsvMapping;
pub use synth::{SynthAttitude, SynthError, SynthOrbit};
pub use table::{BodyState, Epoch, InputError, Residual, StateTable};
