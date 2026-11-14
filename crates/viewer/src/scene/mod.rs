//! The 3D view: spacecraft mesh, coloured cells, direction markers, the
//! orbit camera and the colour legend.

pub mod camera;
pub mod colour;
pub mod gpu;
pub mod markers;
pub mod mesh;

use std::sync::Arc;

use eframe::egui::{self, Color32, Pos2, RichText, Vec2};
use eframe::egui_wgpu;
use glam::Vec3;

pub use camera::{Camera, Preset};
pub use colour::ColourMode;
pub use gpu::SceneRenderer;

use crate::ui::theme::Theme;
use crate::worker::{Shared, Snapshot};

/// State of the 3D view kept between frames.
pub struct SceneView {
    pub renderer: SceneRenderer,
    pub camera: Camera,
    pub mode: ColourMode,
    /// Colour-scale top for string current, A (whole-run maximum once the
    /// run is known).
    pub current_max_a: f64,
    /// The snapshot and mode the uploaded cell colours belong to.
    coloured: Option<(Arc<Snapshot>, ColourMode, u64)>,
}

impl SceneView {
    pub fn new(rs: &egui_wgpu::RenderState, sh: &Shared) -> Self {
        let (centre, radius) = mesh::bounds(&sh.scene);
        let lift = (radius as f64 * 2e-4).max(sh.scene.ray_offset_m);
        Self {
            renderer: SceneRenderer::new(rs, &sh.scene, lift),
            camera: Camera::new(centre, radius),
            mode: ColourMode::Illumination,
            current_max_a: 0.0,
            coloured: None,
        }
    }

    /// Draws the view into `ui`'s remaining space. Returns true while the
    /// camera is still moving (needs another frame).
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        rs: &egui_wgpu::RenderState,
        sh: &Shared,
        snap: Option<&Arc<Snapshot>>,
        theme: &Theme,
        reduced_motion: bool,
    ) -> bool {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());

        // Input lands on the springs' targets this frame.
        if response.dragged_by(egui::PointerButton::Primary) {
            let d = response.drag_delta();
            self.camera.orbit(d.x, d.y);
        }
        if response.hovered() {
            // Raw wheel events: the camera spring does the smoothing, so
            // egui's own scroll smoothing would only add lag.
            let scroll: f32 = ui.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::MouseWheel { unit, delta, .. } => Some(match unit {
                            egui::MouseWheelUnit::Point => delta.y,
                            egui::MouseWheelUnit::Line => delta.y * 40.0,
                            egui::MouseWheelUnit::Page => delta.y * 400.0,
                        }),
                        _ => None,
                    })
                    .sum()
            });
            if scroll != 0.0 {
                self.camera.zoom(scroll);
            }
        }
        if response.double_clicked() {
            self.camera.reset();
        }
        let dt = ui.input(|i| i.stable_dt);
        let moving = self.camera.update(dt);

        let Some(snap) = snap else {
            ui.painter().rect_filled(rect, 0.0, theme.viewport);
            return moving;
        };

        let fresh = match &self.coloured {
            Some((s, mode, max)) => {
                Arc::ptr_eq(s, snap) && *mode == self.mode && *max == self.current_max_a.to_bits()
            }
            None => false,
        };
        if !fresh {
            let colours = colour::cell_colours(
                &sh.scene,
                &sh.cell_string,
                snap,
                self.mode,
                self.current_max_a,
            );
            self.renderer.set_cell_colours(rs, &colours);
            self.coloured = Some((snap.clone(), self.mode, self.current_max_a.to_bits()));
        }

        let ppp = ui.ctx().pixels_per_point();
        let px = [
            (rect.width() * ppp).round() as u32,
            (rect.height() * ppp).round() as u32,
        ];
        let aspect = rect.width() / rect.height().max(1.0);
        let vp = self.camera.view_proj(aspect);
        let m = markers::markers(
            snap,
            &sh.body_names,
            self.camera.centre,
            self.camera.radius,
            theme,
        );
        let sun = snap.sample.sun.sun_dir_b.as_vec3();
        let tex = self.renderer.render(
            rs,
            px,
            &gpu::Frame {
                view_proj: vp,
                eye: self.camera.eye(),
                sun_dir: sun,
                visible: snap.sample.sun.visible as f32,
                structure: theme.structure,
                clear: theme.viewport,
                lines: &m.lines,
            },
        );
        ui.painter().image(
            tex,
            rect,
            egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        // Labels at projected line ends.
        let painter = ui.painter_at(rect);
        for l in &m.labels {
            let c = vp * l.at.extend(1.0);
            if c.w <= 0.0 {
                continue;
            }
            let ndc = c.truncate() / c.w;
            let p = Pos2::new(
                rect.min.x + (ndc.x + 1.0) * 0.5 * rect.width(),
                rect.min.y + (1.0 - ndc.y) * 0.5 * rect.height(),
            );
            painter.text(
                p,
                egui::Align2::LEFT_BOTTOM,
                &l.text,
                egui::FontId::proportional(12.0),
                l.colour,
            );
        }

        self.overlay(ui, rect, sh, sun, theme, reduced_motion);
        moving
    }

    /// Colour-mode switch, camera presets and the legend over the view.
    fn overlay(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        sh: &Shared,
        sun: Vec3,
        theme: &Theme,
        reduced_motion: bool,
    ) {
        let circuit = matches!(sh.model, sunlyt_core::ArrayModel::Circuit(_));
        let mut bar = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink(10.0))
                .layout(egui::Layout::left_to_right(egui::Align::Min)),
        );
        egui::Frame::popup(bar.style())
            .fill(theme.surface_raised)
            .show(&mut bar, |ui| {
                ui.horizontal(|ui| {
                    for mode in [ColourMode::Illumination, ColourMode::StringCurrent] {
                        let enabled = mode == ColourMode::Illumination || circuit;
                        ui.add_enabled_ui(enabled, |ui| {
                            ui.selectable_value(&mut self.mode, mode, mode.label())
                                .on_disabled_hover_text("Needs an [electrical] table");
                        });
                    }
                    ui.separator();
                    ui.label(RichText::new("View").color(theme.text_muted));
                    for p in Preset::ALL {
                        let hint = match p {
                            Preset::Sun => "Look from the Sun",
                            _ => "Look from this body axis",
                        };
                        if ui.small_button(p.label()).on_hover_text(hint).clicked() {
                            self.camera.preset(p, sun, reduced_motion);
                        }
                    }
                    ui.label(
                        RichText::new("Double-click: reset")
                            .small()
                            .color(theme.text_muted),
                    );
                });
            });

        // Legend, bottom left.
        let (w, h) = (220.0, 10.0);
        let origin = Pos2::new(rect.min.x + 14.0, rect.max.y - 52.0);
        let painter = ui.painter_at(rect);
        let panel =
            egui::Rect::from_min_size(origin - Vec2::new(8.0, 22.0), Vec2::new(w + 16.0, 60.0));
        painter.rect_filled(panel, 6.0, theme.surface_raised.gamma_multiply(0.92));
        painter.text(
            origin - Vec2::new(0.0, 6.0),
            egui::Align2::LEFT_BOTTOM,
            self.mode.legend_title(),
            egui::FontId::proportional(11.0),
            theme.text_muted,
        );
        let n = 64;
        for i in 0..n {
            let x0 = origin.x + w * i as f32 / n as f32;
            let r = egui::Rect::from_min_max(
                Pos2::new(x0, origin.y),
                Pos2::new(x0 + w / n as f32 + 0.5, origin.y + h),
            );
            painter.rect_filled(r, 0.0, self.mode.map((i as f64 + 0.5) / n as f64));
        }
        let (lo, mid, hi) = match self.mode {
            ColourMode::Illumination => ("0".to_owned(), "0.5".to_owned(), "1".to_owned()),
            ColourMode::StringCurrent => {
                let m = self.current_max_a;
                (
                    "0 A".to_owned(),
                    format!("{:.3} A", m / 2.0),
                    format!("{m:.3} A"),
                )
            }
        };
        let font = egui::FontId::monospace(11.0);
        let y = origin.y + h + 3.0;
        painter.text(
            Pos2::new(origin.x, y),
            egui::Align2::LEFT_TOP,
            lo,
            font.clone(),
            theme.text,
        );
        painter.text(
            Pos2::new(origin.x + w / 2.0, y),
            egui::Align2::CENTER_TOP,
            mid,
            font.clone(),
            theme.text,
        );
        painter.text(
            Pos2::new(origin.x + w, y),
            egui::Align2::RIGHT_TOP,
            hi,
            font,
            theme.text,
        );
    }
}
