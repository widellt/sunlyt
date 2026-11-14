//! Triangle mesh of the whole spacecraft, loaded from OBJ.
//!
//! Every triangle is an occluder. Triangles keep the index of the OBJ
//! object (`o`) or group (`g`) they came from, so panels can be found by
//! name.

use std::io::BufRead;
use std::path::Path;

use glam::DVec3;

use super::GeometryError;

/// All triangles of the spacecraft in the body frame, metres.
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub positions: Vec<DVec3>,
    pub triangles: Vec<[u32; 3]>,
    /// OBJ object/group index of each triangle (into `objects`).
    pub tri_object: Vec<u32>,
    pub objects: Vec<String>,
}

impl Mesh {
    /// Loads an OBJ file. `scale` converts file units to metres (e.g.
    /// 0.001 for millimetres). Polygons are triangulated; points, lines
    /// and materials are ignored.
    pub fn from_obj_path(path: &Path, scale: f64) -> Result<Self, GeometryError> {
        let opts = load_options();
        let (models, _materials) = tobj::load_obj(path, &opts)
            .map_err(|e| GeometryError::Obj(format!("{}: {e}", path.display())))?;
        Ok(Self::from_models(models, scale))
    }

    /// Loads OBJ text from a reader (no material files).
    pub fn from_obj_reader<R: BufRead>(reader: &mut R, scale: f64) -> Result<Self, GeometryError> {
        let (models, _materials) = tobj::load_obj_buf(reader, &load_options(), |_| {
            Err(tobj::LoadError::OpenFileFailed)
        })
        .map_err(|e| GeometryError::Obj(e.to_string()))?;
        Ok(Self::from_models(models, scale))
    }

    fn from_models(models: Vec<tobj::Model>, scale: f64) -> Self {
        let mut m = Self::default();
        for model in models {
            let base = m.positions.len() as u32;
            let obj = match m.objects.iter().position(|n| *n == model.name) {
                Some(i) => i as u32,
                None => {
                    m.objects.push(model.name.clone());
                    (m.objects.len() - 1) as u32
                }
            };
            m.positions.extend(
                model
                    .mesh
                    .positions
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|p| DVec3::new(p[0], p[1], p[2]) * scale),
            );
            for t in model.mesh.indices.as_chunks::<3>().0 {
                m.triangles.push([base + t[0], base + t[1], base + t[2]]);
                m.tri_object.push(obj);
            }
        }
        m
    }

    pub fn object_index(&self, name: &str) -> Option<usize> {
        self.objects.iter().position(|n| n == name)
    }

    /// Vertices of triangle `i`.
    pub fn tri(&self, i: usize) -> [DVec3; 3] {
        let [a, b, c] = self.triangles[i];
        [
            self.positions[a as usize],
            self.positions[b as usize],
            self.positions[c as usize],
        ]
    }

    /// Unnormalised normal (twice the area) of triangle `i`, from its
    /// counter-clockwise winding.
    pub fn tri_cross(&self, i: usize) -> DVec3 {
        let [a, b, c] = self.tri(i);
        (b - a).cross(c - a)
    }

    /// Length of the bounding-box diagonal, metres.
    pub fn extent(&self) -> f64 {
        let (lo, hi) = self.positions.iter().fold(
            (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        );
        if self.positions.is_empty() {
            0.0
        } else {
            (hi - lo).length()
        }
    }
}

fn load_options() -> tobj::LoadOptions {
    tobj::LoadOptions {
        triangulate: true,
        ignore_points: true,
        ignore_lines: true,
        ..Default::default()
    }
}
