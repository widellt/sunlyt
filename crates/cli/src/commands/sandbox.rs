//! `sunlyt sandbox`: one flat panel, face on, with its cells, strings,
//! shadows, the Sun, an eclipse inset and per-string I-V curves. Built
//! only with the `viewer` cargo feature (default).
//!
//! Two inputs:
//! - a `sandbox.toml` (a panel, the Sun, box occluders), or
//! - `--scene … --panel … --csv … --at …`: one panel of a scene, with
//!   every other triangle as an occluder, at a trajectory instant.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sunlyt_core::EnvConfig;
use sunlyt_viewer::sandbox::{self, model::Sandbox};

use super::load_mapping;

#[derive(clap::Args)]
pub struct Args {
    /// A sandbox TOML (see docs/guide/sandbox.md). Omit with --scene.
    #[arg(conflicts_with_all = ["scene", "panel", "csv", "at"])]
    file: Option<PathBuf>,
    /// From-scene mode: the scene TOML.
    #[arg(long, requires_all = ["panel", "csv"])]
    scene: Option<PathBuf>,
    /// From-scene mode: the panel's `name` (or OBJ object).
    #[arg(long)]
    panel: Option<String>,
    /// From-scene mode: the trajectory CSV.
    #[arg(long)]
    csv: Option<PathBuf>,
    /// Column mapping: a built-in preset name or a path to a TOML file.
    #[arg(long, default_value = "stk-body-vectors")]
    mapping: String,
    /// From-scene mode: the instant, UTC `YYYY-MM-DDTHH:MM:SS`, or seconds
    /// after the first row. Default: the first row.
    #[arg(long)]
    at: Option<String>,
    /// Irradiance at 1 AU, W/m².
    #[arg(long, default_value_t = sunlyt_core::env::SOLAR_CONSTANT_W_M2)]
    solar_constant: f64,
    /// Write cells.csv, strings.csv and iv.csv for the instant into this
    /// directory and exit without opening a window.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Where the window's Save button writes (box mode). Default:
    /// `<file stem>.saved.toml` next to the file.
    #[arg(long)]
    save_to: Option<PathBuf>,
}

pub fn run(args: &Args) -> Result<()> {
    let env = EnvConfig {
        solar_constant_w_m2: args.solar_constant,
        ..Default::default()
    };
    let (sb, save_to) = match (&args.file, &args.scene) {
        (Some(file), None) => {
            let text = std::fs::read_to_string(file)
                .with_context(|| format!("reading {}", file.display()))?;
            let label = file.display().to_string();
            let mut sb = Sandbox::from_toml(&text, file.parent().unwrap_or(Path::new(".")), label)?;
            sb.env = env;
            let save_to = args.save_to.clone().unwrap_or_else(|| {
                let stem = file
                    .file_stem()
                    .map_or("sandbox".into(), |s| s.to_string_lossy());
                file.with_file_name(format!("{stem}.saved.toml"))
            });
            (sb, Some(save_to))
        }
        (None, Some(scene)) => {
            let csv = args.csv.as_ref().expect("clap requires --csv");
            let table = sunlyt_core::StateTable::from_csv_path(csv, &load_mapping(&args.mapping)?)?;
            if table.is_empty() {
                bail!("the trajectory has no rows");
            }
            let t = match &args.at {
                None => table.times()[0],
                Some(s) => match s.parse::<f64>() {
                    Ok(t) => t,
                    Err(_) => {
                        let at = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
                            .with_context(|| {
                                format!("--at {s:?}: expected YYYY-MM-DDTHH:MM:SS or seconds")
                            })?;
                        (at - table.epoch()).num_milliseconds() as f64 / 1000.0
                    }
                },
            };
            let times = table.times();
            if t < times[0] || t > *times.last().unwrap() {
                bail!("--at is outside the trajectory");
            }
            let panel = args.panel.as_deref().expect("clap requires --panel");
            (Sandbox::from_scene(scene, panel, table, env, t)?, None)
        }
        _ => bail!("give a sandbox TOML, or --scene with --panel and --csv"),
    };
    if let Some(dir) = &args.out {
        let ev = sb.evaluate();
        sandbox::export(&sb, &ev, dir).with_context(|| format!("writing {}", dir.display()))?;
        println!(
            "power {:.3} W (rays), {:.3} W (exact shadows); visible disk {:.4}",
            ev.ray.power_w, ev.exact.power_w, ev.ray.sun.visible
        );
        println!("wrote {}/{{cells,strings,iv}}.csv", dir.display());
        return Ok(());
    }
    sandbox::run(sb, save_to)
}
