//! Power models: sunlight on the arrays → electrical power.
//!
//! - `plate`: the single-panel baseline.
//! - `array`: every cell of a scene, at constant efficiency or through the
//!   circuit; reproduces `plate` for one unshaded panel at constant
//!   efficiency.
//! - `cell`: single-diode cell fitted to a datasheet, plus presets.
//! - `circuit`: series strings, bypass and blocking diodes, regulation.

pub mod array;
pub mod cell;
pub mod circuit;
pub mod plate;

pub use array::{ArrayIllum, ArrayModel, ArraySample};
pub use cell::{
    CellAtTemp, CellDatasheet, CellError, CellModel, CellTempRef, DatasheetPoint, DiodeParams,
    MIN_CELL_TEMPERATURE_K,
};
pub use circuit::{Circuit, ElectricalConfig, Regulation, StringPoint};
pub use plate::{FlatPlate, PlateSample};
