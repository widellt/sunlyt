//! Speed gate: ~10,000 cells × 12,000 steps in under 10 s, for every
//! power model, with and without the thermal model.
//!
//! Fully synthetic (no mission data): a bus with four 1 m wings, a dish
//! and a mast, in a 100 km-altitude lunar orbit with a spinning attitude.
//! Heavy, so it is opt-in:
//!
//! ```text
//! cargo test --release -p sunlyt-core --test array_speed -- --ignored --nocapture
//! ```

use std::path::Path;
use std::time::Instant;

use chrono::{NaiveDate, TimeDelta};
use glam::{DQuat, DVec3};
use sunlyt_core::geometry::shapes::ObjWriter;
use sunlyt_core::geometry::{Mesh, PanelLayout, SceneConfig};
use sunlyt_core::{
    ArrayModel, ArrayRun, CsvMapping, ElectricalConfig, EnvConfig, RayCaster, Scene, StateTable,
    thermal::ThermalConfig,
};

const ROWS: usize = 12_000;

/// STK-style CSV: body spins about +X at 6°/min; the Sun stays in the body
/// Y–Z plane; the orbit plane contains the Sun, so every orbit eclipses.
fn synthetic_csv() -> String {
    let mut s = String::from(
        "Time (UTCG),EarthJ2000-x (km),EarthJ2000-y (km),EarthJ2000-z (km),\
         J2000-qx,J2000-qy,J2000-qz,J2000-qs,\
         EarthBdyX (km),EarthBdyY (km),EarthBdyZ (km),\
         MoonBdyX (km),MoonBdyY (km),MoonBdyZ (km),\
         SunBdyX (km),SunBdyY (km),SunBdyZ (km)\n",
    );
    let t0 = NaiveDate::from_ymd_opt(2030, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let sun_i = DVec3::new(0.0, 1.0, 0.0) * 1.496e8;
    let earth_i = DVec3::new(-384_400.0, 0.0, 0.0); // from the Moon
    let (r, mu): (f64, f64) = (1_837.4, 4_902.8); // km, km³/s²
    let n = (mu / (r * r * r)).sqrt();
    for k in 0..ROWS {
        let t = k as f64 * 60.0;
        // Orbit in the inertial Y–Z plane (contains the Sun direction).
        let sc = DVec3::new(0.0, (n * t).cos(), (n * t).sin()) * r;
        let q = DQuat::from_rotation_x(6f64.to_radians() / 60.0 * t);
        let to_b = |v: DVec3| q.conjugate() * v;
        let (e, m, su) = (to_b(earth_i - sc), to_b(-sc), to_b(sun_i - sc));
        let pos = sc - earth_i; // Earth-centred position, for the residual
        let time = (t0 + TimeDelta::minutes(k as i64)).format("%-m/%-d/%Y %-H:%M");
        s.push_str(&format!(
            "{time},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            pos.x, pos.y, pos.z, q.x, q.y, q.z, q.w, e.x, e.y, e.z, m.x, m.y, m.z, su.x, su.y, su.z,
        ));
    }
    s
}

fn craft() -> Scene {
    let mut w = ObjWriter::new();
    w.cuboid("BUS", DVec3::splat(-0.5), DVec3::splat(0.5))
        .rect(
            "WING_PY",
            DVec3::new(0.0, 1.1, 0.0),
            DVec3::Z * 0.5,
            DVec3::X * 0.5,
        )
        .rect(
            "WING_NY",
            DVec3::new(0.0, -1.1, 0.0),
            DVec3::Z * 0.5,
            DVec3::X * 0.5,
        )
        .rect(
            "WING_PZ",
            DVec3::new(0.0, 0.0, 1.1),
            DVec3::X * 0.5,
            DVec3::Y * 0.5,
        )
        .rect(
            "WING_NZ",
            DVec3::new(0.0, 0.0, -1.1),
            DVec3::Y * 0.5,
            DVec3::X * 0.5,
        )
        .sphere("DISH", DVec3::new(0.0, 0.7, 0.7), 0.3, 24)
        .cuboid(
            "MAST",
            DVec3::new(-0.05, -0.75, 0.5),
            DVec3::new(0.05, -0.65, 1.4),
        );
    let mesh = Mesh::from_obj_reader(&mut w.finish().as_bytes(), 1.0).unwrap();
    let panel = |object: &str| PanelLayout {
        name: None,
        object: object.into(),
        normal: None,
        rows: 50,
        cols: 50,
        cell: [0.019, 0.019],
        gap: [0.001, 0.001],
        u_axis: None,
        offset: [0.0, 0.0],
        strings: None,
    };
    Scene::build(
        mesh,
        &SceneConfig {
            obj: String::new(),
            scale: 1.0,
            samples: 2,
            panels: ["WING_PY", "WING_NY", "WING_PZ", "WING_NZ"]
                .map(panel)
                .to_vec(),
        },
    )
    .unwrap()
}

#[test]
#[ignore = "heavy; run in release with --ignored"]
fn ten_thousand_cells_twelve_thousand_steps_under_ten_seconds() {
    let csv = synthetic_csv();
    let table =
        StateTable::from_csv_reader(csv.as_bytes(), &CsvMapping::stk_body_vectors()).unwrap();
    assert_eq!(table.len(), ROWS);
    let r = table.attitude_residual().unwrap();
    assert!(r.max_rel < 1e-6, "synthetic attitude inconsistent: {r:?}");

    let t0 = Instant::now();
    let scene = craft();
    let rc = RayCaster::new(&scene);
    let build = t0.elapsed();
    let env = EnvConfig::default();
    eprintln!(
        "{} cells × {} samples, {} strings, {} triangles, {} BVH nodes; build {:.1} ms",
        scene.cells.len(),
        scene.samples_per_cell,
        scene.strings.len(),
        rc.bvh().triangle_count(),
        rc.bvh().node_count(),
        build.as_secs_f64() * 1e3
    );
    assert!(scene.cells.len() >= 10_000);

    let circuit = |regulation: &str| {
        let cfg = ElectricalConfig::from_scene_str(&format!(
            "[electrical]\ncell = \"azur-3g30a-advanced-4x8\"\nregulation = \"{regulation}\""
        ))
        .unwrap()
        .unwrap();
        ArrayModel::Circuit(cfg.build(Path::new(".")).unwrap().1)
    };
    let thermal = ThermalConfig::from_scene_str("[thermal]")
        .unwrap()
        .unwrap()
        .build(table.body_names())
        .unwrap()
        .0;
    for (name, model, th) in [
        (
            "constant efficiency",
            ArrayModel::Constant { efficiency: 0.3 },
            None,
        ),
        ("circuit, fixed bus", circuit("fixed-bus"), None),
        ("circuit, ideal MPPT", circuit("mppt"), None),
        ("fixed bus + thermal", circuit("fixed-bus"), Some(&thermal)),
        ("MPPT + thermal", circuit("mppt"), Some(&thermal)),
    ] {
        let go = |tb: &StateTable, serial: bool| match (th, serial) {
            (None, false) => ArrayRun::compute(tb, &scene, &rc, &model, &env),
            (None, true) => ArrayRun::compute_serial(tb, &scene, &rc, &model, &env),
            (Some(th), false) => ArrayRun::compute_thermal(tb, &scene, &rc, &model, &env, th),
            (Some(th), true) => ArrayRun::compute_thermal_serial(tb, &scene, &rc, &model, &env, th),
        };
        let t1 = Instant::now();
        let run = go(&table, false);
        let compute = t1.elapsed();

        let s = run.summary(table.body_names().count());
        let partial = run
            .samples
            .iter()
            .filter(|x| x.panel_sunlyt.iter().any(|&f| f > 0.0 && f < x.sun.visible))
            .count();
        eprintln!(
            "{name:<20} compute {:.2} s for {} steps ({:.0} µs/step); Moon umbra {} rows; \
             rows with partial self-shadow {partial}; mean {:.1} W",
            compute.as_secs_f64(),
            table.len(),
            compute.as_secs_f64() * 1e6 / table.len() as f64,
            s.umbra[table.body_index("Moon").unwrap()],
            s.mean_w
        );
        assert!(partial > 100, "scene should self-shadow often");
        assert!(compute.as_secs_f64() < 10.0, "{name} too slow: {compute:?}");

        // Parallel equals serial, on a slice of the run.
        let rows: Vec<usize> = (0..400).collect();
        let sub = table.subset(&rows);
        let (a, b) = (go(&sub, false), go(&sub, true));
        assert_eq!(a.samples, b.samples, "{name}: parallel ≠ serial");
    }
}
