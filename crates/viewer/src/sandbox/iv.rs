//! Per-string I-V curves with the bus line and the operating points (the
//! I-V plot deferred from slice 6).
//!
//! Strings under identical light share one curve, labelled with how many
//! strings it stands for. Each curve takes the colour of its first string,
//! the same colour as that string's wiring line in the panel view.

use eframe::egui::{self, RichText};
use egui_plot::{Legend, Line, Plot, PlotPoints, Points, VLine};
use sunlyt_core::{ArrayModel, Regulation};

use super::model::{Evaluation, StringCurve};
use crate::ui::theme::Theme;

fn label(c: &StringCurve) -> String {
    let ids: Vec<String> = c.strings.iter().map(|s| (s + 1).to_string()).collect();
    match ids.len() {
        1 => format!("String {}", ids[0]),
        2 | 3 => format!("Strings {}", ids.join(", ")),
        n => format!("{n} strings ({}, {}, …)", ids[0], ids[1]),
    }
}

/// Draws the plot; returns the first string of the curve under the
/// pointer, if any.
pub fn iv_plot(
    ui: &mut egui::Ui,
    model: &ArrayModel,
    ev: &Evaluation,
    hover: Option<usize>,
    theme: &Theme,
) -> Option<usize> {
    let ArrayModel::Circuit(circuit) = model else {
        ui.label(
            RichText::new("No [electrical] table: constant 30 % efficiency, no I-V curves.")
                .color(theme.text_muted),
        );
        return None;
    };
    let bus = match circuit.regulation {
        Regulation::FixedBus { bus_v } => Some(bus_v + circuit.blocking_vf_v),
        Regulation::Mppt => None,
    };
    let v_max = ev
        .curves
        .iter()
        .map(|c| c.points[0].1)
        .fold(bus.unwrap_or(1.0), f64::max);
    let i_max = ev
        .curves
        .iter()
        .flat_map(|c| c.points.iter().map(|p| p.0))
        .fold(1e-3, f64::max);
    let hovered_curve = hover.and_then(|s| ev.curves.iter().position(|c| c.strings.contains(&s)));

    let resp = Plot::new("sandbox-iv")
        .legend(Legend::default().position(egui_plot::Corner::LeftBottom))
        .x_axis_label("String voltage (V)")
        .y_axis_label("Current (A)")
        .y_axis_min_width(56.0)
        .include_x(0.0)
        .include_x(v_max * 1.04)
        .include_y(0.0)
        .include_y(i_max * 1.08)
        .allow_scroll(false)
        .show(ui, |pui| {
            if let Some(v) = bus {
                pui.add(
                    VLine::new(format!("Bus + blocking diode {v:.1} V"), v)
                        .color(theme.text_muted)
                        .width(1.0)
                        .style(egui_plot::LineStyle::dashed_loose()),
                );
            }
            for (k, c) in ev.curves.iter().enumerate() {
                let colour = theme.series(c.strings[0]);
                let on = hovered_curve == Some(k);
                // Only the forward quadrant: below 0 V the bypass diodes
                // clamp every cell and the curve is a vertical wall.
                let pts: PlotPoints = c
                    .points
                    .iter()
                    .filter(|p| p.1 >= 0.0)
                    .map(|&(i, v)| [v, i])
                    .collect();
                pui.line(
                    Line::new(label(c), pts)
                        .color(colour)
                        .width(if on { 3.0 } else { 1.75 }),
                );
                pui.points(
                    Points::new("", vec![[c.op.voltage_v, c.op.current_a]])
                        .radius(if on { 6.0 } else { 4.5 })
                        .color(colour)
                        .allow_hover(false),
                );
            }
            pui.pointer_coordinate().and_then(|p| {
                if !pui.response().hovered() {
                    return None;
                }
                // The curve nearest the pointer in current at its voltage.
                ev.curves
                    .iter()
                    .filter_map(|c| {
                        let w = c
                            .points
                            .windows(2)
                            .find(|w| w[0].1 >= p.x && w[1].1 <= p.x)?;
                        let t = (w[0].1 - p.x) / (w[0].1 - w[1].1).max(1e-300);
                        let i = w[0].0 + t * (w[1].0 - w[0].0);
                        Some(((i - p.y).abs(), c.strings[0]))
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .filter(|(d, _)| *d < 0.06 * i_max)
                    .map(|(_, s)| s)
            })
        });
    resp.inner
}

/// The operating points under the plot: one row per distinct curve.
pub fn op_table(ui: &mut egui::Ui, ev: &Evaluation, theme: &Theme) {
    egui::Grid::new("sandbox-ops")
        .num_columns(4)
        .spacing([18.0, 2.0])
        .show(ui, |ui| {
            for h in ["Curve", "Current", "Voltage", "Power each"] {
                ui.label(RichText::new(h).small().color(theme.text_muted));
            }
            ui.end_row();
            for c in &ev.curves {
                ui.label(RichText::new(label(c)).color(theme.series(c.strings[0])));
                ui.label(RichText::new(format!("{:7.4} A", c.op.current_a)).monospace());
                ui.label(RichText::new(format!("{:7.3} V", c.op.voltage_v)).monospace());
                ui.label(RichText::new(format!("{:8.3} W", c.op.power_w)).monospace());
                ui.end_row();
            }
        });
}
