//! Interactive viewer of `sunlyt` runs (design D8, D9): eframe/egui with a
//! custom wgpu 3D pass. All numbers come from `sunlyt-core`, the same code
//! as the batch CLI; at a row time the viewer shows the batch sample
//! itself.
//!
//! Layout (one folder per concern):
//! - [`worker`] — batch run and one-instant snapshots off the UI thread
//! - [`clock`] — simulated time, playback, row stepping
//! - [`scene`] — 3D view: mesh, cell colours, markers, camera, legend
//! - [`timeline`] — transport controls, scrubber, eclipse bands
//! - [`plots`] — stacked power / temperature / visible-disk plots, CSV export
//! - [`ui`] — theme tokens, motion curves, the numeric readout
//!
//! Look and motion follow `.claude/skills/sunlyt-viewer-design`.

mod app;
pub mod clock;
pub mod plots;
pub mod sandbox;
pub mod scene;
pub mod timeline;
pub mod ui;
pub mod worker;

use anyhow::{Result, anyhow};
use eframe::egui;
use sunlyt_core::thermal::ThermalModel;
use sunlyt_core::{ArrayModel, EnvConfig, Scene, StateTable};

/// Everything a viewer session needs, already loaded.
pub struct Inputs {
    pub table: StateTable,
    pub scene: Scene,
    pub model: ArrayModel,
    pub env: EnvConfig,
    pub thermal: Option<ThermalModel>,
    /// Shown in the title bar (e.g. the scene file name).
    pub source: String,
}

/// Opens the viewer window and blocks until it is closed.
pub fn run(inputs: Inputs) -> Result<()> {
    if inputs.table.is_empty() {
        return Err(anyhow!("the trajectory has no rows"));
    }
    let title = format!("sunlyt — {}", inputs.source);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([1480.0, 940.0])
            .with_min_inner_size([960.0, 640.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    let Inputs {
        table,
        scene,
        model,
        env,
        thermal,
        source,
    } = inputs;
    let shared = worker::Shared::new(table, scene, model, env, thermal);
    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| Ok(Box::new(app::ViewerApp::new(cc, shared, source)))),
    )
    .map_err(|e| anyhow!("viewer: {e}"))
}
