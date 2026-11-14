//! `sunlyt` command-line front end. Each subcommand lives in `commands/`.

mod commands;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Spacecraft solar-array power simulator.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Load a trajectory CSV and print a summary and consistency checks.
    Inspect {
        /// Trajectory CSV.
        csv: PathBuf,
        /// Column mapping: a built-in preset name or a path to a TOML file.
        #[arg(long, default_value = "stk-body-vectors")]
        mapping: String,
    },
    /// Power of one flat panel (no self-shadowing) over the trajectory,
    /// with eclipses. Writes one CSV row per table row.
    Plate(commands::plate::Args),
    /// Power of the cells on an OBJ spacecraft, with self-shadowing and
    /// eclipses, through strings and the cell I-V model when the scene has
    /// an `[electrical]` table.
    Array(commands::array::Args),
    /// Fit a solar-cell datasheet and print (or write) its I-V curve.
    Cell(commands::cell::Args),
    /// Check a scene without a trajectory: OBJ groups, panels, cell grids,
    /// strings and the bus; `--svg` draws each panel's layout.
    Check(commands::check::Args),
    /// Write a synthetic lunar-orbit trajectory CSV (no mission data).
    Synth(commands::synth::Args),
    /// Open the interactive 3D viewer on the same inputs as `array`:
    /// cells coloured live, timeline with eclipse bands, synced plots.
    #[cfg(feature = "viewer")]
    View(commands::view::Args),
    /// One flat panel, face on: cells, strings, shadows (rays and exact
    /// clipping), the Sun, an eclipse inset and per-string I-V curves. From a
    /// sandbox TOML, or one panel of a scene at a trajectory instant.
    #[cfg(feature = "viewer")]
    Sandbox(commands::sandbox::Args),
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Inspect { csv, mapping } => commands::inspect::run(&csv, &mapping),
        Command::Plate(args) => commands::plate::run(&args),
        Command::Array(args) => commands::array::run(&args),
        Command::Cell(args) => commands::cell::run(&args),
        Command::Check(args) => commands::check::run(&args),
        Command::Synth(args) => commands::synth::run(&args),
        #[cfg(feature = "viewer")]
        Command::View(args) => commands::view::run(&args),
        #[cfg(feature = "viewer")]
        Command::Sandbox(args) => commands::sandbox::run(&args),
    }
}
