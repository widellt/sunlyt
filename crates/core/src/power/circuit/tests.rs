use super::*;
use crate::power::cell::DiodeParams;

/// An ideal cell (Rs = 0, Rsh = ∞), so string answers have closed forms:
/// V_cell(I) = a·ln((Iph − I)/I0 + 1).
const IPH: f64 = 0.5;
const I0: f64 = 1e-15;
const A: f64 = 0.07;
const G: f64 = 1000.0;

fn ideal(regulation: Regulation) -> Circuit {
    Circuit {
        cell: CellAtTemp {
            temperature_k: 301.15,
            reference_irradiance_w_m2: G,
            at_reference: DiodeParams {
                iph_a: IPH,
                i0_a: I0,
                a_v: A,
                rs_ohm: 0.0,
                gsh_s: 0.0,
            },
        },
        temperature_ref: None,
        cell_area_m2: None,
        bypass_vf_v: 0.8,
        blocking_vf_v: 0.7,
        regulation,
    }
}

fn azur(regulation: Regulation) -> Circuit {
    let cfg =
        ElectricalConfig::from_scene_str("[electrical]\ncell = \"azur-3g30a-advanced-4x8\"\n")
            .unwrap()
            .unwrap();
    let (_, mut c) = cfg.build(Path::new(".")).unwrap();
    c.regulation = regulation;
    c
}

/// Hand calculation: 12 ideal cells in series, 6 in full light (Iph 0.5 A)
/// and 6 in half light (0.25 A), each with a 0.8 V bypass diode, onto a
/// fixed bus through a 0.7 V blocking diode.
///
/// - Low bus (8 V, target 8.7 V): the string runs above 0.25 A, so the
///   shaded cells are bypassed: 6·a·ln((Iph − I)/I0 + 1) − 6·0.8 = 8.7
///   ⇒ I = Iph − I0·(exp((8.7/6 + 0.8)/a) − 1).
/// - High bus (26 V, target 26.7 V): every cell conducts, and the product
///   of the two log arguments is K = exp(26.7/(6a)). With p = Iph + I0 − I,
///   p·(p − 0.25) = I0²·K ⇒ p = (0.25 + √(0.0625 + 4·I0²·K))/2.
#[test]
fn half_shaded_string_matches_hand_calculation() {
    let groups = [(G / 2.0, 6), (G, 6)];
    let c = ideal(Regulation::FixedBus { bus_v: 8.0 });
    let low = c.solve(&groups);
    let want = IPH - I0 * (((8.7 / 6.0) + 0.8) / A).exp_m1();
    assert!(want > 0.25 && want < IPH);
    assert!(
        (low.current_a - want).abs() < 1e-12,
        "{} vs {want}",
        low.current_a
    );
    assert!((low.power_w - 8.0 * want).abs() < 1e-11);

    let c = ideal(Regulation::FixedBus { bus_v: 26.0 });
    let high = c.solve(&groups);
    let k = (26.7 / (6.0 * A)).exp();
    let p = 0.5 * (0.25 + (0.0625 + 4.0 * I0 * I0 * k).sqrt());
    let want = IPH + I0 - p;
    assert!(want > 0.0 && want < 0.25);
    assert!(
        (high.current_a - want).abs() < 1e-12,
        "{} vs {want}",
        high.current_a
    );
    println!(
        "half-shaded string: 8 V bus → {:.6} A (shaded cells bypassed), 26 V bus → {:.6} A",
        low.current_a, high.current_a
    );
}

/// Series adds voltage at equal current: N identical cells give N × V_cell.
#[test]
fn series_adds_voltage() {
    let c = azur(Regulation::Mppt);
    let d = c.cell.at(1361.0);
    for i in [0.0, 0.1, 0.3, 0.45] {
        let (v, dv) = c.string_v(&[(1361.0, 17)], i);
        assert!((v - 17.0 * d.v_at(i)).abs() < 1e-12);
        assert!((dv - 17.0 * d.dv_di(i)).abs() < 1e-9 * dv.abs());
    }
}

/// A cell in the dark cannot carry current forward: its bypass diode
/// clamps it at −Vf, never below.
#[test]
fn bypass_clamps_at_minus_vf() {
    let c = azur(Regulation::Mppt);
    for i in [1e-6, 0.1, 0.5] {
        assert_eq!(c.string_v(&[(0.0, 1)], i), (-c.bypass_vf_v, 0.0));
        assert_eq!(c.string_v(&[(0.0, 3)], i).0, -3.0 * c.bypass_vf_v);
    }
    // A lit cell pushed past its photocurrent is clamped the same way.
    let iph = c.cell.at(1361.0).iph_a;
    assert_eq!(c.string_v(&[(1361.0, 1)], iph * 1.01).0, -c.bypass_vf_v);
}

/// The blocking diode stops reverse current: a string whose open-circuit
/// voltage is below the bus gives exactly zero, not negative.
#[test]
fn blocking_diode_gives_zero_not_negative() {
    let c = azur(Regulation::FixedBus { bus_v: 28.0 });
    let few = c.solve(&[(1361.0, 10)]); // Voc ≈ 27 V < 28.7 V
    assert_eq!((few.current_a, few.power_w), (0.0, 0.0));
    let dark = c.solve(&[(0.0, 14)]);
    assert_eq!((dark.current_a, dark.power_w), (0.0, 0.0));
}

/// Raising the bus never raises the string current; the fixed bus never
/// beats the ideal tracker; power is never negative.
#[test]
fn bus_sweep_invariants() {
    let groups = [(400.0, 3), (1361.0, 13)];
    let mppt = azur(Regulation::Mppt).solve(&groups);
    let mut prev = f64::INFINITY;
    for k in 0..=400 {
        let bus = 45.0 * k as f64 / 400.0;
        let p = azur(Regulation::FixedBus { bus_v: bus }).solve(&groups);
        assert!(p.current_a <= prev + 1e-15, "current rose at {bus} V");
        assert!(p.power_w >= 0.0);
        assert!(
            p.power_w <= mppt.power_w * (1.0 + 1e-12),
            "{bus} V beats MPPT"
        );
        prev = p.current_a;
    }
}

/// With partial shading P(I) has two local maxima. The tracker must find
/// the global one: checked against a brute-force scan of 200,001 currents.
/// Golden-section locates the argmax to ~√ε, so P agrees to ~1e-12.
#[test]
fn mppt_finds_global_maximum() {
    let c = azur(Regulation::Mppt);
    for groups in [
        vec![(500.0, 8), (1361.0, 8)],
        vec![(1361.0, 2), (300.0, 14)],
        vec![(0.0, 4), (700.0, 6), (1361.0, 6)],
        vec![(1361.0, 16)],
    ] {
        let best = c.solve(&groups);
        let iph = c.cell.at(1361.0).iph_a;
        let brute = (0..=200_000)
            .map(|k| {
                let i = iph * k as f64 / 200_000.0;
                i * (c.string_v(&groups, i).0 - c.blocking_vf_v)
            })
            .fold(0.0, f64::max);
        assert!(
            best.power_w >= brute * (1.0 - 1e-9),
            "{groups:?}: {} < {brute}",
            best.power_w
        );
        assert!(
            best.power_w <= brute * (1.0 + 1e-6),
            "{groups:?}: {} ≫ {brute}",
            best.power_w
        );
    }
}

/// Unshaded string under MPPT lies between the cells' MPP less the diode
/// drop at Imp (a feasible point, so a lower bound) and N × Pmp with no
/// drop (an upper bound). The bounds are 3.7 % apart for 16 cells; the
/// tracker lands 5e-6 above the lower one. With the drop, dP/dI = 0 needs
/// V + I·dV/dI = Vf/N > 0, which holds at a slightly lower current (higher
/// voltage) than the cell's own MPP.
#[test]
fn uniform_string_mpp_is_n_cells() {
    let c = azur(Regulation::Mppt);
    let d = c.cell.at(1367.0);
    let (vmp, imp) = d.mpp();
    let s = c.solve(&[(1367.0, 16)]);
    let lower = imp * (16.0 * vmp - c.blocking_vf_v);
    let upper = 16.0 * vmp * imp;
    assert!(
        s.power_w >= lower * (1.0 - 1e-12),
        "{} < {lower}",
        s.power_w
    );
    assert!(
        s.power_w < upper && s.power_w < lower * (1.0 + 1e-4),
        "{}",
        s.power_w
    );
    assert!(
        s.current_a < imp,
        "the drop moves the optimum to lower current"
    );
}

#[test]
fn group_irradiance_counts_equal_values() {
    let mut g = vec![3.0, 1.0, 3.0, 0.0, 1.0, 3.0];
    let mut out = Vec::new();
    group_irradiance(&mut g, &mut out);
    assert_eq!(out, vec![(0.0, 1), (1.0, 2), (3.0, 3)]);
}

#[test]
fn electrical_config_parses_and_validates() {
    assert!(
        ElectricalConfig::from_scene_str("obj = \"x.obj\"")
            .unwrap()
            .is_none()
    );
    let cfg = ElectricalConfig::from_scene_str(
        "obj = \"x.obj\"\n[electrical]\ncell = \"azur-3g30a-advanced-4x8\"\n\
         condition = \"1e15\"\ntemperature_c = 60\nregulation = \"mppt\"\n",
    )
    .unwrap()
    .unwrap();
    let (m, c) = cfg.build(Path::new(".")).unwrap();
    assert_eq!(m.condition.name, "1E15");
    assert_eq!(c.regulation, Regulation::Mppt);
    assert_eq!(c.bypass_vf_v, 0.8); // from the datasheet
    assert!((c.cell.temperature_k - 333.15).abs() < 1e-12);
    let typo = ElectricalConfig::from_scene_str("[electrical]\ncell = \"x\"\nbus = 28\n");
    assert!(typo.is_err());
    let bad_bus = ElectricalConfig::from_scene_str(
        "[electrical]\ncell = \"azur-3g30a-advanced-4x8\"\nbus_v = 0\n",
    )
    .unwrap()
    .unwrap();
    assert!(bad_bus.build(Path::new(".")).is_err());
}

// ── I-V curve for the sandbox plot (slice 8) ────────────────────────────

/// The curve against the ideal-cell closed form, V(I) = Σ nᵢ·max(a·ln((Iphᵢ
/// − I)/I0 + 1), −Vf), written out here independently of `DiodeParams`.
#[test]
fn string_curve_matches_closed_form() {
    let groups = [(G / 2.0, 6), (G, 6)];
    let c = ideal(Regulation::Mppt);
    let curve = c.string_curve(&groups, 200);
    assert!(curve.len() >= 200);
    let cell_v = |iph: f64, i: f64| {
        let x = (iph - i) / I0 + 1.0;
        if x > 0.0 {
            (A * x.ln()).max(-0.8)
        } else {
            -0.8
        }
    };
    for w in curve.windows(2) {
        assert!(w[1].0 > w[0].0, "current must ascend");
        assert!(w[1].1 <= w[0].1, "voltage must not rise with current");
    }
    for &(i, v) in &curve {
        let want = 6.0 * cell_v(IPH / 2.0, i) + 6.0 * cell_v(IPH, i);
        assert!(
            (v - want).abs() < 1e-9 * want.abs().max(1.0),
            "I {i}: {v} vs {want}"
        );
    }
    // Ends: open circuit, and every cell bypassed past the top level.
    let voc = 6.0 * A * (IPH / 2.0 / I0 + 1.0).ln() + 6.0 * A * (IPH / I0 + 1.0).ln();
    assert_eq!(curve[0].0, 0.0);
    assert!((curve[0].1 - voc).abs() < 1e-12);
    assert!((curve.last().unwrap().1 + 12.0 * 0.8).abs() < 1e-12);
    // There is a point near each knee: within 1 % of each photocurrent.
    for level in [IPH / 2.0, IPH] {
        assert!(curve.iter().any(|&(i, _)| i < level && i > 0.99 * level));
    }
}

/// The operating points read off the curve agree with the solvers: the
/// fixed-bus point by interpolating the curve at the bus voltage, the MPP
/// by the curve's best sample (which can only be lower).
#[test]
fn operating_points_lie_on_the_curve() {
    let c = azur(Regulation::Mppt);
    for groups in [vec![(1361.0, 16)], vec![(1361.0, 10), (500.0, 6)]] {
        let curve = c.string_curve(&groups, 4000);
        let mpp = c.mpp(&groups);
        let best = curve
            .iter()
            .map(|&(i, v)| i * (v - c.blocking_vf_v))
            .fold(0.0, f64::max);
        assert!(best <= mpp.power_w * (1.0 + 1e-12));
        assert!(
            best > mpp.power_w * (1.0 - 1e-3),
            "{best} vs {}",
            mpp.power_w
        );

        let bus = 28.0;
        let mut fixed = c;
        fixed.regulation = Regulation::FixedBus { bus_v: bus };
        let p = fixed.solve(&groups);
        let target = bus + c.blocking_vf_v;
        let read = curve
            .windows(2)
            .find(|w| w[0].1 >= target && w[1].1 <= target)
            .map(|w| {
                let t = (w[0].1 - target) / (w[0].1 - w[1].1);
                w[0].0 + t * (w[1].0 - w[0].0)
            })
            .unwrap_or(0.0);
        eprintln!(
            "groups {groups:?}: solve {:.6} A, read off curve {read:.6} A; MPP {:.4} W, curve best {best:.4} W",
            p.current_a, mpp.power_w
        );
        assert!((read - p.current_a).abs() < 2e-3 * p.current_a.max(1e-3));
    }
}
