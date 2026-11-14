//! How the columns of a trajectory CSV map onto a [`super::StateTable`].

use serde::{Deserialize, Serialize};

/// Describes how to read a trajectory CSV. Usually loaded from TOML; see
/// `presets/mappings/` for examples.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvMapping {
    pub time: TimeColumn,
    /// Metres per unit of every vector column (1000 for km).
    #[serde(default = "default_length_unit")]
    pub length_unit_m: f64,
    /// Spacecraft-to-Sun vector, body frame.
    pub sun_body: [String; 3],
    /// Bodies that can occult the Sun or radiate onto the panels.
    #[serde(default)]
    pub bodies: Vec<BodyColumns>,
    /// Attitude quaternion. Required: body-frame vectors alone cannot tell
    /// how the spacecraft rotates between rows, so a file without attitude
    /// is rejected rather than interpolated by guesswork.
    pub attitude: AttitudeColumns,
    /// Inertial spacecraft position relative to one of `bodies`. Only used to
    /// cross-check the attitude convention.
    #[serde(default)]
    pub position: Option<PositionColumns>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeColumn {
    pub column: String,
    /// `chrono` strftime format, e.g. `%m/%d/%Y %H:%M`. Times are taken as UTC.
    pub format: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyColumns {
    pub name: String,
    /// Equatorial radius.
    pub radius_km: f64,
    /// Polar radius for an oblate spheroid; omit for a sphere.
    #[serde(default)]
    pub polar_radius_km: Option<f64>,
    /// Spin (polar) axis in the attitude quaternion's inertial frame. The
    /// default +Z is Earth's mean pole of J2000.0; precession moves the true
    /// pole only ~0.15° by 2027, which is negligible for occultation.
    #[serde(default = "default_pole")]
    pub pole_inertial: [f64; 3],
    /// Spacecraft-to-body vector, body frame.
    pub columns: [String; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttitudeColumns {
    /// Column names in file order (vector part first for `scalar_last`).
    pub columns: [String; 4],
    pub order: QuatOrder,
    pub direction: QuatDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuatOrder {
    ScalarLast,
    ScalarFirst,
}

/// What the quaternion means. `InertialToBody` (STK's convention) satisfies
/// `v_body = R(q)ᵀ · v_inertial`, with R(q) the usual active rotation matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuatDirection {
    InertialToBody,
    BodyToInertial,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionColumns {
    pub columns: [String; 3],
    /// Name of the body (from `bodies`) the position is measured from.
    pub center: String,
}

fn default_length_unit() -> f64 {
    1000.0
}

fn default_pole() -> [f64; 3] {
    [0.0, 0.0, 1.0]
}

const STK_BODY_VECTORS: &str = include_str!("../../presets/mappings/stk-body-vectors.toml");

impl CsvMapping {
    /// STK report with Earth-J2000 position, J2000→body quaternion and
    /// body-frame Sun/Earth/Moon vectors in km.
    pub fn stk_body_vectors() -> Self {
        toml::from_str(STK_BODY_VECTORS).expect("built-in preset must parse")
    }

    /// Looks up a built-in preset by name.
    pub fn preset(name: &str) -> Option<Self> {
        match name {
            "stk-body-vectors" => Some(Self::stk_body_vectors()),
            _ => None,
        }
    }

    pub const PRESETS: &[&str] = &["stk-body-vectors"];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stk_preset_parses() {
        let m = CsvMapping::stk_body_vectors();
        assert_eq!(m.bodies.len(), 2);
        assert_eq!(m.attitude.direction, QuatDirection::InertialToBody);
        assert_eq!(m.position.unwrap().center, "Earth");
        assert_eq!(m.bodies[0].polar_radius_km, Some(6356.752314245));
        assert_eq!(m.bodies[1].polar_radius_km, None);
    }

    #[test]
    fn mapping_without_attitude_is_rejected() {
        let text: String = STK_BODY_VECTORS
            .lines()
            .filter(|l| {
                !(l.starts_with("[attitude]")
                    || l.contains("J2000-q")
                    || l.starts_with("order")
                    || l.starts_with("direction"))
            })
            .map(|l| format!("{l}\n"))
            .collect();
        let err = toml::from_str::<CsvMapping>(&text).unwrap_err();
        assert!(err.to_string().contains("attitude"), "{err}");
    }
}
