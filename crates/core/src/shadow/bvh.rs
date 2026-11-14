//! Bounding-volume hierarchy over the mesh triangles, for any-hit shadow
//! rays. Built with binned SAH; built and traversed with explicit stacks
//! (no recursion), in f64 so results match the brute-force reference
//! exactly.

use glam::DVec3;

use crate::geometry::Mesh;

/// Triangle prepared for Möller–Trumbore: v0 and the two edges.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tri {
    v0: DVec3,
    e1: DVec3,
    e2: DVec3,
}

impl Tri {
    pub(crate) fn new([a, b, c]: [DVec3; 3]) -> Self {
        Self {
            v0: a,
            e1: b - a,
            e2: c - a,
        }
    }

    /// Whether the ray `o + t·d`, t > 0, hits the triangle (either side).
    #[inline]
    pub(crate) fn hit(&self, o: DVec3, d: DVec3) -> bool {
        let p = d.cross(self.e2);
        let det = self.e1.dot(p);
        if det == 0.0 {
            return false; // ray parallel to the plane
        }
        let inv = 1.0 / det;
        let s = o - self.v0;
        let u = s.dot(p) * inv;
        if !(0.0..=1.0).contains(&u) {
            return false;
        }
        let q = s.cross(self.e1);
        let v = d.dot(q) * inv;
        if v < 0.0 || u + v > 1.0 {
            return false;
        }
        self.e2.dot(q) * inv > 0.0
    }
}

#[derive(Debug, Clone, Copy)]
struct Node {
    lo: DVec3,
    hi: DVec3,
    /// Leaf: first triangle. Interior: index of the left child (the right
    /// child follows it).
    start: u32,
    /// Leaf: triangle count (> 0). Interior: 0.
    count: u32,
}

#[derive(Debug, Clone)]
pub struct Bvh {
    nodes: Vec<Node>,
    tris: Vec<Tri>,
}

const LEAF_MAX: usize = 4;
const BINS: usize = 16;

impl Bvh {
    pub fn build(mesh: &Mesh) -> Self {
        let n = mesh.triangles.len();
        let verts: Vec<[DVec3; 3]> = (0..n).map(|i| mesh.tri(i)).collect();
        let lo_of = |t: &[DVec3; 3]| t[0].min(t[1]).min(t[2]);
        let hi_of = |t: &[DVec3; 3]| t[0].max(t[1]).max(t[2]);
        let centroid: Vec<DVec3> = verts.iter().map(|t| (t[0] + t[1] + t[2]) / 3.0).collect();
        let mut order: Vec<usize> = (0..n).collect();
        let mut nodes = vec![Node {
            lo: DVec3::ZERO,
            hi: DVec3::ZERO,
            start: 0,
            count: n as u32,
        }];
        if n == 0 {
            return Self {
                nodes: Vec::new(),
                tris: Vec::new(),
            };
        }
        // (node index, range start, range end)
        let mut stack = vec![(0usize, 0usize, n)];
        while let Some((ni, s, e)) = stack.pop() {
            let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
            let (mut clo, mut chi) = (lo, hi);
            for &t in &order[s..e] {
                lo = lo.min(lo_of(&verts[t]));
                hi = hi.max(hi_of(&verts[t]));
                clo = clo.min(centroid[t]);
                chi = chi.max(centroid[t]);
            }
            nodes[ni] = Node {
                lo,
                hi,
                start: s as u32,
                count: (e - s) as u32,
            };
            if e - s <= LEAF_MAX {
                continue;
            }
            let Some(mid) = sah_split(&mut order[s..e], &verts, &centroid, clo, chi) else {
                continue; // no useful split: keep as a leaf
            };
            let left = nodes.len();
            nodes.push(nodes[ni]);
            nodes.push(nodes[ni]);
            nodes[ni].start = left as u32;
            nodes[ni].count = 0;
            stack.push((left, s, s + mid));
            stack.push((left + 1, s + mid, e));
        }
        let tris = order.iter().map(|&t| Tri::new(verts[t])).collect();
        Self { nodes, tris }
    }

    pub fn triangle_count(&self) -> usize {
        self.tris.len()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the ray from `o` along `d` (t > 0) hits any triangle.
    /// `inv_d` is `1 / d` component-wise (shared by every ray of a step).
    pub fn any_hit(&self, o: DVec3, d: DVec3, inv_d: DVec3) -> bool {
        if self.nodes.is_empty() {
            return false;
        }
        let mut stack = [0u32; 64];
        let mut sp = 1;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp] as usize];
            if !slab_hit(node.lo, node.hi, o, inv_d) {
                continue;
            }
            if node.count > 0 {
                let s = node.start as usize;
                if self.tris[s..s + node.count as usize]
                    .iter()
                    .any(|t| t.hit(o, d))
                {
                    return true;
                }
            } else {
                stack[sp] = node.start;
                stack[sp + 1] = node.start + 1;
                sp += 2;
            }
        }
        false
    }
}

/// Ray vs. box for t ≥ 0. NaNs (0·∞ on a slab plane) are ignored by
/// `min`/`max`, which keeps grazing rays conservative.
#[inline]
fn slab_hit(lo: DVec3, hi: DVec3, o: DVec3, inv_d: DVec3) -> bool {
    let t1 = (lo - o) * inv_d;
    let t2 = (hi - o) * inv_d;
    let tmin = t1.min(t2).max_element().max(0.0);
    let tmax = t1.max(t2).min_element();
    tmin <= tmax
}

/// Partitions `idx` by the best binned-SAH plane; returns the left count.
fn sah_split(
    idx: &mut [usize],
    verts: &[[DVec3; 3]],
    centroid: &[DVec3],
    clo: DVec3,
    chi: DVec3,
) -> Option<usize> {
    let ext = chi - clo;
    let area = |lo: DVec3, hi: DVec3| {
        let d = (hi - lo).max(DVec3::ZERO);
        d.x * d.y + d.y * d.z + d.z * d.x
    };
    let mut best: Option<(f64, usize, usize)> = None; // (cost, axis, bin)
    for axis in 0..3 {
        if ext[axis] <= 0.0 {
            continue;
        }
        let bin_of = |t: usize| {
            (((centroid[t][axis] - clo[axis]) / ext[axis] * BINS as f64) as usize).min(BINS - 1)
        };
        let mut count = [0usize; BINS];
        let mut blo = [DVec3::splat(f64::INFINITY); BINS];
        let mut bhi = [DVec3::splat(f64::NEG_INFINITY); BINS];
        for &t in idx.iter() {
            let b = bin_of(t);
            count[b] += 1;
            for v in verts[t] {
                blo[b] = blo[b].min(v);
                bhi[b] = bhi[b].max(v);
            }
        }
        // Sweep from the right to get suffix bounds.
        let mut rarea = [0.0; BINS];
        let mut rcount = [0usize; BINS];
        let (mut lo, mut hi, mut c) = (
            DVec3::splat(f64::INFINITY),
            DVec3::splat(f64::NEG_INFINITY),
            0,
        );
        for b in (1..BINS).rev() {
            lo = lo.min(blo[b]);
            hi = hi.max(bhi[b]);
            c += count[b];
            rarea[b] = area(lo, hi);
            rcount[b] = c;
        }
        let (mut lo, mut hi, mut c) = (
            DVec3::splat(f64::INFINITY),
            DVec3::splat(f64::NEG_INFINITY),
            0,
        );
        for b in 0..BINS - 1 {
            lo = lo.min(blo[b]);
            hi = hi.max(bhi[b]);
            c += count[b];
            if c == 0 || rcount[b + 1] == 0 {
                continue;
            }
            let cost = area(lo, hi) * c as f64 + rarea[b + 1] * rcount[b + 1] as f64;
            if best.is_none_or(|(bc, _, _)| cost < bc) {
                best = Some((cost, axis, b));
            }
        }
    }
    let (_, axis, bin) = best?;
    let split = |t: usize| {
        (((centroid[t][axis] - clo[axis]) / ext[axis] * BINS as f64) as usize).min(BINS - 1) <= bin
    };
    let mut left = 0;
    for i in 0..idx.len() {
        if split(idx[i]) {
            idx.swap(i, left);
            left += 1;
        }
    }
    (left > 0 && left < idx.len()).then_some(left)
}
