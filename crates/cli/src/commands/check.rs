//! `sunlyt check`: load a scene without a trajectory and report what was
//! found: OBJ groups, which ones carry panels, each panel's face, grid and
//! wiring, and (with `[electrical]`) how a string in full sun meets the
//! bus. `--svg DIR` draws each panel's layout.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sunlyt_core::env::SOLAR_CONSTANT_W_M2;
use sunlyt_core::geometry::check::{bounds, groups, layout_svg, panel_info};
use sunlyt_core::power::cell::ZERO_C_K;
use sunlyt_core::thermal::ThermalConfig;
use sunlyt_core::{ElectricalConfig, Regulation, Scene};

use super::array::describe_circuit;

#[derive(clap::Args)]
pub struct Args {
    /// Scene TOML.
    scene: PathBuf,
    /// Write one layout drawing per panel (`<panel>.svg`) into this folder.
    #[arg(long)]
    svg: Option<PathBuf>,
}

fn file_safe(name: &str) -> String {
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

pub fn run(args: &Args) -> Result<()> {
    let scene = Scene::load(&args.scene)
        .with_context(|| format!("loading scene {}", args.scene.display()))?;
    let (lo, hi) = bounds(&scene);
    let size = hi - lo;
    println!(
        "mesh        {} triangles, {:.3} × {:.3} × {:.3} m (body X × Y × Z)",
        scene.mesh.triangles.len(),
        size.x,
        size.y,
        size.z
    );
    println!("groups      (OBJ `o`/`g` names; everything that is not a panel casts shadows)");
    for g in groups(&scene) {
        let role = if g.panels.is_empty() {
            "occluder".to_owned()
        } else {
            let names: Vec<&str> = g
                .panels
                .iter()
                .map(|&k| scene.panels[k].name.as_str())
                .collect();
            format!("panel: {}", names.join(", "))
        };
        println!("  {:<16} {:>7} triangles  {role}", g.name, g.triangles);
    }

    for (k, p) in scene.panels.iter().enumerate() {
        let i = panel_info(&scene, k);
        let lens = &i.string_lengths;
        let (smin, smax) = (
            lens.iter().min().copied().unwrap_or(0),
            lens.iter().max().copied().unwrap_or(0),
        );
        println!();
        println!(
            "panel       {} (object {})",
            p.name, scene.mesh.objects[p.object]
        );
        println!(
            "  normal    ({:+.4}, {:+.4}, {:+.4})   cells face this way",
            p.normal.x, p.normal.y, p.normal.z
        );
        println!(
            "  u, v      ({:+.3}, {:+.3}, {:+.3}), ({:+.3}, {:+.3}, {:+.3})   columns run along u, rows along v",
            p.u.x, p.u.y, p.u.z, p.v.x, p.v.y, p.v.z
        );
        println!(
            "  face      {:.4} × {:.4} m, {:.4} m²",
            i.face_m.x, i.face_m.y, p.area_m2
        );
        println!(
            "  grid      {} rows × {} cols = {} cells, {:.4} × {:.4} m, centre offset ({:+.4}, {:+.4}) m, margin {:.4} m",
            p.rows,
            p.cols,
            p.cells.len(),
            i.grid_m.x,
            i.grid_m.y,
            i.grid_offset_m.x,
            i.grid_offset_m.y,
            i.margin_m
        );
        let len = if smin == smax {
            format!("{smin}")
        } else {
            format!("{smin}–{smax}")
        };
        println!("  strings   {} of {len} cells", lens.len());
    }

    let dir = args.scene.parent().unwrap_or(Path::new("."));
    println!();
    match ElectricalConfig::from_scene_file(&args.scene)? {
        None => println!(
            "electrical  none: constant efficiency (add [electrical] for cells and strings)"
        ),
        Some(cfg) => {
            let (cell, circuit) = cfg.build(dir).context("[electrical]")?;
            describe_circuit(&scene, &cell, &circuit);
            // One string of each length in full normal sun at the
            // configured temperature: does it reach the bus?
            let mut seen = Vec::new();
            for st in &scene.strings {
                let n = st.cells.len();
                if seen.contains(&n) {
                    continue;
                }
                seen.push(n);
                let area = scene.cells[st.cells[0]].area_m2;
                let c = circuit.for_cell_area(area);
                let groups = [(SOLAR_CONSTANT_W_M2, n as u32)];
                let op = c.solve(&groups);
                let mpp = sunlyt_core::Circuit {
                    regulation: Regulation::Mppt,
                    ..c
                }
                .solve(&groups);
                println!(
                    "full sun    {n}-cell string at {:.1} °C: {:.3} A at {:.2} V = {:.2} W; its maximum is {:.2} W at {:.2} V{}",
                    c.cell.temperature_k - ZERO_C_K,
                    op.current_a,
                    op.voltage_v,
                    op.power_w,
                    mpp.power_w,
                    mpp.voltage_v,
                    match circuit.regulation {
                        Regulation::FixedBus { bus_v }
                            if mpp.voltage_v < bus_v + circuit.blocking_vf_v =>
                            "  ⚠ Vmp is below bus + blocking diode: add cells in series",
                        _ => "",
                    }
                );
                // A string whose Vmp sits far above the bus works on its
                // current plateau and wastes the rest: say how much, and
                // the length that would put Vmp just above the bus.
                if let Regulation::FixedBus { bus_v } = circuit.regulation {
                    let need = bus_v + circuit.blocking_vf_v;
                    let share = op.power_w / mpp.power_w;
                    if mpp.voltage_v > need && share < 0.8 {
                        let per_cell = mpp.voltage_v / n as f64;
                        let k = (need / per_cell).ceil() as usize;
                        println!(
                            "  note      the bus takes {:.0} % of that maximum: at {:.1} °C about {k} cells \
                             per string ({:.2} V each at Vmp) would put Vmp just above {need:.1} V",
                            100.0 * share,
                            c.cell.temperature_k - ZERO_C_K,
                            per_cell
                        );
                    }
                }
            }
        }
    }
    match ThermalConfig::from_scene_file(&args.scene)? {
        Some(_) => println!("thermal     [thermal] present: panel temperatures are integrated"),
        None => println!("thermal     none: cells at [electrical] temperature_c"),
    }

    if let Some(out) = &args.svg {
        std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
        println!();
        for (k, p) in scene.panels.iter().enumerate() {
            let path = out.join(format!("{}.svg", file_safe(&p.name)));
            std::fs::write(&path, layout_svg(&scene, k))
                .with_context(|| format!("writing {}", path.display()))?;
            println!("wrote       {}", path.display());
        }
    }
    Ok(())
}
