//! Per-row CSV output.

use std::io::{self, Write};

use crate::geometry::Scene;
use crate::math::AU_M;
use crate::sim::{ArrayRun, PlateRun};
use crate::trajectory::StateTable;

/// One row per table row: time, Sun distance, irradiance, incidence,
/// visible disk fraction, per-body occulted fraction, power.
pub fn write_plate_csv<W: Write>(w: W, table: &StateTable, run: &PlateRun) -> io::Result<()> {
    let mut w = io::BufWriter::new(w);
    write!(
        w,
        "t_s,utc,sun_dist_au,irradiance_w_m2,cos_incidence,visible"
    )?;
    for n in table.body_names() {
        write!(w, ",occulted_{n}")?;
    }
    writeln!(w, ",power_w")?;
    for (&t, s) in run.t.iter().zip(&run.samples) {
        write!(
            w,
            "{t},{},{:.9},{:.6},{:.9},{:.9}",
            table.datetime(t).format("%Y-%m-%dT%H:%M:%S"),
            s.sun.sun_dist_m / AU_M,
            s.sun.irradiance_w_m2,
            s.cos_incidence,
            s.sun.visible
        )?;
        for f in &s.sun.occulted {
            write!(w, ",{f:.9}")?;
        }
        writeln!(w, ",{:.6}", s.power_w)?;
    }
    w.flush()
}

/// One row per table row: time, Sun distance, irradiance, visible disk
/// fraction, per-body occulted fraction, total power, then per panel its
/// power and sunlyt fraction. With the circuit model, each panel also gets
/// its total string current (A) into the bus; whenever cell temperatures
/// are known (circuit or thermal model), each panel gets `temp_c`.
pub fn write_array_csv<W: Write>(
    w: W,
    table: &StateTable,
    scene: &Scene,
    run: &ArrayRun,
) -> io::Result<()> {
    let mut w = io::BufWriter::new(w);
    let circuit = run
        .samples
        .first()
        .is_some_and(|s| !s.string_current_a.is_empty());
    let temps = run
        .samples
        .first()
        .is_some_and(|s| !s.panel_temperature_k.is_empty());
    write!(w, "t_s,utc,sun_dist_au,irradiance_w_m2,visible")?;
    for n in table.body_names() {
        write!(w, ",occulted_{n}")?;
    }
    write!(w, ",power_w")?;
    for p in &scene.panels {
        let n = column_safe(&p.name);
        write!(w, ",power_w_{n},sunlyt_{n}")?;
        if circuit {
            write!(w, ",current_a_{n}")?;
        }
        if temps {
            write!(w, ",temp_c_{n}")?;
        }
    }
    writeln!(w)?;
    for (&t, s) in run.t.iter().zip(&run.samples) {
        write!(
            w,
            "{t},{},{:.9},{:.6},{:.9}",
            table.datetime(t).format("%Y-%m-%dT%H:%M:%S"),
            s.sun.sun_dist_m / AU_M,
            s.sun.irradiance_w_m2,
            s.sun.visible
        )?;
        for f in &s.sun.occulted {
            write!(w, ",{f:.9}")?;
        }
        write!(w, ",{:.6}", s.power_w)?;
        for (k, (p, f)) in s.panel_power_w.iter().zip(&s.panel_sunlyt).enumerate() {
            write!(w, ",{p:.6},{f:.6}")?;
            if circuit {
                let i: f64 = s.string_current_a[scene.panels[k].strings.clone()]
                    .iter()
                    .sum();
                write!(w, ",{i:.6}")?;
            }
            if temps {
                write!(w, ",{:.3}", s.panel_temperature_k[k] - 273.15)?;
            }
        }
        writeln!(w)?;
    }
    w.flush()
}

/// Panel names as CSV column suffixes: letters, digits, `+`, `-`, `_`.
pub fn column_safe(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "+-_".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}
