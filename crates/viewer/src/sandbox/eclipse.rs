//! Eclipse inset: the solar disk and every body's disk at their true
//! angular sizes, as seen from the spacecraft, next to the visible
//! fraction the simulation uses. It is a picture to compare against the
//! number: the overlap drawn here should look like the hidden share.
//!
//! The sky is drawn in the plane tangent to the Sun direction, with up
//! towards the panel normal. Distances on the sky are angles (gnomonic
//! distortion is negligible within a few solar radii of the Sun). An
//! oblate body is drawn as its equatorial circle; the eclipse number uses
//! the true limb.

use eframe::egui::{self, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use glam::DVec3;
use sunlyt_core::AngleExt;
use sunlyt_core::env::SUN_RADIUS_M;

use super::model::{Evaluation, body_disk};
use crate::ui::theme::Theme;

/// Solar radius on screen, px.
const SUN_PX: f32 = 30.0;

pub fn eclipse_inset(
    ui: &mut egui::Ui,
    ev: &Evaluation,
    names: &[String],
    up: DVec3,
    theme: &Theme,
) {
    let sun = &ev.ray.sun;
    ui.horizontal(|ui| {
        let (resp, painter) = ui.allocate_painter(Vec2::new(150.0, 110.0), Sense::hover());
        let rect = resp.rect;
        painter.rect_filled(rect, 4.0, theme.viewport);
        let painter = painter.with_clip_rect(rect);
        let c = rect.center();
        let s = sun.sun_dir_b;
        let sun_r = (SUN_RADIUS_M / sun.sun_dist_m).asin();
        let px_per_rad = SUN_PX as f64 / sun_r;
        // Tangent basis: e2 is "up" (towards the panel normal), e1 right.
        let mut e2 = up - s * up.dot(s);
        if e2.length() < 1e-9 {
            e2 = s.any_orthonormal_vector();
        }
        let e2 = e2.normalize();
        let e1 = e2.cross(s);
        painter.circle_filled(c, SUN_PX, theme.sun);
        for b in &ev.epoch.bodies {
            let (r, dir) = body_disk(b);
            let sep = s.angle_exact(dir);
            if sep - r > 12.0 * sun_r {
                continue; // far off the inset
            }
            let t = (dir - s * dir.dot(s)).normalize_or_zero();
            let at = Vec2::new(t.dot(e1) as f32, -(t.dot(e2) as f32));
            let centre = c + at * (sep * px_per_rad) as f32;
            let radius = (r * px_per_rad) as f32;
            let colour = if names.get(b.index).is_some_and(|n| n.contains("Earth")) {
                theme.earth
            } else {
                theme.moon
            };
            disk(&painter, rect, centre, radius, colour);
        }
        painter.circle_stroke(c, SUN_PX, Stroke::new(1.0, theme.sun.gamma_multiply(0.8)));
        let caption = painter.layout_no_wrap(
            "Sun and bodies, true scale".into(),
            egui::FontId::proportional(9.5),
            theme.text_muted,
        );
        let at = rect.left_bottom() + Vec2::new(6.0, -4.0 - caption.size().y);
        painter.rect_filled(
            egui::Rect::from_min_size(at, caption.size()).expand(2.0),
            2.0,
            theme.viewport,
        );
        painter.galley(at, caption, theme.text_muted);
        ui.vertical(|ui| {
            ui.label(
                RichText::new(format!("{:7.3} %", 100.0 * sun.visible))
                    .monospace()
                    .size(15.0),
            );
            ui.label(
                RichText::new("of the disk visible")
                    .small()
                    .color(theme.text_muted),
            );
            ui.add_space(4.0);
            for (k, f) in sun.occulted.iter().enumerate() {
                if *f > 0.0 {
                    let name = names.get(k).map_or("body", String::as_str);
                    ui.label(
                        RichText::new(format!("{name}: {:.3} % hidden", 100.0 * f))
                            .small()
                            .monospace(),
                    );
                }
            }
            let state = match sun.visible {
                v if v >= 1.0 => "Full sun",
                v if v <= 0.0 => "Umbra",
                _ => "Penumbra",
            };
            ui.label(RichText::new(state).small().color(theme.text_muted));
        });
    });
}

/// A filled disk that may be thousands of pixels across (the Moon from low
/// orbit is ~270 solar radii): drawn as the pie slice whose arc covers the
/// inset, which stays exact where it is visible.
fn disk(painter: &egui::Painter, rect: Rect, centre: Pos2, radius: f32, colour: egui::Color32) {
    if radius < 400.0 {
        painter.circle_filled(centre, radius, colour);
        return;
    }
    let to_inset = rect.center() - centre;
    let d = to_inset.length().max(1e-3);
    let a0 = to_inset.y.atan2(to_inset.x);
    let half = ((rect.size().length() / d).min(1.0)).asin().max(0.01) * 1.5;
    let n = 96;
    let mut pts = vec![centre];
    for i in 0..=n {
        let a = a0 - half + 2.0 * half * i as f32 / n as f32;
        pts.push(centre + Vec2::new(a.cos(), a.sin()) * radius);
    }
    painter.add(egui::Shape::convex_polygon(pts, colour, Stroke::NONE));
}
