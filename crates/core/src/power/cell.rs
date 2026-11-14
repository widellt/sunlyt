//! Solar cell: the single-diode model fitted to datasheet points (design D4).
//!
//! I = Iph − I0·(exp((V + I·Rs)/a) − 1) − (V + I·Rs)·Gsh
//!
//! with a = n·kT/q summed over the junctions (a triple-junction cell has
//! n ≈ 3–4), Rs the series resistance and Gsh = 1/Rsh the shunt
//! conductance. Space datasheets give four points (Isc, Voc, Vmp/Imp) and
//! the fact that Pmp is a maximum, so with Gsh given (default 0, an ideal
//! shunt) the remaining four parameters are fixed exactly by them.
//!
//! Translation to other conditions:
//! - Iph ∝ irradiance and follows the datasheet ΔIsc/ΔT.
//! - a ∝ T (kelvin).
//! - I0 is set so that Voc follows the datasheet ΔVoc/ΔT exactly at the
//!   reference irradiance.
//! - Rs is constant; Gsh ∝ irradiance (De Soto et al. 2006).
//!
//! What this neglects:
//! - the spectrum (AM0 WRC is assumed, as on the datasheet) and current
//!   mismatch between the junctions;
//! - the Vmp and Imp temperature coefficients, which come out of the model
//!   instead and are checked against the datasheet in the tests;
//! - reverse breakdown: a cell driven past Iph is clamped by its bypass
//!   diode (see `circuit`), never by its own breakdown.

use std::path::Path;

use serde::Deserialize;

/// 0 °C in kelvin.
pub const ZERO_C_K: f64 = 273.15;

#[derive(Debug, thiserror::Error)]
pub enum CellError {
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("cell datasheet: {0}")]
    Config(String),
    #[error("unknown cell preset {0:?}; built-in presets: {1}")]
    UnknownPreset(String, String),
    #[error("cell {cell:?} has no condition {name:?}; it has: {have}")]
    UnknownCondition {
        cell: String,
        name: String,
        have: String,
    },
    #[error("fitting {0}")]
    Fit(String),
}

/// Built-in cell presets, by name.
const PRESETS: &[(&str, &str)] = &[
    (
        "azur-3g30a-advanced-4x8",
        include_str!("../../presets/cells/azur-3g30a-advanced-4x8.toml"),
    ),
    (
        "solaero-imm-alpha",
        include_str!("../../presets/cells/solaero-imm-alpha.toml"),
    ),
];

/// A cell datasheet: test conditions plus one or more sets of I-V points
/// (BOL, and after given radiation fluences).
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CellDatasheet {
    pub name: String,
    /// Where the numbers come from (document number, revision, page).
    pub source: String,
    pub area_cm2: f64,
    pub reference_irradiance_w_m2: f64,
    pub reference_temperature_c: f64,
    /// Forward voltage of the cell's bypass diode, if the datasheet gives it.
    #[serde(default)]
    pub bypass_vf_v: Option<f64>,
    /// Shunt resistance, if known; default: ideal (infinite).
    #[serde(default)]
    pub rsh_ohm: Option<f64>,
    #[serde(rename = "condition")]
    pub conditions: Vec<DatasheetPoint>,
}

/// I-V points and temperature coefficients at one condition, as printed.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DatasheetPoint {
    pub name: String,
    pub voc_v: f64,
    pub isc_a: f64,
    pub vmp_v: f64,
    pub imp_a: f64,
    pub dvoc_dt_v_per_k: f64,
    pub disc_dt_a_per_k: f64,
    pub dvmp_dt_v_per_k: f64,
    pub dimp_dt_a_per_k: f64,
}

impl CellDatasheet {
    pub fn preset_names() -> impl Iterator<Item = &'static str> {
        PRESETS.iter().map(|(n, _)| *n)
    }

    pub fn from_toml_str(s: &str) -> Result<Self, CellError> {
        let d: Self = toml::from_str(s).map_err(|e| CellError::Config(e.to_string()))?;
        if d.conditions.is_empty() {
            return Err(CellError::Config("no [[condition]]".into()));
        }
        if !(d.area_cm2 > 0.0 && d.reference_irradiance_w_m2 > 0.0) {
            return Err(CellError::Config(
                "area and reference irradiance must be positive".into(),
            ));
        }
        Ok(d)
    }

    /// A built-in preset by name, or a TOML file (a name ending in `.toml`,
    /// relative to `base_dir`).
    pub fn load(name_or_path: &str, base_dir: &Path) -> Result<Self, CellError> {
        if name_or_path.ends_with(".toml") {
            let p = base_dir.join(name_or_path);
            let s = std::fs::read_to_string(&p).map_err(|e| CellError::Io {
                path: p.display().to_string(),
                source: e,
            })?;
            return Self::from_toml_str(&s);
        }
        let (_, text) = PRESETS
            .iter()
            .find(|(n, _)| *n == name_or_path)
            .ok_or_else(|| {
                CellError::UnknownPreset(
                    name_or_path.into(),
                    Self::preset_names().collect::<Vec<_>>().join(", "),
                )
            })?;
        Self::from_toml_str(text)
    }

    pub fn condition(&self, name: &str) -> Result<&DatasheetPoint, CellError> {
        self.conditions
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| CellError::UnknownCondition {
                cell: self.name.clone(),
                name: name.into(),
                have: self
                    .conditions
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            })
    }
}

/// The five single-diode parameters at one irradiance and temperature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiodeParams {
    /// Photocurrent, A.
    pub iph_a: f64,
    /// Diode saturation current, A.
    pub i0_a: f64,
    /// Modified ideality factor n·kT/q (summed over junctions), V.
    pub a_v: f64,
    /// Series resistance, Ω.
    pub rs_ohm: f64,
    /// Shunt conductance 1/Rsh, S (0 = ideal shunt).
    pub gsh_s: f64,
}

impl DiodeParams {
    /// Residual of the diode equation at (v, i): model current minus `i`.
    pub fn current_residual(&self, v: f64, i: f64) -> f64 {
        let x = v + i * self.rs_ohm;
        self.iph_a - self.i0_a * (x / self.a_v).exp_m1() - x * self.gsh_s - i
    }

    /// dI/dV of the curve at the point (v, i).
    pub fn di_dv(&self, v: f64, i: f64) -> f64 {
        let x = v + i * self.rs_ohm;
        let g = self.i0_a / self.a_v * (x / self.a_v).exp() + self.gsh_s;
        -g / (1.0 + self.rs_ohm * g)
    }

    /// Diode voltage V + I·Rs at current `i`. −∞ with an ideal shunt when
    /// `i` ≥ Iph + I0 (the cell cannot carry that current forward).
    pub fn diode_v(&self, i: f64) -> f64 {
        let b = self.iph_a - i + self.i0_a;
        if self.gsh_s == 0.0 {
            return if b > 0.0 {
                self.a_v * (b / self.i0_a).ln()
            } else {
                f64::NEG_INFINITY
            };
        }
        // I0·e^(x/a) + Gsh·x = B  ⇒  x = B/Gsh − a·W((I0/(Gsh·a))·e^(B/(Gsh·a))).
        let ga = self.gsh_s * self.a_v;
        let l = (self.i0_a / ga).ln() + b / ga;
        b / self.gsh_s - self.a_v * lambert_w_exp(l)
    }

    /// Terminal voltage at current `i` (explicit in I).
    pub fn v_at(&self, i: f64) -> f64 {
        self.diode_v(i) - i * self.rs_ohm
    }

    /// dV/dI at current `i` (negative). Undefined where `v_at` is −∞.
    pub fn dv_di(&self, i: f64) -> f64 {
        let x = self.diode_v(i);
        -1.0 / (self.i0_a / self.a_v * (x / self.a_v).exp() + self.gsh_s) - self.rs_ohm
    }

    /// Current at voltage `v`, in closed form with Lambert W
    /// (Jain & Kapoor 2004).
    pub fn i_at(&self, v: f64) -> f64 {
        let (a, rs) = (self.a_v, self.rs_ohm);
        if rs == 0.0 {
            return self.iph_a - self.i0_a * (v / a).exp_m1() - v * self.gsh_s;
        }
        let k = 1.0 + rs * self.gsh_s;
        let l = (rs * self.i0_a / (a * k)).ln() + (rs * (self.iph_a + self.i0_a) + v) / (a * k);
        (self.iph_a + self.i0_a - v * self.gsh_s) / k - a / rs * lambert_w_exp(l)
    }

    /// Current at voltage `v` by safeguarded Newton on the implicit
    /// equation: the independent reference for [`i_at`](Self::i_at).
    pub fn i_at_newton(&self, v: f64) -> f64 {
        let f = |i: f64| self.current_residual(v, i);
        // f is strictly decreasing in i: bracket the root, then iterate.
        let (mut lo, mut hi) = (-1.0, self.iph_a + 1.0);
        while f(lo) < 0.0 {
            lo *= 2.0;
        }
        while f(hi) > 0.0 {
            hi *= 2.0;
        }
        let mut i = 0.5 * (lo + hi);
        for _ in 0..200 {
            let fi = f(i);
            if fi > 0.0 {
                lo = i;
            } else {
                hi = i;
            }
            let x = v + i * self.rs_ohm;
            let d = -(self.i0_a / self.a_v * (x / self.a_v).exp() + self.gsh_s) * self.rs_ohm - 1.0;
            let mut next = i - fi / d;
            if !(next > lo && next < hi) {
                next = 0.5 * (lo + hi);
            }
            if (next - i).abs() <= 1e-15 * (1.0 + i.abs()) {
                return next;
            }
            i = next;
        }
        i
    }

    /// Open-circuit voltage.
    pub fn voc(&self) -> f64 {
        self.v_at(0.0)
    }

    /// Maximum power point (V, I), by golden-section search on P(V) over
    /// [0, Voc] (P is unimodal there for one cell).
    pub fn mpp(&self) -> (f64, f64) {
        let p = |v: f64| v * self.i_at(v);
        let v = golden_max(p, 0.0, self.voc());
        (v, self.i_at(v))
    }
}

/// W(eˡ): the principal Lambert W of exp(l), computed without forming
/// exp(l) (which overflows for the shunt form). Solves w + ln w = l by
/// Newton on u = ln w; g(u) = eᵘ + u − l is convex and increasing, and the
/// start point has g > 0, so the iteration decreases monotonically to the
/// root.
pub fn lambert_w_exp(l: f64) -> f64 {
    let mut u = if l > 1.0 { l.ln() } else { l };
    for _ in 0..100 {
        let w = u.exp();
        let du = (w + u - l) / (w + 1.0);
        u -= du;
        if du.abs() <= 1e-16 * u.abs().max(1.0) {
            break;
        }
    }
    u.exp()
}

/// Argmax of a unimodal function on [lo, hi], to about 1e-13 of the span.
pub(crate) fn golden_max(f: impl Fn(f64) -> f64, mut lo: f64, mut hi: f64) -> f64 {
    const R: f64 = 0.618_033_988_749_894_8; // (√5 − 1) / 2
    let mut x1 = hi - R * (hi - lo);
    let mut x2 = lo + R * (hi - lo);
    let (mut f1, mut f2) = (f(x1), f(x2));
    let tol = 1e-13 * (hi - lo).abs().max(1e-300);
    while hi - lo > tol {
        if f1 < f2 {
            lo = x1;
            x1 = x2;
            f1 = f2;
            x2 = lo + R * (hi - lo);
            f2 = f(x2);
        } else {
            hi = x2;
            x2 = x1;
            f2 = f1;
            x1 = hi - R * (hi - lo);
            f1 = f(x1);
        }
    }
    0.5 * (lo + hi)
}

/// A cell fitted at its datasheet reference condition.
#[derive(Debug, Clone, PartialEq)]
pub struct CellModel {
    pub name: String,
    pub condition: DatasheetPoint,
    pub area_m2: f64,
    pub reference_irradiance_w_m2: f64,
    pub reference_temperature_k: f64,
    pub bypass_vf_v: Option<f64>,
    /// Parameters at the reference irradiance and temperature.
    pub reference: DiodeParams,
}

impl CellModel {
    pub fn fit(sheet: &CellDatasheet, condition: &str) -> Result<Self, CellError> {
        let point = sheet.condition(condition)?;
        let gsh = match sheet.rsh_ohm {
            Some(r) if r > 0.0 => 1.0 / r,
            Some(_) => return Err(CellError::Config("rsh_ohm must be positive".into())),
            None => 0.0,
        };
        // Ideal shunt (or the file's Rsh) and a series resistance first;
        // when the points are too square for that, Rs = 0 and a fitted
        // shunt (see `fit_point_shunt`).
        let reference = match (fit_point(point, gsh), sheet.rsh_ohm) {
            (Ok(p), _) => Ok(p),
            (Err(e), Some(_)) => Err(e),
            (Err(e), None) => fit_point_shunt(point).map_err(|e2| format!("{e}; {e2}")),
        }
        .map_err(|m| CellError::Fit(format!("{} {}: {m}", sheet.name, point.name)))?;
        Ok(Self {
            name: sheet.name.clone(),
            condition: point.clone(),
            area_m2: sheet.area_cm2 * 1e-4,
            reference_irradiance_w_m2: sheet.reference_irradiance_w_m2,
            reference_temperature_k: sheet.reference_temperature_c + ZERO_C_K,
            bypass_vf_v: sheet.bypass_vf_v,
            reference,
        })
    }

    /// The cell at temperature `t_k`, ready to scale with irradiance.
    pub fn at_temperature(&self, t_k: f64) -> Result<CellAtTemp, CellError> {
        self.temperature_ref().try_at(t_k).ok_or_else(|| {
            CellError::Fit(format!(
                "{t_k} K is outside the range where the linear temperature coefficients \
                 give a positive Isc and Voc"
            ))
        })
    }

    /// The part of the model that moves the cell between temperatures.
    pub fn temperature_ref(&self) -> CellTempRef {
        let p = &self.condition;
        CellTempRef {
            reference: self.reference,
            reference_temperature_k: self.reference_temperature_k,
            reference_irradiance_w_m2: self.reference_irradiance_w_m2,
            isc_a: p.isc_a,
            voc_v: p.voc_v,
            disc_dt_a_per_k: p.disc_dt_a_per_k,
            dvoc_dt_v_per_k: p.dvoc_dt_v_per_k,
        }
    }
}

/// What it takes to move a fitted cell to another temperature. Small and
/// `Copy`, so a circuit can carry it and re-evaluate the cell at each
/// panel's temperature every step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellTempRef {
    pub reference: DiodeParams,
    pub reference_temperature_k: f64,
    pub reference_irradiance_w_m2: f64,
    pub isc_a: f64,
    pub voc_v: f64,
    pub disc_dt_a_per_k: f64,
    pub dvoc_dt_v_per_k: f64,
}

impl CellTempRef {
    /// The cell at `t_k`, or `None` where the linear coefficients would give
    /// Isc ≤ 0 or Voc ≤ 0.
    pub fn try_at(&self, t_k: f64) -> Option<CellAtTemp> {
        let r = &self.reference;
        let dt = t_k - self.reference_temperature_k;
        let isc = self.isc_a + self.disc_dt_a_per_k * dt;
        let voc = self.voc_v + self.dvoc_dt_v_per_k * dt;
        if !(isc > 0.0 && voc > 0.0 && t_k > 0.0) {
            return None;
        }
        let a = r.a_v * t_k / self.reference_temperature_k;
        let iph = r.iph_a * isc / self.isc_a;
        let i0 = (iph - voc * r.gsh_s) / (voc / a).exp_m1();
        Some(CellAtTemp {
            temperature_k: t_k,
            reference_irradiance_w_m2: self.reference_irradiance_w_m2,
            at_reference: DiodeParams {
                iph_a: iph,
                i0_a: i0,
                a_v: a,
                rs_ohm: r.rs_ohm,
                gsh_s: r.gsh_s,
            },
        })
    }

    /// The cell at `t_k`, with `t_k` clamped to [`MIN_CELL_TEMPERATURE_K`]
    /// and to where Isc and Voc stay at or above 10 % of their reference
    /// values (about +400 °C for space cells). Datasheet coefficients cover
    /// 28–140 °C; the clamp only keeps the model finite far outside that
    /// (below ~70 K, a ∝ T makes exp(Voc/a) overflow).
    pub fn at(&self, t_k: f64) -> CellAtTemp {
        let (mut lo, mut hi) = (
            MIN_CELL_TEMPERATURE_K - self.reference_temperature_k,
            f64::INFINITY,
        );
        for (x, c) in [
            (self.isc_a, self.disc_dt_a_per_k),
            (self.voc_v, self.dvoc_dt_v_per_k),
        ] {
            if c < 0.0 {
                hi = hi.min(0.9 * x / -c);
            } else if c > 0.0 {
                lo = lo.max(-0.9 * x / c);
            }
        }
        let dt = (t_k - self.reference_temperature_k).clamp(lo, hi);
        self.try_at(self.reference_temperature_k + dt)
            .expect("clamped temperature keeps Isc and Voc positive")
    }
}

/// A cell at one temperature; [`at`](Self::at) scales it to an irradiance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellAtTemp {
    pub temperature_k: f64,
    pub reference_irradiance_w_m2: f64,
    /// Parameters at the reference irradiance.
    pub at_reference: DiodeParams,
}

/// Coldest temperature the cell model is evaluated at, K (−203 °C). Colder
/// panels are possible in long eclipses far from planets, but the linear
/// datasheet coefficients mean nothing there; the cells produce nothing in
/// the dark anyway.
pub const MIN_CELL_TEMPERATURE_K: f64 = 70.0;

impl CellAtTemp {
    /// The same technology in a cell `ratio` times the datasheet cell's
    /// area: Iph, I0 and Gsh scale with area, Rs inversely; a (and so Voc)
    /// is unchanged.
    pub fn scaled(&self, ratio: f64) -> CellAtTemp {
        let r = &self.at_reference;
        CellAtTemp {
            at_reference: DiodeParams {
                iph_a: r.iph_a * ratio,
                i0_a: r.i0_a * ratio,
                gsh_s: r.gsh_s * ratio,
                rs_ohm: r.rs_ohm / ratio,
                a_v: r.a_v,
            },
            ..*self
        }
    }

    /// Parameters at effective irradiance `g_w_m2` (incidence, eclipse and
    /// shadowing already applied).
    #[inline]
    pub fn at(&self, g_w_m2: f64) -> DiodeParams {
        let s = (g_w_m2 / self.reference_irradiance_w_m2).max(0.0);
        DiodeParams {
            iph_a: self.at_reference.iph_a * s,
            gsh_s: self.at_reference.gsh_s * s,
            ..self.at_reference
        }
    }
}

/// Fits (Iph, I0, a, Rs) for a given shunt conductance so the curve passes
/// through (0, Isc), (Voc, 0) and (Vmp, Imp) with dP/dV = 0 at Vmp.
///
/// For a trial (a, Rs), Iph and I0 follow linearly from the Isc and Voc
/// points. The current through (Vmp, Imp) decreases with Rs, so Rs is found
/// by bisection for each a. The MPP slope condition is then bracketed on a
/// log grid of a and bisected. Nested bisection is slower than 2-D Newton
/// but cannot diverge, and the fit runs once per run.
fn fit_point(p: &DatasheetPoint, gsh: f64) -> Result<DiodeParams, String> {
    let (voc, isc, vmp, imp) = (p.voc_v, p.isc_a, p.vmp_v, p.imp_a);
    if !(0.0 < vmp && vmp < voc && 0.0 < imp && imp < isc) {
        return Err("needs 0 < Vmp < Voc and 0 < Imp < Isc".into());
    }
    let params = |a: f64, rs: f64| {
        let e_oc = (voc / a).exp();
        let e_sc = (isc * rs / a).exp();
        let i0 = (isc * (1.0 + rs * gsh) - voc * gsh) / (e_oc - e_sc);
        DiodeParams {
            iph_a: i0 * (e_oc - 1.0) + voc * gsh,
            i0_a: i0,
            a_v: a,
            rs_ohm: rs,
            gsh_s: gsh,
        }
    };
    let through_mpp = |a: f64, rs: f64| params(a, rs).current_residual(vmp, imp);
    let rs_max = (voc - vmp) / imp;
    let rs_of = |a: f64| -> Option<f64> {
        if through_mpp(a, 0.0) <= 0.0 {
            return None; // too soft: even Rs = 0 passes below the MPP
        }
        Some(bisect(|rs| through_mpp(a, rs), 0.0, rs_max))
    };
    // Feasible a: (0, a_max], where Rs = 0 passes exactly through the MPP.
    let a_lo = voc / 600.0; // keeps exp(Voc/a) finite
    if through_mpp(a_lo, 0.0) <= 0.0 {
        return Err("points are too square for a single diode".into());
    }
    let a_max = bisect(|a| through_mpp(a, 0.0), a_lo, voc);
    let slope = |a: f64| -> Option<f64> {
        let rs = rs_of(a)?;
        Some(params(a, rs).di_dv(vmp, imp) + imp / vmp)
    };
    const N: usize = 400;
    let grid = |k: usize| a_lo * (a_max / a_lo).powf(k as f64 / N as f64) * (1.0 - 1e-12);
    let mut prev: Option<(f64, f64)> = None;
    for k in 0..=N {
        let a = grid(k);
        let Some(s) = slope(a) else { continue };
        if let Some((a0, s0)) = prev
            && s0.signum() != s.signum()
        {
            let a = bisect(|a| slope(a).unwrap_or(f64::NAN), a0, a);
            let rs = rs_of(a).ok_or("lost the Rs root")?;
            return Ok(params(a, rs));
        }
        prev = Some((a, s));
    }
    Err("no ideality factor puts the maximum power at (Vmp, Imp)".into())
}

/// Fits (Iph, I0, a, Gsh) with Rs = 0 so the curve passes through (0, Isc),
/// (Voc, 0) and (Vmp, Imp) with dP/dV = 0 at Vmp.
///
/// Used when [`fit_point`] finds no solution with an ideal shunt: datasheet
/// *typical* values are averaged column by column, and for very square
/// cells (e.g. FF 0.85 at 4.8 V) even Rs = 0 puts the curve's MPP at a lower
/// voltage than the printed Vmp. A finite shunt bends the curve down near
/// the MPP and restores a fit; series resistance only makes it worse. Both
/// choices are the same five-parameter model, so the cell pins its four
/// datasheet conditions exactly either way.
///
/// With Rs = 0, Iph = Isc and I0 = (Isc − Voc·Gsh)/(e^(Voc/a) − 1). The
/// current through Vmp falls as Gsh rises, so Gsh is found by bisection for
/// each a, and the MPP slope condition is bracketed on a grid of a.
fn fit_point_shunt(p: &DatasheetPoint) -> Result<DiodeParams, String> {
    let (voc, isc, vmp, imp) = (p.voc_v, p.isc_a, p.vmp_v, p.imp_a);
    if !(0.0 < vmp && vmp < voc && 0.0 < imp && imp < isc) {
        return Err("needs 0 < Vmp < Voc and 0 < Imp < Isc".into());
    }
    let params = |a: f64, gsh: f64| DiodeParams {
        iph_a: isc,
        i0_a: (isc - voc * gsh) / ((voc / a).exp() - 1.0),
        a_v: a,
        rs_ohm: 0.0,
        gsh_s: gsh,
    };
    let through_mpp = |a: f64, gsh: f64| params(a, gsh).current_residual(vmp, imp);
    // Gsh < Isc/Voc keeps I0 > 0. At that limit I(Vmp) = Isc·(1 − Vmp/Voc),
    // which is below Imp for any real cell, so the bracket holds.
    let gsh_max = isc / voc * (1.0 - 1e-12);
    if through_mpp(voc, gsh_max) >= 0.0 {
        return Err("(Vmp, Imp) lies below the straight line from Isc to Voc".into());
    }
    let gsh_of = |a: f64| -> Option<f64> {
        if through_mpp(a, 0.0) <= 0.0 {
            return None; // already below the MPP with an ideal shunt
        }
        Some(bisect(|g| through_mpp(a, g), 0.0, gsh_max))
    };
    let a_lo = voc / 600.0;
    if through_mpp(a_lo, 0.0) <= 0.0 {
        return Err("points are too square for a single diode".into());
    }
    let a_max = bisect(|a| through_mpp(a, 0.0), a_lo, voc);
    let slope = |a: f64| -> Option<f64> {
        let g = gsh_of(a)?;
        Some(params(a, g).di_dv(vmp, imp) + imp / vmp)
    };
    const N: usize = 400;
    let grid = |k: usize| a_lo * (a_max / a_lo).powf(k as f64 / N as f64) * (1.0 - 1e-12);
    let mut prev: Option<(f64, f64)> = None;
    for k in 0..=N {
        let a = grid(k);
        let Some(s) = slope(a) else { continue };
        if let Some((a0, s0)) = prev
            && s0.signum() != s.signum()
        {
            let a = bisect(|a| slope(a).unwrap_or(f64::NAN), a0, a);
            let g = gsh_of(a).ok_or("lost the Gsh root")?;
            return Ok(params(a, g));
        }
        prev = Some((a, s));
    }
    Err(
        "no ideality factor puts the maximum power at (Vmp, Imp), with series or shunt resistance"
            .into(),
    )
}

/// Root of `f` on [lo, hi] by bisection, given a sign change; to the last
/// bit (≤ 200 halvings).
fn bisect(f: impl Fn(f64) -> f64, mut lo: f64, mut hi: f64) -> f64 {
    let s_lo = f(lo).signum();
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if mid <= lo || mid >= hi {
            break;
        }
        if f(mid).signum() == s_lo {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

#[cfg(test)]
mod tests;
