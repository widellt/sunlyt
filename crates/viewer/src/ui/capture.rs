//! Scripted capture for checks and docs, driven by environment variables
//! (unset in normal use):
//!
//! - `SUNLYT_SCREENSHOT=out.png` — once the run has finished and the
//!   instant is exact, save a screenshot of the window and close it.
//! - `SUNLYT_AT=<t>` — first jump to `t`: seconds after the epoch, UTC
//!   `YYYY-MM-DDTHH:MM:SS`, or `penumbra:<n>` (middle of the n-th band).
//! - `SUNLYT_SWEEP=<frames>` — before the screenshot, scrub across the
//!   whole mission in that many frames and print frame-time statistics
//!   (the 16 ms / 200 ms feedback budget).
//! - `SUNLYT_MODE=current` — colour cells by string current.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Stage {
    WaitRun,
    Sweep(usize),
    Settle(usize),
    Requested,
    Done,
}

pub struct Capture {
    pub path: PathBuf,
    pub at: Option<String>,
    pub current: bool,
    sweep: usize,
    stage: Stage,
    frame_ms: Vec<f32>,
    snapshot_ms: Vec<f32>,
    stale_frames: usize,
    done: Arc<AtomicBool>,
}

impl Capture {
    pub fn from_env() -> Option<Self> {
        let path = PathBuf::from(std::env::var_os("SUNLYT_SCREENSHOT")?);
        Some(Self {
            path,
            at: std::env::var("SUNLYT_AT").ok(),
            current: std::env::var("SUNLYT_MODE").is_ok_and(|m| m == "current"),
            sweep: std::env::var("SUNLYT_SWEEP")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            stage: Stage::WaitRun,
            frame_ms: Vec::new(),
            snapshot_ms: Vec::new(),
            stale_frames: 0,
            done: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Records how long the instant worker took for its latest result
    /// (during the sweep only).
    pub fn record_snapshot_ms(&mut self, ms: f32) {
        if matches!(self.stage, Stage::Sweep(_)) && ms > 0.0 {
            self.snapshot_ms.push(ms);
        }
    }

    /// Advances the script. `run_done`: the batch run is in; `exact`: the
    /// shown snapshot is for the requested time. Returns a time to seek to.
    pub fn step(
        &mut self,
        ctx: &egui::Context,
        run_done: bool,
        exact: bool,
        range: (f64, f64),
        target: f64,
        cpu_ms: Option<f32>,
    ) -> Option<f64> {
        ctx.request_repaint();
        match self.stage {
            Stage::WaitRun if run_done => {
                self.stage = if self.sweep > 0 {
                    Stage::Sweep(0)
                } else {
                    Stage::Settle(0)
                };
                Some(target)
            }
            Stage::Sweep(i) => {
                if let Some(ms) = cpu_ms {
                    self.frame_ms.push(ms);
                }
                if !exact {
                    self.stale_frames += 1;
                }
                if i >= self.sweep {
                    self.report();
                    self.stage = Stage::Settle(0);
                    return Some(target);
                }
                self.stage = Stage::Sweep(i + 1);
                let (t0, t1) = range;
                Some(t0 + (t1 - t0) * i as f64 / self.sweep as f64)
            }
            Stage::Settle(n) => {
                // A few exact frames so springs, fades and colours settle.
                self.stage = if exact && n >= 20 {
                    let (path, done, ctx2) = (self.path.clone(), self.done.clone(), ctx.clone());
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                        egui::ScreenshotCallback::new(move |image| {
                            save_png(&path, &image);
                            done.store(true, Ordering::Release);
                            ctx2.request_repaint();
                        }),
                    ));
                    Stage::Requested
                } else {
                    Stage::Settle(n + 1)
                };
                None
            }
            _ => None,
        }
    }

    fn report(&self) {
        let stats = |v: &[f32]| {
            let mut v = v.to_vec();
            v.sort_by(f32::total_cmp);
            if v.is_empty() {
                return "n/a".to_owned();
            }
            let pct = |p: f64| v[((v.len() - 1) as f64 * p) as usize];
            format!(
                "median {:.2} ms, p95 {:.2} ms, max {:.2} ms",
                pct(0.5),
                pct(0.95),
                v[v.len() - 1]
            )
        };
        println!(
            "sweep: {} frames; UI CPU per frame {}; instant worker {}; \
             frames still showing the previous instant: {}",
            self.frame_ms.len(),
            stats(&self.frame_ms),
            stats(&self.snapshot_ms),
            self.stale_frames
        );
    }

    /// Closes the window once the screenshot is saved; true when done.
    pub fn take(&mut self, ctx: &egui::Context) -> bool {
        if self.stage == Stage::Requested && self.done.load(Ordering::Acquire) {
            self.stage = Stage::Done;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.stage == Stage::Done
    }
}

pub(crate) fn save_png(path: &Path, image: &egui::ColorImage) {
    let [w, h] = image.size;
    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    let r = std::fs::File::create(path)
        .map_err(png::EncodingError::from)
        .and_then(|f| {
            let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header()?.write_image_data(&rgba)
        });
    match r {
        Ok(()) => println!("screenshot: {}", path.display()),
        Err(e) => eprintln!("screenshot {}: {e}", path.display()),
    }
}
