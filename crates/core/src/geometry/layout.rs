//! Scene configuration (TOML): which OBJ to load and how cells are laid
//! out on each panel (design D3b).
//!
//! ```toml
//! obj = "craft.obj"          # relative to this file
//! scale = 1.0                # OBJ units → metres
//! samples = 2                # n×n sample points per cell
//!
//! [[panel]]
//! name = "+Y wing"
//! object = "WING_PY"         # OBJ `o`/`g` name of the substrate
//! normal = [0, 1, 0]         # cell side; needed when the object is a slab
//! rows = 10                  # along v
//! cols = 20                  # along u
//! cell = [0.08, 0.04]        # cell size (u, v), metres
//! gap = [0.002, 0.002]       # spacing between cells (u, v), metres
//! u_axis = [1, 0, 0]         # optional; projected onto the panel plane
//! offset = [0, 0]            # optional; grid centre from face centroid (u, v)
//! strings = { series = 20 }  # optional wiring; default: one string per row
//! ```
//!
//! An `[electrical]` table may sit in the same file; the power model reads
//! it (see `power::circuit::ElectricalConfig`), and the scene ignores it;
//! likewise `[thermal]` (`thermal::ThermalConfig`).

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SceneConfig {
    /// OBJ path, relative to the config file.
    pub obj: String,
    /// OBJ units → metres.
    #[serde(default = "one")]
    pub scale: f64,
    /// Sample points per cell side (n×n per cell).
    #[serde(default = "two")]
    pub samples: u32,
    #[serde(rename = "panel")]
    pub panels: Vec<PanelLayout>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PanelLayout {
    /// Display name; defaults to the object name.
    #[serde(default)]
    pub name: Option<String>,
    /// OBJ object or group holding the substrate.
    pub object: String,
    /// Outward normal of the cell side, body frame. When given, only the
    /// object's triangles facing this way form the cell face (so a slab
    /// with a back side works); otherwise the whole object must be planar
    /// and its winding gives the normal.
    #[serde(default)]
    pub normal: Option<[f64; 3]>,
    pub rows: u32,
    pub cols: u32,
    /// Cell size along (u, v), metres.
    pub cell: [f64; 2],
    /// Spacing between neighbouring cells along (u, v), metres.
    #[serde(default)]
    pub gap: [f64; 2],
    /// In-plane direction of the columns; default: body X projected onto
    /// the plane (body Y if the panel faces ±X).
    #[serde(default)]
    pub u_axis: Option<[f64; 3]>,
    /// Grid centre relative to the face centroid along (u, v), metres.
    #[serde(default)]
    pub offset: [f64; 2],
    /// How the cells are wired into series strings. Default: each row is
    /// one string.
    #[serde(default)]
    pub strings: Option<StringLayout>,
}

/// Series-string wiring of a panel's cells.
///
/// ```toml
/// strings = { series = 12, order = "rows" }   # walk rows back and forth,
///                                             # 12 cells per string
/// strings = [[[0, 0], [0, 1]], [[1, 1], [1, 0]]]  # explicit [row, col] lists
/// ```
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum StringLayout {
    /// Walk the grid serpentine-fashion (row 0 left to right, row 1 right
    /// to left, …) and cut it into strings of `series` cells.
    Chunked {
        series: u32,
        #[serde(default)]
        order: WalkOrder,
    },
    /// Every cell, as `[row, col]`, in exactly one string.
    Explicit(Vec<Vec<[u32; 2]>>),
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WalkOrder {
    #[default]
    Rows,
    Columns,
}

fn one() -> f64 {
    1.0
}

fn two() -> u32 {
    2
}
