//! Transport controls and the scrubber with eclipse bands. Nothing here
//! animates: every input moves simulated time on the same frame.

mod bands;

pub use bands::{BAND_SAMPLE_S, Bands};

use eframe::egui::{self, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use crate::clock::{Clock, SPEEDS};
use crate::ui::theme::Theme;

fn speed_label(s: f64) -> String {
    match s {
        x if x >= 1e5 => "10⁵×".into(),
        x if x >= 1e4 => "10⁴×".into(),
        x if x >= 3600.0 => "3600×".into(),
        x => format!("{x:.0}×"),
    }
}

#[derive(Clone, Copy)]
enum Icon {
    Play,
    Pause,
    StepBack,
    StepForward,
}

/// A transport button with a painted icon (the bundled fonts have no media
/// glyphs). Darkens on press on the same frame, like every egui button.
fn icon_button(ui: &mut egui::Ui, icon: Icon, theme: &Theme) -> egui::Response {
    let size = Vec2::new(34.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let v = ui.style().interact(&response);
    let painter = ui.painter();
    painter.rect(
        rect,
        v.corner_radius,
        v.weak_bg_fill,
        v.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let c = rect.center();
    let ink = theme.text;
    let tri = |p: &egui::Painter, x0: f32, x1: f32, h: f32| {
        p.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(x0, c.y - h),
                Pos2::new(x1, c.y),
                Pos2::new(x0, c.y + h),
            ],
            ink,
            Stroke::NONE,
        ));
    };
    let bar = |p: &egui::Painter, x: f32, h: f32| {
        p.rect_filled(
            Rect::from_min_max(Pos2::new(x, c.y - h), Pos2::new(x + 2.5, c.y + h)),
            0.5,
            ink,
        );
    };
    match icon {
        Icon::Play => tri(painter, c.x - 4.0, c.x + 6.0, 6.0),
        Icon::Pause => {
            bar(painter, c.x - 4.5, 5.5);
            bar(painter, c.x + 2.0, 5.5);
        }
        Icon::StepBack => {
            bar(painter, c.x - 6.0, 5.0);
            tri(painter, c.x + 5.0, c.x - 3.0, 5.0);
        }
        Icon::StepForward => {
            tri(painter, c.x - 5.0, c.x + 3.0, 5.0);
            bar(painter, c.x + 3.5, 5.0);
        }
    }
    response
}

/// Controls row plus the scrubber track.
pub fn timeline(
    ui: &mut egui::Ui,
    clock: &mut Clock,
    times: &[f64],
    bands: Option<&Bands>,
    theme: &Theme,
    utc: &dyn Fn(f64) -> String,
) {
    ui.horizontal(|ui| {
        if icon_button(ui, Icon::StepBack, theme)
            .on_hover_text("Previous row  (←, Shift+← for 10)")
            .clicked()
        {
            clock.step_rows(times, -1);
        }
        let play = if clock.playing {
            Icon::Pause
        } else {
            Icon::Play
        };
        if icon_button(ui, play, theme)
            .on_hover_text("Play or pause  (Space)")
            .clicked()
        {
            clock.toggle();
        }
        if icon_button(ui, Icon::StepForward, theme)
            .on_hover_text("Next row  (→, Shift+→ for 10)")
            .clicked()
        {
            clock.step_rows(times, 1);
        }
        ui.add_space(8.0);
        ui.label(RichText::new("Speed").color(theme.text_muted));
        for (i, &s) in SPEEDS.iter().enumerate() {
            ui.selectable_value(
                &mut clock.speed,
                i,
                RichText::new(speed_label(s)).monospace(),
            )
            .on_hover_text("Simulated seconds per second  ([ and ] to change)");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{} UTC", utc(clock.t)))
                    .monospace()
                    .size(14.0),
            );
        });
    });
    ui.add_space(4.0);
    track(ui, clock, bands, theme, utc);
}

fn track(
    ui: &mut egui::Ui,
    clock: &mut Clock,
    bands: Option<&Bands>,
    theme: &Theme,
    utc: &dyn Fn(f64) -> String,
) {
    let (t0, t1) = clock.range();
    let span = (t1 - t0).max(1e-9);
    let w = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(w, 30.0), Sense::click_and_drag());
    let x_of = |t: f64| rect.min.x + ((t - t0) / span) as f32 * rect.width();
    let t_of = |x: f32| t0 + ((x - rect.min.x) / rect.width()).clamp(0.0, 1.0) as f64 * span;

    if let Some(p) = response.interact_pointer_pos()
        && (response.dragged() || response.clicked() || response.is_pointer_button_down_on())
    {
        clock.seek(t_of(p.x));
    }

    let painter = ui.painter_at(rect.expand(2.0));
    painter.rect_filled(rect, 4.0, theme.surface_raised);
    if let Some(b) = bands {
        for (list, fill) in [(&b.umbra, theme.umbra), (&b.penumbra, theme.penumbra)] {
            for &(a, z) in list {
                // At least one pixel wide, so ~10 s penumbrae stay visible
                // on a multi-day track.
                let (xa, xz) = (x_of(a), x_of(z).max(x_of(a) + 1.0));
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(xa, rect.min.y), Pos2::new(xz, rect.max.y)),
                    0.0,
                    fill,
                );
            }
        }
    }
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, theme.rule),
        egui::StrokeKind::Inside,
    );

    let x = x_of(clock.t);
    painter.line_segment(
        [
            Pos2::new(x, rect.min.y - 2.0),
            Pos2::new(x, rect.max.y + 2.0),
        ],
        Stroke::new(2.0, theme.accent),
    );

    // Hover readout: the time under the pointer, at once (it previews a
    // seek, so a tooltip delay would be wrong here).
    if let Some(p) = response.hover_pos()
        && !response.dragged()
    {
        let t = t_of(p.x);
        painter.line_segment(
            [Pos2::new(p.x, rect.min.y), Pos2::new(p.x, rect.max.y)],
            Stroke::new(1.0, theme.text_muted),
        );
        let state = bands.map_or("", |b| {
            if b.umbra.iter().any(|&(a, z)| t >= a && t <= z) {
                " · umbra"
            } else if b.penumbra.iter().any(|&(a, z)| t >= a && t <= z) {
                " · penumbra"
            } else {
                ""
            }
        });
        egui::Tooltip::always_open(
            ui.ctx().clone(),
            ui.layer_id(),
            egui::Id::unique("track-hover"),
            egui::PopupAnchor::Pointer,
        )
        .gap(12.0)
        .show(|ui| {
            ui.label(RichText::new(format!("{}{state}", utc(t))).monospace());
        });
    }

    // Ends and the band legend under the track.
    ui.horizontal(|ui| {
        let small = |s: String| RichText::new(s).small().monospace().color(theme.text_muted);
        ui.label(small(utc(t0)));
        for (fill, name) in [(theme.umbra, "umbra"), (theme.penumbra, "penumbra")] {
            ui.add_space(10.0);
            let (r, _) = ui.allocate_exact_size(Vec2::new(14.0, 10.0), Sense::hover());
            ui.painter().rect_filled(r, 2.0, fill);
            ui.painter().rect_stroke(
                r,
                2.0,
                Stroke::new(1.0, theme.rule),
                egui::StrokeKind::Inside,
            );
            ui.label(RichText::new(name).small().color(theme.text_muted));
        }
        if bands.is_none() {
            ui.label(
                RichText::new("(bands appear when the run finishes)")
                    .small()
                    .color(theme.text_muted),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(small(utc(t1)));
        });
    });
}
