//! The panel sandbox (slice 8): one flat panel, face on, with its cells,
//! strings, shadows, the Sun direction, an eclipse inset and per-string
//! I-V curves. It cross-checks the full simulation with independent
//! calculations (exact shadow clipping, I-V curves) and needs no wgpu.
//!
//! - [`config`] — `sandbox.toml`
//! - [`model`] — the one-panel scene and its evaluation at one instant
//! - [`view2d`] — the panel face on, with shadows and wiring
//! - [`iv`] — per-string I-V curves, bus line, operating points
//! - [`eclipse`] — Sun and body disks at true angular size
//!
//! Evaluation runs on the UI thread when an input changes; its time is
//! shown in the readout and `SUNLYT_SWEEP` prints its distribution.

pub mod config;
pub mod eclipse;
pub mod iv;
pub mod model;
pub mod view2d;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Result, anyhow};
use eframe::egui::{self, Key, Modifiers, RichText};
use sunlyt_core::env::sunlight;
use sunlyt_core::power::cell::ZERO_C_K;
use sunlyt_core::{ArrayModel, Regulation};

use crate::ui::theme::Theme;
use model::{Evaluation, Sandbox, Source, az_el, sun_from_az_el};
use view2d::{Fill, Layout2d};

/// Opens the sandbox window and blocks until it is closed. `save_to`:
/// where Save writes the sandbox TOML (box mode).
pub fn run(sb: Sandbox, save_to: Option<PathBuf>) -> Result<()> {
    let title = format!("sunlyt sandbox — {}", sb.label);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([960.0, 640.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            Theme::install(&cc.egui_ctx);
            Ok(Box::new(SandboxApp::new(sb, save_to)))
        }),
    )
    .map_err(|e| anyhow!("sandbox: {e}"))
}

/// The first penumbra at or after `after`: the instant half the disk is
/// hidden, found by bisection between the rows that straddle it.
pub fn next_penumbra(sb: &Sandbox, after: f64) -> Option<f64> {
    let Source::Trajectory { table, .. } = &sb.source else {
        return None;
    };
    let vis = |t: f64| table.sample(t).map(|e| sunlight(&e, &sb.env).visible);
    let times = table.times();
    let start = times.partition_point(|&t| t <= after).saturating_sub(1);
    for w in times[start..].windows(2) {
        let (va, vb) = (vis(w[0])?, vis(w[1])?);
        if (va > 0.5) == (vb > 0.5) {
            continue;
        }
        let (mut lo, mut hi) = (w[0], w[1]);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if (vis(mid)? > 0.5) == (va > 0.5) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        if lo >= after {
            return Some(lo);
        }
    }
    None
}

/// Writes `cells.csv`, `strings.csv` and `iv.csv` into `dir`: the numbers
/// the window draws.
pub fn export(sb: &Sandbox, ev: &Evaluation, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let layout = Layout2d::new(&sb.scene);
    let mut f = std::io::BufWriter::new(std::fs::File::create(dir.join("cells.csv"))?);
    writeln!(f, "row,col,string,u_m,v_m,lit_rays,lit_exact")?;
    for (k, c) in sb.scene.cells.iter().enumerate() {
        let (lo, hi) = layout.cells[k];
        let m = (lo + hi) / 2.0;
        writeln!(
            f,
            "{},{},{},{:.6},{:.6},{:.6},{:.9}",
            c.row,
            c.col,
            layout.string_of[k] + 1,
            m.x,
            m.y,
            ev.lit_ray[k],
            ev.lit_exact[k]
        )?;
    }
    f.flush()?;
    let mut f = std::io::BufWriter::new(std::fs::File::create(dir.join("strings.csv"))?);
    writeln!(f, "string,cells,current_a,voltage_v")?;
    for (s, st) in sb.scene.strings.iter().enumerate() {
        let (i, v) = (
            ev.ray.string_current_a.get(s).copied().unwrap_or(f64::NAN),
            ev.ray.string_voltage_v.get(s).copied().unwrap_or(f64::NAN),
        );
        writeln!(f, "{},{},{i:.6},{v:.6}", s + 1, st.cells.len())?;
    }
    f.flush()?;
    let mut f = std::io::BufWriter::new(std::fs::File::create(dir.join("iv.csv"))?);
    writeln!(f, "curve,first_string,strings,current_a,voltage_v")?;
    for (k, c) in ev.curves.iter().enumerate() {
        for &(i, v) in &c.points {
            writeln!(
                f,
                "{},{},{},{i:.9},{v:.9}",
                k + 1,
                c.strings[0] + 1,
                c.strings.len()
            )?;
        }
    }
    f.flush()
}

struct SandboxApp {
    sb: Sandbox,
    layout: Layout2d,
    ev: Evaluation,
    eval_ms: f64,
    fill: Fill,
    hover: Option<usize>,
    /// Box mode: the Sun as the sliders hold it.
    az_el: (f64, f64),
    save_to: Option<PathBuf>,
    status: Option<String>,
    capture: Option<SandboxCapture>,
}

impl SandboxApp {
    fn new(sb: Sandbox, save_to: Option<PathBuf>) -> Self {
        let layout = Layout2d::new(&sb.scene);
        let started = Instant::now();
        let ev = sb.evaluate();
        let eval_ms = started.elapsed().as_secs_f64() * 1e3;
        let az_el = az_el(ev.sun_dir);
        Self {
            sb,
            layout,
            ev,
            eval_ms,
            fill: Fill::Rays,
            hover: None,
            az_el,
            save_to,
            status: None,
            capture: SandboxCapture::from_env(),
        }
    }

    fn reevaluate(&mut self) {
        let started = Instant::now();
        self.ev = self.sb.evaluate();
        self.eval_ms = started.elapsed().as_secs_f64() * 1e3;
        if let Some(c) = &mut self.capture {
            c.eval_ms.push(self.eval_ms);
        }
    }

    fn set_time(&mut self, t: f64) {
        if let Source::Trajectory { table, t: cur } = &mut self.sb.source {
            let times = table.times();
            *cur = t.clamp(times[0], *times.last().unwrap());
        }
        self.reevaluate();
    }

    /// `SUNLYT_AT`: seconds, UTC, or `penumbra:<n>` (from 0).
    fn seek_spec(&mut self, at: &str) {
        let Source::Trajectory { table, .. } = &self.sb.source else {
            return;
        };
        let t = if let Some(n) = at.strip_prefix("penumbra:") {
            let k: usize = n.parse().unwrap_or(0);
            let mut t = table.times()[0];
            let mut found = None;
            for _ in 0..=k {
                found = next_penumbra(&self.sb, t);
                match found {
                    Some(x) => t = x + 1.0,
                    None => break,
                }
            }
            found
        } else if let Ok(s) = at.parse::<f64>() {
            Some(s)
        } else {
            chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|d| (d - table.epoch()).num_milliseconds() as f64 / 1000.0)
        };
        if let Some(t) = t {
            self.set_time(t);
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        let mut step = 0i64;
        ctx.input_mut(|i| {
            for (m, k, d) in [
                (Modifiers::SHIFT, Key::ArrowRight, 10),
                (Modifiers::SHIFT, Key::ArrowLeft, -10),
                (Modifiers::NONE, Key::ArrowRight, 1),
                (Modifiers::NONE, Key::ArrowLeft, -1),
            ] {
                if i.consume_key(m, k) {
                    step += d;
                }
            }
        });
        if step == 0 {
            return;
        }
        match &self.sb.source {
            Source::Trajectory { table, t } => {
                let times = table.times();
                let k = times.partition_point(|&x| x < *t) as i64;
                let k = (k + step).clamp(0, times.len() as i64 - 1) as usize;
                let t = times[k];
                self.set_time(t);
            }
            Source::Fixed { .. } => {
                self.az_el.0 = (self.az_el.0 + step as f64).rem_euclid(360.0);
                self.apply_sun();
            }
        }
    }

    fn apply_sun(&mut self) {
        if let Source::Fixed { sun_dir, .. } = &mut self.sb.source {
            *sun_dir = sun_from_az_el(self.az_el.0, self.az_el.1);
        }
        self.reevaluate();
    }

    fn save(&mut self) {
        let (Some(text), Some(path)) = (&self.sb.text, &self.save_to) else {
            return;
        };
        self.status = Some(
            match config::with_sun(text, self.az_el.0, self.az_el.1)
                .and_then(|s| std::fs::write(path, s).map_err(|e| e.to_string()))
            {
                Ok(()) => format!("Saved {}", path.display()),
                Err(e) => format!("Save failed: {e}"),
            },
        );
    }

    fn sun_controls(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        match &self.sb.source {
            Source::Fixed { .. } => {
                let (mut az, mut el) = self.az_el;
                let mut changed = false;
                egui::Grid::new("sun").num_columns(2).show(ui, |ui| {
                    ui.label("Azimuth");
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut az, 0.0..=360.0)
                                .suffix("°")
                                .fixed_decimals(1),
                        )
                        .on_hover_text("From the panel's +u axis towards +v  (← → step 1°)")
                        .changed();
                    ui.end_row();
                    ui.label("Elevation");
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut el, -10.0..=90.0)
                                .suffix("°")
                                .fixed_decimals(1),
                        )
                        .on_hover_text("Above the panel plane; below 0 the panel faces away")
                        .changed();
                    ui.end_row();
                });
                if changed {
                    self.az_el = (az, el);
                    self.apply_sun();
                }
                if let Some(path) = self.save_to.clone()
                    && ui
                        .button("Save")
                        .on_hover_text(format!(
                            "Write the sandbox with this Sun direction to {} \
                             (TOML comments are not kept)",
                            path.display()
                        ))
                        .clicked()
                {
                    self.save();
                }
            }
            Source::Trajectory { table, t } => {
                let (t0, t1) = (table.times()[0], *table.times().last().unwrap());
                let mut tt = *t;
                let utc = table.datetime(tt).format("%Y-%m-%d %H:%M:%S").to_string();
                ui.label(RichText::new(format!("{utc} UTC")).monospace().size(15.0));
                ui.label(
                    RichText::new(format!("{tt:.3} s after the epoch"))
                        .small()
                        .color(theme.text_muted),
                );
                let slider = ui.add(
                    egui::Slider::new(&mut tt, t0..=t1)
                        .show_value(false)
                        .smart_aim(false),
                );
                let mut seek = slider.changed().then_some(tt);
                let times = table.times();
                ui.horizontal(|ui| {
                    if ui
                        .button("Previous row")
                        .on_hover_text("Previous row  (←)")
                        .clicked()
                    {
                        let k = times.partition_point(|&x| x < tt);
                        seek = Some(times[k.saturating_sub(1)]);
                    }
                    if ui
                        .button("Next row")
                        .on_hover_text("Next row  (→)")
                        .clicked()
                    {
                        let k = times.partition_point(|&x| x <= tt);
                        seek = Some(times[k.min(times.len() - 1)]);
                    }
                    if ui
                        .button("Next penumbra")
                        .on_hover_text("The next instant with half the solar disk hidden")
                        .clicked()
                    {
                        seek = next_penumbra(&self.sb, tt + 1.0).or(Some(tt));
                    }
                });
                if let Some(t) = seek {
                    self.set_time(t);
                }
                let (az, el) = az_el(self.ev.sun_dir);
                ui.label(
                    RichText::new(format!(
                        "Sun in the panel frame: az {az:5.1}°, el {el:5.1}°"
                    ))
                    .monospace()
                    .small(),
                );
            }
        }
    }

    fn panel_readout(&self, ui: &mut egui::Ui) {
        let ev = &self.ev;
        let scene = &self.sb.scene;
        let diff = ev.ray.power_w - ev.exact.power_w;
        let rel = if ev.exact.power_w > 0.0 {
            100.0 * diff / ev.exact.power_w
        } else {
            0.0
        };
        let partly = ev
            .lit_exact
            .iter()
            .filter(|&&f| f > 1e-9 && f < 1.0 - 1e-9)
            .count();
        let shaded_strings = scene
            .strings
            .iter()
            .filter(|s| s.cells.iter().any(|&c| ev.lit_exact[c] < 1.0 - 1e-9))
            .count();
        let worst = ev
            .lit_ray
            .iter()
            .zip(&ev.lit_exact)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        let n = (scene.samples_per_cell as f64).sqrt();
        let cos = scene.panels[0].normal.dot(ev.sun_dir).max(0.0);
        egui::Grid::new("panel")
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                let row = |ui: &mut egui::Ui, k: &str, v: String, tip: &str| {
                    ui.label(k).on_hover_text(tip);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(v).monospace());
                    });
                    ui.end_row();
                };
                row(
                    ui,
                    "Power, rays",
                    format!("{:9.3} W", ev.ray.power_w),
                    "The full simulation's number: ray-cast shadows",
                );
                row(
                    ui,
                    "Power, exact shadows",
                    format!("{:9.3} W", ev.exact.power_w),
                    "The same circuit on exactly clipped shadows",
                );
                row(
                    ui,
                    "Rays − exact",
                    format!("{diff:+8.3} W {rel:+6.2} %"),
                    "Sampling error of the n×n shadow rays",
                );
                row(
                    ui,
                    "Irradiance",
                    format!("{:9.1} W/m²", ev.ray.sun.irradiance_w_m2),
                    "At the spacecraft, before eclipse and cos θ",
                );
                row(ui, "cos θ", format!("{cos:9.4}"), "Sun incidence on the panel");
                row(
                    ui,
                    "Cells partly shaded",
                    format!("{partly} of {}", scene.cells.len()),
                    "Exact lit fraction strictly between 0 and 1",
                );
                row(
                    ui,
                    "Strings with shade",
                    format!("{shaded_strings} of {}", scene.strings.len()),
                    "Strings with any cell not fully lit (exact)",
                );
                row(
                    ui,
                    "Worst cell |rays − exact|",
                    format!("{worst:9.4}"),
                    &format!(
                        "Bound for {n:.0}×{n:.0} rays per cell: 1/n = {:.4} (a shadow corner puts two edges in a cell)",
                        1.0 / n
                    ),
                );
                if let ArrayModel::Circuit(c) = &self.sb.model {
                    row(
                        ui,
                        "Cell temperature",
                        format!("{:9.1} °C", c.cell.temperature_k - ZERO_C_K),
                        "Fixed: [electrical] temperature_c. The sandbox has no thermal model.",
                    );
                    let reg = match c.regulation {
                        Regulation::FixedBus { bus_v } => format!("{bus_v:.1} V bus"),
                        Regulation::Mppt => "ideal MPPT".into(),
                    };
                    row(ui, "Regulation", reg, "From [electrical]");
                }
                row(
                    ui,
                    "Evaluation",
                    format!("{:9.2} ms", self.eval_ms),
                    "Rays, exact clipping and curves for this instant",
                );
            });
    }
}

fn heading(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(text).small().color(theme.text_muted));
    ui.add_space(2.0);
}

impl eframe::App for SandboxApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let theme = Theme::current(&ctx);
        self.keys(&ctx);
        if let Some(t) = self.capture.as_mut().and_then(|c| c.sweep_step(&self.sb)) {
            self.set_time(t);
        }
        // SUNLYT_AT applies once any sweep is over.
        if let Some(at) = self
            .capture
            .as_mut()
            .filter(|c| c.swept >= c.sweep)
            .and_then(|c| c.at.take())
        {
            self.seek_spec(&at);
        }

        egui::Panel::top("sb-bar")
            .frame(egui::Frame::side_top_panel(&ctx.global_style()).inner_margin(8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("sunlyt sandbox").strong());
                    ui.label(RichText::new(&self.sb.label).color(theme.text_muted));
                    ui.separator();
                    let n = (self.sb.scene.samples_per_cell as f64).sqrt();
                    ui.label(
                        RichText::new(format!(
                            "{} cells, {} strings, {n:.0}×{n:.0} rays per cell",
                            self.sb.scene.cells.len(),
                            self.sb.scene.strings.len(),
                        ))
                        .color(theme.text_muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        for f in Fill::ALL.iter().rev() {
                            ui.selectable_value(&mut self.fill, *f, f.label());
                        }
                        ui.label(RichText::new("Cells").small().color(theme.text_muted));
                    });
                });
            });

        egui::Panel::right("sb-controls")
            .resizable(true)
            .default_size(360.0)
            .min_size(300.0)
            .frame(egui::Frame::side_top_panel(&ctx.global_style()).inner_margin(12.0))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    heading(ui, &theme, "SUN");
                    self.sun_controls(ui, &theme);
                    heading(ui, &theme, "ECLIPSE");
                    let up = self.sb.scene.panels[0].normal;
                    eclipse::eclipse_inset(ui, &self.ev, &self.sb.body_names(), up, &theme);
                    heading(ui, &theme, "PANEL");
                    self.panel_readout(ui);
                    if let Some(s) = &self.status {
                        ui.add_space(6.0);
                        ui.label(RichText::new(s).small().color(theme.text_muted));
                    }
                });
            });

        let room = ui.available_height();
        let mut hover = None;
        egui::Panel::bottom("sb-iv")
            .resizable(true)
            .default_size((room * 0.38).max(240.0))
            .min_size(160.0)
            .frame(egui::Frame::side_top_panel(&ctx.global_style()).inner_margin(10.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "String I-V curves at this instant · dots: operating points · \
                         hover a curve or a cell to pair them",
                    )
                    .small()
                    .color(theme.text_muted),
                );
                ui.columns(2, |cols| {
                    hover = iv::iv_plot(&mut cols[0], &self.sb.model, &self.ev, self.hover, &theme);
                    egui::ScrollArea::vertical()
                        .id_salt("ops")
                        .show(&mut cols[1], |ui| iv::op_table(ui, &self.ev, &theme));
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme.viewport))
            .show(ui, |ui| {
                let h = view2d::panel_view(
                    ui,
                    &self.layout,
                    &self.sb.scene,
                    &self.ev,
                    self.fill,
                    self.hover,
                    &theme,
                );
                hover = hover.or(h);
            });
        // The highlight follows the pointer on the next frame (no easing).
        if hover != self.hover {
            self.hover = hover;
            ctx.request_repaint();
        }

        if let Some(c) = &mut self.capture {
            c.step(&ctx);
        }
    }
}

/// `SUNLYT_SCREENSHOT` for the sandbox: settle a few frames, save, close.
/// `SUNLYT_AT` seeks first (trajectory mode); `SUNLYT_SWEEP=<n>` first
/// steps through `n` instants of the trajectory and prints evaluation
/// times.
struct SandboxCapture {
    path: PathBuf,
    at: Option<String>,
    sweep: usize,
    swept: usize,
    eval_ms: Vec<f64>,
    frames: usize,
    requested: bool,
    done: Arc<AtomicBool>,
}

impl SandboxCapture {
    fn from_env() -> Option<Self> {
        Some(Self {
            path: PathBuf::from(std::env::var_os("SUNLYT_SCREENSHOT")?),
            at: std::env::var("SUNLYT_AT").ok(),
            sweep: std::env::var("SUNLYT_SWEEP")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            swept: 0,
            eval_ms: Vec::new(),
            frames: 0,
            requested: false,
            done: Arc::new(AtomicBool::new(false)),
        })
    }

    /// The next sweep instant while sweeping (trajectory mode only).
    fn sweep_step(&mut self, sb: &Sandbox) -> Option<f64> {
        let Source::Trajectory { table, .. } = &sb.source else {
            return None;
        };
        if self.swept >= self.sweep {
            return None;
        }
        let (t0, t1) = (table.times()[0], *table.times().last().unwrap());
        let t = t0 + (t1 - t0) * self.swept as f64 / self.sweep as f64;
        self.swept += 1;
        Some(t)
    }

    fn step(&mut self, ctx: &egui::Context) {
        ctx.request_repaint();
        if self.swept < self.sweep {
            return;
        }
        if self.sweep > 0 && self.frames == 0 {
            let mut v = self.eval_ms.clone();
            v.sort_by(f64::total_cmp);
            if let Some(&max) = v.last() {
                println!(
                    "sweep: {} instants; evaluation median {:.2} ms, p95 {:.2} ms, max {max:.2} ms",
                    v.len(),
                    v[v.len() / 2],
                    v[(v.len() - 1) * 95 / 100]
                );
            }
        }
        self.frames += 1;
        if self.frames == 10 && !self.requested {
            self.requested = true;
            let (path, done, ctx2) = (self.path.clone(), self.done.clone(), ctx.clone());
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                egui::ScreenshotCallback::new(move |image| {
                    crate::ui::capture::save_png(&path, &image);
                    done.store(true, Ordering::Release);
                    ctx2.request_repaint();
                }),
            ));
        }
        if self.done.load(Ordering::Acquire) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
