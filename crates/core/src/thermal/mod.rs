//! Panel temperature: one lumped node per panel (design D6).
//!
//! C · dT/dt = q_in − ε_f·σ·T⁴ − ε_b·σ·(T⁴ − T_sink⁴) − p_elec
//!
//! per unit panel area, with
//! - C the areal heat capacity (substrate + cells + coverglass), J/(m²·K);
//! - q_in the absorbed sunlight (front and, if exposed, back), planetary
//!   albedo and planetary IR, W/m² (see [`ThermalModel::absorbed`]);
//! - ε_f, ε_b the front and back IR emittances; the back radiates to deep
//!   space (T_sink = 0), or to a sink such as the bus interior for a
//!   body-mounted panel;
//! - p_elec the electrical power taken off the panel per unit area, which
//!   leaves as electricity rather than heat.
//!
//! Panels are independent (no conduction or radiation between them), so
//! each panel's history is integrated on its own, sequentially in time.
//! Between table rows the inputs are linear in time; the ODE is integrated
//! with classic RK4 on substeps no longer than a quarter of the radiative
//! time constant C / (4σ(ε_f + ε_b)T³).
//!
//! Neglected: temperature gradients across a panel (all cells share one
//! temperature), conduction to the bus, shadowing of the back side and of
//! planets by the spacecraft, and the IR the spacecraft's own surfaces
//! exchange with the panel.

pub mod planet;

use std::path::Path;

use serde::Deserialize;

use crate::geometry::Scene;
use crate::power::ArrayIllum;
use crate::power::cell::ZERO_C_K;
use crate::trajectory::Epoch;

pub use planet::{BodyRadiation, SurfaceEmission};

/// Stefan–Boltzmann constant, W/(m²·K⁴) (CODATA 2018).
pub const SIGMA: f64 = 5.670_374_419e-8;

/// The cosmic background, K: the coldest any panel can get here.
pub const T_SPACE_K: f64 = 2.7;

#[derive(Debug, thiserror::Error)]
pub enum ThermalError {
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("[thermal]: {0}")]
    Config(String),
}

/// `[thermal]` in the scene TOML. Without it, the cells stay at the
/// `[electrical]` temperature.
///
/// ```toml
/// [thermal]
/// heat_capacity_j_m2_k = 1800     # substrate + cells, per m² of panel
/// absorptance_front = 0.91        # cells with coverglass, solar band
/// emittance_front = 0.85          # coverglass, IR
/// absorptance_back = 0.90         # back face, solar band
/// emittance_back = 0.85           # back face, IR
/// # back_sink_c = 20              # body-mounted: back radiates to the bus
/// # initial_c = 20                # default: steady state at the first row
/// planets = true                  # albedo and IR from known bodies
/// ```
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ThermalConfig {
    /// Default 1800 J/(m²·K): ~1.0 kg/m² aluminium honeycomb panel
    /// (c ≈ 900 J/(kg·K)) plus ~0.9 kg/m² of cells, coverglass and adhesive
    /// (c ≈ 1000). Order-of-magnitude typical; set it from your design.
    #[serde(default = "c1800")]
    pub heat_capacity_j_m2_k: f64,
    /// Default 0.91: the cell assembly datasheet's absorptivity bound
    /// (AZUR SPACE DB 00010891-01, "≤ 0.91 with 100 µm AR coverglass").
    #[serde(default = "a091")]
    pub absorptance_front: f64,
    /// Default 0.85: typical ceria-doped coverglass. Set from your design.
    #[serde(default = "e085")]
    pub emittance_front: f64,
    /// Default 0.90: typical black-painted back. Set from your design.
    #[serde(default = "a090")]
    pub absorptance_back: f64,
    /// Default 0.85: typical black-painted back. Set from your design.
    #[serde(default = "e085")]
    pub emittance_back: f64,
    /// When set, the back faces the spacecraft: it radiates to this
    /// temperature and receives no sunlight or planet flux.
    #[serde(default)]
    pub back_sink_c: Option<f64>,
    /// Starting temperature; default: steady state at the first row.
    #[serde(default)]
    pub initial_c: Option<f64>,
    #[serde(default = "yes")]
    pub planets: bool,
}

fn c1800() -> f64 {
    1800.0
}
fn a091() -> f64 {
    0.91
}
fn a090() -> f64 {
    0.90
}
fn e085() -> f64 {
    0.85
}
fn yes() -> bool {
    true
}

impl ThermalConfig {
    pub fn from_scene_file(path: &Path) -> Result<Option<Self>, ThermalError> {
        let text = std::fs::read_to_string(path).map_err(|e| ThermalError::Io {
            path: path.display().to_string(),
            source: e,
        })?;
        Self::from_scene_str(&text)
    }

    pub fn from_scene_str(text: &str) -> Result<Option<Self>, ThermalError> {
        let mut table: toml::Table =
            toml::from_str(text).map_err(|e| ThermalError::Config(e.to_string()))?;
        match table.remove("thermal") {
            None => Ok(None),
            Some(v) => v
                .try_into()
                .map(Some)
                .map_err(|e: toml::de::Error| ThermalError::Config(e.to_string())),
        }
    }

    /// The model for a table whose bodies are named `body_names` (in body
    /// index order). Bodies without built-in properties contribute no
    /// albedo or IR; their names are returned so the caller can say so.
    pub fn build<'a>(
        &self,
        body_names: impl Iterator<Item = &'a str>,
    ) -> Result<(ThermalModel, Vec<String>), ThermalError> {
        let unit = |x: f64, what: &str| {
            if (0.0..=1.0).contains(&x) {
                Ok(x)
            } else {
                Err(ThermalError::Config(format!("{what} must be in 0–1")))
            }
        };
        if self.heat_capacity_j_m2_k.is_nan() || self.heat_capacity_j_m2_k <= 0.0 {
            return Err(ThermalError::Config(
                "heat_capacity_j_m2_k must be positive".into(),
            ));
        }
        let panel = PanelThermal {
            heat_capacity_j_m2_k: self.heat_capacity_j_m2_k,
            absorptance_front: unit(self.absorptance_front, "absorptance_front")?,
            emittance_front: unit(self.emittance_front, "emittance_front")?,
            absorptance_back: unit(self.absorptance_back, "absorptance_back")?,
            emittance_back: unit(self.emittance_back, "emittance_back")?,
            back_sink_k: self.back_sink_c.map(|c| c + ZERO_C_K),
        };
        if panel.emittance_front + panel.emittance_back == 0.0 {
            return Err(ThermalError::Config(
                "a panel with no emittance cannot cool".into(),
            ));
        }
        let mut unknown = Vec::new();
        let bodies = body_names
            .map(|n| {
                let r = BodyRadiation::for_name(n);
                if r.is_none() && self.planets {
                    unknown.push(n.to_string());
                }
                r.filter(|_| self.planets)
            })
            .collect();
        Ok((
            ThermalModel {
                panel,
                initial_k: self.initial_c.map(|c| c + ZERO_C_K),
                bodies,
            },
            unknown,
        ))
    }
}

/// Thermal properties of one panel (all panels share them for now).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelThermal {
    pub heat_capacity_j_m2_k: f64,
    pub absorptance_front: f64,
    pub emittance_front: f64,
    pub absorptance_back: f64,
    pub emittance_back: f64,
    /// `Some`: the back faces the spacecraft and radiates to this sink.
    pub back_sink_k: Option<f64>,
}

impl PanelThermal {
    fn sink4(&self) -> f64 {
        self.back_sink_k.map_or(0.0, |t| t.powi(4))
    }

    /// Net heating, W/m²: q_in − emitted − p_elec.
    pub fn net_w_m2(&self, t_k: f64, q_in: f64, p_elec: f64) -> f64 {
        let t4 = t_k.powi(4);
        q_in - SIGMA * (self.emittance_front * t4 + self.emittance_back * (t4 - self.sink4()))
            - p_elec
    }

    /// Steady-state temperature for constant inputs (closed form):
    /// T⁴ = (q_in − p_elec + ε_b·σ·T_sink⁴) / ((ε_f + ε_b)·σ).
    pub fn steady_k(&self, q_in: f64, p_elec: f64) -> f64 {
        let eps = self.emittance_front + self.emittance_back;
        let num = q_in - p_elec + self.emittance_back * SIGMA * self.sink4();
        (num / (eps * SIGMA)).max(T_SPACE_K.powi(4)).powf(0.25)
    }

    /// Integrates from `t0_k` over `dt_s`, with absorbed flux and electrical
    /// power linear in time from (q0, p0) to (q1, p1), W/m².
    pub fn step(&self, t0_k: f64, dt_s: f64, q: (f64, f64), p: (f64, f64)) -> f64 {
        let lin = |a: (f64, f64)| move |tau: f64| a.0 + (a.1 - a.0) * tau / dt_s;
        self.step_with(t0_k, dt_s, q.0.max(q.1), 1, lin(q), lin(p))
    }

    /// Integrates from `t0_k` over `dt_s` with absorbed flux `q(τ)` and
    /// electrical power `p(τ)`, W/m², τ ∈ [0, dt_s]; `q_max` bounds `q`.
    /// RK4 on substeps of at most a quarter of the radiative time constant
    /// at the hottest temperature the interval can reach, rounded up to a
    /// multiple of `align` (so substep edges fall on the kinks of a
    /// piecewise-linear input sampled `align` times per interval).
    pub fn step_with(
        &self,
        t0_k: f64,
        dt_s: f64,
        q_max: f64,
        align: usize,
        q: impl Fn(f64) -> f64,
        p: impl Fn(f64) -> f64,
    ) -> f64 {
        if dt_s <= 0.0 {
            return t0_k;
        }
        let hot = t0_k.max(self.steady_k(q_max, 0.0)).max(150.0);
        let eps = self.emittance_front + self.emittance_back;
        let rate = 4.0 * SIGMA * eps * hot.powi(3) / self.heat_capacity_j_m2_k;
        let align = align.max(1);
        let n = ((dt_s * rate / 0.25).ceil() as usize)
            .max(1)
            .div_ceil(align)
            * align;
        self.step_n_with(t0_k, dt_s, n, q, p)
    }

    /// [`step`](Self::step) with exactly `n` RK4 substeps.
    pub fn step_n(&self, t0_k: f64, dt_s: f64, q: (f64, f64), p: (f64, f64), n: usize) -> f64 {
        let lin = |a: (f64, f64)| move |tau: f64| a.0 + (a.1 - a.0) * tau / dt_s;
        self.step_n_with(t0_k, dt_s, n, lin(q), lin(p))
    }

    /// RK4 with exactly `n` substeps and general inputs.
    pub fn step_n_with(
        &self,
        t0_k: f64,
        dt_s: f64,
        n: usize,
        q: impl Fn(f64) -> f64,
        p: impl Fn(f64) -> f64,
    ) -> f64 {
        if dt_s <= 0.0 || n == 0 {
            return t0_k;
        }
        let h = dt_s / n as f64;
        let f = |tau: f64, t: f64| self.net_w_m2(t, q(tau), p(tau)) / self.heat_capacity_j_m2_k;
        let mut t = t0_k;
        for i in 0..n {
            let tau = i as f64 * h;
            let k1 = f(tau, t);
            let k2 = f(tau + 0.5 * h, t + 0.5 * h * k1);
            let k3 = f(tau + 0.5 * h, t + 0.5 * h * k2);
            let k4 = f(tau + h, t + h * k3);
            t += h / 6.0 * (k1 + 2.0 * k2 + 2.0 * k3 + k4);
            t = t.max(T_SPACE_K);
        }
        t
    }
}

/// Heat arriving on one panel at one instant, W/m² of panel area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelHeat {
    /// Direct sunlight absorbed per unit of visible solar disk:
    /// α_f · S · cos θ · (geometric lit fraction of the cells) on the front,
    /// plus α_b · S · cos θ_back on an exposed back. Multiply by the
    /// visible fraction to get watts. `None` when the Sun was fully eclipsed
    /// at this instant (rays were skipped, so the lit fraction is unknown).
    pub direct_w_m2: Option<f64>,
    /// Planet albedo and IR, absorbed (front and exposed back).
    pub planet_w_m2: f64,
}

impl PanelHeat {
    /// Total absorbed flux with `visible` of the solar disk showing.
    pub fn total(&self, visible: f64) -> f64 {
        self.direct_w_m2.unwrap_or(0.0) * visible + self.planet_w_m2
    }
}

/// Panel thermal properties plus the radiating bodies of the trajectory.
#[derive(Debug, Clone, PartialEq)]
pub struct ThermalModel {
    pub panel: PanelThermal,
    /// Starting temperature; `None`: steady state at the first row.
    pub initial_k: Option<f64>,
    /// Indexed by body index in the state table.
    pub bodies: Vec<Option<BodyRadiation>>,
}

impl ThermalModel {
    /// Heat on each panel (see [`PanelHeat`]):
    /// - front: α_f · direct sunlight on the lit cell fraction + α_f ·
    ///   albedo + ε_f · planet IR;
    /// - back, when it faces space: α_b · direct sunlight (unshadowed) +
    ///   α_b · albedo + ε_b · planet IR.
    ///
    /// IR is absorbed with the IR emittance (Kirchhoff's law). The front's
    /// lit fraction is the cells' (the substrate between cells is assumed
    /// lit alike).
    pub fn absorbed(&self, scene: &Scene, epoch: &Epoch, il: &ArrayIllum) -> Vec<PanelHeat> {
        let p = &self.panel;
        let back_exposed = p.back_sink_k.is_none();
        let s = il.sun.irradiance_w_m2;
        // Normals of both faces of every panel: [front 0, back 0, front 1, …].
        let normals: Vec<_> = scene
            .panels
            .iter()
            .flat_map(|pl| [pl.normal, -pl.normal])
            .collect();
        let mut planet = vec![(0.0, 0.0); normals.len()];
        for b in &epoch.bodies {
            if let Some(Some(rad)) = self.bodies.get(b.index) {
                planet::add_irradiance(
                    rad,
                    b.vec_b,
                    b.equatorial_radius_m,
                    il.sun.sun_dir_b,
                    s,
                    &normals,
                    &mut planet,
                );
            }
        }
        scene
            .panels
            .iter()
            .enumerate()
            .map(|(k, pl)| {
                let (alb_f, ir_f) = planet[2 * k];
                let mut planet_w_m2 = p.absorptance_front * alb_f + p.emittance_front * ir_f;
                let mut back_direct = 0.0;
                if back_exposed {
                    let (alb_b, ir_b) = planet[2 * k + 1];
                    planet_w_m2 += p.absorptance_back * alb_b + p.emittance_back * ir_b;
                    let cos_back = (-pl.normal.dot(il.sun.sun_dir_b)).max(0.0);
                    back_direct = p.absorptance_back * s * cos_back;
                }
                let direct_w_m2 = (il.sun.visible > 0.0).then(|| {
                    let lit = il.panel_sunlyt[k] / il.sun.visible;
                    p.absorptance_front * s * il.panel_cos[k] * lit + back_direct
                });
                PanelHeat {
                    direct_w_m2,
                    planet_w_m2,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
