//! The panel face on: substrate, cells filled by illumination, each
//! string's wiring as a coloured line through its cells, shadow polygons,
//! and a small sky dome with the Sun.
//!
//! The view's up is the panel's +v and its right +u, so it matches the
//! layout maps of `sunlyt check --svg`.

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use glam::DVec2;
use sunlyt_core::Scene;

use super::model::{Evaluation, az_el};
use crate::scene::ColourMode;
use crate::ui::theme::Theme;

/// What fills the cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    /// Ray-cast lit fraction (what the power uses).
    Rays,
    /// Exact clipped lit fraction.
    Exact,
    /// Rays − exact, on a diverging scale.
    Difference,
}

impl Fill {
    pub const ALL: [Fill; 3] = [Fill::Rays, Fill::Exact, Fill::Difference];

    pub fn label(self) -> &'static str {
        match self {
            Fill::Rays => "Lit (rays)",
            Fill::Exact => "Lit (exact)",
            Fill::Difference => "Rays − exact",
        }
    }
}

/// The panel's fixed 2-D geometry, in panel-plane metres.
pub struct Layout2d {
    /// Substrate face triangles.
    pub face: Vec<[DVec2; 3]>,
    /// Per cell: (min, max) corners.
    pub cells: Vec<(DVec2, DVec2)>,
    /// Per string: cell centres in wiring order.
    pub strings: Vec<Vec<DVec2>>,
    /// String of each cell.
    pub string_of: Vec<usize>,
    pub lo: DVec2,
    pub hi: DVec2,
}

impl Layout2d {
    pub fn new(scene: &Scene) -> Self {
        let p = &scene.panels[0];
        let to2 = |q: glam::DVec3| {
            let r = q - p.centroid;
            DVec2::new(r.dot(p.u), r.dot(p.v))
        };
        let face: Vec<[DVec2; 3]> = (0..scene.mesh.triangles.len())
            .filter(|&t| scene.mesh.tri_object[t] as usize == p.object)
            .filter(|&t| {
                let c = scene.mesh.tri_cross(t);
                c.length() > 0.0 && c.normalize().dot(p.normal) > 0.99
            })
            .map(|t| scene.mesh.tri(t).map(to2))
            .collect();
        let cells: Vec<(DVec2, DVec2)> = p
            .cells
            .clone()
            .map(|i| sunlyt_core::shadow::exact::cell_rect(scene, 0, i))
            .collect();
        let mut string_of = vec![0; cells.len()];
        let strings = scene
            .strings
            .iter()
            .enumerate()
            .map(|(s, st)| {
                st.cells
                    .iter()
                    .map(|&c| {
                        string_of[c] = s;
                        (cells[c].0 + cells[c].1) / 2.0
                    })
                    .collect()
            })
            .collect();
        let (lo, hi) = face
            .iter()
            .flatten()
            .chain(cells.iter().flat_map(|c| [&c.0, &c.1]))
            .fold(
                (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
                |(lo, hi), q| (lo.min(*q), hi.max(*q)),
            );
        Self {
            face,
            cells,
            strings,
            string_of,
            lo,
            hi,
        }
    }
}

/// Diverging map for differences in [−1, 1]: blue (negative) through the
/// neutral surface to orange (positive). Symmetric lightness.
fn diverging(x: f64, theme: &Theme) -> Color32 {
    let x = x.clamp(-1.0, 1.0) as f32;
    let (end, mid) = (
        if x < 0.0 {
            theme.series[0]
        } else {
            theme.series[1]
        },
        theme.surface_raised,
    );
    let t = x.abs().sqrt(); // small differences stay visible
    Color32::from_rgb(
        (mid.r() as f32 + (end.r() as f32 - mid.r() as f32) * t) as u8,
        (mid.g() as f32 + (end.g() as f32 - mid.g() as f32) * t) as u8,
        (mid.b() as f32 + (end.b() as f32 - mid.b() as f32) * t) as u8,
    )
}

/// Draws the panel. `hover` is the highlighted string (from here or the
/// I-V plot); returns the string under the pointer, if any.
pub fn panel_view(
    ui: &mut egui::Ui,
    layout: &Layout2d,
    scene: &Scene,
    ev: &Evaluation,
    fill: Fill,
    hover: Option<usize>,
    theme: &Theme,
) -> Option<usize> {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::hover());
    let area = resp.rect.shrink(28.0);
    let span = layout.hi - layout.lo;
    let scale = (area.width() as f64 / span.x).min(area.height() as f64 / span.y);
    let mid = (layout.lo + layout.hi) / 2.0;
    let c = area.center();
    let to_screen = |q: DVec2| {
        Pos2::new(
            c.x + ((q.x - mid.x) * scale) as f32,
            c.y - ((q.y - mid.y) * scale) as f32,
        )
    };

    // Substrate.
    for t in &layout.face {
        painter.add(egui::Shape::convex_polygon(
            t.iter().map(|&q| to_screen(q)).collect(),
            theme.structure.gamma_multiply(0.35),
            Stroke::NONE,
        ));
    }

    // Cells.
    let pointer = resp.hover_pos();
    let mut hovered_cell = None;
    for (k, &(lo, hi)) in layout.cells.iter().enumerate() {
        let r = Rect::from_two_pos(to_screen(lo), to_screen(hi));
        let colour = match fill {
            Fill::Rays => ColourMode::Illumination.map(ev.lit_ray[k]),
            Fill::Exact => ColourMode::Illumination.map(ev.lit_exact[k]),
            Fill::Difference => diverging(ev.lit_ray[k] - ev.lit_exact[k], theme),
        };
        painter.rect_filled(r, 0.0, colour);
        if pointer.is_some_and(|p| r.contains(p)) {
            hovered_cell = Some(k);
        }
    }

    // Wiring: one line per string through its cells, in series order.
    let hover_string = hovered_cell.map(|k| layout.string_of[k]).or(hover);
    for (s, pts) in layout.strings.iter().enumerate() {
        let on = hover_string == Some(s);
        let colour = theme.series(s);
        let pts: Vec<Pos2> = pts.iter().map(|&q| to_screen(q)).collect();
        let w = if on { 3.0 } else { 1.25 };
        let alpha = if hover_string.is_some() && !on {
            0.35
        } else {
            1.0
        };
        painter.add(egui::Shape::line(
            pts.clone(),
            Stroke::new(w, colour.gamma_multiply(alpha)),
        ));
        if let Some(&first) = pts.first() {
            painter.circle_filled(
                first,
                if on { 4.0 } else { 2.5 },
                colour.gamma_multiply(alpha),
            );
        }
    }

    // Shadow polygons (each occluder triangle's projection): outlines.
    let clip = painter.with_clip_rect(
        Rect::from_two_pos(to_screen(layout.lo), to_screen(layout.hi)).expand(12.0),
    );
    let stroke = Stroke::new(1.0, theme.text.gamma_multiply(0.55));
    for poly in &ev.shadows {
        let mut pts: Vec<Pos2> = poly.iter().map(|&q| to_screen(q)).collect();
        if let Some(&f) = pts.first() {
            pts.push(f);
        }
        clip.add(egui::Shape::line(pts, stroke));
    }

    sky_dome(&painter, resp.rect, ev, theme);
    legend(&painter, resp.rect, fill, theme);

    if let Some(k) = hovered_cell {
        let cell = &scene.cells[scene.panels[0].cells.start + k];
        let s = layout.string_of[k];
        resp.on_hover_ui_at_pointer(|ui| {
            ui.label(RichText::new(format!("Row {}, column {}", cell.row, cell.col)).strong());
            egui::Grid::new("cell-tip").num_columns(2).show(ui, |ui| {
                let row = |ui: &mut egui::Ui, k: &str, v: String| {
                    ui.label(k);
                    ui.label(RichText::new(v).monospace());
                    ui.end_row();
                };
                row(ui, "String", format!("{}", s + 1));
                row(ui, "Lit (rays)", format!("{:6.4}", ev.lit_ray[k]));
                row(ui, "Lit (exact)", format!("{:6.4}", ev.lit_exact[k]));
                row(
                    ui,
                    "Rays − exact",
                    format!("{:+7.4}", ev.lit_ray[k] - ev.lit_exact[k]),
                );
                if !ev.ray.string_current_a.is_empty() {
                    row(
                        ui,
                        "String current",
                        format!("{:6.4} A", ev.ray.string_current_a[s]),
                    );
                }
            });
        });
    }
    hovered_cell.map(|k| layout.string_of[k])
}

/// Sky dome in the top-left corner: the panel normal at the centre, the
/// horizon (panel plane) at the rim, the Sun at its elevation.
fn sky_dome(painter: &egui::Painter, rect: Rect, ev: &Evaluation, theme: &Theme) {
    let r = 34.0;
    let c = rect.left_top() + Vec2::new(16.0 + r, 16.0 + r);
    painter.circle(c, r, theme.surface_raised, Stroke::new(1.0, theme.rule));
    painter.circle_stroke(c, r * 0.5, Stroke::new(1.0, theme.rule.gamma_multiply(0.6)));
    let (az, el) = az_el(ev.sun_dir);
    let rho = r * (1.0 - el.max(0.0) as f32 / 90.0);
    let a = az.to_radians() as f32;
    let sun = c + Vec2::new(a.cos(), -a.sin()) * rho;
    if el >= 0.0 {
        painter.circle_filled(sun, 5.0, theme.sun);
    } else {
        painter.circle_stroke(sun, 5.0, Stroke::new(1.5, theme.sun));
    }
    painter.text(
        c + Vec2::new(r + 8.0, -8.0),
        egui::Align2::LEFT_CENTER,
        format!("Sun az {az:5.1}°"),
        egui::FontId::monospace(11.0),
        theme.text,
    );
    painter.text(
        c + Vec2::new(r + 8.0, 8.0),
        egui::Align2::LEFT_CENTER,
        format!(
            "    el {el:5.1}°{}",
            if el < 0.0 { " (behind)" } else { "" }
        ),
        egui::FontId::monospace(11.0),
        theme.text,
    );
    painter.text(
        c + Vec2::new(r - 2.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        "+u",
        egui::FontId::proportional(9.0),
        theme.text_muted,
    );
}

fn legend(painter: &egui::Painter, rect: Rect, fill: Fill, theme: &Theme) {
    let w = 160.0;
    let bar = Rect::from_min_size(
        rect.left_bottom() + Vec2::new(16.0, -30.0),
        Vec2::new(w, 8.0),
    );
    let n = 48;
    for i in 0..n {
        let x = (i as f64 + 0.5) / n as f64;
        let colour = match fill {
            Fill::Difference => diverging(2.0 * x - 1.0, theme),
            _ => ColourMode::Illumination.map(x),
        };
        let r = Rect::from_min_size(
            bar.min + Vec2::new(w * i as f32 / n as f32, 0.0),
            Vec2::new(w / n as f32 + 0.5, 8.0),
        );
        painter.rect_filled(r, 0.0, colour);
    }
    let (lo, hi) = match fill {
        Fill::Difference => ("−1", "+1"),
        _ => ("0", "1"),
    };
    let font = egui::FontId::monospace(10.0);
    painter.text(
        bar.left_bottom() + Vec2::new(0.0, 3.0),
        egui::Align2::LEFT_TOP,
        lo,
        font.clone(),
        theme.text_muted,
    );
    painter.text(
        bar.right_bottom() + Vec2::new(0.0, 3.0),
        egui::Align2::RIGHT_TOP,
        hi,
        font,
        theme.text_muted,
    );
    painter.text(
        bar.left_top() - Vec2::new(0.0, 3.0),
        egui::Align2::LEFT_BOTTOM,
        match fill {
            Fill::Rays => "Geometric lit fraction, rays",
            Fill::Exact => "Geometric lit fraction, exact",
            Fill::Difference => "Lit fraction, rays − exact",
        },
        egui::FontId::proportional(11.0),
        theme.text_muted,
    );
}
