use glam::{DVec2, DVec3};

use super::*;
use crate::geometry::shapes::ObjWriter;
use crate::geometry::{Mesh, PanelLayout, SceneConfig};

fn scene(w: &ObjWriter, panels: Vec<PanelLayout>, samples: u32) -> Scene {
    let mesh = Mesh::from_obj_reader(&mut w.clone().finish().as_bytes(), 1.0).unwrap();
    Scene::build(
        mesh,
        &SceneConfig {
            obj: String::new(),
            scale: 1.0,
            samples,
            panels,
        },
    )
    .unwrap()
}

fn panel(object: &str, n: u32, cell: f64) -> PanelLayout {
    PanelLayout {
        name: None,
        object: object.into(),
        normal: None,
        rows: n,
        cols: n,
        cell: [cell, cell],
        gap: [0.0, 0.0],
        u_axis: None,
        offset: [0.0, 0.0],
        strings: None,
    }
}

fn sun(elev_deg: f64, az_deg: f64) -> DVec3 {
    let (e, a) = (elev_deg.to_radians(), az_deg.to_radians());
    DVec3::new(e.cos() * a.cos(), e.cos() * a.sin(), e.sin())
}

#[test]
fn triangle_hit_basics() {
    let t = Tri::new([DVec3::ZERO, DVec3::X, DVec3::Y]);
    let up = DVec3::Z;
    assert!(t.hit(DVec3::new(0.2, 0.2, -1.0), up));
    assert!(t.hit(DVec3::new(0.2, 0.2, 1.0), -up)); // back side blocks too
    assert!(!t.hit(DVec3::new(0.2, 0.2, 1.0), up)); // behind the origin
    assert!(!t.hit(DVec3::new(0.6, 0.6, -1.0), up)); // outside
    assert!(!t.hit(DVec3::new(0.2, 0.2, 0.0), DVec3::X)); // parallel
}

#[test]
fn open_sky_and_facing_away() {
    let mut w = ObjWriter::new();
    w.rect("P", DVec3::ZERO, DVec3::X * 0.5, DVec3::Y * 0.5);
    let s = scene(&w, vec![panel("P", 4, 0.25)], 2);
    let rc = RayCaster::new(&s);
    let mut out = vec![0.0; s.cells.len()];
    rc.lit_fractions(&s, sun(30.0, 10.0), &mut out);
    assert!(out.iter().all(|&f| f == 1.0));
    rc.lit_fractions(&s, sun(-30.0, 10.0), &mut out);
    assert!(out.iter().all(|&f| f == 0.0));
}

/// A cluttered scene: bus, two wings, an antenna sphere and a mast.
fn clutter() -> Scene {
    let mut w = ObjWriter::new();
    w.cuboid(
        "BUS",
        DVec3::new(-0.5, -0.5, -0.5),
        DVec3::new(0.5, 0.5, 0.5),
    )
    .rect(
        "WING_PY",
        DVec3::new(0.0, 1.8, 0.0),
        DVec3::X * 0.5,
        DVec3::Y * 1.2,
    )
    .rect(
        "WING_NY",
        DVec3::new(0.0, -1.8, 0.0),
        DVec3::X * 0.5,
        DVec3::Y * 1.2,
    )
    .sphere("DISH", DVec3::new(0.3, 0.9, 0.6), 0.35, 12)
    .cuboid(
        "MAST",
        DVec3::new(-0.05, -1.2, 0.0),
        DVec3::new(0.05, -1.1, 1.0),
    );
    scene(
        &w,
        vec![panel("WING_PY", 20, 0.05), panel("WING_NY", 20, 0.05)],
        2,
    )
}

#[test]
fn bvh_matches_brute_force_exactly() {
    let s = clutter();
    let (rc, bf) = (RayCaster::new(&s), BruteForce::new(&s));
    assert!(rc.bvh().node_count() > 1);
    let (mut a, mut b) = (vec![0.0; s.cells.len()], vec![0.0; s.cells.len()]);
    let mut shaded_somewhere = false;
    for i in 0..60 {
        let d = sun(5.0 + (i as f64 * 37.0) % 80.0, i as f64 * 53.0);
        rc.lit_fractions(&s, d, &mut a);
        bf.lit_fractions(&s, d, &mut b);
        assert_eq!(a, b, "sun {d}");
        shaded_somewhere |= a.iter().any(|&f| f > 0.0 && f < 1.0);
    }
    assert!(
        shaded_somewhere,
        "test scene never produced a partial shadow"
    );
}

// ── Analytic gate: a cube hovering over a plate ─────────────────────────

/// Area of the convex hull of `pts` clipped to the square |x|,|y| ≤ h.
fn clipped_hull_area(pts: &[DVec2], h: f64) -> f64 {
    let mut p: Vec<DVec2> = pts.to_vec();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let cross = |o: DVec2, a: DVec2, b: DVec2| (a - o).perp_dot(b - o);
    let mut hull: Vec<DVec2> = Vec::new();
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &DVec2>> = if pass == 0 {
            Box::new(p.iter())
        } else {
            Box::new(p.iter().rev())
        };
        for &q in iter {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], q) <= 0.0
            {
                hull.pop();
            }
            hull.push(q);
        }
        hull.pop();
    }
    // Sutherland–Hodgman against the four square edges.
    let mut poly = hull;
    for (n, c) in [(DVec2::X, h), (-DVec2::X, h), (DVec2::Y, h), (-DVec2::Y, h)] {
        let inside = |q: DVec2| q.dot(n) <= c;
        let mut out = Vec::new();
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if inside(a) {
                out.push(a);
            }
            if inside(a) != inside(b) {
                let t = (c - a.dot(n)) / (b - a).dot(n);
                out.push(a + (b - a) * t);
            }
        }
        poly = out;
    }
    let mut area = 0.0;
    for i in 0..poly.len() {
        area += poly[i].perp_dot(poly[(i + 1) % poly.len()]);
    }
    area.abs() / 2.0
}

#[test]
fn cube_over_plate_matches_projected_shadow() {
    // 1 m plate at z = 0 facing +Z; 0.4 m cube hovering above it.
    let (lo, hi) = (DVec3::new(-0.1, -0.25, 0.3), DVec3::new(0.3, 0.15, 0.7));
    let mut w = ObjWriter::new();
    w.rect("PLATE", DVec3::ZERO, DVec3::X * 0.5, DVec3::Y * 0.5)
        .cuboid("CUBE", lo, hi);
    let s = scene(&w, vec![panel("PLATE", 100, 0.01)], 2);
    let rc = RayCaster::new(&s);
    let mut lit = vec![0.0; s.cells.len()];
    for (elev, az) in [
        (90.0, 0.0),
        (60.0, 20.0),
        (45.0, 135.0),
        (35.0, 250.0),
        (25.0, 300.0),
    ] {
        let d = sun(elev, az);
        rc.lit_fractions(&s, d, &mut lit);
        let got = lit.iter().sum::<f64>() / lit.len() as f64;
        // Project the cube's corners along −d onto z = 0.
        let corners: Vec<DVec2> = (0..8)
            .map(|k| {
                let p = DVec3::new(
                    if k & 1 != 0 { hi.x } else { lo.x },
                    if k & 2 != 0 { hi.y } else { lo.y },
                    if k & 4 != 0 { hi.z } else { lo.z },
                );
                let q = p - d * (p.z / d.z);
                DVec2::new(q.x, q.y)
            })
            .collect();
        let want = 1.0 - clipped_hull_area(&corners, 0.5);
        let rel = (got - want).abs() / want;
        eprintln!("elev {elev:>4} az {az:>5}: lit {got:.5} analytic {want:.5} rel err {rel:.2e}");
        assert!(rel < 0.005, "elev {elev} az {az}: {got} vs {want}");
    }
}

// ── Exact polygon clipping (slice 8) ────────────────────────────────────

#[test]
fn union_area_basics() {
    use exact::union_area;
    let sq = |x: f64, y: f64| {
        vec![
            DVec2::new(x, y),
            DVec2::new(x + 1.0, y),
            DVec2::new(x + 1.0, y + 1.0),
            DVec2::new(x, y + 1.0),
        ]
    };
    assert_eq!(union_area(&[]), 0.0);
    assert!((union_area(&[sq(0.0, 0.0)]) - 1.0).abs() < 1e-15);
    assert!((union_area(&[sq(0.0, 0.0), sq(0.0, 0.0)]) - 1.0).abs() < 1e-15);
    assert!((union_area(&[sq(0.0, 0.0), sq(0.5, 0.5)]) - 1.75).abs() < 1e-15);
    assert!((union_area(&[sq(0.0, 0.0), sq(2.0, 0.0)]) - 2.0).abs() < 1e-15);
    // A diamond over the square: square + 4 corner tips of (√2·… ) checked
    // by hand: diamond |x−.5|+|y−.5| ≤ .7071 sticks out by 4 triangles of
    // area (0.7071−0.5)² = 0.04289.
    let r = std::f64::consts::FRAC_1_SQRT_2;
    let diamond = vec![
        DVec2::new(0.5 + r, 0.5),
        DVec2::new(0.5, 0.5 + r),
        DVec2::new(0.5 - r, 0.5),
        DVec2::new(0.5, 0.5 - r),
    ];
    let want = 1.0 + 4.0 * (r - 0.5) * (r - 0.5);
    assert!((union_area(&[sq(0.0, 0.0), diamond]) - want).abs() < 1e-14);
}

/// The cube-over-plate scene of the analytic gate, with `samples` rays.
fn cube_scene(samples: u32) -> (Scene, DVec3, DVec3) {
    let (lo, hi) = (DVec3::new(-0.1, -0.25, 0.3), DVec3::new(0.3, 0.15, 0.7));
    let mut w = ObjWriter::new();
    w.rect("PLATE", DVec3::ZERO, DVec3::X * 0.5, DVec3::Y * 0.5)
        .cuboid("CUBE", lo, hi);
    (scene(&w, vec![panel("PLATE", 50, 0.02)], samples), lo, hi)
}

const SUNS: [(f64, f64); 5] = [
    (90.0, 0.0),
    (60.0, 20.0),
    (45.0, 135.0),
    (35.0, 250.0),
    (25.0, 300.0),
];

#[test]
fn exact_matches_analytic_projection() {
    let (s, lo, hi) = cube_scene(1);
    for (elev, az) in SUNS {
        let d = sun(elev, az);
        let lit = exact::exact_lit_fractions(&s, 0, d);
        let got: f64 = lit.iter().map(|f| 1.0 - f).sum::<f64>() * 0.02 * 0.02;
        let corners: Vec<DVec2> = (0..8)
            .map(|k| {
                let p = DVec3::new(
                    if k & 1 != 0 { hi.x } else { lo.x },
                    if k & 2 != 0 { hi.y } else { lo.y },
                    if k & 4 != 0 { hi.z } else { lo.z },
                );
                let q = p - d * (p.z / d.z);
                DVec2::new(q.x, q.y)
            })
            .collect();
        let want = clipped_hull_area(&corners, 0.5);
        eprintln!("elev {elev:>4} az {az:>5}: shadow {got:.12} m² analytic {want:.12} m²");
        assert!((got - want).abs() < 1e-12, "{got} vs {want}");
    }
}

/// Gate: the ray caster against exact clipping, cell by cell. A cell no
/// shadow edge crosses must agree exactly. A cell an edge crosses: the
/// n×n midpoint grid misreads a straight edge by at most half a sample
/// row (1/(2n)), and at most two edges (a shadow corner) meet in a cell,
/// so |ray − exact| ≤ 1/n.
#[test]
fn ray_caster_matches_exact_per_cell() {
    let mut worst_by_n = Vec::new();
    for n in [2u32, 4, 8] {
        let (s, _, _) = cube_scene(n);
        let rc = RayCaster::new(&s);
        let mut ray = vec![0.0; s.cells.len()];
        let mut worst = 0.0f64;
        for (elev, az) in SUNS {
            let d = sun(elev, az);
            rc.lit_fractions(&s, d, &mut ray);
            let ex = exact::exact_lit_fractions(&s, 0, d);
            let (mut edge_cells, mut sum_diff) = (0, 0.0);
            for (k, (&r, &e)) in ray.iter().zip(&ex).enumerate() {
                let diff = (r - e).abs();
                sum_diff += r - e;
                if e < 1e-9 || e > 1.0 - 1e-9 {
                    assert!(diff < 1e-9, "n {n} cell {k}: no edge, ray {r} exact {e}");
                } else {
                    edge_cells += 1;
                    assert!(diff <= 1.0 / n as f64 + 1e-12, "n {n} cell {k}: {r} vs {e}");
                    worst = worst.max(diff);
                }
            }
            eprintln!(
                "n {n} elev {elev:>4} az {az:>5}: {edge_cells} edge cells, mean diff {:.2e}",
                sum_diff / ray.len() as f64
            );
        }
        eprintln!(
            "n {n}: worst edge-cell error {worst:.4} (bound {:.4})",
            1.0 / n as f64
        );
        worst_by_n.push(worst);
    }
    // Finer sampling must converge on the exact answer.
    assert!(worst_by_n[2] < worst_by_n[0] / 2.0, "{worst_by_n:?}");
}

#[test]
fn exact_on_clutter_agrees_with_rays_in_the_mean() {
    let s = clutter();
    let rc = RayCaster::new(&s);
    let mut ray = vec![0.0; s.cells.len()];
    for i in 0..20 {
        let d = sun(5.0 + (i as f64 * 37.0) % 80.0, i as f64 * 53.0);
        rc.lit_fractions(&s, d, &mut ray);
        for (pi, p) in s.panels.iter().enumerate() {
            let ex = exact::exact_lit_fractions(&s, pi, d);
            let r = &ray[p.cells.clone()];
            for (&a, &b) in r.iter().zip(&ex) {
                assert!((a - b).abs() <= 0.5 + 1e-12, "sun {d}: {a} vs {b}");
                if b < 1e-9 || b > 1.0 - 1e-9 {
                    assert!((a - b).abs() < 1e-9, "sun {d}: no edge but {a} vs {b}");
                }
            }
        }
    }
}
