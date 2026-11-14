//! Self-shadowing: which part of each cell sees the Sun (design D2).
//!
//! The Sun is treated as a point for self-shadowing: its 0.27° radius
//! blurs a shadow edge by about 9 mm per metre from the occluder, small
//! next to a cell and averaging out along an edge. (The finite disk *is*
//! modelled for eclipses by Earth and Moon, in [`crate::env`].)
//!
//! The lit fraction here is purely geometric. Power models multiply it by
//! the eclipse's visible disk fraction; the two are never subtracted.

pub mod bvh;
pub mod exact;

use glam::DVec3;

use crate::geometry::Scene;
use bvh::{Bvh, Tri};

/// Computes per-cell lit fractions for a Sun direction.
pub trait Illuminator: Send + Sync {
    /// Writes the lit fraction (0–1) of every cell of `scene` into `out`
    /// for the unit Sun direction `sun_dir_b` (body frame). Cells facing
    /// away from the Sun get 0.
    fn lit_fractions(&self, scene: &Scene, sun_dir_b: DVec3, out: &mut [f64]);
}

/// Shadow rays from every cell sample point toward the Sun, tested against
/// a BVH of the whole mesh.
#[derive(Debug, Clone)]
pub struct RayCaster {
    bvh: Bvh,
}

impl RayCaster {
    pub fn new(scene: &Scene) -> Self {
        Self {
            bvh: Bvh::build(&scene.mesh),
        }
    }

    pub fn bvh(&self) -> &Bvh {
        &self.bvh
    }
}

impl Illuminator for RayCaster {
    fn lit_fractions(&self, scene: &Scene, sun_dir_b: DVec3, out: &mut [f64]) {
        let inv = sun_dir_b.recip();
        lit_by(scene, sun_dir_b, out, |p| {
            !self.bvh.any_hit(p, sun_dir_b, inv)
        });
    }
}

/// Reference illuminator: tests every ray against every triangle. Slow;
/// used to check [`RayCaster`] bit for bit.
#[derive(Debug, Clone)]
pub struct BruteForce {
    tris: Vec<Tri>,
}

impl BruteForce {
    pub fn new(scene: &Scene) -> Self {
        Self {
            tris: (0..scene.mesh.triangles.len())
                .map(|i| Tri::new(scene.mesh.tri(i)))
                .collect(),
        }
    }
}

impl Illuminator for BruteForce {
    fn lit_fractions(&self, scene: &Scene, sun_dir_b: DVec3, out: &mut [f64]) {
        lit_by(scene, sun_dir_b, out, |p| {
            !self.tris.iter().any(|t| t.hit(p, sun_dir_b))
        });
    }
}

fn lit_by(scene: &Scene, sun: DVec3, out: &mut [f64], sees_sun: impl Fn(DVec3) -> bool) {
    assert_eq!(out.len(), scene.cells.len());
    let k = scene.samples_per_cell as f64;
    for (i, (cell, o)) in scene.cells.iter().zip(out.iter_mut()).enumerate() {
        *o = if cell.normal.dot(sun) <= 0.0 {
            0.0
        } else {
            scene.samples(i).iter().filter(|&&p| sees_sun(p)).count() as f64 / k
        };
    }
}

#[cfg(test)]
mod tests;
