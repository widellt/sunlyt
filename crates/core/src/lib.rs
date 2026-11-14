//! Headless core of `sunlyt`: trajectory input, solar environment, power
//! models and mission runs. No GPU or UI dependencies, so the CLI, the
//! viewer and tests all share one source of truth.
//!
//! Layout (each folder is one concern; later slices add folders, not
//! cross-links):
//! - [`trajectory`] — CSV mapping, state table, interpolation
//! - [`env`] — irradiance and eclipses → [`Sunlight`]
//! - [`geometry`] — OBJ mesh, panels, cell grid
//! - [`shadow`] — self-shadowing (BVH ray caster)
//! - [`power`] — sunlight → electrical power (plate, cells, strings, regulation)
//! - [`thermal`] — panel temperature, planetary albedo and IR
//! - [`sim`] — run a model over a whole mission, in parallel; summaries
//! - [`output`] — write run results (CSV now; Parquet later)
//! - [`math`] — constants and full-precision angle helpers

pub mod env;
pub mod geometry;
pub mod math;
pub mod output;
pub mod power;
pub mod shadow;
pub mod sim;
pub mod thermal;
pub mod trajectory;

pub use env::{EnvConfig, Sunlight, Surface};
pub use geometry::{Scene, SceneConfig};
pub use math::{AU_M, AngleExt};
pub use power::{
    ArrayModel, ArraySample, CellDatasheet, CellModel, Circuit, ElectricalConfig, FlatPlate,
    PlateSample, Regulation,
};
pub use shadow::exact::ExactClipper;
pub use shadow::{Illuminator, RayCaster};
pub use sim::{ArrayRun, PlateRun, RunSummary};
pub use trajectory::{
    BodyState, CsvMapping, Epoch, InputError, Residual, StateTable, SynthAttitude, SynthOrbit,
};
