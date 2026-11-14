//! `sunlyt view`: the interactive 3D viewer on the same inputs as
//! `sunlyt array`. Built only with the `viewer` cargo feature (default).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sunlyt_core::thermal::ThermalConfig;
use sunlyt_core::{ArrayModel, ElectricalConfig, Scene};

use super::TrajectoryArgs;

#[derive(clap::Args)]
pub struct Args {
    #[command(flatten)]
    traj: TrajectoryArgs,
    /// Scene TOML, as for `sunlyt array`.
    #[arg(long)]
    scene: PathBuf,
    /// Use a constant conversion efficiency (0–1) instead of the circuit.
    #[arg(long)]
    efficiency: Option<f64>,
}

pub fn run(args: &Args) -> Result<()> {
    let table = args.traj.load()?;
    let env = args.traj.env(&table)?;
    let scene = Scene::load(&args.scene)
        .with_context(|| format!("loading scene {}", args.scene.display()))?;
    let model = match (
        args.efficiency,
        ElectricalConfig::from_scene_file(&args.scene)?,
    ) {
        (Some(efficiency), _) => ArrayModel::Constant { efficiency },
        (None, None) => ArrayModel::Constant { efficiency: 0.3 },
        (None, Some(cfg)) => {
            let dir = args.scene.parent().unwrap_or(Path::new("."));
            ArrayModel::Circuit(cfg.build(dir).context("[electrical]")?.1)
        }
    };
    let thermal = match ThermalConfig::from_scene_file(&args.scene)? {
        Some(cfg) => Some(cfg.build(table.body_names())?.0),
        None => None,
    };
    let source = args.scene.file_name().map_or_else(
        || args.scene.display().to_string(),
        |n| n.to_string_lossy().into(),
    );
    sunlyt_viewer::run(sunlyt_viewer::Inputs {
        table,
        scene,
        model,
        env,
        thermal,
        source,
    })
}
