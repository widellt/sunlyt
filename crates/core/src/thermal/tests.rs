use glam::{DQuat, DVec3};

use super::planet::{RINGS, SECTORS, add_irradiance, view_factor_sphere};
use super::*;
use crate::env::Sunlight;

const S: f64 = 1361.0;

fn panel() -> PanelThermal {
    PanelThermal {
        heat_capacity_j_m2_k: 1800.0,
        absorptance_front: 0.91,
        emittance_front: 0.85,
        absorptance_back: 0.9,
        emittance_back: 0.85,
        back_sink_k: None,
    }
}

/// Uniform radiance L = 1 W/(m²·sr): irradiance = π · view factor. The
/// quadrature (32 × 64 directions) matches Howell's closed form across
/// tilt and distance, including the partly-below-horizon range. The bound
/// is 0.3 % of the nadir value 1/h²: the midpoint rule is second order
/// except where the plate's horizon cuts the disk (a kink in max(0, n·ω)).
#[test]
fn quadrature_matches_closed_form_view_factor() {
    let uniform = BodyRadiation {
        bond_albedo: 0.0,
        ir: SurfaceEmission::Uniform {
            exitance_w_m2: std::f64::consts::PI,
        },
    };
    let r = 1.0;
    let mut worst = 0.0f64;
    for h in [1.02, 1.1, 1.5, 3.0, 20.0, 220.0] {
        for k in 0..=36 {
            let beta = (k as f64 * 5.0).to_radians();
            let n = DVec3::new(beta.sin(), 0.0, beta.cos());
            let mut out = [(0.0, 0.0)];
            add_irradiance(&uniform, DVec3::Z * h * r, r, DVec3::X, S, &[n], &mut out);
            let f = view_factor_sphere(beta.cos(), h);
            let err = (out[0].1 / std::f64::consts::PI - f).abs() * h * h;
            worst = worst.max(err);
            assert!(
                err < 3e-3,
                "h {h}, β {}°: quadrature {} vs {f}",
                k * 5,
                out[0].1 / std::f64::consts::PI
            );
        }
    }
    println!("view factor: worst error {worst:.1e} of the nadir value ({RINGS} × {SECTORS})");
}

/// The closed form's own limits: nadir 1/h², zero once the sphere is below
/// the horizon, continuous where the sphere starts to set, and the
/// half-space limit 1/2 for a vertical plate touching an infinite plane.
#[test]
fn view_factor_limits() {
    for h in [1.01, 1.5, 4.0, 100.0] {
        assert!((view_factor_sphere(1.0, h) - 1.0 / (h * h)).abs() < 1e-15);
        assert_eq!(view_factor_sphere(-1.0 / h - 1e-9, h), 0.0);
        let c = 1.0 / h;
        let (a, b) = (
            view_factor_sphere(c + 1e-9, h),
            view_factor_sphere(c - 1e-9, h),
        );
        assert!(
            (a - b).abs() < 1e-7,
            "jump at the horizon, h {h}: {a} vs {b}"
        );
        let (a, b) = (
            view_factor_sphere(-c + 1e-9, h),
            view_factor_sphere(-c - 1e-9, h),
        );
        assert!((a - b).abs() < 1e-7, "jump at setting, h {h}: {a} vs {b}");
    }
    assert!((view_factor_sphere(0.0, 1.0 + 1e-9) - 0.5).abs() < 1e-3);
}

/// Energy conservation: every ray leaving the body crosses a sphere around
/// it once, so the irradiance on body-facing plates, summed over that
/// sphere, is the body's total output. Albedo: A·S·πR². Day-side IR in
/// equilibrium: (1 − A)·S·πR² (night floor set to 0 K here). Within 1 %
/// with 2,000 Fibonacci positions.
#[test]
fn planet_model_conserves_energy() {
    let rad = BodyRadiation {
        bond_albedo: 0.12,
        ir: SurfaceEmission::Subsolar {
            emissivity: 0.95,
            night_k: 0.0,
        },
    };
    let (r, d, n) = (1.7374e6, 2.5e6, 2000);
    let sun = DVec3::new(0.3, -0.5, 0.8).normalize();
    let (mut alb, mut ir) = (0.0, 0.0);
    let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
    for k in 0..n {
        let z = 1.0 - 2.0 * (k as f64 + 0.5) / n as f64;
        let rr = (1.0 - z * z).sqrt();
        let pos = DVec3::new(
            rr * (golden * k as f64).cos(),
            rr * (golden * k as f64).sin(),
            z,
        ) * d;
        // Spacecraft at `pos` relative to the body: the body centre is at −pos.
        let mut out = [(0.0, 0.0)];
        add_irradiance(&rad, -pos, r, sun, S, &[-pos.normalize()], &mut out);
        alb += out[0].0;
        ir += out[0].1;
    }
    let shell = 4.0 * std::f64::consts::PI * d * d / n as f64;
    let disk = std::f64::consts::PI * r * r * S;
    let (ea, ei) = (alb * shell / (0.12 * disk), ir * shell / (0.88 * disk));
    println!("conservation: albedo {ea:.5}, IR {ei:.5} (1 = exact)");
    assert!((ea - 1.0).abs() < 0.01 && (ei - 1.0).abs() < 0.01);
}

/// Surface exitance: the subsolar point re-emits (1 − A)·S; the night side
/// sits at the floor εσT⁴; Earth's IR is the same everywhere.
#[test]
fn surface_exitance() {
    let moon = BodyRadiation::moon();
    let (alb, ir) = moon.exitance(1.0, S);
    assert!((alb - 0.12 * S).abs() < 1e-9 && (ir - 0.88 * S).abs() < 1e-9);
    let (alb, ir) = moon.exitance(-0.5, S);
    assert_eq!(alb, 0.0);
    assert!((ir - 0.95 * SIGMA * 95f64.powi(4)).abs() < 1e-12);
    // The implied subsolar temperature is the textbook ~386 K.
    let t_ss = (0.88 * S / (0.95 * SIGMA)).powf(0.25);
    assert!((t_ss - 386.0).abs() < 1.5, "{t_ss}");
    assert_eq!(BodyRadiation::earth().exitance(-1.0, S), (0.0, 240.0));
    assert!(
        BodyRadiation::for_name("MOON").is_some() && BodyRadiation::for_name("Vesta").is_none()
    );
}

/// Gate: a panel held in constant sunlight settles to the closed-form
/// radiative balance T = (α S cos θ / ((ε_f + ε_b) σ))^¼ within 0.5 K
/// (achieved: far below), starting cold, stepping 60 s rows.
#[test]
fn steady_state_matches_closed_form() {
    let p = panel();
    for deg in [0.0, 45.0, 75.0] {
        let q = 0.91 * S * f64::to_radians(deg).cos();
        let exact = (q / (1.7 * SIGMA)).powf(0.25);
        assert!((p.steady_k(q, 0.0) - exact).abs() < 1e-9);
        let mut t = 100.0;
        for _ in 0..600 {
            t = p.step(t, 60.0, (q, q), (0.0, 0.0));
        }
        println!("{deg:>4}°: steady {exact:.4} K, integrated {t:.6} K");
        assert!((t - exact).abs() < 0.5, "{deg}°: {t} vs {exact}");
        assert!((t - exact).abs() < 1e-6, "converged");
    }
}

/// With electrical power taken off, the steady state satisfies the energy
/// balance q_in = emitted + p_elec to round-off, and is cooler than the
/// open-circuit panel.
#[test]
fn energy_balance_with_electrical_power() {
    let p = PanelThermal {
        back_sink_k: Some(293.15),
        ..panel()
    };
    let (q, pe) = (1100.0, 300.0);
    let t = p.steady_k(q, pe);
    assert!(
        p.net_w_m2(t, q, pe).abs() < 1e-9,
        "{}",
        p.net_w_m2(t, q, pe)
    );
    assert!(t < p.steady_k(q, 0.0));
    // A step from the steady state stays there.
    assert!((p.step(t, 60.0, (q, q), (pe, pe)) - t).abs() < 1e-9);
}

/// Radiative cooling in eclipse has an exact solution:
/// dT/dt = −k T⁴  ⇒  T(t) = (T0⁻³ + 3 k t)^(−1/3), k = (ε_f + ε_b)σ/C.
/// Over two hours of 60 s rows the integrator stays within 1 mK.
#[test]
fn eclipse_cooling_matches_exact_solution() {
    let p = panel();
    let k = 1.7 * SIGMA / 1800.0;
    let t0 = 360.0;
    let mut t = t0;
    let mut worst = 0.0f64;
    for i in 1..=120 {
        t = p.step(t, 60.0, (0.0, 0.0), (0.0, 0.0));
        let exact = (t0.powi(-3) + 3.0 * k * 60.0 * i as f64).powf(-1.0 / 3.0);
        worst = worst.max((t - exact).abs());
    }
    println!("cooling: worst error {worst:.2e} K over 2 h, final {t:.3} K");
    assert!(worst < 1e-3);
}

/// RK4 is fourth order: halving the substep cuts the error by 2⁴ = 16
/// against the exact cooling solution. The T⁴ term makes the problem
/// stiff enough that the fifth-order error term still matters at coarse
/// steps: the measured ratio falls 41.6, 38.7, 34.1, 29.5, 25.4, 22.1 from
/// n = 4 to 256 substeps over 600 s. So the check is made in the asymptotic
/// range (n = 256 → 512, errors ~1e-9 K, still far above round-off) and
/// asks for 14–22. The production step (h ≤ τ/4) is accurate to ~1e-5 K
/// per 600 s, see `eclipse_cooling_matches_exact_solution`.
#[test]
fn integrator_is_fourth_order() {
    let p = panel();
    let k = 1.7 * SIGMA / 1800.0;
    let (t0, span): (f64, f64) = (400.0, 600.0);
    let exact = (t0.powi(-3) + 3.0 * k * span).powf(-1.0 / 3.0);
    let run = |n| p.step_n(t0, span, (0.0, 0.0), (0.0, 0.0), n);
    let (e1, e2) = ((run(256) - exact).abs(), (run(512) - exact).abs());
    println!(
        "RK4 order check: error {e1:.3e} → {e2:.3e}, ratio {:.1}",
        e1 / e2
    );
    assert!(e1 / e2 > 14.0 && e1 / e2 < 22.0, "{e1} {e2}");
}

fn one_panel_scene() -> crate::geometry::Scene {
    use crate::geometry::shapes::ObjWriter;
    use crate::geometry::{Mesh, PanelLayout, SceneConfig};
    let mut w = ObjWriter::new();
    w.rect("P", DVec3::ZERO, DVec3::X * 0.5, DVec3::Y * 0.5);
    let mesh = Mesh::from_obj_reader(&mut w.finish().as_bytes(), 1.0).unwrap();
    let panel = PanelLayout {
        name: None,
        object: "P".into(),
        normal: None,
        rows: 2,
        cols: 2,
        cell: [0.5, 0.5],
        gap: [0.0, 0.0],
        u_axis: None,
        offset: [0.0, 0.0],
        strings: None,
    };
    let cfg = SceneConfig {
        obj: String::new(),
        scale: 1.0,
        samples: 1,
        panels: vec![panel],
    };
    crate::geometry::Scene::build(mesh, &cfg).unwrap()
}

fn illum(sun_dir: DVec3, cos: f64, sunlyt: f64) -> ArrayIllum {
    illum_v(sun_dir, cos, sunlyt, 1.0)
}

fn illum_v(sun_dir: DVec3, cos: f64, sunlyt: f64, visible: f64) -> ArrayIllum {
    ArrayIllum {
        sun: Sunlight {
            sun_dir_b: sun_dir,
            sun_dist_m: crate::math::AU_M,
            irradiance_w_m2: S,
            occulted: vec![],
            visible,
        },
        panel_cos: vec![cos],
        panel_lit_area_m2: vec![sunlyt],
        panel_sunlyt: vec![sunlyt],
        groups: vec![],
        group_start: vec![],
    }
}

/// Absorbed flux: direct sunlight on the front (scaled by the sunlyt
/// fraction), on an exposed back, nothing on a back that faces the bus;
/// a Moon below the panel adds albedo and IR to the side that sees it.
#[test]
fn absorbed_flux_by_face() {
    let scene = one_panel_scene();
    let n = scene.panels[0].normal;
    let epoch = |bodies| Epoch {
        t: 0.0,
        sun_b: n * crate::math::AU_M,
        bodies,
        q_bi: DQuat::IDENTITY,
    };
    let (m, _) = ThermalConfig::from_scene_str("[thermal]\n")
        .unwrap()
        .unwrap()
        .build(["Moon"].into_iter())
        .unwrap();
    let front = m.absorbed(&scene, &epoch(vec![]), &illum(n, 1.0, 0.5));
    assert!((front[0].total(1.0) - 0.91 * S * 0.5).abs() < 1e-9);
    let back = m.absorbed(&scene, &epoch(vec![]), &illum(-n, 0.0, 0.0));
    assert!((back[0].total(1.0) - 0.9 * S).abs() < 1e-9);
    // Half the disk showing halves the direct part.
    assert!((back[0].total(0.5) - 0.45 * S).abs() < 1e-9);
    let body_mounted = ThermalModel {
        panel: PanelThermal {
            back_sink_k: Some(293.15),
            ..m.panel
        },
        ..m.clone()
    };
    assert_eq!(
        body_mounted.absorbed(&scene, &epoch(vec![]), &illum(-n, 0.0, 0.0))[0].total(1.0),
        0.0
    );

    // The Moon 100 km below the back face. Visible cap: 19° of arc around
    // the sub-spacecraft point, whose exitance is then nearly uniform.
    let moon = crate::trajectory::BodyState {
        index: 0,
        equatorial_radius_m: 1.7374e6,
        polar_radius_m: 1.7374e6,
        pole_b: DVec3::Z,
        vec_b: -n * 1.8374e6,
    };
    let f = view_factor_sphere(1.0, 1.8374 / 1.7374);
    // Night: Sun beyond the Moon (craft in its shadow), so the point below
    // is at midnight: no albedo, IR from the night floor only, on the back.
    let night = m.absorbed(
        &scene,
        &epoch(vec![moon.clone()]),
        &illum_v(-n, 0.0, 0.0, 0.0),
    )[0];
    assert_eq!(night.direct_w_m2, None, "rays skipped in umbra");
    let night = night.total(0.0);
    let want = 0.85 * 0.95 * SIGMA * 95f64.powi(4) * f;
    assert!((night - want).abs() < 1e-3 * want, "{night} vs {want}");
    // Noon: Sun overhead the front, the back sees the day side; every
    // visible point has cos φ ≥ cos 19°, which bounds the planet term.
    let noon = m.absorbed(&scene, &epoch(vec![moon]), &illum(n, 1.0, 1.0))[0].total(1.0);
    let planet = noon - 0.91 * S;
    let full = S * (0.9 * 0.12 + 0.85 * 0.88) * f;
    let cos19 = (1.7374f64 / 1.8374).acos().cos();
    assert!(
        planet <= full && planet >= cos19 * full,
        "{planet} vs {full}"
    );
    println!("100 km over the Moon: night {night:.1} W/m², noon planet term {planet:.1} W/m²");
}

#[test]
fn config_parses_and_validates() {
    let names = || ["Earth", "Moon", "Phobos"].into_iter();
    assert!(
        ThermalConfig::from_scene_str("obj = \"x\"")
            .unwrap()
            .is_none()
    );
    let (m, unknown) = ThermalConfig::from_scene_str("[thermal]\ninitial_c = 20\nback_sink_c = 20")
        .unwrap()
        .unwrap()
        .build(names())
        .unwrap();
    assert_eq!(unknown, vec!["Phobos"]);
    assert!(m.bodies[0].is_some() && m.bodies[1].is_some() && m.bodies[2].is_none());
    assert!((m.initial_k.unwrap() - 293.15).abs() < 1e-12);
    let (m, unknown) = ThermalConfig::from_scene_str("[thermal]\nplanets = false")
        .unwrap()
        .unwrap()
        .build(names())
        .unwrap();
    assert!(unknown.is_empty() && m.bodies.iter().all(Option::is_none));
    assert!(ThermalConfig::from_scene_str("[thermal]\nemitance_front = 0.8").is_err());
    let bad = ThermalConfig::from_scene_str("[thermal]\nabsorptance_front = 1.5")
        .unwrap()
        .unwrap();
    assert!(bad.build(names()).is_err());
}
