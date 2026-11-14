//! Slice-6 gate: the plots' CSV export holds the batch CSV's numbers, in
//! the same columns and formats. Synthetic data only (no mission data).

use std::collections::HashMap;

use glam::DVec3;
use sunlyt_core::geometry::shapes::ObjWriter;
use sunlyt_core::geometry::{Mesh, PanelLayout, SceneConfig};
use sunlyt_core::output::write_array_csv;
use sunlyt_core::thermal::ThermalConfig;
use sunlyt_core::{ArrayModel, ArrayRun, CsvMapping, EnvConfig, RayCaster, Scene, StateTable};
use sunlyt_viewer::plots::export::write_plot_csv;

/// Fixed attitude; the craft orbits the Moon at 100 km in the Y–Z plane
/// with the Sun along +Y, so the run has eclipses.
fn table(rows: usize) -> StateTable {
    let mut s = String::from(
        "Time (UTCG),EarthJ2000-x (km),EarthJ2000-y (km),EarthJ2000-z (km),\
         J2000-qx,J2000-qy,J2000-qz,J2000-qs,\
         EarthBdyX (km),EarthBdyY (km),EarthBdyZ (km),\
         MoonBdyX (km),MoonBdyY (km),MoonBdyZ (km),\
         SunBdyX (km),SunBdyY (km),SunBdyZ (km)\n",
    );
    let (r, mu) = (1837.4f64, 4902.8f64);
    let n = (mu / r.powi(3)).sqrt();
    for k in 0..rows {
        let t = k as f64 * 120.0;
        let sc = DVec3::new(0.0, (n * t).cos(), (n * t).sin()) * r;
        let (e, m, su) = (
            DVec3::new(-384_400.0, 0.0, 0.0) - sc,
            -sc,
            DVec3::Y * 1.496e8 - sc,
        );
        let (h, mi) = (k * 2 / 60, k * 2 % 60);
        s.push_str(&format!(
            "1/1/2030 {h}:{mi:02},{},{},{},0,0,0,1,{},{},{},{},{},{},{},{},{}\n",
            -e.x, -e.y, -e.z, e.x, e.y, e.z, m.x, m.y, m.z, su.x, su.y, su.z
        ));
    }
    StateTable::from_csv_reader(s.as_bytes(), &CsvMapping::stk_body_vectors()).unwrap()
}

fn scene() -> Scene {
    let mut w = ObjWriter::new();
    w.rect("P", DVec3::ZERO, DVec3::Z * 0.2, DVec3::X * 0.2);
    let mesh = Mesh::from_obj_reader(&mut w.finish().as_bytes(), 1.0).unwrap();
    let panel = PanelLayout {
        name: Some("+Y wing".into()),
        object: "P".into(),
        normal: Some([0.0, 1.0, 0.0]),
        rows: 4,
        cols: 4,
        cell: [0.08, 0.04],
        gap: [0.0, 0.0],
        u_axis: Some([1.0, 0.0, 0.0]),
        offset: [0.0, 0.0],
        strings: None,
    };
    let cfg = SceneConfig {
        obj: String::new(),
        scale: 1.0,
        samples: 1,
        panels: vec![panel],
    };
    Scene::build(mesh, &cfg).unwrap()
}

fn columns(csv: &str) -> HashMap<String, Vec<String>> {
    let mut lines = csv.lines();
    let head: Vec<&str> = lines.next().unwrap().split(',').collect();
    let mut cols: HashMap<String, Vec<String>> = HashMap::new();
    for l in lines {
        for (h, v) in head.iter().zip(l.split(',')) {
            cols.entry((*h).to_owned()).or_default().push(v.to_owned());
        }
    }
    cols
}

#[test]
fn plot_export_equals_batch_columns() {
    let tb = table(150);
    let s = scene();
    let rc = RayCaster::new(&s);
    let env = EnvConfig::default();
    let model = ArrayModel::Constant { efficiency: 0.3 };
    let th = ThermalConfig::from_scene_str("[thermal]\n")
        .unwrap()
        .unwrap()
        .build(tb.body_names())
        .unwrap()
        .0;
    let run = ArrayRun::compute_thermal(&tb, &s, &rc, &model, &env, &th);
    assert!(
        run.samples.iter().any(|x| x.sun.visible == 0.0),
        "no eclipse"
    );

    let (mut batch, mut plots) = (Vec::new(), Vec::new());
    write_array_csv(&mut batch, &tb, &s, &run).unwrap();
    write_plot_csv(&mut plots, &tb, &s, &run).unwrap();
    let (batch, plots) = (
        columns(&String::from_utf8(batch).unwrap()),
        columns(&String::from_utf8(plots).unwrap()),
    );
    let names = [
        "t_s",
        "utc",
        "visible",
        "power_w",
        "power_w_+Y_wing",
        "temp_c_+Y_wing",
    ];
    assert_eq!(plots.len(), names.len());
    for name in names {
        assert_eq!(plots[name], batch[name], "column {name}");
    }
}
