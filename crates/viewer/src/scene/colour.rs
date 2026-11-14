//! Cell colour maps: perceptually uniform, each with a legend and units.
//! The colour is a direct function of the instant's values, never eased.

use eframe::egui::Color32;
use sunlyt_core::Scene;

use crate::worker::Snapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColourMode {
    /// lit fraction × cos θ × visible solar disk, 0–1 (viridis).
    Illumination,
    /// Current of the cell's string, A (cividis). Circuit model only.
    StringCurrent,
}

impl ColourMode {
    pub fn label(self) -> &'static str {
        match self {
            ColourMode::Illumination => "Illumination",
            ColourMode::StringCurrent => "String current",
        }
    }

    pub fn legend_title(self) -> &'static str {
        match self {
            ColourMode::Illumination => "Effective illumination (lit × cos θ × visible disk)",
            ColourMode::StringCurrent => "String current",
        }
    }

    pub fn map(self, x: f64) -> Color32 {
        match self {
            ColourMode::Illumination => lut(&VIRIDIS, x),
            ColourMode::StringCurrent => lut(&CIVIDIS, x),
        }
    }
}

/// matplotlib viridis at 9 even stops.
const VIRIDIS: [u32; 9] = [
    0x440154, 0x472c7a, 0x3b518b, 0x2c718e, 0x21908d, 0x27ad81, 0x5cc863, 0xaadc32, 0xfde725,
];
/// cividis (Nuñez et al. 2018) at 10 even stops.
const CIVIDIS: [u32; 10] = [
    0x00204d, 0x00336f, 0x39486b, 0x575d6d, 0x707173, 0x8a8779, 0xa69d75, 0xc4b56c, 0xe4cf5b,
    0xffea46,
];

fn lut(stops: &[u32], x: f64) -> Color32 {
    let x = if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let f = x * (stops.len() - 1) as f64;
    let i = (f as usize).min(stops.len() - 2);
    let u = f - i as f64;
    let ch = |c: u32, s: u32| ((c >> s) & 0xff) as f64;
    let mix = |s| (ch(stops[i], s) * (1.0 - u) + ch(stops[i + 1], s) * u).round() as u8;
    Color32::from_rgb(mix(16), mix(8), mix(0))
}

/// Per-cell colours for one instant. `current_max_a` is the colour-scale
/// top for string current (fixed for the whole run so colours compare
/// across time).
pub fn cell_colours(
    scene: &Scene,
    cell_string: &[Option<u32>],
    snap: &Snapshot,
    mode: ColourMode,
    current_max_a: f64,
) -> Vec<Color32> {
    let s = &snap.sample;
    let sun = s.sun.sun_dir_b;
    scene
        .cells
        .iter()
        .enumerate()
        .map(|(c, cell)| {
            let x = match mode {
                ColourMode::Illumination => {
                    snap.lit[c] * cell.normal.dot(sun).max(0.0) * s.sun.visible
                }
                ColourMode::StringCurrent => match cell_string[c] {
                    Some(k) if current_max_a > 0.0 => {
                        s.string_current_a.get(k as usize).copied().unwrap_or(0.0) / current_max_a
                    }
                    _ => 0.0,
                },
            };
            mode.map(x)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lut_ends_and_midpoint() {
        assert_eq!(lut(&VIRIDIS, 0.0), Color32::from_rgb(0x44, 0x01, 0x54));
        assert_eq!(lut(&VIRIDIS, 1.0), Color32::from_rgb(0xfd, 0xe7, 0x25));
        assert_eq!(lut(&VIRIDIS, 0.5), Color32::from_rgb(0x21, 0x90, 0x8d));
        assert_eq!(lut(&CIVIDIS, f64::NAN), Color32::from_rgb(0x00, 0x20, 0x4d));
    }
}
