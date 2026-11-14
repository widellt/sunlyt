//! Scene = mesh + panels + cells, built from a [`SceneConfig`].

use std::ops::Range;
use std::path::Path;

use glam::{DVec2, DVec3};

use super::GeometryError;
use super::layout::{PanelLayout, SceneConfig, StringLayout, WalkOrder};
use super::mesh::Mesh;

/// A panel substrate carrying a grid of cells.
#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    pub name: String,
    /// OBJ object index.
    pub object: usize,
    /// Outward unit normal of the cell side, body frame.
    pub normal: DVec3,
    /// Area-weighted centroid of the cell face.
    pub centroid: DVec3,
    /// In-plane unit axes, `u × v = normal`. Columns run along u.
    pub u: DVec3,
    pub v: DVec3,
    pub rows: u32,
    pub cols: u32,
    /// This panel's cells in [`Scene::cells`].
    pub cells: Range<usize>,
    /// This panel's strings in [`Scene::strings`].
    pub strings: Range<usize>,
    /// Area of the cell face (the whole substrate side, not just cells), m².
    pub area_m2: f64,
}

/// Cells wired in series, in wiring order. Every cell is in exactly one
/// string, and a string stays on one panel.
#[derive(Debug, Clone, PartialEq)]
pub struct SeriesString {
    pub panel: u32,
    /// Indices into [`Scene::cells`].
    pub cells: Vec<usize>,
}

/// One solar cell: a flat rectangle on its panel.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub panel: u32,
    pub row: u32,
    pub col: u32,
    pub centre: DVec3,
    pub normal: DVec3,
    pub area_m2: f64,
    /// Side lengths along the panel's `u` and `v`, m (for drawing).
    pub size_m: DVec2,
}

#[derive(Debug, Clone)]
pub struct Scene {
    pub mesh: Mesh,
    pub panels: Vec<Panel>,
    pub cells: Vec<Cell>,
    pub strings: Vec<SeriesString>,
    /// Sample points per cell (n×n).
    pub samples_per_cell: usize,
    /// Sample points of cell `i` are `sample_points[i * k .. (i + 1) * k]`,
    /// already lifted off the surface by `ray_offset_m` along the normal.
    pub sample_points: Vec<DVec3>,
    pub ray_offset_m: f64,
}

impl Scene {
    /// Reads a scene TOML and the OBJ it names (relative to the TOML).
    pub fn load(config_path: &Path) -> Result<Self, GeometryError> {
        let (cfg, mesh) = Self::load_config(config_path)?;
        Self::build(mesh, &cfg)
    }

    /// The scene configuration and its OBJ mesh, not yet built (so a caller
    /// can change the panels first, as the sandbox does).
    pub fn load_config(config_path: &Path) -> Result<(SceneConfig, Mesh), GeometryError> {
        let text = std::fs::read_to_string(config_path).map_err(|e| GeometryError::Io {
            path: config_path.display().to_string(),
            source: e,
        })?;
        // `[electrical]` and `[thermal]` belong to the power and thermal
        // models; everything else must
        // be scene configuration.
        let mut table: toml::Table =
            toml::from_str(&text).map_err(|e| GeometryError::Config(e.to_string()))?;
        table.remove("electrical");
        table.remove("thermal");
        let cfg: SceneConfig = toml::Value::Table(table)
            .try_into()
            .map_err(|e: toml::de::Error| GeometryError::Config(e.to_string()))?;
        let obj = config_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(&cfg.obj);
        let mesh = Mesh::from_obj_path(&obj, cfg.scale)?;
        Ok((cfg, mesh))
    }

    pub fn build(mesh: Mesh, cfg: &SceneConfig) -> Result<Self, GeometryError> {
        if cfg.samples == 0 {
            return Err(GeometryError::Config("samples must be ≥ 1".into()));
        }
        // Lift ray origins off the surface: well above f64 round-off at the
        // scene's size, far below any real gap.
        let ray_offset_m = (mesh.extent() * 1e-7).max(1e-9);
        let n = cfg.samples as usize;
        let mut scene = Scene {
            panels: Vec::new(),
            cells: Vec::new(),
            strings: Vec::new(),
            samples_per_cell: n * n,
            sample_points: Vec::new(),
            ray_offset_m,
            mesh,
        };
        for layout in &cfg.panels {
            scene.add_panel(layout, n)?;
        }
        Ok(scene)
    }

    /// Total cell area, m².
    pub fn cell_area_m2(&self) -> f64 {
        self.cells.iter().map(|c| c.area_m2).sum()
    }

    /// Sample points of cell `i`.
    pub fn samples(&self, i: usize) -> &[DVec3] {
        let k = self.samples_per_cell;
        &self.sample_points[i * k..(i + 1) * k]
    }

    fn add_panel(&mut self, l: &PanelLayout, n: usize) -> Result<(), GeometryError> {
        let name = l.name.clone().unwrap_or_else(|| l.object.clone());
        let bad = |msg: String| GeometryError::Layout {
            panel: name.clone(),
            msg,
        };
        if l.rows == 0 || l.cols == 0 || l.cell[0] <= 0.0 || l.cell[1] <= 0.0 {
            return Err(bad("rows, cols and cell size must be positive".into()));
        }
        if l.gap[0] < 0.0 || l.gap[1] < 0.0 {
            return Err(bad("gap must be ≥ 0".into()));
        }
        let object = self
            .mesh
            .object_index(&l.object)
            .ok_or_else(|| GeometryError::UnknownObject(l.object.clone()))?;

        // Cell face: the object's triangles (facing `normal`, if given).
        let want = l.normal.map(|n| DVec3::from_array(n).normalize());
        let face: Vec<usize> = (0..self.mesh.triangles.len())
            .filter(|&t| self.mesh.tri_object[t] as usize == object)
            .filter(|&t| {
                let c = self.mesh.tri_cross(t);
                c.length() > 0.0 && want.is_none_or(|w| c.normalize().dot(w) > 0.99)
            })
            .collect();
        if face.is_empty() {
            return Err(bad(format!(
                "object {:?} has no triangles{}",
                l.object,
                if want.is_some() {
                    " facing `normal`"
                } else {
                    ""
                }
            )));
        }
        let (mut sum_cross, mut sum_c, mut area) = (DVec3::ZERO, DVec3::ZERO, 0.0);
        for &t in &face {
            let c = self.mesh.tri_cross(t);
            let [a, b, d] = self.mesh.tri(t);
            sum_cross += c;
            sum_c += (a + b + d) / 3.0 * c.length();
            area += c.length();
        }
        // Same-facing coplanar triangles: |Σ cross| = Σ |cross|. Opposite
        // sides (a slab) or a bent face make the sum shorter.
        if sum_cross.length() < 0.999 * area {
            return Err(bad(
                "cell face is not planar or has triangles facing both ways; \n                 give `normal` to select one side of a slab"
                    .into(),
            ));
        }
        let normal = sum_cross.normalize();
        let centroid = sum_c / area;
        let size = (area / 2.0).sqrt();
        let worst = face
            .iter()
            .flat_map(|&t| self.mesh.tri(t))
            .map(|p| (p - centroid).dot(normal).abs())
            .fold(0.0, f64::max);
        if worst > 1e-4 * size + 1e-9 {
            return Err(bad(format!(
                "cell face is not planar (a vertex is {worst:.3e} m off the plane); \
                 give `normal` to select one side of a slab"
            )));
        }

        let axis = l
            .u_axis
            .map(DVec3::from_array)
            .unwrap_or(if normal.x.abs() < 0.9 {
                DVec3::X
            } else {
                DVec3::Y
            });
        let u = axis - normal * axis.dot(normal);
        if u.length() < 1e-6 {
            return Err(bad("u_axis is parallel to the panel normal".into()));
        }
        let u = u.normalize();
        let v = normal.cross(u);

        // Face triangles in plane coordinates, for the inside test.
        let to2 = |p: DVec3| DVec2::new((p - centroid).dot(u), (p - centroid).dot(v));
        let tris2: Vec<[DVec2; 3]> = face.iter().map(|&t| self.mesh.tri(t).map(to2)).collect();
        let tol = 1e-9 * size;
        let inside = |p: DVec2| tris2.iter().any(|t| in_triangle(p, t, tol));

        let [cu, cv] = l.cell;
        let [gu, gv] = l.gap;
        let width = l.cols as f64 * cu + (l.cols - 1) as f64 * gu;
        let height = l.rows as f64 * cv + (l.rows - 1) as f64 * gv;
        let u0 = l.offset[0] - width / 2.0 + cu / 2.0;
        let v0 = l.offset[1] - height / 2.0 + cv / 2.0;

        let panel_index = self.panels.len() as u32;
        let first = self.cells.len();
        for row in 0..l.rows {
            for col in 0..l.cols {
                let pu = u0 + col as f64 * (cu + gu);
                let pv = v0 + row as f64 * (cv + gv);
                for (du, dv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                    let corner = DVec2::new(pu + du * cu / 2.0, pv + dv * cv / 2.0);
                    if !inside(corner) {
                        return Err(bad(format!(
                            "cell row {row}, col {col} extends past the substrate edge"
                        )));
                    }
                }
                let centre = centroid + u * pu + v * pv;
                for i in 0..n {
                    for j in 0..n {
                        let su = ((j as f64 + 0.5) / n as f64 - 0.5) * cu;
                        let sv = ((i as f64 + 0.5) / n as f64 - 0.5) * cv;
                        self.sample_points
                            .push(centre + u * su + v * sv + normal * self.ray_offset_m);
                    }
                }
                self.cells.push(Cell {
                    panel: panel_index,
                    row,
                    col,
                    centre,
                    normal,
                    area_m2: cu * cv,
                    size_m: DVec2::new(cu, cv),
                });
            }
        }
        let wiring = wire_strings(l, first).map_err(bad)?;
        let first_string = self.strings.len();
        self.strings
            .extend(wiring.into_iter().map(|cells| SeriesString {
                panel: panel_index,
                cells,
            }));
        self.panels.push(Panel {
            name,
            object,
            normal,
            centroid,
            u,
            v,
            rows: l.rows,
            cols: l.cols,
            cells: first..self.cells.len(),
            strings: first_string..self.strings.len(),
            area_m2: area / 2.0, // area sums |cross| = 2 × triangle area
        });
        Ok(())
    }
}

/// Cell indices of each string of a panel whose first cell is `first`
/// (cells are stored row-major).
fn wire_strings(l: &PanelLayout, first: usize) -> Result<Vec<Vec<usize>>, String> {
    let (rows, cols) = (l.rows as usize, l.cols as usize);
    let index = |r: usize, c: usize| first + r * cols + c;
    let default = StringLayout::Chunked {
        series: l.cols,
        order: WalkOrder::Rows,
    };
    match l.strings.as_ref().unwrap_or(&default) {
        StringLayout::Chunked { series, order } => {
            let series = *series as usize;
            let n = rows * cols;
            if series == 0 || n % series != 0 {
                return Err(format!(
                    "strings.series = {series} does not divide the {n} cells"
                ));
            }
            // Serpentine walk: every other row (or column) runs backwards.
            let walk: Vec<usize> = match order {
                WalkOrder::Rows => (0..rows)
                    .flat_map(|r| {
                        (0..cols).map(move |k| index(r, if r % 2 == 0 { k } else { cols - 1 - k }))
                    })
                    .collect(),
                WalkOrder::Columns => (0..cols)
                    .flat_map(|c| {
                        (0..rows).map(move |k| index(if c % 2 == 0 { k } else { rows - 1 - k }, c))
                    })
                    .collect(),
            };
            Ok(walk.chunks(series).map(<[usize]>::to_vec).collect())
        }
        StringLayout::Explicit(list) => {
            let mut seen = vec![false; rows * cols];
            let mut out = Vec::with_capacity(list.len());
            for (s, cells) in list.iter().enumerate() {
                if cells.is_empty() {
                    return Err(format!("string {s} is empty"));
                }
                let mut string = Vec::with_capacity(cells.len());
                for &[r, c] in cells {
                    let (r, c) = (r as usize, c as usize);
                    if r >= rows || c >= cols {
                        return Err(format!("string {s}: cell [{r}, {c}] is outside the grid"));
                    }
                    if std::mem::replace(&mut seen[r * cols + c], true) {
                        return Err(format!("cell [{r}, {c}] is in more than one string"));
                    }
                    string.push(index(r, c));
                }
                out.push(string);
            }
            if let Some(k) = seen.iter().position(|&s| !s) {
                return Err(format!("cell [{}, {}] is in no string", k / cols, k % cols));
            }
            Ok(out)
        }
    }
}

/// Point-in-triangle in 2D with an edge tolerance (either winding).
fn in_triangle(p: DVec2, t: &[DVec2; 3], tol: f64) -> bool {
    let cross = |a: DVec2, b: DVec2, c: DVec2| (b - a).perp_dot(c - a);
    let area = cross(t[0], t[1], t[2]);
    let s = area.signum();
    let scale = area.abs().sqrt().max(1e-300);
    [(0, 1), (1, 2), (2, 0)]
        .iter()
        .all(|&(i, j)| s * cross(t[i], t[j], p) / scale >= -tol)
}
