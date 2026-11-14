//! Scene geometry as GPU vertex arrays (body frame, metres, f32).

use bytemuck::{Pod, Zeroable};
use glam::{DVec3, Vec3};
use sunlyt_core::Scene;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct LineVertex {
    pub pos: [f32; 3],
    pub colour: [f32; 4],
}

pub const CELL_VERTICES: usize = 6;

fn v3(d: DVec3) -> [f32; 3] {
    d.as_vec3().to_array()
}

/// Every OBJ triangle, flat-shaded (structure and panel substrates).
pub fn structure(scene: &Scene) -> Vec<Vertex> {
    let m = &scene.mesh;
    let mut out = Vec::with_capacity(m.triangles.len() * 3);
    for t in &m.triangles {
        let [a, b, c] = t.map(|i| m.positions[i as usize]);
        let n = (b - a).cross(c - a).normalize_or_zero();
        for p in [a, b, c] {
            out.push(Vertex {
                pos: v3(p),
                normal: v3(n),
            });
        }
    }
    out
}

/// Two triangles per cell, counter-clockwise about the cell normal, lifted
/// `lift` metres off the substrate so they never z-fight it.
pub fn cells(scene: &Scene, lift: f64) -> Vec<Vertex> {
    let mut out = Vec::with_capacity(scene.cells.len() * CELL_VERTICES);
    for cell in &scene.cells {
        let p = &scene.panels[cell.panel as usize];
        let (hu, hv) = (p.u * cell.size_m.x / 2.0, p.v * cell.size_m.y / 2.0);
        let c = cell.centre + cell.normal * lift;
        let corner = |su: f64, sv: f64| Vertex {
            pos: v3(c + hu * su + hv * sv),
            normal: v3(cell.normal),
        };
        // u × v = normal, so (−,−) → (+,−) → (+,+) is counter-clockwise.
        let q = [
            corner(-1.0, -1.0),
            corner(1.0, -1.0),
            corner(1.0, 1.0),
            corner(-1.0, 1.0),
        ];
        out.extend_from_slice(&[q[0], q[1], q[2], q[0], q[2], q[3]]);
    }
    out
}

/// Centre and radius of a sphere around the mesh (bounding-box based).
pub fn bounds(scene: &Scene) -> (Vec3, f32) {
    let (mut lo, mut hi) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
    for &p in &scene.mesh.positions {
        lo = lo.min(p);
        hi = hi.max(p);
    }
    if lo.x > hi.x {
        return (Vec3::ZERO, 1.0);
    }
    let c = (lo + hi) / 2.0;
    (c.as_vec3(), ((hi - lo).length() / 2.0) as f32)
}
