//! Simulation off the UI thread (design D8). The UI never blocks: it reads
//! the latest published results through `ArcSwap` and asks for new ones.
//!
//! - [`RunWorker`] computes the whole mission once (the batch run, the same
//!   call as `sunlyt array`) and publishes it with the eclipse bands.
//! - [`SnapshotWorker`] evaluates one instant on demand. Only the newest
//!   request matters: older ones are dropped, so scrubbing never queues.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use arc_swap::ArcSwapOption;
use eframe::egui;
use sunlyt_core::sim::array_at;
use sunlyt_core::thermal::ThermalModel;
use sunlyt_core::{
    ArrayModel, ArrayRun, ArraySample, EnvConfig, Epoch, RayCaster, Scene, StateTable,
};

use crate::timeline::Bands;

/// Everything the simulation needs, shared read-only by both workers.
pub struct Shared {
    pub table: StateTable,
    pub scene: Scene,
    pub rc: RayCaster,
    pub model: ArrayModel,
    pub env: EnvConfig,
    pub thermal: Option<ThermalModel>,
    /// String index of each cell (circuit model), for colouring by current.
    pub cell_string: Vec<Option<u32>>,
    pub body_names: Vec<String>,
}

impl Shared {
    pub fn new(
        table: StateTable,
        scene: Scene,
        model: ArrayModel,
        env: EnvConfig,
        thermal: Option<ThermalModel>,
    ) -> Self {
        let rc = RayCaster::new(&scene);
        let mut cell_string = vec![None; scene.cells.len()];
        if matches!(model, ArrayModel::Circuit(_)) {
            for (s, st) in scene.strings.iter().enumerate() {
                for &c in &st.cells {
                    cell_string[c] = Some(s as u32);
                }
            }
        }
        let body_names = table.body_names().map(str::to_owned).collect();
        Self {
            table,
            scene,
            rc,
            model,
            env,
            thermal,
            cell_string,
            body_names,
        }
    }

    /// UTC to the second.
    pub fn utc(&self, t: f64) -> String {
        self.table
            .datetime(t)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    }
}

/// The finished batch run and what the UI derives from it once.
pub struct RunOutput {
    pub run: ArrayRun,
    pub bands: Bands,
    pub compute_s: f64,
}

#[derive(Default)]
struct RunState {
    done_rows: AtomicUsize,
    output: ArcSwapOption<RunOutput>,
}

pub struct RunWorker {
    state: Arc<RunState>,
    total: usize,
}

impl RunWorker {
    pub fn spawn(shared: Arc<Shared>, ctx: egui::Context) -> Self {
        let state = Arc::new(RunState::default());
        let total = shared.table.len();
        let st = state.clone();
        std::thread::Builder::new()
            .name("sunlyt-run".into())
            .spawn(move || {
                let t0 = Instant::now();
                let s = &*shared;
                let run = match &s.thermal {
                    Some(th) => ArrayRun::compute_thermal_progress(
                        &s.table,
                        &s.scene,
                        &s.rc,
                        &s.model,
                        &s.env,
                        th,
                        &st.done_rows,
                    ),
                    None => ArrayRun::compute_progress(
                        &s.table,
                        &s.scene,
                        &s.rc,
                        &s.model,
                        &s.env,
                        &st.done_rows,
                    ),
                };
                let bands = Bands::compute(&s.table, &s.env, &run);
                st.output.store(Some(Arc::new(RunOutput {
                    run,
                    bands,
                    compute_s: t0.elapsed().as_secs_f64(),
                })));
                ctx.request_repaint();
            })
            .expect("spawning the run worker");
        Self { state, total }
    }

    /// Rows finished and total rows.
    pub fn progress(&self) -> (usize, usize) {
        (self.state.done_rows.load(Ordering::Relaxed), self.total)
    }

    pub fn output(&self) -> Option<Arc<RunOutput>> {
        self.state.output.load_full()
    }
}

/// The array at one instant.
pub struct Snapshot {
    pub t: f64,
    pub epoch: Epoch,
    /// At a row time with the run finished: the batch sample itself.
    /// Otherwise from [`array_at`], with panel temperatures interpolated
    /// linearly between the run's rows.
    pub sample: ArraySample,
    /// Geometric lit fraction per cell (0 in umbra: no rays are cast).
    pub lit: Vec<f64>,
    /// `Some(i)` when `t` is exactly row `i`.
    pub row: Option<usize>,
    /// Computed before the batch run finished, so a thermal model's
    /// temperatures were not known yet (the circuit's fixed temperature
    /// stands in). Recomputed once the run arrives.
    pub provisional: bool,
}

/// Evaluates the array at `t`, or `None` outside the table.
pub fn snapshot(sh: &Shared, run: Option<&ArrayRun>, t: f64) -> Option<Snapshot> {
    let times = sh.table.times();
    let k = times.partition_point(|&x| x <= t);
    let row = (k > 0 && times[k - 1] == t).then(|| k - 1);
    let temps: Option<Vec<f64>> = match (&sh.thermal, run) {
        (Some(_), Some(run)) if k > 0 => {
            let i = k - 1;
            let a = &run.samples[i].panel_temperature_k;
            match run.samples.get(i + 1) {
                Some(next) if row.is_none() => {
                    let u = (t - run.t[i]) / (run.t[i + 1] - run.t[i]);
                    let b = &next.panel_temperature_k;
                    Some(a.iter().zip(b).map(|(a, b)| a + (b - a) * u).collect())
                }
                _ => Some(a.clone()),
            }
        }
        _ => None,
    };
    let provisional = sh.thermal.is_some() && run.is_none();
    let (mut sample, lit) = array_at(
        &sh.table,
        &sh.scene,
        &sh.rc,
        &sh.model,
        &sh.env,
        t,
        temps.as_deref(),
    )?;
    if let (Some(i), Some(run)) = (row, run) {
        // One source of truth: the batch numbers (equal to `sample` by the
        // `array_at_rows_equal_batch` test, but never recomputed here).
        sample = run.samples[i].clone();
    }
    Some(Snapshot {
        t,
        epoch: sh.table.sample(t)?,
        sample,
        lit,
        row,
        provisional,
    })
}

struct SnapInner {
    request: Mutex<Option<f64>>,
    wake: Condvar,
    latest: ArcSwapOption<Snapshot>,
    /// Duration of the latest evaluation, µs.
    compute_us: AtomicU64,
}

pub struct SnapshotWorker {
    inner: Arc<SnapInner>,
    last_request: Option<f64>,
}

impl SnapshotWorker {
    pub fn spawn(shared: Arc<Shared>, run: &RunWorker, ctx: egui::Context) -> Self {
        let inner = Arc::new(SnapInner {
            request: Mutex::new(None),
            wake: Condvar::new(),
            latest: ArcSwapOption::empty(),
            compute_us: AtomicU64::new(0),
        });
        let (me, run_state) = (inner.clone(), run.state.clone());
        std::thread::Builder::new()
            .name("sunlyt-instant".into())
            .spawn(move || {
                loop {
                    let t = {
                        let mut req = me.request.lock().unwrap();
                        loop {
                            if let Some(t) = req.take() {
                                break t;
                            }
                            req = me.wake.wait(req).unwrap();
                        }
                    };
                    let out = run_state.output.load_full();
                    let started = Instant::now();
                    let s = snapshot(&shared, out.as_ref().map(|o| &o.run), t);
                    me.compute_us
                        .store(started.elapsed().as_micros() as u64, Ordering::Relaxed);
                    if let Some(s) = s {
                        me.latest.store(Some(Arc::new(s)));
                        ctx.request_repaint();
                    }
                }
            })
            .expect("spawning the instant worker");
        Self {
            inner,
            last_request: None,
        }
    }

    /// Asks for the instant `t` unless it is already requested.
    /// `force` re-requests the same time (e.g. once the run arrives).
    pub fn request(&mut self, t: f64, force: bool) {
        if self.last_request == Some(t) && !force {
            return;
        }
        self.last_request = Some(t);
        *self.inner.request.lock().unwrap() = Some(t);
        self.inner.wake.notify_one();
    }

    /// How long the latest evaluation took, ms.
    pub fn last_compute_ms(&self) -> f32 {
        self.inner.compute_us.load(Ordering::Relaxed) as f32 / 1000.0
    }

    pub fn latest(&self) -> Option<Arc<Snapshot>> {
        self.inner.latest.load_full()
    }

    /// True while the newest request has not been answered yet.
    pub fn pending(&self) -> bool {
        match (self.last_request, self.latest()) {
            (Some(t), Some(s)) => s.t != t,
            (Some(_), None) => true,
            _ => false,
        }
    }
}
