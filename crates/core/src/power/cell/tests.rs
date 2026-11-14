use super::*;

fn azur() -> CellDatasheet {
    CellDatasheet::load("azur-3g30a-advanced-4x8", Path::new(".")).unwrap()
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

/// Literature values: W(1) = Ω (the omega constant), W(e) = 1, W(e^(e+1)) = e.
#[test]
fn lambert_w_golden_values() {
    const OMEGA: f64 = 0.567_143_290_409_784; // Ω = W(1), to f64 precision
    assert!((lambert_w_exp(0.0) - OMEGA).abs() < 1e-15);
    assert!((lambert_w_exp(1.0) - 1.0).abs() < 1e-15);
    let e = std::f64::consts::E;
    assert!((lambert_w_exp(e + 1.0) - e).abs() < 4e-15);
    // Far outside exp's range: the defining relation still holds.
    for l in [-700.0, -30.0, 50.0, 1e4, 1e8] {
        let w = lambert_w_exp(l);
        assert!(
            (w + w.ln() - l).abs() <= 1e-14 * l.abs().max(1.0),
            "l = {l}"
        );
    }
}

/// Every datasheet condition is reproduced: the fit is exact by
/// construction, so the check is that an independent evaluation (the
/// Lambert-W current and a golden-section MPP search that never sees the
/// slope condition) lands on the printed points. Gate: 1 %.
#[test]
fn fit_passes_through_datasheet_points() {
    for name in CellDatasheet::preset_names() {
        let sheet = CellDatasheet::load(name, Path::new(".")).unwrap();
        for c in &sheet.conditions {
            let m = CellModel::fit(&sheet, &c.name).unwrap();
            let d = m.reference;
            let (vmp, imp) = d.mpp();
            let errs = [
                rel(d.i_at(0.0), c.isc_a),
                rel(d.voc(), c.voc_v),
                rel(d.i_at(c.vmp_v), c.imp_a),
                rel(vmp, c.vmp_v),
                rel(imp, c.imp_a),
                rel(vmp * imp, c.vmp_v * c.imp_a),
            ];
            let worst = errs.iter().copied().fold(0.0, f64::max);
            println!(
                "{name} {:>5}: a = {:.5} V (n·Nj = {:.2}), Rs = {:.4} Ω, Rsh = {:.4e} Ω, \
             I0 = {:.3e} A, Iph = {:.6} A, worst datasheet error {worst:.1e}",
                c.name,
                d.a_v,
                d.a_v / (8.617_333_262e-5 * m.reference_temperature_k),
                d.rs_ohm,
                1.0 / d.gsh_s,
                d.i0_a,
                d.iph_a
            );
            assert!(worst < 1e-6, "{name} {}: {errs:?}", c.name);
            assert!(d.rs_ohm >= 0.0 && d.gsh_s >= 0.0 && d.i0_a > 0.0 && d.a_v > 0.0);
        }
    }
}

/// The shunt fallback is used only when the series fit has no solution:
/// AZUR fits with an ideal shunt (Gsh = 0, Rs > 0); SolAero IMM-α's typical
/// points are squarer than any Rs ≥ 0 allows (with Rs = 0 and no shunt the
/// curve through Isc, Voc and (Vmp, Imp) peaks at ~4.23 V, below the
/// printed 4.28 V), so it fits with Rs = 0 and a finite shunt.
#[test]
fn shunt_fit_only_when_series_fit_is_infeasible() {
    for c in &azur().conditions {
        let d = CellModel::fit(&azur(), &c.name).unwrap().reference;
        assert!(d.gsh_s == 0.0 && d.rs_ohm > 0.0, "{}", c.name);
    }
    let imm = CellDatasheet::load("solaero-imm-alpha", Path::new(".")).unwrap();
    for c in &imm.conditions {
        assert!(
            fit_point(c, 0.0).is_err(),
            "{}: series fit unexpectedly works",
            c.name
        );
        let d = CellModel::fit(&imm, &c.name).unwrap().reference;
        assert!(d.rs_ohm == 0.0 && d.gsh_s > 0.0, "{}", c.name);
        // A physical shunt: thousands of Ω·cm² or more for a 1 cm² cell.
        assert!(1.0 / d.gsh_s > 1e3, "{}: Rsh {}", c.name, 1.0 / d.gsh_s);
    }
    // Ideal-diode check of the claim above, by bisection on dP/dV = 0.
    let c = &imm.conditions[0];
    let a = (c.voc_v - c.vmp_v) / -(1.0 - c.imp_a / c.isc_a).ln();
    let i0 = c.isc_a / (c.voc_v / a).exp_m1();
    let dpdv = |v: f64| c.isc_a - i0 * (v / a).exp_m1() - v * i0 / a * (v / a).exp();
    let vmp = bisect(dpdv, 0.0, c.voc_v);
    assert!(vmp < c.vmp_v - 0.03, "ideal-diode MPP {vmp} V");
}

/// Lambert-W (closed form) and Newton (implicit equation) agree, with an
/// ideal and a finite shunt, through forward, MPP and reverse bias; and
/// V(I) inverts I(V) to within its conditioning: on the flat part of the
/// curve |dV/dI| reaches 1e7–1e13 Ω with an ideal shunt, so the current's
/// last-bit rounding alone moves V by ulp(I)·|dV/dI|. The bound is that,
/// times 8, plus 1e-12 V.
#[test]
fn lambert_w_matches_newton_and_inverts() {
    let base = CellModel::fit(&azur(), "BOL").unwrap().reference;
    for gsh in [0.0, 1.0 / 500.0, 1.0 / 20.0] {
        let d = DiodeParams { gsh_s: gsh, ..base };
        for k in 0..=300 {
            let v = -1.0 + (d.voc() + 1.1) * k as f64 / 300.0;
            let (iw, inew) = (d.i_at(v), d.i_at_newton(v));
            assert!(
                (iw - inew).abs() <= 1e-12,
                "gsh {gsh}, v {v}: {iw} vs {inew}"
            );
            if v > 0.0 {
                let back = d.v_at(iw);
                let tol = 1e-12 + 8.0 * f64::EPSILON * iw.abs() * d.dv_di(iw).abs();
                assert!(
                    (back - v).abs() <= tol,
                    "gsh {gsh}, v {v}: V(I(V)) = {back}, tol {tol:e}"
                );
            }
        }
    }
}

/// Ideal diode (Rs = 0, Rsh = ∞) has closed forms: Voc = a·ln(Iph/I0 + 1)
/// and Vmp = a·(W(e·(Iph + I0)/I0) − 1). The MPP is also found here by
/// bisecting dP/dV = 0 directly, independent of both W and golden search.
/// Golden-section search compares P values, and P is flat at its maximum
/// (P − Pmax ∝ (V − Vmp)²), so it locates Vmp only to ~√ε ≈ 1e-8 relative
/// while Pmax itself is exact; hence the looser bound for `mpp()`.
#[test]
fn ideal_diode_closed_forms() {
    let d = DiodeParams {
        iph_a: 0.5,
        i0_a: 1e-20,
        a_v: 0.1,
        rs_ohm: 0.0,
        gsh_s: 0.0,
    };
    let voc = d.a_v * (d.iph_a / d.i0_a + 1.0).ln();
    assert!(rel(d.voc(), voc) < 1e-14);
    let vmp_w = d.a_v * (lambert_w_exp(1.0 + ((d.iph_a + d.i0_a) / d.i0_a).ln()) - 1.0);
    let dpdv =
        |v: f64| d.iph_a - d.i0_a * (v / d.a_v).exp_m1() - v * d.i0_a / d.a_v * (v / d.a_v).exp();
    let vmp_b = bisect(dpdv, 0.0, voc);
    let (vmp_g, imp_g) = d.mpp();
    assert!(rel(vmp_w, vmp_b) < 1e-12, "{vmp_w} vs {vmp_b}");
    assert!(rel(vmp_g, vmp_b) < 1e-7, "{vmp_g} vs {vmp_b}");
    assert!(rel(vmp_g * imp_g, vmp_b * d.i_at(vmp_b)) < 1e-14);
}

/// Voc follows the datasheet coefficient exactly (that is how I0(T) is set)
/// and Isc follows it to the Rs·I0 correction. Vmp, Imp and Pmp are model
/// predictions; they are compared with the datasheet's linear coefficients
/// over its stated 28–140 °C range.
///
/// Measured: Pmp within 0.8 % everywhere. Vmp and Imp separately drift up
/// to +2.3 % / −1.7 % at 140 °C after 1E15: the model holds Rs constant
/// and scales a ∝ T, which moves the MPP a little along the curve (V up,
/// I down) while keeping its power. The datasheet coefficients are
/// population-typical straight lines, so the bounds are Pmp < 1 % (what
/// the bus sees) and Vmp, Imp < 2.5 %.
#[test]
fn temperature_follows_datasheet_coefficients() {
    let sheet = azur();
    let mut worst = 0.0f64;
    for c in &sheet.conditions {
        let m = CellModel::fit(&sheet, &c.name).unwrap();
        for t_c in [28.0, 60.0, 100.0, 140.0] {
            let dt = t_c - 28.0;
            let d = m.at_temperature(t_c + ZERO_C_K).unwrap().at(1367.0);
            let voc = c.voc_v + c.dvoc_dt_v_per_k * dt;
            let isc = c.isc_a + c.disc_dt_a_per_k * dt;
            let vmp = c.vmp_v + c.dvmp_dt_v_per_k * dt;
            let imp = c.imp_a + c.dimp_dt_a_per_k * dt;
            let (mv, mi) = d.mpp();
            assert!(rel(d.voc(), voc) < 1e-12, "{} {t_c} °C Voc", c.name);
            assert!(rel(d.i_at(0.0), isc) < 1e-6, "{} {t_c} °C Isc", c.name);
            let (ev, ei, ep) = (rel(mv, vmp), rel(mi, imp), rel(mv * mi, vmp * imp));
            worst = worst.max(ev).max(ei);
            assert!(ep < 0.01, "{} {t_c} °C Pmp {ep}", c.name);
            println!(
                "{:>7} {t_c:>5} °C: Vmp {:+.2} %, Imp {:+.2} %, Pmp {:+.2} % vs datasheet coefficients",
                c.name,
                100.0 * (mv - vmp) / vmp,
                100.0 * (mi - imp) / imp,
                100.0 * (mv * mi - vmp * imp) / (vmp * imp)
            );
        }
    }
    assert!(worst < 0.025, "worst Vmp/Imp {worst}");
}

/// Irradiance scaling: Isc ∝ G exactly (Rs·Gsh = 0 here); Voc drops by
/// a·ln 2 at half light, to the I0/Iph correction.
#[test]
fn irradiance_scaling() {
    let m = CellModel::fit(&azur(), "BOL").unwrap();
    let c = m.at_temperature(m.reference_temperature_k).unwrap();
    let (full, half) = (c.at(1367.0), c.at(683.5));
    assert!(rel(half.i_at(0.0), 0.5 * full.i_at(0.0)) < 1e-9);
    let drop = full.voc() - half.voc();
    assert!(rel(drop, full.a_v * 2f64.ln()) < 1e-6, "{drop}");
    // Dark cell: no current at 0 V, no voltage open-circuit.
    let dark = c.at(0.0);
    assert!(dark.i_at(0.0).abs() < 1e-24, "{}", dark.i_at(0.0));
    assert_eq!(dark.voc(), 0.0);
}

/// Invariants: I(V) non-increasing (to 1 ulp on the flat part), P ≥ 0 on
/// [0, Voc].
#[test]
fn iv_curve_invariants() {
    let d = CellModel::fit(&azur(), "1E15").unwrap().reference;
    let mut prev = f64::INFINITY;
    for k in 0..=1000 {
        let v = d.voc() * k as f64 / 1000.0;
        let i = d.i_at(v);
        assert!(i <= prev + 1e-16, "increasing at {v}");
        assert!(v * i >= -1e-15, "negative power at {v}");
        prev = i;
    }
}

#[test]
fn errors_name_what_is_wrong() {
    let e = CellDatasheet::load("no-such-cell", Path::new(".")).unwrap_err();
    assert!(e.to_string().contains("azur-3g30a-advanced-4x8"), "{e}");
    let e = CellModel::fit(&azur(), "1E99").unwrap_err();
    assert!(e.to_string().contains("BOL"), "{e}");
    let mut bad = azur();
    bad.conditions[0].vmp_v = 3.0;
    assert!(CellModel::fit(&bad, "BOL").is_err());
}

/// Area scaling: a cell r times larger has I(V) exactly r times larger
/// (Iph, I0, Gsh ∝ area, Rs ∝ 1/area), so Isc and Pmp scale by r and Voc
/// and Vmp are unchanged.
#[test]
fn area_scaling_multiplies_current() {
    let m = CellModel::fit(&azur(), "BOL").unwrap();
    let c = m.at_temperature(m.reference_temperature_k).unwrap();
    for r in [0.12, 1.06, 2.65] {
        let (a, b) = (c.at(1361.0), c.scaled(r).at(1361.0));
        for k in 0..=50 {
            let v = a.voc() * k as f64 / 50.0;
            assert!(
                (b.i_at(v) - r * a.i_at(v)).abs() < 1e-12 * r,
                "r {r}, v {v}"
            );
        }
        assert!((b.voc() - a.voc()).abs() < 1e-12);
        let ((va, ia), (vb, ib)) = (a.mpp(), b.mpp());
        assert!((vb * ib - r * va * ia).abs() < 1e-12 * r);
    }
}

/// Far below the datasheet range the model is clamped, not overflowing.
#[test]
fn extreme_cold_stays_finite() {
    let r = CellModel::fit(&azur(), "BOL").unwrap().temperature_ref();
    for t in [0.0, 3.0, 40.0, MIN_CELL_TEMPERATURE_K] {
        let d = r.at(t).at(1361.0);
        assert!(d.voc().is_finite() && d.mpp().0.is_finite(), "{t} K");
        assert_eq!(r.at(t).temperature_k, MIN_CELL_TEMPERATURE_K);
    }
}

/// IMM-α (shunt fit) against its datasheet temperature coefficients over
/// 28–140 °C. Voc follows its coefficient exactly by construction; Isc
/// does too, since Rs = 0 makes Isc = Iph. Vmp, Imp and Pmp are model
/// predictions. The sheet marks its coefficients as projected from
/// similar devices, so this measures how far apart they are.
///
/// Measured (2026-11-14): BOL Pmp within +0.97 % up to 140 °C; 1E15 within
/// 1.3 %; 5E15 within 0.7 %; 5E14 reaches +2.9 % at 100 °C and +4.7 % at
/// 140 °C. Why: with Rs and Rsh fixed, the Voc and Isc coefficients fix
/// the model's temperature behaviour, so the Vmp/Imp coefficients can only
/// be predicted, not imposed. The projected 5E14 values ask the fill factor
/// to fall faster with temperature than the curve does (dVmp/dT −12.5 vs
/// dVoc/dT −11.7 mV/°C, dJmp/dT 5.2 vs dJsc/dT 9.9 µA/cm²/°C), and the
/// projected Jmp coefficients are not even monotonic in fluence (6.7, 5.2,
/// 3.3, 7.6). Bounds: BOL < 1 % (the measured AZUR cell gets 0.8 %), any
/// condition < 5 %. Revisit with measured coefficients.
#[test]
fn imm_alpha_temperature_vs_projected_coefficients() {
    let sheet = CellDatasheet::load("solaero-imm-alpha", Path::new(".")).unwrap();
    let mut worst_p = 0.0f64;
    let mut worst_bol = 0.0f64;
    for c in &sheet.conditions {
        let m = CellModel::fit(&sheet, &c.name).unwrap();
        for t_c in [28.0, 60.0, 100.0, 140.0] {
            let dt = t_c - 28.0;
            let d = m
                .at_temperature(t_c + ZERO_C_K)
                .unwrap()
                .at(sheet.reference_irradiance_w_m2);
            let voc = c.voc_v + c.dvoc_dt_v_per_k * dt;
            let isc = c.isc_a + c.disc_dt_a_per_k * dt;
            let vmp = c.vmp_v + c.dvmp_dt_v_per_k * dt;
            let imp = c.imp_a + c.dimp_dt_a_per_k * dt;
            let (mv, mi) = d.mpp();
            assert!(rel(d.voc(), voc) < 1e-12, "{} {t_c} °C Voc", c.name);
            assert!(rel(d.i_at(0.0), isc) < 1e-9, "{} {t_c} °C Isc", c.name);
            let ep = (mv * mi - vmp * imp) / (vmp * imp);
            worst_p = worst_p.max(ep.abs());
            if c.name == "BOL" {
                worst_bol = worst_bol.max(ep.abs());
            }
            println!(
                "IMM-α {:>5} {t_c:>5} °C: Vmp {:+.2} %, Imp {:+.2} %, Pmp {:+.2} % vs projected coefficients",
                c.name,
                100.0 * (mv - vmp) / vmp,
                100.0 * (mi - imp) / imp,
                100.0 * ep
            );
        }
    }
    println!("IMM-α worst Pmp deviation {:.2} %", 100.0 * worst_p);
    assert!(worst_bol < 0.01, "BOL worst Pmp {worst_bol}");
    assert!(worst_p < 0.05, "worst Pmp {worst_p}");
}
