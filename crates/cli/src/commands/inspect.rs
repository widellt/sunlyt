//! `sunlyt inspect`: load a trajectory and print a summary and consistency checks.

use std::path::Path;

use anyhow::Result;
use sunlyt_core::{AU_M, StateTable};

use super::load_mapping;

pub fn run(csv: &Path, mapping: &str) -> Result<()> {
    let map = load_mapping(mapping)?;
    let table = StateTable::from_csv_path(csv, &map)?;
    let t = table.times();
    let n = table.len();
    println!("rows        {n}");
    println!(
        "span        {} → {}  ({:.3} days)",
        table.epoch(),
        table.datetime(table.duration()),
        table.duration() / 86_400.0
    );
    let (mut dmin, mut dmax) = (f64::INFINITY, 0f64);
    for w in t.windows(2) {
        let d = w[1] - w[0];
        dmin = dmin.min(d);
        dmax = dmax.max(d);
    }
    if n > 1 {
        println!("step        {dmin} s … {dmax} s");
    }

    let rows: Vec<_> = (0..n).map(|i| table.row(i)).collect();
    let range = |f: &dyn Fn(usize) -> f64| {
        (0..n).fold((f64::INFINITY, 0f64), |(lo, hi), i| {
            let v = f(i);
            (lo.min(v), hi.max(v))
        })
    };
    let (lo, hi) = range(&|i| rows[i].sun_b.length() / AU_M);
    println!("sun dist    {lo:.5} … {hi:.5} AU");
    for (b, name) in table.body_names().enumerate() {
        let (lo, hi) = range(&|i| rows[i].bodies[b].vec_b.length() / 1000.0);
        let (alo, ahi) = range(&|i| rows[i].bodies[b].altitude_m() / 1000.0);
        let body = &rows[0].bodies[b];
        let shape = if body.is_sphere() {
            format!("sphere r={:.3} km", body.equatorial_radius_m / 1000.0)
        } else {
            format!(
                "spheroid a={:.3} b={:.3} km",
                body.equatorial_radius_m / 1000.0,
                body.polar_radius_m / 1000.0
            )
        };
        println!(
            "{name:<11} {lo:.1} … {hi:.1} km from centre, alt {alo:.1} … {ahi:.1} km  [{shape}]"
        );
    }
    match table.attitude_residual() {
        Some(r) => println!(
            "attitude    residual max {:.3} km (rel {:.1e}, row {}), rms {:.3} km  → {}",
            r.max_m / 1000.0,
            r.max_rel,
            r.worst_row + 1,
            r.rms_m / 1000.0,
            if r.max_rel < 1e-4 {
                "convention OK"
            } else {
                "CHECK QUATERNION CONVENTION"
            }
        ),
        None => println!("attitude    no position columns to cross-check against"),
    }
    Ok(())
}
