//! Exact self-shadowing by polygon clipping: an independent check of the
//! ray caster (slice 8, S2).
//!
//! Every triangle of the mesh is cut to the half-space above the panel
//! plane, then projected along the Sun direction onto that plane. The
//! shadow on the panel is the union of these convex polygons. Each cell
//! rectangle's covered area is computed exactly with a strip sweep:
//!
//! - Within a cell, the polygons are clipped to the rectangle.
//! - Breakpoints in u are every polygon vertex and every crossing between
//!   edges of two polygons.
//! - Between two breakpoints every polygon's cross-section is an interval
//!   whose ends move linearly in u, and no ends swap order, so the length
//!   of the union of intervals is linear in u. Its value at the strip's
//!   midpoint times the strip width is therefore the exact strip area.
//!
//! No sampling is involved, so the only error is floating-point round-off
//! (~1e-12 relative at panel scale).
//!
//! Like [`super::RayCaster`], the Sun is a point here and geometry closer
//! than `scene.ray_offset_m` to the panel plane does not shadow it (that
//! is the height where the rays start). Both sides of every triangle
//! block light.

use glam::{DVec2, DVec3};

use crate::geometry::Scene;

/// A convex polygon in panel-plane coordinates (u, v), metres from the
/// panel centroid, counter-clockwise.
pub type Poly2 = Vec<DVec2>;

/// The shadow polygons that every mesh triangle casts on panel `panel`'s
/// plane for the unit Sun direction `sun_dir_b` (body frame). Empty when
/// the Sun is behind or in the plane. The polygons may overlap; the shadow
/// is their union.
pub fn shadow_polygons(scene: &Scene, panel: usize, sun_dir_b: DVec3) -> Vec<Poly2> {
    let p = &scene.panels[panel];
    let cos_sun = sun_dir_b.dot(p.normal);
    if cos_sun <= 0.0 {
        return Vec::new();
    }
    let floor = scene.ray_offset_m;
    let mut out = Vec::new();
    for t in 0..scene.mesh.triangles.len() {
        let tri = scene.mesh.tri(t);
        let h = tri.map(|q| (q - p.centroid).dot(p.normal));
        if h.iter().all(|&x| x <= floor) {
            continue;
        }
        // Keep the part of the triangle above the floor (convex, ≤ 4 vertices).
        let mut above: Vec<(DVec3, f64)> = Vec::with_capacity(4);
        for k in 0..3 {
            let (a, ha) = (tri[k], h[k]);
            let (b, hb) = (tri[(k + 1) % 3], h[(k + 1) % 3]);
            if ha > floor {
                above.push((a, ha));
            }
            if (ha > floor) != (hb > floor) {
                let s = (floor - ha) / (hb - ha);
                above.push((a + (b - a) * s, floor));
            }
        }
        // Slide each point down the Sun ray onto the plane.
        let mut poly: Poly2 = above
            .iter()
            .map(|&(q, hq)| {
                let r = q - sun_dir_b * (hq / cos_sun) - p.centroid;
                DVec2::new(r.dot(p.u), r.dot(p.v))
            })
            .collect();
        let a = signed_area(&poly);
        if a.abs() <= 1e-30 {
            continue; // edge-on to the Sun: casts no area
        }
        if a < 0.0 {
            poly.reverse();
        }
        out.push(poly);
    }
    out
}

/// Exact lit fraction of each cell of panel `panel` (in the panel's cell
/// order) for the unit Sun direction `sun_dir_b`. Cells facing away get 0,
/// as in [`super::Illuminator`].
pub fn exact_lit_fractions(scene: &Scene, panel: usize, sun_dir_b: DVec3) -> Vec<f64> {
    let p = &scene.panels[panel];
    if sun_dir_b.dot(p.normal) <= 0.0 {
        return vec![0.0; p.cells.len()];
    }
    let shadows = shadow_polygons(scene, panel, sun_dir_b);
    let boxes: Vec<(DVec2, DVec2)> = shadows.iter().map(|s| bbox(s)).collect();
    p.cells
        .clone()
        .map(|i| {
            let (lo, hi) = cell_rect(scene, panel, i);
            let mut clipped: Vec<Poly2> = Vec::new();
            for (s, (blo, bhi)) in shadows.iter().zip(&boxes) {
                if blo.x >= hi.x || bhi.x <= lo.x || blo.y >= hi.y || bhi.y <= lo.y {
                    continue;
                }
                let c = clip_to_rect(s, lo, hi);
                if c.len() >= 3 && signed_area(&c) > 0.0 {
                    clipped.push(c);
                }
            }
            let area = (hi.x - lo.x) * (hi.y - lo.y);
            let covered = union_area(&clipped);
            (1.0 - covered / area).clamp(0.0, 1.0)
        })
        .collect()
}

/// Corners (min, max) of cell `i`'s rectangle in panel-plane coordinates.
pub fn cell_rect(scene: &Scene, panel: usize, i: usize) -> (DVec2, DVec2) {
    let p = &scene.panels[panel];
    let c = &scene.cells[i];
    let r = c.centre - p.centroid;
    let mid = DVec2::new(r.dot(p.u), r.dot(p.v));
    (mid - c.size_m / 2.0, mid + c.size_m / 2.0)
}

fn signed_area(p: &[DVec2]) -> f64 {
    let n = p.len();
    (0..n).map(|i| p[i].perp_dot(p[(i + 1) % n])).sum::<f64>() / 2.0
}

fn bbox(p: &[DVec2]) -> (DVec2, DVec2) {
    p.iter().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(lo, hi), q| (lo.min(*q), hi.max(*q)),
    )
}

/// Sutherland–Hodgman clip of a convex polygon to an axis-aligned box.
fn clip_to_rect(poly: &[DVec2], lo: DVec2, hi: DVec2) -> Poly2 {
    let mut p = poly.to_vec();
    for (n, c) in [
        (DVec2::X, hi.x),
        (-DVec2::X, -lo.x),
        (DVec2::Y, hi.y),
        (-DVec2::Y, -lo.y),
    ] {
        if p.is_empty() {
            break;
        }
        let inside = |q: DVec2| q.dot(n) <= c;
        let mut out = Vec::with_capacity(p.len() + 1);
        for i in 0..p.len() {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            if inside(a) {
                out.push(a);
            }
            if inside(a) != inside(b) {
                let t = (c - a.dot(n)) / (b - a).dot(n);
                let mut q = a + (b - a) * t;
                // Land exactly on the boundary, so strip breakpoints match.
                if n.x != 0.0 {
                    q.x = c * n.x;
                } else {
                    q.y = c * n.y;
                }
                out.push(q);
            }
        }
        p = out;
    }
    p
}

/// Exact area of the union of convex counter-clockwise polygons.
pub fn union_area(polys: &[Poly2]) -> f64 {
    match polys.len() {
        0 => return 0.0,
        1 => return signed_area(&polys[0]),
        _ => {}
    }
    let mut xs: Vec<f64> = polys.iter().flatten().map(|q| q.x).collect();
    let edges: Vec<Vec<(DVec2, DVec2)>> = polys
        .iter()
        .map(|p| (0..p.len()).map(|i| (p[i], p[(i + 1) % p.len()])).collect())
        .collect();
    for (a, ea) in edges.iter().enumerate() {
        for eb in &edges[a + 1..] {
            for &(p0, p1) in ea {
                for &(q0, q1) in eb {
                    if let Some(x) = crossing_x(p0, p1, q0, q1) {
                        xs.push(x);
                    }
                }
            }
        }
    }
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    let mut area = 0.0;
    let mut spans: Vec<(f64, f64)> = Vec::with_capacity(polys.len());
    for w in xs.windows(2) {
        let (xa, xb) = (w[0], w[1]);
        if xb <= xa {
            continue;
        }
        let xm = 0.5 * (xa + xb);
        spans.clear();
        spans.extend(polys.iter().filter_map(|p| section(p, xm)));
        area += (xb - xa) * union_length(&mut spans);
    }
    area
}

/// The u of the proper crossing of segments p0p1 and q0q1, if any.
fn crossing_x(p0: DVec2, p1: DVec2, q0: DVec2, q1: DVec2) -> Option<f64> {
    let r = p1 - p0;
    let s = q1 - q0;
    let den = r.perp_dot(s);
    if den == 0.0 {
        return None; // parallel: endpoints already give the breakpoints
    }
    let t = (q0 - p0).perp_dot(s) / den;
    let u = (q0 - p0).perp_dot(r) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(p0.x + r.x * t)
}

/// The interval a vertical line at `x` cuts from a convex polygon.
fn section(p: &[DVec2], x: f64) -> Option<(f64, f64)> {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for i in 0..p.len() {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        if (a.x <= x) != (b.x <= x) {
            let y = a.y + (b.y - a.y) * (x - a.x) / (b.x - a.x);
            lo = lo.min(y);
            hi = hi.max(y);
        }
    }
    (hi > lo).then_some((lo, hi))
}

fn union_length(spans: &mut [(f64, f64)]) -> f64 {
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut total = 0.0;
    let mut cur: Option<(f64, f64)> = None;
    for &(a, b) in spans.iter() {
        cur = match cur {
            Some((c0, c1)) if a <= c1 => Some((c0, c1.max(b))),
            Some((c0, c1)) => {
                total += c1 - c0;
                Some((a, b))
            }
            None => Some((a, b)),
        };
    }
    if let Some((c0, c1)) = cur {
        total += c1 - c0;
    }
    total
}

/// [`exact_lit_fractions`] for every panel, as an [`super::Illuminator`],
/// so the power path can run on exact shadows in place of rays.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExactClipper;

impl super::Illuminator for ExactClipper {
    fn lit_fractions(&self, scene: &Scene, sun_dir_b: DVec3, out: &mut [f64]) {
        assert_eq!(out.len(), scene.cells.len());
        for (p, panel) in scene.panels.iter().enumerate() {
            out[panel.cells.clone()].copy_from_slice(&exact_lit_fractions(scene, p, sun_dir_b));
        }
    }
}
