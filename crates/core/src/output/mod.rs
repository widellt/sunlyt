//! Writing run results. CSV now; Parquet behind a feature flag later.

pub mod csv;

pub use self::csv::{write_array_csv, write_plate_csv};
