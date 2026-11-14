//! `sunlyt plate`: power of one flat panel over the trajectory.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use glam::DVec3;
use sunlyt_core::output::write_plate_csv;
use sunlyt_core::{FlatPlate, PlateRun};

use super::{TrajectoryArgs, print_summary, print_timings};

#[derive(clap::Args)]
pub struct Args {
    #[command(flatten)]
    traj: TrajectoryArgs,
    /// Panel outward normal in the body frame, `x,y,z`.
    #[arg(long, default_value = "0,0,1", value_parser = parse_vec3, allow_hyphen_values = true)]
    normal: DVec3,
    /// Panel area, m².
    #[arg(long, default_value_t = 1.0)]
    area: f64,
    /// Conversion efficiency, 0–1.
    #[arg(long, default_value_t = 0.3)]
    efficiency: f64,
    /// Output CSV (default: summary only).
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Print how long each phase took.
    #[arg(long)]
    timings: bool,
}

fn parse_vec3(s: &str) -> Result<DVec3, String> {
    let v: Vec<f64> = s
        .split(',')
        .map(|x| x.trim().parse::<f64>().map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    match v[..] {
        [x, y, z] if x != 0.0 || y != 0.0 || z != 0.0 => Ok(DVec3::new(x, y, z)),
        _ => Err("expected three numbers x,y,z, not all zero".into()),
    }
}

pub fn run(args: &Args) -> Result<()> {
    let t0 = Instant::now();
    let table = args.traj.load()?;
    let env = args.traj.env(&table)?;
    let t_load = t0.elapsed();
    let panel = FlatPlate::new(args.normal, args.area, args.efficiency);

    let t1 = Instant::now();
    let run = PlateRun::compute(&table, &panel, &env);
    let t_compute = t1.elapsed();

    let t2 = Instant::now();
    if let Some(p) = &args.output {
        let f = std::fs::File::create(p).with_context(|| format!("creating {}", p.display()))?;
        write_plate_csv(f, &table, &run)?;
    }
    let t_write = t2.elapsed();

    let n = panel.normal_b;
    println!(
        "panel       normal ({:.3}, {:.3}, {:.3}), {} m², η {}",
        n.x, n.y, n.z, args.area, args.efficiency
    );
    print_summary(&table, &run.summary(table.body_names().count()));
    if let Some(p) = &args.output {
        println!("wrote       {}", p.display());
    }
    if args.timings {
        print_timings(&[("load", t_load), ("compute", t_compute), ("write", t_write)]);
    }
    Ok(())
}
