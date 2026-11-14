//! Stacked plots sharing the time axis (dataviz skill: one y-axis per
//! chart, never a dual axis): power, panel temperature, visible solar disk.
//! They draw the batch run's rows, the same numbers `sunlyt array` writes.

pub mod export;

use std::ops::RangeInclusive;

use eframe::egui::{self, Color32, RichText, Stroke};
use egui_plot::{GridMark, Legend, Line, Plot, PlotPoints, Points, Span, Text, VLine};
use sunlyt_core::power::cell::ZERO_C_K;
use sunlyt_core::{ArrayRun, Scene, StateTable};

use crate::timeline::Bands;
use crate::ui::theme::Theme;

/// One drawn series.
pub struct Series {
    pub name: String,
    /// Index into the theme's categorical palette; `None`: text ink (the
    /// total, which is not a category).
    pub slot: Option<usize>,
    pub y: Vec<f64>,
}

pub struct Chart {
    pub title: &'static str,
    pub unit: &'static str,
    pub series: Vec<Series>,
}

/// Plot data, built once when the run arrives.
pub struct PlotData {
    pub t: Vec<f64>,
    pub charts: Vec<Chart>,
}

impl PlotData {
    pub fn new(scene: &Scene, run: &ArrayRun) -> Self {
        let s = &run.samples;
        let mut power = vec![Series {
            name: "Total".into(),
            slot: None,
            y: s.iter().map(|x| x.power_w).collect(),
        }];
        for (k, p) in scene.panels.iter().enumerate() {
            power.push(Series {
                name: p.name.clone(),
                slot: Some(k),
                y: s.iter().map(|x| x.panel_power_w[k]).collect(),
            });
        }
        let mut charts = vec![Chart {
            title: "Power",
            unit: "W",
            series: power,
        }];
        if s.first().is_some_and(|x| !x.panel_temperature_k.is_empty()) {
            charts.push(Chart {
                title: "Temp.",
                unit: "°C",
                series: scene
                    .panels
                    .iter()
                    .enumerate()
                    .map(|(k, p)| Series {
                        name: p.name.clone(),
                        slot: Some(k),
                        y: s.iter()
                            .map(|x| x.panel_temperature_k[k] - ZERO_C_K)
                            .collect(),
                    })
                    .collect(),
            });
        }
        charts.push(Chart {
            title: "Visible",
            unit: "%",
            series: vec![Series {
                name: "Visible".into(),
                slot: None,
                y: s.iter().map(|x| 100.0 * x.sun.visible).collect(),
            }],
        });
        Self {
            t: run.t.clone(),
            charts,
        }
    }

    fn nearest_row(&self, t: f64) -> usize {
        let k = self.t.partition_point(|&x| x < t);
        match (k.checked_sub(1), self.t.get(k)) {
            (Some(i), Some(&b)) if t - self.t[i] < b - t => i,
            (Some(i), None) => i,
            _ => k.min(self.t.len().saturating_sub(1)),
        }
    }
}

/// What the user did in the plots this frame.
#[derive(Default)]
pub struct PlotEvents {
    /// Clicked at this time: seek there.
    pub seek: Option<f64>,
}

fn utc_axis(table: &StateTable) -> impl Fn(GridMark, &RangeInclusive<f64>) -> String + '_ {
    move |m, range| {
        let span = range.end() - range.start();
        // Seconds only once the view is zoomed in to minutes.
        let fmt = if span > 600.0 {
            "%m-%d %H:%M"
        } else {
            "%H:%M:%S"
        };
        table.datetime(m.value).format(fmt).to_string()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn plots(
    ui: &mut egui::Ui,
    data: &PlotData,
    bands: &Bands,
    table: &StateTable,
    cursor_t: f64,
    theme: &Theme,
) -> PlotEvents {
    let mut ev = PlotEvents::default();
    let n = data.charts.len();
    let gap = 6.0;
    let h = ((ui.available_height() - gap * (n as f32 - 1.0)) / n as f32).max(70.0);
    let link = egui::Id::unique("sunlyt-time");
    let (t0, t1) = (
        data.t.first().copied().unwrap_or(0.0),
        data.t.last().copied().unwrap_or(1.0),
    );
    let mut hovered_row = None;
    let mut hovered_chart = None;

    for (ci, chart) in data.charts.iter().enumerate() {
        let last = ci + 1 == n;
        let colour = |s: &Series| s.slot.map_or(theme.text, |k| theme.series(k));
        let mut plot = Plot::new(("sunlyt-plot", ci))
            .height(h)
            .link_axis(link, [true, false])
            .link_cursor(link, [true, false])
            .default_x_bounds(t0, t1)
            .auto_bounds([false, true])
            .allow_zoom([true, false])
            .allow_scroll([true, false])
            .allow_drag([true, false])
            .allow_boxed_zoom(false)
            .show_x(false)
            .show_y(false)
            .show_axes([last, true])
            .y_axis_min_width(56.0)
            .y_axis_label(format!("{} ({})", chart.title, chart.unit))
            .x_axis_formatter(utc_axis(table));
        if chart.title == "Visible" {
            plot = plot.include_y(0.0).include_y(100.0);
        }
        if chart.series.len() >= 2 {
            plot = plot.legend(Legend::default().position(egui_plot::Corner::LeftTop));
        }
        let resp = plot.show(ui, |pui| {
            for &(a, z) in &bands.umbra {
                pui.add(Span::new("", a..=z).fill(theme.umbra).border_width(0.0));
            }
            for &(a, z) in &bands.penumbra {
                pui.add(Span::new("", a..=z).fill(theme.penumbra).border_width(0.0));
            }
            for s in &chart.series {
                let pts: PlotPoints = data.t.iter().zip(&s.y).map(|(&t, &y)| [t, y]).collect();
                pui.line(
                    Line::new(s.name.clone(), pts)
                        .color(colour(s))
                        .width(if s.slot.is_none() { 2.0 } else { 1.5 })
                        .allow_hover(false),
                );
            }
            // Direct labels (≤ 4 series) at the right edge of the view.
            let b = pui.plot_bounds();
            if (2..=4).contains(&chart.series.len()) {
                let tx = b.max()[0] - 0.01 * b.width();
                let i = data.nearest_row(tx);
                for s in &chart.series {
                    pui.add(
                        Text::new(
                            "",
                            [data.t[i], s.y[i]].into(),
                            RichText::new(&s.name).small().color(theme.text_muted),
                        )
                        .anchor(egui::Align2::RIGHT_BOTTOM),
                    );
                }
            }
            pui.add(
                VLine::new("", cursor_t)
                    .color(theme.accent)
                    .width(1.5)
                    .allow_hover(false),
            );
            let hover = pui
                .response()
                .hovered()
                .then(|| pui.pointer_coordinate())
                .flatten();
            let row = hover.map(|p| data.nearest_row(p.x));
            if let Some(i) = row {
                for s in &chart.series {
                    pui.points(
                        Points::new("", vec![[data.t[i], s.y[i]]])
                            .radius(4.0)
                            .color(colour(s))
                            .allow_hover(false),
                    );
                }
            }
            row
        });
        if let Some(i) = resp.inner {
            hovered_row = Some(i);
            hovered_chart = Some(ci);
        }
        if resp.response.clicked()
            && let Some(p) = resp.response.interact_pointer_pos()
        {
            let t = resp.transform.value_from_position(p).x;
            ev.seek = Some(t.clamp(t0, t1));
        }
        if !last {
            ui.add_space(gap);
        }
    }

    // Snapping tooltip: UTC and every value of the hovered chart, at once.
    if let (Some(i), Some(ci)) = (hovered_row, hovered_chart) {
        let chart = &data.charts[ci];
        egui::Tooltip::always_open(
            ui.ctx().clone(),
            ui.layer_id(),
            egui::Id::unique("plot-hover"),
            egui::PopupAnchor::Pointer,
        )
        .gap(14.0)
        .show(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} UTC",
                    table.datetime(data.t[i]).format("%Y-%m-%d %H:%M:%S")
                ))
                .monospace(),
            );
            egui::Grid::new("plot-tip").num_columns(2).show(ui, |ui| {
                for s in &chart.series {
                    ui.horizontal(|ui| {
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(10.0, 2.0), egui::Sense::hover());
                        let c = s.slot.map_or(theme.text, |k| theme.series(k));
                        ui.painter().rect_filled(r, 1.0, c);
                        ui.label(&s.name);
                    });
                    ui.label(RichText::new(format!("{:>9.2} {}", s.y[i], chart.unit)).monospace());
                    ui.end_row();
                }
            });
        });
    }
    let _ = (Color32::TRANSPARENT, Stroke::NONE);
    ev
}
