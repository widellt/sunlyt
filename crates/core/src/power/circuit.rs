//! Strings of cells and their regulation (design D4).
//!
//! A string is cells in series, each with its own bypass diode, feeding the
//! bus through a blocking diode. Series means equal current, so the string
//! voltage at current I is the sum of the cell voltages:
//!
//! V_string(I) = Σ max(V_cell(I; Gᵢ), −Vf_bypass)
//!
//! A cell that cannot carry I forward (I above its photocurrent, e.g.
//! shaded) is reverse-biased until its bypass diode conducts and holds it
//! at −Vf. V_string is continuous and non-increasing in I, so the operating
//! point is a 1-D root (fixed bus) or a 1-D maximum per smooth interval
//! (MPPT).
//!
//! Deviation from design D4: instead of composing piecewise-linear I-V
//! curves, each string is solved exactly with safeguarded Newton on
//! V_string(I). This has no discretisation error and costs a few
//! evaluations per distinct irradiance in the string (cells under the same
//! light are grouped). Strings in parallel on a fixed bus are independent,
//! so no network solve is needed; MPPT here is an ideal tracker per
//! string, which bounds what any real MPPT can deliver.
//!
//! Neglected: the bypass diode's I-V curve (a constant Vf clamp), harness
//! resistance, and per-k-cell bypass groups (one diode per cell, as on the
//! assembly datasheets).

use std::path::Path;

use serde::Deserialize;

use super::cell::{
    CellAtTemp, CellDatasheet, CellError, CellModel, CellTempRef, ZERO_C_K, golden_max,
};

/// How a section's power is taken off the strings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Regulation {
    /// Strings are clamped to a fixed bus (direct energy transfer or a
    /// sequential shunt): V_string = bus + blocking-diode drop.
    FixedBus { bus_v: f64 },
    /// Ideal maximum-power tracker per string (efficiency 1). The real
    /// vehicle's MPPT is v2; this is the upper bound.
    Mppt,
}

/// Electrical parameters shared by all strings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circuit {
    /// The cell at the configured (fixed) temperature.
    pub cell: CellAtTemp,
    /// How to move the cell to another temperature; `None` keeps `cell`.
    pub temperature_ref: Option<CellTempRef>,
    /// The datasheet cell's area, m². Layout cells of another size are
    /// scaled to their own area ([`CellAtTemp::scaled`]); `None` uses the
    /// cell as is.
    pub cell_area_m2: Option<f64>,
    pub bypass_vf_v: f64,
    pub blocking_vf_v: f64,
    pub regulation: Regulation,
}

impl Circuit {
    /// The same circuit with its cells at `t_k` (when it knows how to
    /// translate them; see [`CellTempRef::at`] for the clamp).
    pub fn with_temperature(&self, t_k: f64) -> Circuit {
        match self.temperature_ref {
            Some(r) => Circuit {
                cell: r.at(t_k),
                ..*self
            },
            None => *self,
        }
    }

    /// The same circuit for layout cells of `cell_area_m2`.
    pub fn for_cell_area(&self, cell_area_m2: f64) -> Circuit {
        match self.cell_area_m2 {
            Some(a) if a > 0.0 && cell_area_m2 != a => Circuit {
                cell: self.cell.scaled(cell_area_m2 / a),
                ..*self
            },
            _ => *self,
        }
    }
}

/// Operating point of one string.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StringPoint {
    pub current_a: f64,
    /// String terminal voltage, before the blocking diode.
    pub voltage_v: f64,
    /// Power delivered past the blocking diode: I · (V − Vf_blocking).
    pub power_w: f64,
}

impl Circuit {
    /// String voltage at current `i` and its slope dV/dI. `groups` lists
    /// (effective irradiance W/m², number of cells).
    pub fn string_v(&self, groups: &[(f64, u32)], i: f64) -> (f64, f64) {
        let (mut v, mut dv) = (0.0, 0.0);
        for &(g, n) in groups {
            let d = self.cell.at(g);
            let vc = d.v_at(i);
            if vc > -self.bypass_vf_v {
                v += n as f64 * vc;
                dv += n as f64 * d.dv_di(i);
            } else {
                v -= n as f64 * self.bypass_vf_v;
            }
        }
        (v, dv)
    }

    /// Operating point of a string under the configured regulation.
    pub fn solve(&self, groups: &[(f64, u32)]) -> StringPoint {
        match self.regulation {
            Regulation::FixedBus { bus_v } => self.at_voltage(groups, bus_v + self.blocking_vf_v),
            Regulation::Mppt => self.mpp(groups),
        }
    }

    /// The string current where V_string = `target_v` (> 0). Zero if even
    /// open circuit is below it: the blocking diode stops reverse current.
    pub fn at_voltage(&self, groups: &[(f64, u32)], target_v: f64) -> StringPoint {
        let point = |i: f64, v: f64| StringPoint {
            current_a: i,
            voltage_v: v,
            power_w: i * (v - self.blocking_vf_v),
        };
        let (v0, _) = self.string_v(groups, 0.0);
        if v0 <= target_v {
            return point(0.0, v0);
        }
        // Above the largest photocurrent every cell is bypassed or reverse
        // biased, so V_string < 0 < target.
        let iph_max = groups
            .iter()
            .map(|&(g, _)| self.cell.at(g).iph_a)
            .fold(0.0, f64::max);
        // Safeguarded Newton. Convergence is judged only by the bracket or
        // the residual, never by the step: near a cell's photocurrent V(I)
        // is steep, so Newton steps there are tiny while still far from the
        // root. A Newton step that fails to halve the bracket is replaced by
        // bisection, which bounds the work at ~2 × 52 iterations.
        let (mut lo, mut hi) = (0.0, iph_max * (1.0 + 1e-9) + 1e-12);
        let mut i = 0.5 * (lo + hi);
        for _ in 0..200 {
            let (v, dv) = self.string_v(groups, i);
            let f = v - target_v;
            if f.abs() <= 1e-13 * target_v {
                break;
            }
            let width = hi - lo;
            if f > 0.0 {
                lo = i;
            } else {
                hi = i;
            }
            if hi - lo <= 4.0 * f64::EPSILON * hi {
                i = 0.5 * (lo + hi);
                break;
            }
            let newton = i - f / dv;
            i = if dv < 0.0 && newton > lo && newton < hi && hi - lo <= 0.5 * width {
                newton
            } else {
                0.5 * (lo + hi)
            };
        }
        point(i, target_v)
    }

    /// The string's I-V curve as `n` or more points (current A, string
    /// voltage V before the blocking diode), current ascending from 0 to
    /// just past the largest photocurrent, where every cell is bypassed.
    ///
    /// Each cell's voltage falls steeply as the current nears its
    /// photocurrent, so each interval between photocurrent levels gets its
    /// share of points, packed cubically towards the knee at its top. The
    /// points lie exactly on [`string_v`](Self::string_v); only drawing
    /// straight lines between them approximates.
    pub fn string_curve(&self, groups: &[(f64, u32)], n: usize) -> Vec<(f64, f64)> {
        let mut levels: Vec<f64> = groups
            .iter()
            .map(|&(g, _)| self.cell.at(g).iph_a)
            .filter(|&l| l > 0.0)
            .collect();
        levels.sort_by(f64::total_cmp);
        levels.dedup();
        let Some(&top) = levels.last() else {
            return vec![(0.0, self.string_v(groups, 0.0).0)];
        };
        let per = (n.max(2) / levels.len()).max(2);
        let mut out = Vec::with_capacity(per * levels.len() + 2);
        let mut lo = 0.0;
        for &hi in &levels {
            for k in 0..per {
                let s = k as f64 / per as f64;
                let i = lo + (hi - lo) * (1.0 - (1.0 - s).powi(3));
                out.push((i, self.string_v(groups, i).0));
            }
            lo = hi;
        }
        let end = top * (1.0 + 1e-9) + 1e-12;
        out.push((end, self.string_v(groups, end).0));
        out
    }

    /// Ideal tracker: the global maximum of I · (V_string(I) − Vf_blocking).
    /// With partial shading P(I) has one local maximum per distinct
    /// photocurrent level, so each interval between levels is searched.
    pub fn mpp(&self, groups: &[(f64, u32)]) -> StringPoint {
        let mut levels: Vec<f64> = groups
            .iter()
            .map(|&(g, _)| self.cell.at(g).iph_a)
            .filter(|&l| l > 0.0)
            .collect();
        levels.sort_by(f64::total_cmp);
        levels.dedup();
        let p = |i: f64| i * (self.string_v(groups, i).0 - self.blocking_vf_v);
        let mut best = (0.0, 0.0);
        let mut lo = 0.0;
        for &hi in &levels {
            let i = golden_max(p, lo, hi);
            let pi = p(i);
            if pi > best.1 {
                best = (i, pi);
            }
            lo = hi;
        }
        let i = best.0;
        StringPoint {
            current_a: i,
            voltage_v: self.string_v(groups, i).0,
            power_w: best.1,
        }
    }
}

/// `[electrical]` in the scene TOML.
///
/// ```toml
/// [electrical]
/// cell = "azur-3g30a-advanced-4x8"   # built-in preset, or a .toml path
/// condition = "BOL"                  # or a fluence listed in the preset
/// temperature_c = 28.0               # fixed until the thermal model
/// regulation = "fixed-bus"           # or "mppt"
/// bus_v = 28.0
/// blocking_vf_v = 0.7
/// # bypass_vf_v = 0.8                # default: the cell datasheet's value
/// ```
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ElectricalConfig {
    pub cell: String,
    #[serde(default = "bol")]
    pub condition: String,
    #[serde(default = "t28")]
    pub temperature_c: f64,
    #[serde(default)]
    pub regulation: RegulationKind,
    #[serde(default = "v28")]
    pub bus_v: f64,
    /// Default 0.7 V: a typical silicon PN diode near 1 A. Not from a
    /// datasheet; set it from your harness design.
    #[serde(default = "v07")]
    pub blocking_vf_v: f64,
    #[serde(default)]
    pub bypass_vf_v: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum RegulationKind {
    #[default]
    FixedBus,
    Mppt,
}

fn bol() -> String {
    "BOL".into()
}
fn t28() -> f64 {
    28.0
}
fn v28() -> f64 {
    28.0
}
fn v07() -> f64 {
    0.7
}

impl ElectricalConfig {
    /// The `[electrical]` table of a scene TOML, if it has one.
    pub fn from_scene_file(path: &Path) -> Result<Option<Self>, CellError> {
        let text = std::fs::read_to_string(path).map_err(|e| CellError::Io {
            path: path.display().to_string(),
            source: e,
        })?;
        Self::from_scene_str(&text)
    }

    pub fn from_scene_str(text: &str) -> Result<Option<Self>, CellError> {
        let mut table: toml::Table =
            toml::from_str(text).map_err(|e| CellError::Config(e.to_string()))?;
        match table.remove("electrical") {
            None => Ok(None),
            Some(v) => v
                .try_into()
                .map(Some)
                .map_err(|e: toml::de::Error| CellError::Config(format!("[electrical]: {e}"))),
        }
    }

    /// Loads and fits the cell (`base_dir` resolves a `.toml` cell path)
    /// and builds the circuit.
    pub fn build(&self, base_dir: &Path) -> Result<(CellModel, Circuit), CellError> {
        let sheet = CellDatasheet::load(&self.cell, base_dir)?;
        let model = CellModel::fit(&sheet, &self.condition)?;
        let cell = model.at_temperature(self.temperature_c + ZERO_C_K)?;
        let bypass_vf_v = self.bypass_vf_v.or(model.bypass_vf_v).unwrap_or(0.8);
        if !(bypass_vf_v > 0.0 && self.blocking_vf_v >= 0.0) {
            return Err(CellError::Config(
                "bypass_vf_v must be > 0 and blocking_vf_v ≥ 0".into(),
            ));
        }
        let regulation = match self.regulation {
            RegulationKind::FixedBus if self.bus_v > 0.0 => {
                Regulation::FixedBus { bus_v: self.bus_v }
            }
            RegulationKind::FixedBus => {
                return Err(CellError::Config("bus_v must be positive".into()));
            }
            RegulationKind::Mppt => Regulation::Mppt,
        };
        let temperature_ref = Some(model.temperature_ref());
        let cell_area_m2 = Some(model.area_m2);
        Ok((
            model,
            Circuit {
                cell,
                temperature_ref,
                cell_area_m2,
                bypass_vf_v,
                blocking_vf_v: self.blocking_vf_v,
                regulation,
            },
        ))
    }
}

/// Groups cell irradiances into (value, count), in place of `out`. Equal
/// values are common: cells on one panel share the incidence angle, and
/// self-shadowing is quantised to 1/n² per cell.
pub fn group_irradiance(g: &mut [f64], out: &mut Vec<(f64, u32)>) {
    out.clear();
    g.sort_unstable_by(f64::total_cmp);
    for &x in g.iter() {
        match out.last_mut() {
            Some((v, n)) if *v == x => *n += 1,
            _ => out.push((x, 1)),
        }
    }
}

#[cfg(test)]
mod tests;
