//! Array runs: cells, self-shadowing and eclipses at every row.

use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;

use super::RunSummary;
use crate::env::{EnvConfig, sunlight};
use crate::geometry::Scene;
use crate::power::{ArrayIllum, ArrayModel, ArraySample};
use crate::shadow::Illuminator;
use crate::thermal::{PanelHeat, ThermalModel};
use crate::trajectory::StateTable;

/// Array results at every table row, in time order. Per-cell lit fractions
/// are not stored (rows × cells is too large); use [`array_at`] for one
/// instant.
#[derive(Debug, Clone)]
pub struct ArrayRun {
    pub t: Vec<f64>,
    pub samples: Vec<ArraySample>,
}

impl ArrayRun {
    /// Evaluates every row in parallel; each worker reuses one scratch
    /// buffer of per-cell lit fractions.
    pub fn compute(
        table: &StateTable,
        scene: &Scene,
        illum: &dyn Illuminator,
        model: &ArrayModel,
        env: &EnvConfig,
    ) -> Self {
        Self::compute_progress(table, scene, illum, model, env, &AtomicUsize::new(0))
    }

    /// [`compute`](Self::compute), adding 1 to `progress` as each row
    /// finishes (for a progress bar; the results are identical).
    pub fn compute_progress(
        table: &StateTable,
        scene: &Scene,
        illum: &dyn Illuminator,
        model: &ArrayModel,
        env: &EnvConfig,
        progress: &AtomicUsize,
    ) -> Self {
        let n = scene.cells.len();
        let samples = (0..table.len())
            .into_par_iter()
            .map_init(
                || vec![0.0; n],
                |lit, i| {
                    let s = model.sample(scene, illum, &table.row(i), env, lit);
                    progress.fetch_add(1, Ordering::Relaxed);
                    s
                },
            )
            .collect();
        Self {
            t: table.times().to_vec(),
            samples,
        }
    }

    /// Single-threaded [`compute`](Self::compute), for checking that
    /// parallel results are identical and for benchmarks.
    pub fn compute_serial(
        table: &StateTable,
        scene: &Scene,
        illum: &dyn Illuminator,
        model: &ArrayModel,
        env: &EnvConfig,
    ) -> Self {
        let mut lit = vec![0.0; scene.cells.len()];
        Self {
            t: table.times().to_vec(),
            samples: (0..table.len())
                .map(|i| model.sample(scene, illum, &table.row(i), env, &mut lit))
                .collect(),
        }
    }

    /// Evaluates every row with panel temperatures from the thermal model
    /// (design D8):
    ///
    /// 1. Pass A, parallel over rows (stateless): sunlight, rays, the cell
    ///    irradiance groups of every string, and each panel's absorbed heat
    ///    flux (sunlight, planet albedo and IR).
    /// 2. Pass B, parallel over panels, sequential in time: at each row the
    ///    circuit is solved at the panel's temperature (the reported
    ///    output), the next row's power is predicted at the same
    ///    temperature, and the temperature is integrated across the
    ///    interval with heat and electrical power linear in between.
    ///
    /// The first row starts from `thermal.initial_k`, or from the steady
    /// state of the first row's inputs (iterated with the electrical power,
    /// which depends on temperature).
    pub fn compute_thermal(
        table: &StateTable,
        scene: &Scene,
        illum: &dyn Illuminator,
        model: &ArrayModel,
        env: &EnvConfig,
        thermal: &ThermalModel,
    ) -> Self {
        Self::thermal_run(table, scene, illum, model, env, thermal, true, None)
    }

    /// [`compute_thermal`](Self::compute_thermal), adding 1 to `progress`
    /// as each row's geometry (pass A, nearly all of the work) finishes.
    pub fn compute_thermal_progress(
        table: &StateTable,
        scene: &Scene,
        illum: &dyn Illuminator,
        model: &ArrayModel,
        env: &EnvConfig,
        thermal: &ThermalModel,
        progress: &AtomicUsize,
    ) -> Self {
        Self::thermal_run(
            table,
            scene,
            illum,
            model,
            env,
            thermal,
            true,
            Some(progress),
        )
    }

    /// Single-threaded [`compute_thermal`](Self::compute_thermal).
    pub fn compute_thermal_serial(
        table: &StateTable,
        scene: &Scene,
        illum: &dyn Illuminator,
        model: &ArrayModel,
        env: &EnvConfig,
        thermal: &ThermalModel,
    ) -> Self {
        Self::thermal_run(table, scene, illum, model, env, thermal, false, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn thermal_run(
        table: &StateTable,
        scene: &Scene,
        illum: &dyn Illuminator,
        model: &ArrayModel,
        env: &EnvConfig,
        thermal: &ThermalModel,
        parallel: bool,
        progress: Option<&AtomicUsize>,
    ) -> Self {
        let (n, nc) = (table.len(), scene.cells.len());
        let t = table.times();
        let pass_a = |lit: &mut Vec<f64>, i: usize| {
            let e = table.row(i);
            let il = model.illuminate(scene, illum, &e, env, lit);
            let heat = thermal.absorbed(scene, &e, &il);
            // The visible solar disk inside the interval to the next row,
            // from the interpolated trajectory (cheap: no rays).
            let vis_inner = match t.get(i + 1) {
                Some(&t1) => {
                    let m = ((t1 - t[i]) / ECLIPSE_SAMPLE_S).ceil().max(1.0) as usize;
                    (1..m)
                        .map(|j| {
                            let tj = t[i] + (t1 - t[i]) * j as f64 / m as f64;
                            table
                                .sample(tj)
                                .map_or(0.0, |ej| sunlight(&ej, env).visible)
                        })
                        .collect()
                }
                None => Vec::new(),
            };
            if let Some(p) = progress {
                p.fetch_add(1, Ordering::Relaxed);
            }
            Row {
                il,
                heat,
                vis_inner,
            }
        };
        let rows: Vec<Row> = if parallel {
            (0..n)
                .into_par_iter()
                .map_init(|| vec![0.0; nc], pass_a)
                .collect()
        } else {
            let mut lit = vec![0.0; nc];
            (0..n).map(|i| pass_a(&mut lit, i)).collect()
        };

        let track = |p: usize| track_panel(scene, model, thermal, &rows, t, p);
        let tracks: Vec<PanelTrack> = if parallel {
            (0..scene.panels.len()).into_par_iter().map(track).collect()
        } else {
            (0..scene.panels.len()).map(track).collect()
        };

        let ns = match model {
            ArrayModel::Circuit(_) => scene.strings.len(),
            ArrayModel::Constant { .. } => 0,
        };
        let samples = rows
            .into_iter()
            .enumerate()
            .map(|(k, Row { il, .. })| {
                let (mut i_str, mut v_str) = (vec![0.0; ns], vec![0.0; ns]);
                for (p, tr) in tracks.iter().enumerate() {
                    let r = scene.panels[p].strings.clone();
                    if ns > 0 && !r.is_empty() {
                        let m = r.len();
                        i_str[r.clone()].copy_from_slice(&tr.current_a[k * m..(k + 1) * m]);
                        v_str[r].copy_from_slice(&tr.voltage_v[k * m..(k + 1) * m]);
                    }
                }
                let power: Vec<f64> = tracks.iter().map(|tr| tr.power_w[k]).collect();
                ArraySample {
                    power_w: power.iter().sum(),
                    sun: il.sun,
                    panel_power_w: power,
                    panel_sunlyt: il.panel_sunlyt,
                    panel_temperature_k: tracks.iter().map(|tr| tr.temperature_k[k]).collect(),
                    string_current_a: i_str,
                    string_voltage_v: v_str,
                }
            })
            .collect();
        Self {
            t: t.to_vec(),
            samples,
        }
    }

    pub fn summary(&self, bodies: usize) -> RunSummary {
        let p: Vec<f64> = self.samples.iter().map(|s| s.power_w).collect();
        RunSummary::new(
            &self.t,
            &p,
            self.samples.iter().map(|s| s.sun.occulted.as_slice()),
            bodies,
        )
    }

    /// Energy per panel over the run, Wh.
    pub fn panel_energy_wh(&self) -> Vec<f64> {
        let np = self.samples.first().map_or(0, |s| s.panel_power_w.len());
        (0..np)
            .map(|p| {
                let pw: Vec<f64> = self.samples.iter().map(|s| s.panel_power_w[p]).collect();
                super::energy_wh(&self.t, &pw)
            })
            .collect()
    }
}

/// Spacing of the eclipse samples inside each row interval, s. From a
/// 100 km lunar orbit the Sun's 0.53° disk takes ~9 s to set behind the
/// limb, and a panel's thermal time constant is ~100 s, so 2 s resolves
/// ingress and egress; between rows the eclipse fraction is otherwise only
/// known at the rows themselves (60 s apart in the sample data).
pub const ECLIPSE_SAMPLE_S: f64 = 2.0;

/// Pass A output for one row.
struct Row {
    il: ArrayIllum,
    heat: Vec<PanelHeat>,
    /// Visible solar disk at the interior sample times of the interval to
    /// the next row (empty for the last row).
    vis_inner: Vec<f64>,
}

/// One panel's history from pass B.
struct PanelTrack {
    temperature_k: Vec<f64>,
    power_w: Vec<f64>,
    /// Row-major: row k holds this panel's strings at k·m..(k+1)·m.
    current_a: Vec<f64>,
    voltage_v: Vec<f64>,
}

fn track_panel(
    scene: &Scene,
    model: &ArrayModel,
    thermal: &ThermalModel,
    rows: &[Row],
    t: &[f64],
    p: usize,
) -> PanelTrack {
    let n = rows.len();
    let panel = &scene.panels[p];
    let area = panel.area_m2;
    let strings = panel.strings.clone();
    let ns = match model {
        ArrayModel::Circuit(_) => scene.strings.len(),
        ArrayModel::Constant { .. } => 0,
    };
    let m = if ns > 0 { strings.len() } else { 0 };
    let (mut i_s, mut v_s) = (vec![0.0; ns], vec![0.0; ns]);
    let mut out = PanelTrack {
        temperature_k: Vec::with_capacity(n),
        power_w: Vec::with_capacity(n),
        current_a: Vec::with_capacity(n * m),
        voltage_v: Vec::with_capacity(n * m),
    };
    if n == 0 {
        return out;
    }
    let th = &thermal.panel;
    let mut temp = match thermal.initial_k {
        Some(t0) => t0,
        None => {
            // Steady state with P_elec(T): a contraction (dP/dT is a few
            // 0.1 %/K), so plain iteration converges in a handful of steps.
            let q0 = rows[0].heat[p].total(rows[0].il.sun.visible);
            let mut tt = th.steady_k(q0, 0.0);
            for _ in 0..100 {
                let pe = model.panel_output(scene, &rows[0].il, p, Some(tt), &mut i_s, &mut v_s);
                let next = th.steady_k(q0, pe / area);
                let done = (next - tt).abs() < 1e-9;
                tt = next;
                if done {
                    break;
                }
            }
            tt
        }
    };
    let mut nodes = Vec::new();
    for k in 0..n {
        let pk = model.panel_output(scene, &rows[k].il, p, Some(temp), &mut i_s, &mut v_s);
        out.temperature_k.push(temp);
        out.power_w.push(pk);
        if m > 0 {
            out.current_a.extend_from_slice(&i_s[strings.clone()]);
            out.voltage_v.extend_from_slice(&v_s[strings.clone()]);
        }
        if k + 1 == n {
            break;
        }
        // Next row's power, predicted at this row's temperature.
        let pnext = model.panel_output(scene, &rows[k + 1].il, p, Some(temp), &mut i_s, &mut v_s);
        let (r0, r1) = (&rows[k], &rows[k + 1]);
        let (v0, v1) = (r0.il.sun.visible, r1.il.sun.visible);
        let (h0, h1) = (r0.heat[p], r1.heat[p]);
        // Direct sunlight and electrical power per unit of visible disk,
        // linear between rows; an end in umbra (rays skipped) borrows the
        // other end's value. Then × the sampled visible fraction.
        let fill =
            |a: Option<f64>, b: Option<f64>| (a.or(b).unwrap_or(0.0), b.or(a).unwrap_or(0.0));
        let (d0, d1) = fill(h0.direct_w_m2, h1.direct_w_m2);
        let per_vis = |pw: f64, v: f64| (v > 0.0).then(|| pw / v);
        let (e0, e1) = fill(per_vis(pk, v0), per_vis(pnext, v1));
        nodes.clear();
        nodes.push(v0);
        nodes.extend_from_slice(&r0.vis_inner);
        nodes.push(v1);
        let segs = nodes.len() - 1;
        let dt = t[k + 1] - t[k];
        let nodes = &nodes;
        let vis = move |tau: f64| {
            let x = (tau / dt * segs as f64).clamp(0.0, segs as f64);
            let j = (x as usize).min(segs - 1);
            nodes[j] + (nodes[j + 1] - nodes[j]) * (x - j as f64)
        };
        let lerp = move |a: f64, b: f64, tau: f64| a + (b - a) * tau / dt;
        let q = |tau: f64| lerp(d0, d1, tau) * vis(tau) + lerp(h0.planet_w_m2, h1.planet_w_m2, tau);
        let pe = |tau: f64| lerp(e0, e1, tau) * vis(tau) / area;
        let q_max = d0.max(d1) + h0.planet_w_m2.max(h1.planet_w_m2);
        temp = th.step_with(temp, dt, q_max, segs, q, pe);
    }
    out
}

/// Array output and per-cell lit fractions at any time `t` (interpolated),
/// or `None` outside the table. For the viewer. `panel_temp_k` (e.g. a
/// thermal run's temperatures interpolated to `t`) sets the cells'
/// temperature per panel; `None` uses the circuit's fixed temperature.
pub fn array_at(
    table: &StateTable,
    scene: &Scene,
    illum: &dyn Illuminator,
    model: &ArrayModel,
    env: &EnvConfig,
    t: f64,
    panel_temp_k: Option<&[f64]>,
) -> Option<(ArraySample, Vec<f64>)> {
    let e = table.sample(t)?;
    let mut lit = vec![0.0; scene.cells.len()];
    let il = model.illuminate(scene, illum, &e, env, &mut lit);
    Some((model.output(scene, il, panel_temp_k), lit))
}
