//! Colour tokens, defined once per mode and referenced by role. Widgets
//! never hard-code colours (viewer-design skill).
//!
//! The categorical `series` slots are the dataviz skill's validated
//! reference palette (fixed order; adjacent-pair CVD ΔE ≥ 8.4 in both
//! modes). Colour follows the entity: panel `k` always takes slot `k`.

use eframe::egui::{self, Color32};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub dark: bool,
    /// Window and panel background.
    pub surface: Color32,
    /// Raised surfaces: legend chips, overlays on the 3D view.
    pub surface_raised: Color32,
    /// 3D viewport clear colour.
    pub viewport: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    /// Hairlines, plot grid, timeline track.
    pub rule: Color32,
    /// Timeline and plot bands. Shades of the text colour, not hues, so
    /// they read without colour vision; they are also labelled.
    pub umbra: Color32,
    pub penumbra: Color32,
    /// Spacecraft structure (occluders) in the 3D view.
    pub structure: Color32,
    pub sun: Color32,
    pub earth: Color32,
    pub moon: Color32,
    /// Body axes X, Y, Z.
    pub axes: [Color32; 3],
    pub series: [Color32; 8],
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            dark: true,
            surface: hex(0x1a1a19),
            surface_raised: hex(0x242423),
            viewport: hex(0x121211),
            text: hex(0xf0efec),
            text_muted: hex(0xa3a29a),
            accent: hex(0x3987e5),
            rule: hex(0x34342f),
            umbra: Color32::from_rgba_unmultiplied(240, 239, 236, 40),
            penumbra: Color32::from_rgba_unmultiplied(240, 239, 236, 18),
            structure: hex(0x8d8c86),
            sun: hex(0xf2c14e),
            earth: hex(0x3987e5),
            moon: hex(0xc3c2b7),
            axes: [hex(0xe66767), hex(0x199e70), hex(0x3987e5)],
            series: [
                hex(0x3987e5),
                hex(0xd95926),
                hex(0x199e70),
                hex(0xc98500),
                hex(0xd55181),
                hex(0x008300),
                hex(0x9085e9),
                hex(0xe66767),
            ],
        }
    }

    pub fn light() -> Self {
        Self {
            dark: false,
            surface: hex(0xfcfcfb),
            surface_raised: hex(0xf1f0ec),
            viewport: hex(0xe9e8e3),
            text: hex(0x0b0b0b),
            text_muted: hex(0x52514e),
            accent: hex(0x2a78d6),
            rule: hex(0xd9d8d2),
            umbra: Color32::from_rgba_unmultiplied(11, 11, 11, 46),
            penumbra: Color32::from_rgba_unmultiplied(11, 11, 11, 20),
            structure: hex(0x9a9993),
            sun: hex(0xc98500),
            earth: hex(0x2a78d6),
            moon: hex(0x6b6a64),
            axes: [hex(0xe34948), hex(0x1baf7a), hex(0x2a78d6)],
            series: [
                hex(0x2a78d6),
                hex(0xeb6834),
                hex(0x1baf7a),
                hex(0xeda100),
                hex(0xe87ba4),
                hex(0x008300),
                hex(0x6250d6),
                hex(0xe34948),
            ],
        }
    }

    /// The theme matching egui's current light/dark mode (which follows
    /// the OS by default).
    pub fn current(ctx: &egui::Context) -> Self {
        match ctx.theme() {
            egui::Theme::Dark => Self::dark(),
            egui::Theme::Light => Self::light(),
        }
    }

    /// Series colour of entity `k` (panel index). Past eight entities the
    /// palette repeats, which the legend disambiguates; boxwing has two.
    pub fn series(&self, k: usize) -> Color32 {
        self.series[k % self.series.len()]
    }

    /// Pushes the tokens into egui's visuals for both modes, plus timing
    /// that the design rules fix (tooltip delay).
    pub fn install(ctx: &egui::Context) {
        for theme in [Self::dark(), Self::light()] {
            let mut v = if theme.dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            };
            v.panel_fill = theme.surface;
            v.window_fill = theme.surface_raised;
            v.extreme_bg_color = theme.viewport;
            v.faint_bg_color = theme.surface_raised;
            v.override_text_color = Some(theme.text);
            v.selection.bg_fill = theme.accent;
            v.hyperlink_color = theme.accent;
            v.widgets.noninteractive.bg_stroke.color = theme.rule;
            let mode = if theme.dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            };
            ctx.set_visuals_of(mode, v);
        }
        ctx.all_styles_mut(|s| {
            s.interaction.tooltip_delay = super::motion::TOOLTIP_DELAY_S;
            // Once one tooltip is open, neighbours appear at once.
            s.interaction.tooltip_grace_time = 0.3;
            s.interaction.show_tooltips_only_when_still = false;
            s.animation_time = super::motion::PANEL_S;
        });
    }
}
