//! Slice-5 gates on whole runs: panel temperature coupled to the circuit.
//! Synthetic trajectories only (no mission data).

use chrono::{NaiveDate, TimeDelta};
use glam::{DQuat, DVec3};
use sunlyt_core::geometry::shapes::ObjWriter;
use sunlyt_core::geometry::{Mesh, PanelLayout, SceneConfig, StringLayout, WalkOrder};
use sunlyt_core::thermal::{SIGMA, ThermalConfig, ThermalModel};
use sunlyt_core::{
    ArrayModel, ArrayRun, CsvMapping, ElectricalConfig, EnvConfig, RayCaster, Scene, StateTable,
};

const R_MOON_KM: f64 = 1737.4;

/// STK-style CSV at `step_min` cadence, fixed attitude (body = inertial).
/// The Sun is along inertial +Y. `orbit_km`: radius of a circular lunar
/// orbit in the Y–Z plane (eclipses every orbit), or `None` to sit
/// 1,000,000 km from the Moon, which then neither eclipses nor heats.
fn csv(rows: usize, step_min: i64, orbit_km: Option<f64>) -> String {
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
    let sun_i = DVec3::Y * 1.496e8;
    let earth_i = DVec3::new(-384_400.0, 0.0, 0.0);
    let mu: f64 = 4_902.8;
    for k in 0..rows {
        let t = k as f64 * 60.0 * step_min as f64;
        let sc = match orbit_km {
            Some(r) => {
                let n = (mu / (r * r * r)).sqrt();
                DVec3::new(0.0, (n * t).cos(), (n * t).sin()) * r
            }
            None => DVec3::new(1.0e6, 0.0, 0.0),
        };
        let q = DQuat::IDENTITY;
        let (e, m, su) = (earth_i - sc, -sc, sun_i - sc);
        let pos = sc - earth_i;
        let time = (t0 + TimeDelta::minutes(k as i64 * step_min)).format("%-m/%-d/%Y %-H:%M");
        s.push_str(&format!(
            "{time},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            pos.x, pos.y, pos.z, q.x, q.y, q.z, q.w, e.x, e.y, e.z, m.x, m.y, m.z, su.x, su.y, su.z,
        ));
    }
    s
}

fn table(rows: usize, step_min: i64, orbit_km: Option<f64>) -> StateTable {
    StateTable::from_csv_reader(
        csv(rows, step_min, orbit_km).as_bytes(),
        &CsvMapping::stk_body_vectors(),
    )
    .unwrap()
}

/// One free-standing 1 × 1 m panel facing the Sun (+Y), 24 × 12 cells of
/// 8 × 4 cm in 18 strings of 16.
fn scene() -> Scene {
    let mut w = ObjWriter::new();
    w.rect("P", DVec3::ZERO, DVec3::Z * 0.5, DVec3::X * 0.5);
    let mesh = Mesh::from_obj_reader(&mut w.finish().as_bytes(), 1.0).unwrap();
    let panel = PanelLayout {
        name: None,
        object: "P".into(),
        normal: Some([0.0, 1.0, 0.0]),
        rows: 24,
        cols: 12,
        cell: [0.08, 0.04],
        gap: [0.0, 0.0],
        u_axis: Some([1.0, 0.0, 0.0]),
        offset: [0.0, 0.0],
        strings: Some(StringLayout::Chunked {
            series: 16,
            order: WalkOrder::Rows,
        }),
    };
    let cfg = SceneConfig {
        obj: String::new(),
        scale: 1.0,
        samples: 1,
        panels: vec![panel],
    };
    Scene::build(mesh, &cfg).unwrap()
}

fn circuit(regulation: &str) -> ArrayModel {
    let cfg = ElectricalConfig::from_scene_str(&format!(
        "[electrical]\ncell = \"azur-3g30a-advanced-4x8\"\nregulation = \"{regulation}\""
    ))
    .unwrap()
    .unwrap();
    ArrayModel::Circuit(cfg.build(std::path::Path::new(".")).unwrap().1)
}

fn thermal(extra: &str, table: &StateTable) -> ThermalModel {
    ThermalConfig::from_scene_str(&format!("[thermal]\n{extra}"))
        .unwrap()
        .unwrap()
        .build(table.body_names())
        .unwrap()
        .0
}

/// Gate: in constant sunlight with no electrical load and no planets, the
/// whole-run temperature equals the closed-form radiative balance
/// T = (α S / ((ε_f + ε_b) σ))^¼ within 0.5 K, from a cold start.
#[test]
fn constant_sun_settles_to_closed_form() {
    let tb = table(240, 1, None);
    let s = scene();
    let rc = RayCaster::new(&s);
    let th = thermal("initial_c = -100\nplanets = false", &tb);
    let model = ArrayModel::Constant { efficiency: 0.0 };
    let run = ArrayRun::compute_thermal(&tb, &s, &rc, &model, &EnvConfig::default(), &th);
    let last = run.samples.last().unwrap();
    // The craft sits 1e6 km off the Sun line: cos θ = 0.99998, not 1.
    let cos = tb.row(tb.len() - 1).sun_b.normalize().y;
    let exact = (0.91 * last.sun.irradiance_w_m2 * cos / (1.7 * SIGMA)).powf(0.25);
    let t = last.panel_temperature_k[0];
    println!("constant sun: {t:.4} K vs closed form {exact:.4} K");
    assert!((t - exact).abs() < 0.5);
    assert!((t - exact).abs() < 1e-3, "settled after 4 h");
    // Monotone warm-up from the cold start.
    let temps: Vec<f64> = run
        .samples
        .iter()
        .map(|x| x.panel_temperature_k[0])
        .collect();
    assert!(temps.windows(2).all(|w| w[1] >= w[0] - 1e-9));
}

/// With the circuit drawing power, the settled panel satisfies the energy
/// balance q_in = emitted + P/A, runs cooler than open circuit, and the
/// default start (steady state of the first row) is already settled.
#[test]
fn electrical_power_cools_the_panel_and_balances() {
    let tb = table(120, 1, None);
    let s = scene();
    let rc = RayCaster::new(&s);
    let env = EnvConfig::default();
    let th = thermal("planets = false", &tb);
    let model = circuit("fixed-bus");
    let run = ArrayRun::compute_thermal(&tb, &s, &rc, &model, &env, &th);
    let open = ArrayRun::compute_thermal(
        &tb,
        &s,
        &rc,
        &ArrayModel::Constant { efficiency: 0.0 },
        &env,
        &th,
    );
    let (first, last) = (&run.samples[0], run.samples.last().unwrap());
    let q = 0.91 * last.sun.irradiance_w_m2 * tb.row(tb.len() - 1).sun_b.normalize().y;
    let area = s.panels[0].area_m2;
    let t = last.panel_temperature_k[0];
    let residual = th.panel.net_w_m2(t, q, last.power_w / area);
    println!(
        "loaded: {:.2} °C at {:.1} W; open circuit {:.2} °C; residual {residual:.1e} W/m²",
        t - 273.15,
        last.power_w,
        open.samples.last().unwrap().panel_temperature_k[0] - 273.15
    );
    assert!(residual.abs() < 1e-6);
    assert!(t < open.samples.last().unwrap().panel_temperature_k[0] - 5.0);
    assert!(
        (first.panel_temperature_k[0] - t).abs() < 1e-6,
        "steady start"
    );
    assert!(last.power_w > 0.0);
}

/// Lunar orbit at 100 km: the panel cools in every eclipse and recovers;
/// temperatures stay physical and change no faster than the radiative
/// limit allows; parallel equals serial bit for bit.
#[test]
fn eclipses_cool_and_parallel_equals_serial() {
    let tb = table(600, 1, Some(R_MOON_KM + 100.0));
    let s = scene();
    let rc = RayCaster::new(&s);
    let env = EnvConfig::default();
    let th = thermal("", &tb);
    let model = circuit("fixed-bus");
    let run = ArrayRun::compute_thermal(&tb, &s, &rc, &model, &env, &th);
    let serial = ArrayRun::compute_thermal_serial(&tb, &s, &rc, &model, &env, &th);
    assert_eq!(run.samples, serial.samples);

    let temps: Vec<f64> = run
        .samples
        .iter()
        .map(|x| x.panel_temperature_k[0])
        .collect();
    let lit: Vec<f64> = run.samples.iter().map(|x| x.sun.visible).collect();
    let (min, max) = temps
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), &t| (a.min(t), b.max(t)));
    let umbra = lit.iter().filter(|&&v| v == 0.0).count();
    println!(
        "100 km lunar orbit: {:.1} … {:.1} °C, {umbra} of {} rows in umbra",
        min - 273.15,
        max - 273.15,
        tb.len()
    );
    assert!(umbra > 100, "orbit should eclipse");
    assert!(min > 100.0 && max < 420.0);
    // Cooling in eclipse: at the end of each umbra the panel is colder
    // than at its start.
    let mut k = 1;
    while k < lit.len() {
        if lit[k] == 0.0 && lit[k - 1] > 0.0 {
            let start = k;
            while k < lit.len() && lit[k] == 0.0 {
                k += 1;
            }
            if k < lit.len() {
                assert!(temps[k - 1] < temps[start] - 20.0, "eclipse at row {start}");
            }
        }
        k += 1;
    }
    // Rate bound: |dT/dt| ≤ (q_max + σ ε T⁴ + P/A) / C.
    let worst = temps
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f64::max);
    let bound = 60.0 * (3000.0 + 1.7 * SIGMA * max.powi(4)) / 1800.0;
    assert!(worst < bound, "{worst} K per row");
}

/// The Moon's albedo and IR heat a panel in low orbit: over the same orbit,
/// with planets on, the panel is warmer on average than with them off.
#[test]
fn lunar_heating_raises_temperature() {
    let tb = table(360, 1, Some(R_MOON_KM + 100.0));
    let s = scene();
    let rc = RayCaster::new(&s);
    let env = EnvConfig::default();
    let model = circuit("fixed-bus");
    let mean = |th: &ThermalModel| {
        let run = ArrayRun::compute_thermal(&tb, &s, &rc, &model, &env, th);
        run.samples
            .iter()
            .map(|x| x.panel_temperature_k[0])
            .sum::<f64>()
            / tb.len() as f64
    };
    let (with, without) = (
        mean(&thermal("", &tb)),
        mean(&thermal("planets = false", &tb)),
    );
    println!("mean panel temperature: with Moon {with:.1} K, without {without:.1} K");
    assert!(with > without + 5.0);
}

/// Cadence: the same orbit tabulated every 1, 2 and 4 minutes. Inside each
/// interval the eclipse is sampled every 2 s from the interpolated
/// trajectory, so the thermal step itself is converged; what remains is
/// where the interpolated trajectory puts the shadow boundary. Positions
/// are interpolated linearly between rows, so the chord sags off the orbit
/// by r(1 − cos(nΔt/2)) (2.7 km at 120 s here), which moves ingress by a
/// second or two while the panel cools at ~1 K/s. That error is second
/// order in the row spacing: measured 2.8 K (2 vs 1 min) and 11.1 K (4 vs
/// 1 min), a ratio of 3.97. Without the 2 s eclipse samples the 2-minute
/// difference was 16 K. Extrapolated, 60 s rows carry about ±1 K at
/// ingress and egress, less elsewhere.
#[test]
fn cadence_error_is_second_order_in_row_spacing() {
    let s = scene();
    let rc = RayCaster::new(&s);
    let env = EnvConfig::default();
    let model = ArrayModel::Constant { efficiency: 0.0 };
    let tabs: Vec<StateTable> = [1, 2, 4]
        .iter()
        .map(|&m| table(800 / m as usize, m, Some(R_MOON_KM + 100.0)))
        .collect();
    let th = thermal("planets = false", &tabs[0]);
    let runs: Vec<Vec<f64>> = tabs
        .iter()
        .map(|tb| {
            ArrayRun::compute_thermal(tb, &s, &rc, &model, &env, &th)
                .samples
                .iter()
                .map(|x| x.panel_temperature_k[0])
                .collect()
        })
        .collect();
    let diff = |i: usize, m: usize| {
        (0..runs[i].len())
            .map(|k| (runs[i][k] - runs[0][k * m]).abs())
            .fold(0.0, f64::max)
    };
    let (d2, d4) = (diff(1, 2), diff(2, 4));
    println!(
        "cadence: 2 vs 1 min {d2:.3} K, 4 vs 1 min {d4:.3} K, ratio {:.2}",
        d4 / d2
    );
    assert!(d2 < 3.5, "{d2}");
    assert!(d4 / d2 > 3.0 && d4 / d2 < 6.0, "{d2} {d4}");
}

/// Slice-6 contract: the viewer's one-instant path (`array_at` at a row
/// time with that row's panel temperatures) reproduces the batch run bit
/// for bit, so viewer numbers between rows join the batch rows exactly.
/// Also checks that the progress counters reach the row count.
#[test]
fn array_at_rows_equal_batch() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use sunlyt_core::sim::array_at;

    // 100 km orbit, 2-minute rows: eclipse ingress and egress in the run.
    let tb = table(200, 2, Some(R_MOON_KM + 100.0));
    let s = scene();
    let rc = RayCaster::new(&s);
    let env = EnvConfig::default();
    let model = circuit("fixed-bus");
    let th = thermal("", &tb);

    let progress = AtomicUsize::new(0);
    let run = ArrayRun::compute_thermal_progress(&tb, &s, &rc, &model, &env, &th, &progress);
    assert_eq!(progress.load(Ordering::Relaxed), tb.len());
    assert!(run.samples.iter().any(|x| x.sun.visible == 0.0));
    for (i, want) in run.samples.iter().enumerate() {
        let (got, _) = array_at(
            &tb,
            &s,
            &rc,
            &model,
            &env,
            run.t[i],
            Some(&want.panel_temperature_k),
        )
        .unwrap();
        assert_eq!(&got, want, "row {i}");
    }

    let progress = AtomicUsize::new(0);
    let run = ArrayRun::compute_progress(&tb, &s, &rc, &model, &env, &progress);
    assert_eq!(progress.load(Ordering::Relaxed), tb.len());
    for (i, want) in run.samples.iter().enumerate() {
        let (got, _) = array_at(&tb, &s, &rc, &model, &env, run.t[i], None).unwrap();
        assert_eq!(&got, want, "row {i}");
    }
}
