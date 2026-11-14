//! `sunlyt array`: cells on an OBJ spacecraft with self-shadowing and
//! eclipses. With an `[electrical]` table in the scene TOML the cells are
//! wired into strings and solved through the I-V model; otherwise (or with
//! `--efficiency`) every cell converts at a constant efficiency. With a
//! `[thermal]` table, each panel's temperature is integrated through the
//! run and the cells are solved at it.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use sunlyt_core::output::write_array_csv;
use sunlyt_core::power::cell::ZERO_C_K;
use sunlyt_core::thermal::ThermalConfig;
use sunlyt_core::{ArrayModel, ArrayRun, ElectricalConfig, RayCaster, Regulation, Scene};

use super::{TrajectoryArgs, print_summary, print_timings};

#[derive(clap::Args)]
pub struct Args {
    #[command(flatten)]
    traj: TrajectoryArgs,
    /// Scene TOML: OBJ path, panels, cell layout and wiring, and optionally
    /// `[electrical]` (cell preset, temperature, regulation).
    #[arg(long)]
    scene: PathBuf,
    /// Use a constant conversion efficiency (0–1) instead of the circuit.
    /// Default 0.3 when the scene has no `[electrical]` table.
    #[arg(long)]
    efficiency: Option<f64>,
    /// Output CSV (default: summary only).
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Print how long each phase took.
    #[arg(long)]
    timings: bool,
}

pub fn run(args: &Args) -> Result<()> {
    let t0 = Instant::now();
    let table = args.traj.load()?;
    let env = args.traj.env(&table)?;
    let t_load = t0.elapsed();

    let t1 = Instant::now();
    let scene = Scene::load(&args.scene)
        .with_context(|| format!("loading scene {}", args.scene.display()))?;
    let rc = RayCaster::new(&scene);
    let electrical = ElectricalConfig::from_scene_file(&args.scene)?;
    let model = match (args.efficiency, electrical) {
        (Some(efficiency), _) => ArrayModel::Constant { efficiency },
        (None, None) => ArrayModel::Constant { efficiency: 0.3 },
        (None, Some(cfg)) => {
            let dir = args.scene.parent().unwrap_or(Path::new("."));
            let (cell, circuit) = cfg.build(dir).context("[electrical]")?;
            describe_circuit(&scene, &cell, &circuit);
            ArrayModel::Circuit(circuit)
        }
    };
    let thermal = match ThermalConfig::from_scene_file(&args.scene)? {
        None => None,
        Some(cfg) => {
            let (m, unknown) = cfg.build(table.body_names())?;
            let p = &m.panel;
            println!(
                "thermal     C {} J/(m²·K), front α {} ε {}, back {}",
                p.heat_capacity_j_m2_k,
                p.absorptance_front,
                p.emittance_front,
                match p.back_sink_k {
                    Some(t) => format!("ε {} to {:.1} °C", p.emittance_back, t - ZERO_C_K),
                    None => format!("α {} ε {} to space", p.absorptance_back, p.emittance_back),
                }
            );
            if !unknown.is_empty() {
                println!(
                    "note        no albedo/IR model for {} (built in: Moon, Earth)",
                    unknown.join(", ")
                );
            }
            Some(m)
        }
    };
    let t_scene = t1.elapsed();

    let t2 = Instant::now();
    let run = match &thermal {
        Some(th) => ArrayRun::compute_thermal(&table, &scene, &rc, &model, &env, th),
        None => ArrayRun::compute(&table, &scene, &rc, &model, &env),
    };
    let t_compute = t2.elapsed();

    let t3 = Instant::now();
    if let Some(p) = &args.output {
        let f = std::fs::File::create(p).with_context(|| format!("creating {}", p.display()))?;
        write_array_csv(f, &table, &scene, &run)?;
    }
    let t_write = t3.elapsed();

    println!(
        "scene       {} panels, {} cells ({:.3} m²), {} rays/cell, {} triangles",
        scene.panels.len(),
        scene.cells.len(),
        scene.cell_area_m2(),
        scene.samples_per_cell,
        scene.mesh.triangles.len()
    );
    if let ArrayModel::Constant { efficiency } = model {
        println!("model       constant efficiency {efficiency}");
    }
    print_summary(&table, &run.summary(table.body_names().count()));
    for (k, (p, e)) in scene.panels.iter().zip(run.panel_energy_wh()).enumerate() {
        if thermal.is_some() {
            let temps = run
                .samples
                .iter()
                .map(|s| s.panel_temperature_k[k] - ZERO_C_K);
            let (lo, hi) = temps
                .clone()
                .fold((f64::MAX, f64::MIN), |(a, b), t| (a.min(t), b.max(t)));
            let mean = temps.sum::<f64>() / run.samples.len().max(1) as f64;
            println!(
                "  {:<10} {e:.3} Wh, {lo:.1} … {hi:.1} °C (mean {mean:.1})",
                p.name
            );
        } else {
            println!("  {:<10} {e:.3} Wh", p.name);
        }
    }
    if let Some(p) = &args.output {
        println!("wrote       {}", p.display());
    }
    if args.timings {
        print_timings(&[
            ("load", t_load),
            ("scene", t_scene),
            ("compute", t_compute),
            ("write", t_write),
        ]);
    }
    Ok(())
}

pub(super) fn describe_circuit(
    scene: &Scene,
    cell: &sunlyt_core::CellModel,
    circuit: &sunlyt_core::Circuit,
) {
    let reg = match circuit.regulation {
        Regulation::FixedBus { bus_v } => format!("fixed {bus_v} V bus"),
        Regulation::Mppt => "ideal MPPT per string".into(),
    };
    let lens = scene.strings.iter().map(|s| s.cells.len());
    let (lo, hi) = (lens.clone().min().unwrap_or(0), lens.max().unwrap_or(0));
    let len = if lo == hi {
        format!("{lo}")
    } else {
        format!("{lo}–{hi}")
    };
    println!(
        "cell        {} ({}), {:.1} °C",
        cell.name,
        cell.condition.name,
        circuit.cell.temperature_k - ZERO_C_K
    );
    println!(
        "circuit     {} strings of {len} cells, {reg}, bypass {} V, blocking {} V",
        scene.strings.len(),
        circuit.bypass_vf_v,
        circuit.blocking_vf_v
    );
    // Layout cells of another size are scaled to their own area. For a
    // per-cm² cell file that is the intent; otherwise it usually means the
    // layout and the datasheet disagree, so say so.
    if let Some(c) = scene.cells.first() {
        let ratio = c.area_m2 / cell.area_m2;
        if (cell.area_m2 * 1e4 - 1.0).abs() < 1e-9 {
            println!(
                "cell size   per-cm² cell file, scaled to the layout's {:.2} cm² cells",
                c.area_m2 * 1e4
            );
        } else if (ratio - 1.0).abs() > 1e-3 {
            println!(
                "note        layout cells are {:.2} cm², the datasheet cell {:.2} cm²: \
                 current scaled by {ratio:.3}",
                c.area_m2 * 1e4,
                cell.area_m2 * 1e4
            );
        }
    }
}
