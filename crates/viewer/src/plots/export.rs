//! Plots as a table: exactly the numbers drawn, in the batch CSV's column
//! names and number formats, so the export can be diffed against
//! `sunlyt array -o`.

use std::io::{self, Write};

use sunlyt_core::output::csv::column_safe;
use sunlyt_core::power::cell::ZERO_C_K;
use sunlyt_core::{ArrayRun, Scene, StateTable};

/// `t_s, utc, visible, power_w`, then per panel `power_w_<name>` and (with
/// temperatures) `temp_c_<name>`.
pub fn write_plot_csv<W: Write>(
    w: W,
    table: &StateTable,
    scene: &Scene,
    run: &ArrayRun,
) -> io::Result<()> {
    let mut w = io::BufWriter::new(w);
    let temps = run
        .samples
        .first()
        .is_some_and(|s| !s.panel_temperature_k.is_empty());
    write!(w, "t_s,utc,visible,power_w")?;
    for p in &scene.panels {
        let n = column_safe(&p.name);
        write!(w, ",power_w_{n}")?;
        if temps {
            write!(w, ",temp_c_{n}")?;
        }
    }
    writeln!(w)?;
    for (&t, s) in run.t.iter().zip(&run.samples) {
        write!(
            w,
            "{t},{},{:.9},{:.6}",
            table.datetime(t).format("%Y-%m-%dT%H:%M:%S"),
            s.sun.visible,
            s.power_w
        )?;
        for k in 0..scene.panels.len() {
            write!(w, ",{:.6}", s.panel_power_w[k])?;
            if temps {
                write!(w, ",{:.3}", s.panel_temperature_k[k] - ZERO_C_K)?;
            }
        }
        writeln!(w)?;
    }
    w.flush()
}
