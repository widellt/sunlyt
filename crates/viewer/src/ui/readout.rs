//! The numbers at the cursor. Monospace (tabular) figures with fixed
//! decimals and units, so values don't jitter while playing.

use eframe::egui::{self, RichText};
use sunlyt_core::power::cell::ZERO_C_K;
use sunlyt_core::{AU_M, ArrayModel};

use super::theme::Theme;
use crate::worker::{Shared, Snapshot};

fn value(ui: &mut egui::Ui, text: String) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(RichText::new(text).monospace());
    });
}

fn heading(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(text).small().color(theme.text_muted));
    ui.add_space(2.0);
}

pub fn readout(ui: &mut egui::Ui, sh: &Shared, snap: &Snapshot, rows: usize, theme: &Theme) {
    let s = &snap.sample;
    ui.label(
        RichText::new(format!("{} UTC", sh.utc(snap.t)))
            .monospace()
            .size(15.0),
    );
    let where_ = match snap.row {
        Some(i) => format!("row {} of {rows}", i + 1),
        None => {
            let k = sh.table.times().partition_point(|&x| x <= snap.t);
            format!("between rows {k} and {} (interpolated)", k + 1)
        }
    };
    ui.label(RichText::new(where_).small().color(theme.text_muted));
    if snap.provisional {
        ui.label(
            RichText::new("Fixed cell temperature until the run finishes")
                .small()
                .color(theme.text_muted),
        );
    }

    heading(ui, theme, "ARRAY");
    egui::Grid::new("array")
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            ui.label("Power");
            value(ui, format!("{:>9.2} W", s.power_w));
            ui.end_row();
            ui.label("Solar disk visible");
            value(ui, format!("{:>9.2} %", 100.0 * s.sun.visible));
            ui.end_row();
            ui.label("Irradiance");
            value(ui, format!("{:>9.1} W/m²", s.sun.irradiance_w_m2));
            ui.end_row();
            ui.label("Sun distance");
            value(ui, format!("{:>9.5} AU", s.sun.sun_dist_m / AU_M));
            ui.end_row();
            for (k, name) in sh.body_names.iter().enumerate() {
                if let Some(b) = snap.epoch.bodies.get(k) {
                    ui.label(format!("{name} altitude"));
                    value(ui, format!("{:>9.1} km", b.altitude_m() / 1000.0));
                    ui.end_row();
                }
                if let Some(&f) = s.sun.occulted.get(k)
                    && f > 0.0
                {
                    ui.label(format!("Hidden by {name}"));
                    value(ui, format!("{:>9.2} %", 100.0 * f));
                    ui.end_row();
                }
            }
        });

    heading(ui, theme, "PANELS");
    let temps = !s.panel_temperature_k.is_empty();
    egui::Grid::new("panels")
        .num_columns(if temps { 4 } else { 3 })
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("");
            value(ui, "P".into());
            value(ui, "lit".into());
            if temps {
                value(ui, "T".into());
            }
            ui.end_row();
            for (k, p) in sh.scene.panels.iter().enumerate() {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 2.0, theme.series(k));
                    ui.label(&p.name);
                });
                value(ui, format!("{:>7.2} W", s.panel_power_w[k]));
                value(ui, format!("{:>5.1} %", 100.0 * s.panel_sunlyt[k]));
                if temps {
                    value(
                        ui,
                        format!("{:>6.1} °C", s.panel_temperature_k[k] - ZERO_C_K),
                    );
                }
                ui.end_row();
            }
        });

    if let ArrayModel::Circuit(_) = sh.model
        && !s.string_current_a.is_empty()
    {
        heading(ui, theme, "STRINGS");
        egui::Grid::new("strings")
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                for (k, p) in sh.scene.panels.iter().enumerate() {
                    let i = &s.string_current_a[p.strings.clone()];
                    if i.is_empty() {
                        continue;
                    }
                    let (lo, hi) = i
                        .iter()
                        .fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
                    let total: f64 = i.iter().sum();
                    ui.horizontal(|ui| {
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, 2.0, theme.series(k));
                        ui.label(format!("{} ({} strings)", p.name, i.len()));
                    });
                    value(ui, format!("{total:>7.3} A"));
                    ui.end_row();
                    ui.label(RichText::new("  min … max").color(theme.text_muted));
                    value(ui, format!("{lo:.3} … {hi:.3} A"));
                    ui.end_row();
                }
            });
    }
}
