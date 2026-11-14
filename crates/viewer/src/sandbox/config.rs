//! `sandbox.toml`: one flat panel, the Sun, box occluders and optional
//! eclipsing bodies (slice 8, S3).
//!
//! The panel keys are a scene's `[[panel]]` keys, and `[electrical]` is a
//! scene's `[electrical]`, so what works here carries straight over to a
//! scene file. The sandbox frame is the panel's own: x along the panel's
//! u axis (columns), y along v (rows), z out of the cell side.
//!
//! ```toml
//! samples = 4                 # n×n shadow rays per cell
//! size = [0.6, 0.4]           # substrate (u, v), metres; centred on the origin
//!
//! [panel]                     # [[panel]] keys, minus object / normal / u_axis
//! rows = 4
//! cols = 6
//! cell = [0.08, 0.08]
//! gap = [0.004, 0.004]
//! strings = { series = 6 }
//!
//! [sun]
//! azimuth_deg = 30            # from +u towards +v
//! elevation_deg = 50          # above the panel
//! # vector = [0.3, 0.2, 0.9]  # instead of azimuth/elevation (normalised)
//! distance_au = 1.0
//!
//! [[box]]                     # occluder; any number
//! centre = [0.1, 0.05, 0.25]  # (u, v, height), metres
//! size = [0.15, 0.1, 0.1]
//! rotation_deg = 20           # about the panel normal
//!
//! [[body]]                    # optional eclipsing sphere, e.g. the Moon
//! name = "Moon"
//! direction = [0.0, 0.0, 1.0] # from the panel, sandbox frame
//! distance_km = 384400
//! radius_km = 1737.4
//!
//! [electrical]                # as in a scene; omit for 30 % constant efficiency
//! cell = "solaero-imm-alpha"
//! ```

use serde::Deserialize;
use sunlyt_core::geometry::PanelLayout;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SandboxConfig {
    #[serde(default = "two")]
    pub samples: u32,
    /// Substrate size along (u, v), metres.
    pub size: [f64; 2],
    /// Filled in from the `[panel]` table by [`SandboxConfig::from_str`].
    #[serde(skip)]
    pub panel: Option<PanelLayout>,
    #[serde(default)]
    pub sun: SunConfig,
    #[serde(default, rename = "box")]
    pub boxes: Vec<BoxConfig>,
    #[serde(default, rename = "body")]
    pub bodies: Vec<BodyConfig>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SunConfig {
    #[serde(default)]
    pub azimuth_deg: f64,
    #[serde(default = "ninety")]
    pub elevation_deg: f64,
    #[serde(default)]
    pub vector: Option<[f64; 3]>,
    #[serde(default = "one")]
    pub distance_au: f64,
}

impl Default for SunConfig {
    fn default() -> Self {
        Self {
            azimuth_deg: 0.0,
            elevation_deg: 90.0,
            vector: None,
            distance_au: 1.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BoxConfig {
    /// Centre (u, v, height above the panel), metres.
    pub centre: [f64; 3],
    pub size: [f64; 3],
    #[serde(default)]
    pub rotation_deg: f64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BodyConfig {
    pub name: String,
    /// Direction from the panel to the body centre, sandbox frame.
    pub direction: [f64; 3],
    pub distance_km: f64,
    pub radius_km: f64,
}

/// Keys a scene panel has that make no sense on the sandbox's own panel.
const SCENE_ONLY: [&str; 3] = ["object", "normal", "u_axis"];

impl SandboxConfig {
    /// Parses a sandbox TOML. `[electrical]` is left for
    /// `ElectricalConfig::from_scene_str`.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> Result<Self, String> {
        let mut table: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
        table.remove("electrical");
        let Some(toml::Value::Table(mut panel)) = table.remove("panel") else {
            return Err("missing [panel] table".into());
        };
        for k in SCENE_ONLY {
            if panel.contains_key(k) {
                return Err(format!(
                    "[panel] {k}: the sandbox panel is the plane z = 0 facing +z with \
                     columns along x; leave `{k}` out"
                ));
            }
        }
        panel.insert("object".into(), PANEL_OBJECT.into());
        panel.insert(
            "normal".into(),
            toml::Value::try_from([0.0, 0.0, 1.0]).unwrap(),
        );
        panel.insert(
            "u_axis".into(),
            toml::Value::try_from([1.0, 0.0, 0.0]).unwrap(),
        );
        let panel: PanelLayout = toml::Value::Table(panel)
            .try_into()
            .map_err(|e: toml::de::Error| format!("[panel]: {e}"))?;
        let mut cfg: SandboxConfig = toml::Value::Table(table)
            .try_into()
            .map_err(|e: toml::de::Error| e.to_string())?;
        if !(cfg.size[0] > 0.0 && cfg.size[1] > 0.0) {
            return Err("size must be positive".into());
        }
        for (i, b) in cfg.boxes.iter().enumerate() {
            if b.size.iter().any(|&s| s <= 0.0) {
                return Err(format!("box {i}: size must be positive"));
            }
        }
        for b in &cfg.bodies {
            if !(b.radius_km > 0.0 && b.distance_km > b.radius_km) {
                return Err(format!(
                    "body {:?}: need 0 < radius_km < distance_km",
                    b.name
                ));
            }
        }
        if cfg.sun.distance_au <= 0.0 {
            return Err("sun.distance_au must be positive".into());
        }
        cfg.panel = Some(panel);
        Ok(cfg)
    }
}

/// The OBJ object name of the sandbox's own substrate.
pub const PANEL_OBJECT: &str = "SANDBOX_PANEL";

/// `text` with `[sun]` set to the given azimuth and elevation (any
/// `vector` removed). Other tables keep their values; TOML comments are
/// not preserved.
pub fn with_sun(text: &str, azimuth_deg: f64, elevation_deg: f64) -> Result<String, String> {
    let mut table: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    let sun = table
        .entry("sun")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let toml::Value::Table(sun) = sun else {
        return Err("[sun] is not a table".into());
    };
    sun.remove("vector");
    let round = |x: f64| (x * 1e6).round() / 1e6;
    sun.insert("azimuth_deg".into(), round(azimuth_deg).into());
    sun.insert("elevation_deg".into(), round(elevation_deg).into());
    toml::to_string(&table).map_err(|e| e.to_string())
}

fn two() -> u32 {
    2
}
fn one() -> f64 {
    1.0
}
fn ninety() -> f64 {
    90.0
}
