//! Slice-1 gate against a real trajectory, which is never committed.
//! Set `SUNLYT_SAMPLE_CSV` to an STK body-vector report to run it; without
//! it the test passes as a no-op so CI stays green.

use sunlyt_core::{AngleExt, CsvMapping, StateTable};

fn percentile(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

#[test]
fn real_trajectory_gate() {
    let Ok(path) = std::env::var("SUNLYT_SAMPLE_CSV") else {
        eprintln!("SUNLYT_SAMPLE_CSV not set; skipping");
        return;
    };
    let table = StateTable::from_csv_path(path.as_ref(), &CsvMapping::stk_body_vectors()).unwrap();
    eprintln!("rows: {}", table.len());

    let r = table
        .attitude_residual()
        .expect("needs attitude + position");
    eprintln!(
        "attitude residual: max {:.3} km (rel {:.2e}), rms {:.3} km",
        r.max_m / 1e3,
        r.max_rel,
        r.rms_m / 1e3
    );
    assert!(r.max_rel < 1e-4, "quaternion convention mismatch: {r:?}");

    // Leave-one-out: keep even rows, predict odd rows, compare directions.
    let even: Vec<usize> = (0..table.len()).step_by(2).collect();
    let sub = table.subset(&even);
    let mut sun_err = Vec::new();
    let mut naive_err = Vec::new();
    let mut body_err: Vec<Vec<f64>> = vec![Vec::new(); table.body_names().count()];
    for i in (1..table.len() - 1).step_by(2) {
        let truth = table.row(i);
        let got = sub.sample(truth.t).unwrap();
        sun_err.push(got.sun_b.angle_exact(truth.sun_b).to_degrees());
        let (a, b) = (table.row(i - 1), table.row(i + 1));
        let naive = a.sun_b.lerp(b.sun_b, 0.5);
        naive_err.push(naive.angle_exact(truth.sun_b).to_degrees());
        for (k, errs) in body_err.iter_mut().enumerate() {
            errs.push(
                got.bodies[k]
                    .vec_b
                    .angle_exact(truth.bodies[k].vec_b)
                    .to_degrees(),
            );
        }
    }
    let sun_med = percentile(&mut sun_err, 0.5);
    let sun_p99 = percentile(&mut sun_err, 0.99);
    eprintln!(
        "sun dir, 2-step gap:   median {sun_med:.4}°  p99 {sun_p99:.4}°  max {:.4}°",
        sun_err.last().unwrap()
    );
    eprintln!(
        "naive body-frame lerp: median {:.4}°  p99 {:.4}°",
        percentile(&mut naive_err, 0.5),
        percentile(&mut naive_err, 0.99)
    );
    for (name, errs) in table.body_names().zip(&mut body_err) {
        eprintln!(
            "{name} dir:             median {:.4}°  p99 {:.4}°",
            percentile(errs, 0.5),
            percentile(errs, 0.99)
        );
    }
    assert!(sun_med < 0.1, "median sun interpolation error {sun_med}°");
}

/// Slice-2 gate: eclipse counts and a power-vs-time plot of one plate.
/// The plot goes to `target/sunlyt-gate/plate_power.svg` (git-ignored).
#[test]
fn real_trajectory_eclipse_gate() {
    use sunlyt_core::{EnvConfig, FlatPlate};
    let Ok(path) = std::env::var("SUNLYT_SAMPLE_CSV") else {
        eprintln!("SUNLYT_SAMPLE_CSV not set; skipping");
        return;
    };
    let table = StateTable::from_csv_path(path.as_ref(), &CsvMapping::stk_body_vectors()).unwrap();
    let env = EnvConfig::default();
    let plate = FlatPlate::new(glam::DVec3::NEG_Y, 1.0, 0.3);
    let names: Vec<&str> = table.body_names().collect();
    let nb = names.len();
    let (mut umbra, mut penumbra, mut over_half, mut centre_behind) =
        (vec![0; nb], vec![0; nb], vec![0; nb], vec![0; nb]);
    let mut series = Vec::with_capacity(table.len());
    for i in 0..table.len() {
        let e = table.row(i);
        let s = plate.sample(&e, &env);
        for (k, &f) in s.sun.occulted.iter().enumerate() {
            umbra[k] += (f >= 1.0) as usize;
            penumbra[k] += (f > 0.0 && f < 1.0) as usize;
            over_half[k] += (f > 0.5) as usize;
            // Independent check: Sun centre inside the body's (spherical
            // mean) limb, as counted when the design was researched.
            let b = &e.bodies[k];
            let limb = (b.equatorial_radius_m / b.vec_b.length()).min(1.0).asin();
            centre_behind[k] += (e.sun_b.angle_exact(b.vec_b) < limb) as usize;
        }
        series.push((e.t, s.power_w));
    }
    for k in 0..nb {
        eprintln!(
            "{:<6} umbra {:>5}  penumbra {:>3}  >50% hidden {:>5}  centre behind {:>5}",
            names[k], umbra[k], penumbra[k], over_half[k], centre_behind[k]
        );
    }
    let moon = table.body_index("Moon").expect("Moon column");
    let earth = table.body_index("Earth").expect("Earth column");
    assert_eq!(
        umbra[earth] + penumbra[earth],
        0,
        "no Earth eclipses expected"
    );
    assert!(
        umbra[moon] + penumbra[moon] > 0,
        "the sample should include Moon eclipses"
    );
    // Disk overlap and point test must agree except inside the penumbra.
    assert!(over_half[moon].abs_diff(centre_behind[moon]) <= penumbra[moon]);

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/sunlyt-gate");
    std::fs::create_dir_all(&dir).unwrap();
    let svg = dir.join("plate_power.svg");
    std::fs::write(&svg, power_svg(&series)).unwrap();
    eprintln!("plot: {}", svg.display());
}

fn power_svg(series: &[(f64, f64)]) -> String {
    let (w, h, pad) = (1200.0, 360.0, 48.0);
    let t1 = series.last().map_or(1.0, |s| s.0).max(1.0);
    let pmax = series.iter().map(|s| s.1).fold(1.0, f64::max);
    let x = |t: f64| pad + t / t1 * (w - 2.0 * pad);
    let y = |p: f64| h - pad - p / pmax * (h - 2.0 * pad);
    let pts: Vec<String> = series
        .iter()
        .map(|&(t, p)| format!("{:.1},{:.1}", x(t), y(p)))
        .collect();
    let mut ticks = String::new();
    for d in 0..=(t1 / 86_400.0) as usize {
        let tx = x(d as f64 * 86_400.0);
        ticks += &format!(
            r##"<line x1="{tx:.1}" y1="{}" x2="{tx:.1}" y2="{}" stroke="#ddd"/><text x="{tx:.1}" y="{}" text-anchor="middle">day {d}</text>"##,
            pad,
            h - pad,
            h - pad + 18.0
        );
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" font-family="sans-serif" font-size="12">
<rect width="100%" height="100%" fill="white"/>{ticks}
<text x="{pad}" y="{}">Flat plate (−Y, 1 m², η 0.3) power, W — peak {pmax:.1} W</text>
<polyline fill="none" stroke="#2563eb" stroke-width="0.8" points="{}"/>
<line x1="{pad}" y1="{}" x2="{}" y2="{}" stroke="#333"/></svg>"##,
        pad - 16.0,
        pts.join(" "),
        h - pad,
        w - pad,
        h - pad
    )
}

/// Parallel and serial runs must agree bit for bit; prints their timings.
#[test]
fn real_trajectory_parallel_matches_serial() {
    use std::time::Instant;
    use sunlyt_core::{EnvConfig, FlatPlate, PlateRun};
    let Ok(path) = std::env::var("SUNLYT_SAMPLE_CSV") else {
        eprintln!("SUNLYT_SAMPLE_CSV not set; skipping");
        return;
    };
    let t0 = Instant::now();
    let table = StateTable::from_csv_path(path.as_ref(), &CsvMapping::stk_body_vectors()).unwrap();
    let load = t0.elapsed();
    let (plate, env) = (
        FlatPlate::new(glam::DVec3::NEG_Y, 1.0, 0.3),
        EnvConfig::default(),
    );
    let best = |f: &dyn Fn() -> PlateRun| {
        (0..5)
            .map(|_| {
                let t = Instant::now();
                let r = f();
                (t.elapsed(), r)
            })
            .min_by_key(|(d, _)| *d)
            .unwrap()
    };
    let (ts, serial) = best(&|| PlateRun::compute_serial(&table, &plate, &env));
    let (tp, parallel) = best(&|| PlateRun::compute(&table, &plate, &env));
    eprintln!(
        "load {:.1} ms | compute serial {:.2} ms, parallel {:.2} ms ({} rows, {:.0} ns/row serial)",
        load.as_secs_f64() * 1e3,
        ts.as_secs_f64() * 1e3,
        tp.as_secs_f64() * 1e3,
        table.len(),
        ts.as_secs_f64() * 1e9 / table.len() as f64
    );
    assert_eq!(serial.samples, parallel.samples);
}
