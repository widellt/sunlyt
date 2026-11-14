//! Writes simple shapes as OBJ text, for tests, benchmarks and synthetic
//! examples. Faces wind counter-clockwise seen from outside.

use std::f64::consts::PI;
use std::fmt::Write;

use glam::DVec3;

#[derive(Debug, Default, Clone)]
pub struct ObjWriter {
    text: String,
    vertices: usize,
}

impl ObjWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn finish(self) -> String {
        self.text
    }

    fn object(&mut self, name: &str) {
        let _ = writeln!(self.text, "o {name}");
    }

    fn vertex(&mut self, p: DVec3) -> usize {
        let _ = writeln!(self.text, "v {} {} {}", p.x, p.y, p.z);
        self.vertices += 1;
        self.vertices // OBJ indices are 1-based
    }

    fn face(&mut self, idx: &[usize]) {
        self.text.push('f');
        for i in idx {
            let _ = write!(self.text, " {i}");
        }
        self.text.push('\n');
    }

    /// One-sided quad; normal = (b − a) × (c − b).
    pub fn quad(&mut self, name: &str, corners: [DVec3; 4]) -> &mut Self {
        self.object(name);
        let i = corners.map(|p| self.vertex(p));
        self.face(&i);
        self
    }

    /// Rectangle centred at `centre` spanning ±`half_u`·u and ±`half_v`·v,
    /// facing `u × v`.
    pub fn rect(&mut self, name: &str, centre: DVec3, u: DVec3, v: DVec3) -> &mut Self {
        self.quad(
            name,
            [
                centre - u - v,
                centre + u - v,
                centre + u + v,
                centre - u + v,
            ],
        )
    }

    /// Axis-aligned box, outward-facing.
    pub fn cuboid(&mut self, name: &str, lo: DVec3, hi: DVec3) -> &mut Self {
        self.object(name);
        let c = |x: bool, y: bool, z: bool| {
            DVec3::new(
                if x { hi.x } else { lo.x },
                if y { hi.y } else { lo.y },
                if z { hi.z } else { lo.z },
            )
        };
        let mut v = [0usize; 8];
        for (k, slot) in v.iter_mut().enumerate() {
            *slot = self.vertex(c(k & 1 != 0, k & 2 != 0, k & 4 != 0));
        }
        // Index bits: x=1, y=2, z=4.
        for f in [
            [0, 2, 3, 1], // −z
            [4, 5, 7, 6], // +z
            [0, 1, 5, 4], // −y
            [2, 6, 7, 3], // +y
            [0, 4, 6, 2], // −x
            [1, 3, 7, 5], // +x
        ] {
            self.face(&f.map(|k| v[k]));
        }
        self
    }

    /// Box centred at `centre` with half-sizes `half` along the
    /// right-handed orthonormal `axes`, outward-facing.
    pub fn oriented_box(
        &mut self,
        name: &str,
        centre: DVec3,
        half: DVec3,
        axes: [DVec3; 3],
    ) -> &mut Self {
        self.object(name);
        let mut v = [0usize; 8];
        for (k, slot) in v.iter_mut().enumerate() {
            let s = |bit: usize| if k & bit != 0 { 1.0 } else { -1.0 };
            let p = centre
                + axes[0] * (s(1) * half.x)
                + axes[1] * (s(2) * half.y)
                + axes[2] * (s(4) * half.z);
            *slot = self.vertex(p);
        }
        // Same faces as `cuboid` (index bits: x=1, y=2, z=4).
        for f in [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ] {
            self.face(&f.map(|k| v[k]));
        }
        self
    }

    /// Latitude/longitude sphere with `n` bands (2n² triangles roughly).
    pub fn sphere(&mut self, name: &str, centre: DVec3, r: f64, n: usize) -> &mut Self {
        self.object(name);
        let n = n.max(2);
        let mut idx = vec![vec![0usize; 2 * n]; n + 1];
        for (i, row) in idx.iter_mut().enumerate() {
            let th = PI * i as f64 / n as f64;
            for (j, slot) in row.iter_mut().enumerate() {
                let ph = 2.0 * PI * j as f64 / (2 * n) as f64;
                let p = DVec3::new(th.sin() * ph.cos(), th.sin() * ph.sin(), th.cos());
                *slot = self.vertex(centre + p * r);
            }
        }
        for i in 0..n {
            for j in 0..2 * n {
                let j1 = (j + 1) % (2 * n);
                let (a, b, c, d) = (idx[i][j], idx[i + 1][j], idx[i + 1][j1], idx[i][j1]);
                if i != 0 {
                    self.face(&[a, b, d]);
                }
                if i != n - 1 {
                    self.face(&[b, c, d]);
                }
            }
        }
        self
    }
}
