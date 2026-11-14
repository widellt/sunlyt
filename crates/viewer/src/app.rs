//! The window: top bar, 3D view, readout, timeline and plots.

use std::sync::Arc;

use eframe::egui::{self, Key, Modifiers, RichText};

use crate::clock::Clock;
use crate::plots::{self, PlotData};
use crate::scene::SceneView;
use crate::timeline;
use crate::ui::capture::Capture;
use crate::ui::motion;
use crate::ui::readout::readout;
use crate::ui::theme::Theme;
use crate::worker::{RunWorker, Shared, SnapshotWorker};

pub struct ViewerApp {
    sh: Arc<Shared>,
    run: RunWorker,
    snaps: SnapshotWorker,
    clock: Clock,
    scene: Option<SceneView>,
    plot_data: Option<PlotData>,
    /// The run has arrived and the derived data is built.
    run_seen: bool,
    reduced_motion: bool,
    /// A short message (e.g. where the CSV went) and when it was set.
    status: Option<(String, f64)>,
    source: String,
    capture: Option<Capture>,
}

impl ViewerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, sh: Shared, source: String) -> Self {
        Theme::install(&cc.egui_ctx);
        let sh = Arc::new(sh);
        let run = RunWorker::spawn(sh.clone(), cc.egui_ctx.clone());
        let snaps = SnapshotWorker::spawn(sh.clone(), &run, cc.egui_ctx.clone());
        let times = sh.table.times();
        let clock = Clock::new(times[0], *times.last().unwrap());
        let scene = cc
            .wgpu_render_state
            .as_ref()
            .map(|rs| SceneView::new(rs, &sh));
        Self {
            sh,
            run,
            snaps,
            clock,
            scene,
            plot_data: None,
            run_seen: false,
            reduced_motion: false,
            status: None,
            source,
            capture: Capture::from_env(),
        }
    }

    /// Keyboard shortcuts, consumed before any widget sees them (Space
    /// would otherwise also press a focused button).
    fn keys(&mut self, ctx: &egui::Context) {
        let times = self.sh.table.times();
        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::Space) {
                self.clock.toggle();
            }
            if i.consume_key(Modifiers::SHIFT, Key::ArrowRight) {
                self.clock.step_rows(times, 10);
            }
            if i.consume_key(Modifiers::SHIFT, Key::ArrowLeft) {
                self.clock.step_rows(times, -10);
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
                self.clock.step_rows(times, 1);
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                self.clock.step_rows(times, -1);
            }
            if i.consume_key(Modifiers::NONE, Key::Home) {
                self.clock.playing = false;
                self.clock.seek(f64::MIN);
            }
            if i.consume_key(Modifiers::NONE, Key::End) {
                self.clock.playing = false;
                self.clock.seek(f64::MAX);
            }
            if i.consume_key(Modifiers::NONE, Key::CloseBracket) {
                self.clock.faster();
            }
            if i.consume_key(Modifiers::NONE, Key::OpenBracket) {
                self.clock.slower();
            }
        });
    }

    fn export(&mut self, now: f64) {
        let Some(out) = self.run.output() else {
            return;
        };
        let name = format!(
            "sunlyt-plots-{}.csv",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
        );
        let msg = match std::fs::File::create(&name).and_then(|f| {
            plots::export::write_plot_csv(f, &self.sh.table, &self.sh.scene, &out.run)
        }) {
            Ok(()) => {
                let path = std::fs::canonicalize(&name)
                    .map(|p| p.display().to_string())
                    .unwrap_or(name);
                format!("Saved {path}")
            }
            Err(e) => format!("Export failed: {e}"),
        };
        self.status = Some((msg, now));
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, theme: &Theme, now: f64) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("sunlyt").strong());
            ui.label(RichText::new(&self.source).color(theme.text_muted));
            ui.separator();
            let (done, total) = self.run.progress();
            match self.run.output() {
                Some(out) => {
                    ui.label(
                        RichText::new(format!(
                            "Run: {total} rows, {} cells, {:.2} s",
                            self.sh.scene.cells.len(),
                            out.compute_s
                        ))
                        .color(theme.text_muted),
                    );
                }
                None => {
                    let f = done as f32 / total.max(1) as f32;
                    ui.add(
                        egui::ProgressBar::new(f)
                            .desired_width(220.0)
                            .text(format!("Computing run: {done} / {total} rows")),
                    );
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Right-to-left: listed in reverse. Text, not the emoji
                // glyphs of egui's own switch (absent from the fonts).
                let mut pref = ui.ctx().options(|o| o.theme_preference);
                let before = pref;
                for (p, name) in [
                    (egui::ThemePreference::Dark, "Dark"),
                    (egui::ThemePreference::Light, "Light"),
                    (egui::ThemePreference::System, "System"),
                ] {
                    ui.selectable_value(&mut pref, p, name);
                }
                if pref != before {
                    ui.ctx().set_theme(pref);
                }
                ui.separator();
                ui.checkbox(&mut self.reduced_motion, "Reduced motion")
                    .on_hover_text("Camera presets jump instead of gliding");
                if let Some((msg, at)) = &self.status {
                    let on = now - at < 6.0;
                    let a = motion::fade(ui.ctx(), egui::Id::unique("status"), on);
                    if a > 0.0 {
                        ui.label(RichText::new(msg).color(theme.text_muted.gamma_multiply(a)));
                    }
                }
            });
        });
    }
}

impl eframe::App for ViewerApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let theme = Theme::current(&ctx);
        let now = ctx.input(|i| i.time);
        let dt = ctx.input(|i| i.stable_dt) as f64;

        let out = self.run.output();
        let mut force = false;
        if let Some(out) = &out
            && !self.run_seen
        {
            self.run_seen = true;
            self.plot_data = Some(PlotData::new(&self.sh.scene, &out.run));
            if let Some(sv) = &mut self.scene {
                sv.current_max_a = out
                    .run
                    .samples
                    .iter()
                    .flat_map(|s| s.string_current_a.iter().copied())
                    .fold(0.0, f64::max);
            }
            // Snapshots taken before the run used provisional temperatures.
            force = true;
        }

        self.keys(&ctx);
        self.clock.tick(dt);
        if let Some(c) = &mut self.capture {
            let latest = self.snaps.latest();
            let exact = latest
                .as_ref()
                .is_some_and(|s| s.t == self.clock.t && !s.provisional);
            let target =
                c.at.as_deref()
                    .and_then(|a| match a.strip_prefix("penumbra:") {
                        // Middle of the n-th penumbra band (from 0), or of the
                        // first one after a UTC time.
                        Some(n) => {
                            let bands = &out.as_ref()?.bands.penumbra;
                            let (a, z) = match n.parse::<usize>() {
                                Ok(k) => *bands.get(k)?,
                                Err(_) => {
                                    let after = parse_time(&self.sh, n)?;
                                    *bands.iter().find(|b| b.0 >= after)?
                                }
                            };
                            Some(0.5 * (a + z))
                        }
                        None => parse_time(&self.sh, a),
                    })
                    .unwrap_or(self.clock.t);
            let cpu_ms = frame.info().cpu_usage.map(|s| s * 1000.0);
            let range = self.clock.range();
            if let Some(t) = c.step(&ctx, out.is_some(), exact, range, target, cpu_ms) {
                self.clock.playing = false;
                self.clock.seek(t);
            }
            if c.current
                && let Some(sv) = &mut self.scene
            {
                sv.mode = crate::scene::ColourMode::StringCurrent;
            }
            c.record_snapshot_ms(self.snaps.last_compute_ms());
        }
        self.snaps.request(self.clock.t, force);
        let snap = self.snaps.latest();
        if let (Some(sv), Some(s)) = (&mut self.scene, &snap)
            && !self.run_seen
            && sv.current_max_a == 0.0
        {
            // Until the run is known, scale current by this instant.
            sv.current_max_a = s
                .sample
                .string_current_a
                .iter()
                .copied()
                .fold(0.0, f64::max);
        }

        egui::Panel::top("bar")
            .frame(egui::Frame::side_top_panel(&ctx.global_style()).inner_margin(8.0))
            .show(ui, |ui| self.top_bar(ui, &theme, now));

        egui::Panel::right("readout")
            .resizable(true)
            .default_size(320.0)
            .min_size(260.0)
            .frame(egui::Frame::side_top_panel(&ctx.global_style()).inner_margin(12.0))
            .show(ui, |ui| match &snap {
                Some(s) => {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        readout(ui, &self.sh, s, self.sh.table.len(), &theme);
                    });
                }
                None => {
                    ui.label(RichText::new("Evaluating…").color(theme.text_muted));
                }
            });

        let mut export = false;
        // The 3D view keeps at least half the height below the top bar.
        let room = ui.available_height();
        egui::Panel::bottom("time")
            .resizable(true)
            .default_size((room * 0.42).max(260.0))
            .min_size(140.0)
            .max_size((room * 0.5).max(200.0))
            .frame(egui::Frame::side_top_panel(&ctx.global_style()).inner_margin(10.0))
            .show(ui, |ui| {
                let sh = self.sh.clone();
                let utc = |t: f64| sh.utc(t);
                timeline::timeline(
                    ui,
                    &mut self.clock,
                    sh.table.times(),
                    out.as_ref().map(|o| &o.bands),
                    &theme,
                    &utc,
                );
                ui.add_space(6.0);
                match (&self.plot_data, &out) {
                    (Some(data), Some(o)) => {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(
                                    "Batch run rows · drag to pan, scroll to zoom, click to seek, double-click to reset",
                                )
                                .small()
                                .color(theme.text_muted),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    export = ui
                                        .small_button("Export CSV")
                                        .on_hover_text(
                                            "The plotted numbers, with the batch CSV's columns, to the working directory",
                                        )
                                        .clicked();
                                },
                            );
                        });
                        let ev = plots::plots(ui, data, &o.bands, &sh.table, self.clock.t, &theme);
                        if let Some(t) = ev.seek {
                            self.clock.seek(t);
                        }
                    }
                    _ => {
                        ui.label(
                            RichText::new("Plots appear when the run finishes.")
                                .color(theme.text_muted),
                        );
                    }
                }
            });
        if export {
            self.export(now);
        }

        let mut moving = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme.viewport))
            .show(ui, |ui| {
                match (&mut self.scene, frame.wgpu_render_state()) {
                    (Some(sv), Some(rs)) => {
                        moving =
                            sv.show(ui, rs, &self.sh, snap.as_ref(), &theme, self.reduced_motion);
                    }
                    _ => {
                        ui.label("No wgpu device: the 3D view is unavailable.");
                    }
                }
            });

        if self.clock.playing || moving || self.snaps.pending() {
            ctx.request_repaint();
        } else if self.run.output().is_none() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        } else if self.status.as_ref().is_some_and(|(_, at)| now - at < 6.5) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(c) = &mut self.capture {
            c.take(&ctx);
        }
    }
}

/// Seconds after the epoch, or UTC `YYYY-MM-DDTHH:MM:SS`.
fn parse_time(sh: &Shared, s: &str) -> Option<f64> {
    if let Ok(t) = s.parse::<f64>() {
        return Some(t);
    }
    let at = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok()?;
    Some((at - sh.table.epoch()).num_milliseconds() as f64 / 1000.0)
}
