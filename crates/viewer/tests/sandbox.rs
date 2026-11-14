//! Slice-8 gates for the panel sandbox. Synthetic data only.
//!
//! - From-scene mode: a panel's power equals the full simulation's power
//!   for that panel at the same instant (same rays, same circuit; the
//!   sandbox only drops the other panels' cells).
//! - Box mode: the example files load; the ray caster and exact clipping
//!   agree within the sampling bound; Save round-trips.
//! - Every sandbox TOML in docs/guide and examples/ parses.

use std::path::{Path, PathBuf};

use glam::DVec3;
use sunlyt_core::{
    ArrayModel, ArrayRun, CsvMapping, ElectricalConfig, EnvConfig, RayCaster, Scene, StateTable,
    SynthAttitude, SynthOrbit, sim::array_at,
};
use sunlyt_viewer::sandbox::config::{SandboxConfig, with_sun};
use sunlyt_viewer::sandbox::model::{Sandbox, Source};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Two hours of β = 0 lunar orbit (an eclipse every orbit) with the Sun
/// off the wings' normal, so the mast and bus shade the wings.
fn trajectory() -> StateTable {
    let orbit = SynthOrbit {
        start: chrono::NaiveDateTime::parse_from_str("2030-01-01T00:00:00", "%Y-%m-%dT%H:%M:%S")
            .unwrap(),
        duration_s: 2.0 * 3600.0,
        step_s: 60.0,
        altitude_km: 100.0,
        beta_deg: 0.0,
        attitude: SynthAttitude::SunPointing {
            axis: DVec3::new(0.35, -0.45, 0.82).normalize(),
        },
    };
    let mut csv = Vec::new();
    orbit.write_csv(&mut csv).unwrap();
    StateTable::from_csv_reader(csv.as_slice(), &CsvMapping::stk_body_vectors()).unwrap()
}

#[test]
fn from_scene_power_equals_full_simulation() {
    let scene_path = root().join("examples/boxwing/scene.toml");
    let table = trajectory();
    let env = EnvConfig::default();
    let scene = Scene::load(&scene_path).unwrap();
    let cfg = ElectricalConfig::from_scene_file(&scene_path)
        .unwrap()
        .unwrap();
    let model = ArrayModel::Circuit(cfg.build(&root().join("examples/boxwing")).unwrap().1);
    let run = ArrayRun::compute(&table, &scene, &RayCaster::new(&scene), &model, &env);

    let (mut checked, mut shaded, mut penumbra) = (0, 0, 0);
    let mut worst_exact_rel: f64 = 0.0;
    for (p, name) in ["+Y wing", "-Y wing"].iter().enumerate() {
        let mut sb =
            Sandbox::from_scene(&scene_path, name, table.clone(), env.clone(), 0.0).unwrap();
        for (i, &t) in table.times().iter().enumerate() {
            let s = &run.samples[i];
            let in_penumbra = s.sun.visible > 0.0 && s.sun.visible < 1.0;
            if i % 6 != 0 && !in_penumbra {
                continue;
            }
            let Source::Trajectory { t: st, .. } = &mut sb.source else {
                unreachable!()
            };
            *st = t;
            let ev = sb.evaluate();
            let want = s.panel_power_w[p];
            assert_eq!(
                ev.ray.power_w, want,
                "{name} row {i}: sandbox {} W vs full run {want} W",
                ev.ray.power_w
            );
            checked += 1;
            penumbra += in_penumbra as usize;
            if ev.lit_ray.iter().any(|&f| f > 0.0 && f < 1.0) {
                shaded += 1;
            }
            if want > 1.0 {
                worst_exact_rel = worst_exact_rel.max((ev.exact.power_w - want).abs() / want);
            }
        }
    }
    // Penumbrae last seconds at 100 km, so 60 s rows miss them. Find each
    // one between rows by bisection on the visible disk and compare with
    // the full simulation at that instant (`array_at`, the viewer's path).
    let rays = RayCaster::new(&scene);
    let visible = |t: f64| {
        array_at(&table, &scene, &rays, &model, &env, t, None)
            .unwrap()
            .0
    };
    let times = table.times();
    for w in run.samples.windows(2).zip(times.windows(2)) {
        let ([a, b], [ta, tb]) = (w.0, w.1) else {
            unreachable!()
        };
        if (a.sun.visible > 0.5) == (b.sun.visible > 0.5) {
            continue;
        }
        let (mut lo, mut hi) = (*ta, *tb);
        let rising = a.sun.visible < 0.5;
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if (visible(mid).sun.visible > 0.5) == rising {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        let full = visible(lo);
        for (p, name) in ["+Y wing", "-Y wing"].iter().enumerate() {
            let sb =
                Sandbox::from_scene(&scene_path, name, table.clone(), env.clone(), lo).unwrap();
            let ev = sb.evaluate();
            assert_eq!(ev.ray.sun.visible, full.sun.visible);
            assert_eq!(ev.ray.power_w, full.panel_power_w[p], "{name} at t = {lo}");
            penumbra += 1;
        }
        eprintln!(
            "penumbra at t = {lo:.3} s: visible {:.4}, panels {:.3} W / {:.3} W",
            full.sun.visible, full.panel_power_w[0], full.panel_power_w[1]
        );
    }
    eprintln!(
        "from scene: {checked} row instants equal to the full run ({shaded} with partly \
         shaded cells), {penumbra} panel-penumbra instants equal; exact-shadow power within \
         {:.3} % of the rays",
        100.0 * worst_exact_rel
    );
    assert!(shaded > 0, "the test never shades a cell partly");
    assert!(penumbra > 0, "the test has no penumbra instant");
}

#[test]
fn box_mode_examples_load_and_agree_with_exact() {
    for file in ["sandbox.toml", "penumbra.toml"] {
        let path = root().join("examples/sandbox").join(file);
        let text = std::fs::read_to_string(&path).unwrap();
        let sb = Sandbox::from_toml(&text, path.parent().unwrap(), file.into()).unwrap();
        let ev = sb.evaluate();
        let n = (sb.scene.samples_per_cell as f64).sqrt();
        let mut partial_strings = 0;
        for s in &sb.scene.strings {
            let lit: Vec<f64> = s.cells.iter().map(|&c| ev.lit_exact[c]).collect();
            if lit.iter().any(|&f| f < 1.0 - 1e-9) && lit.iter().any(|&f| f > 1.0 - 1e-9) {
                partial_strings += 1;
            }
        }
        for (k, (&r, &e)) in ev.lit_ray.iter().zip(&ev.lit_exact).enumerate() {
            assert!(
                (r - e).abs() <= 1.0 / n + 1e-12,
                "{file} cell {k}: {r} vs {e}"
            );
        }
        let rel = (ev.ray.power_w - ev.exact.power_w).abs() / ev.exact.power_w;
        eprintln!(
            "{file}: {:.3} W (rays) vs {:.3} W (exact), visible disk {:.4}, {} shadow polygons, \
             {partial_strings} partly shaded strings, {} distinct I-V curves",
            ev.ray.power_w,
            ev.exact.power_w,
            ev.ray.sun.visible,
            ev.shadows.len(),
            ev.curves.len()
        );
        assert!(partial_strings > 0, "{file}: no partly shaded string");
        assert!(rel < 0.02, "{file}: rays vs exact power {rel}");
        // Every operating point lies on its curve's voltage range.
        for c in &ev.curves {
            let vmax = c.points[0].1;
            assert!(c.op.voltage_v <= vmax + 1e-9);
        }
        if file == "penumbra.toml" {
            let v = ev.ray.sun.visible;
            assert!(v > 0.3 && v < 0.5, "penumbra example: visible {v}");
        } else {
            assert_eq!(ev.ray.sun.visible, 1.0);
        }
    }
}

#[test]
fn save_round_trips_the_sun() {
    let path = root().join("examples/sandbox/sandbox.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    let saved = with_sun(&text, 123.25, 33.5).unwrap();
    let cfg = SandboxConfig::from_str(&saved).unwrap();
    assert_eq!(cfg.sun.azimuth_deg, 123.25);
    assert_eq!(cfg.sun.elevation_deg, 33.5);
    let before = SandboxConfig::from_str(&text).unwrap();
    assert_eq!(cfg.boxes, before.boxes);
    assert_eq!(cfg.panel, before.panel);
    assert_eq!(
        ElectricalConfig::from_scene_str(&saved).unwrap(),
        ElectricalConfig::from_scene_str(&text).unwrap()
    );
}

#[test]
fn sandbox_rejects_scene_only_keys() {
    let bad = "size = [1, 1]\n[panel]\nobject = \"X\"\nrows = 1\ncols = 1\ncell = [0.1, 0.1]\n";
    let e = SandboxConfig::from_str(bad).unwrap_err();
    assert!(e.contains("object"), "{e}");
}

/// Sandbox TOML in the guide and examples (files and ```toml blocks with a
/// top-level `size`) parses through the real types.
#[test]
fn sandbox_docs_parse() {
    let mut texts: Vec<(String, String)> = Vec::new();
    for dir in ["docs/guide", "examples"] {
        for entry in walk(&root().join(dir)) {
            let s = std::fs::read_to_string(&entry).unwrap();
            let name = entry.display().to_string();
            if entry.extension().is_some_and(|e| e == "toml") {
                texts.push((name, s));
            } else if entry.extension().is_some_and(|e| e == "md") {
                let mut cur: Option<String> = None;
                for line in s.lines() {
                    match &mut cur {
                        Some(b) if line.trim_start().starts_with("```") => {
                            texts.push((name.clone(), std::mem::take(b)));
                            cur = None;
                        }
                        Some(b) => {
                            b.push_str(line);
                            b.push('\n');
                        }
                        None if line.trim_start().starts_with("```toml") => {
                            cur = Some(String::new())
                        }
                        None => {}
                    }
                }
            }
        }
    }
    let mut n = 0;
    for (name, text) in texts {
        let Ok(table) = toml::from_str::<toml::Table>(&text) else {
            continue;
        };
        if !table.contains_key("size") {
            continue;
        }
        SandboxConfig::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        n += 1;
    }
    eprintln!("{n} sandbox TOML documents parsed");
    assert!(n >= 3, "expected the two examples and the guide's block");
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}
