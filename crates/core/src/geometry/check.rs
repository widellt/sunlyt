//! Scene inspection without a trajectory, for `sunlyt check`. It reports
//! what the loader found and draws each panel's layout as SVG, so you can
//! see where the cells sit and how they are wired before running anything.

use std::fmt::Write as _;

use glam::{DVec2, DVec3};

use super::scene::Scene;

/// One OBJ object/group and what it is in the scene.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupInfo {
    pub name: String,
    pub triangles: usize,
    /// Panels laid on this group (indices into `Scene::panels`).
    pub panels: Vec<usize>,
}

/// One panel's grid against its face.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelInfo {
    /// Face size along (u, v): extent of the cell-side triangles, m.
    pub face_m: DVec2,
    /// Cell grid size along (u, v), cells and gaps, m.
    pub grid_m: DVec2,
    /// Grid centre relative to the face's bounding-box centre, (u, v), m.
    pub grid_offset_m: DVec2,
    /// Smallest distance from a cell edge to the face's bounding box, m
    /// (an approximation of the margin for non-rectangular faces).
    pub margin_m: f64,
    /// Cells per string, in wiring order of the panel's strings.
    pub string_lengths: Vec<usize>,
}

/// Mesh extent, m.
pub fn bounds(scene: &Scene) -> (DVec3, DVec3) {
    let mut lo = DVec3::splat(f64::MAX);
    let mut hi = DVec3::splat(f64::MIN);
    for &p in &scene.mesh.positions {
        lo = lo.min(p);
        hi = hi.max(p);
    }
    (lo, hi)
}

pub fn groups(scene: &Scene) -> Vec<GroupInfo> {
    let m = &scene.mesh;
    let mut out: Vec<GroupInfo> = m
        .objects
        .iter()
        .map(|n| GroupInfo {
            name: n.clone(),
            triangles: 0,
            panels: Vec::new(),
        })
        .collect();
    for &o in &m.tri_object {
        out[o as usize].triangles += 1;
    }
    for (k, p) in scene.panels.iter().enumerate() {
        out[p.object].panels.push(k);
    }
    out
}

/// The panel's cell-side triangles in its (u, v) frame, origin at the
/// panel centroid.
pub fn face_uv(scene: &Scene, panel: usize) -> Vec<[DVec2; 3]> {
    let p = &scene.panels[panel];
    let to_uv = |x: DVec3| {
        let d = x - p.centroid;
        DVec2::new(d.dot(p.u), d.dot(p.v))
    };
    (0..scene.mesh.triangles.len())
        .filter(|&i| scene.mesh.tri_object[i] as usize == p.object)
        .filter_map(|i| {
            let [a, b, c] = scene.mesh.tri(i);
            let n = (b - a).cross(c - a).normalize_or_zero();
            (n.dot(p.normal) > 0.999).then(|| [to_uv(a), to_uv(b), to_uv(c)])
        })
        .collect()
}

/// Cell rectangles in the panel's (u, v) frame: (centre, size).
fn cells_uv(scene: &Scene, panel: usize) -> Vec<(DVec2, DVec2)> {
    let p = &scene.panels[panel];
    scene.cells[p.cells.clone()]
        .iter()
        .map(|c| {
            let d = c.centre - p.centroid;
            (DVec2::new(d.dot(p.u), d.dot(p.v)), c.size_m)
        })
        .collect()
}

pub fn panel_info(scene: &Scene, panel: usize) -> PanelInfo {
    let p = &scene.panels[panel];
    let (mut flo, mut fhi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
    for t in face_uv(scene, panel) {
        for q in t {
            flo = flo.min(q);
            fhi = fhi.max(q);
        }
    }
    let (mut glo, mut ghi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
    for (c, s) in cells_uv(scene, panel) {
        glo = glo.min(c - s / 2.0);
        ghi = ghi.max(c + s / 2.0);
    }
    let margin = (glo - flo).min_element().min((fhi - ghi).min_element());
    PanelInfo {
        face_m: fhi - flo,
        grid_m: ghi - glo,
        grid_offset_m: (glo + ghi) / 2.0 - (flo + fhi) / 2.0,
        margin_m: margin,
        string_lengths: scene.strings[p.strings.clone()]
            .iter()
            .map(|s| s.cells.len())
            .collect(),
    }
}

/// Categorical colours for strings (the viewer's palette order). Strings
/// beyond eight reuse colours; the numbers on the drawing tell them apart.
const STRING_COLOURS: [&str; 8] = [
    "#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#6250d6", "#e34948",
];

/// The panel's face, cells coloured by string, each string's wiring path
/// from its first cell (dot) to its last, string numbers, and the u/v axes.
/// u points right and v up; the view is from the cell side.
pub fn layout_svg(scene: &Scene, panel: usize) -> String {
    let p = &scene.panels[panel];
    let face = face_uv(scene, panel);
    let cells = cells_uv(scene, panel);
    let (mut lo, mut hi) = (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN));
    for q in face.iter().flatten() {
        lo = lo.min(*q);
        hi = hi.max(*q);
    }
    for (c, s) in &cells {
        lo = lo.min(*c - *s / 2.0);
        hi = hi.max(*c + *s / 2.0);
    }
    if lo.x > hi.x {
        lo = DVec2::splat(-0.5);
        hi = DVec2::splat(0.5);
    }
    // 900 px across the larger side, plus a margin for labels.
    let ext = hi - lo;
    let k = 900.0 / ext.max_element().max(1e-9);
    let pad = 60.0;
    let (w, h) = (ext.x * k + 2.0 * pad, ext.y * k + 2.0 * pad + 40.0);
    let xy = |q: DVec2| (pad + (q.x - lo.x) * k, 40.0 + pad + (hi.y - q.y) * k);

    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}" font-family="system-ui, sans-serif" font-size="12">"#
    );
    let _ = writeln!(s, r##"<rect width="100%" height="100%" fill="#fcfcfb"/>"##);
    let _ = writeln!(
        s,
        r##"<text x="{pad}" y="24" font-size="16" fill="#0b0b0b">{} — {} × {} cells, {} strings · viewed from the cell side · normal ({:.3}, {:.3}, {:.3})</text>"##,
        xml(&p.name),
        p.rows,
        p.cols,
        p.strings.len(),
        p.normal.x,
        p.normal.y,
        p.normal.z
    );
    for t in &face {
        let pts: Vec<String> = t
            .iter()
            .map(|&q| {
                let (x, y) = xy(q);
                format!("{x:.1},{y:.1}")
            })
            .collect();
        let _ = writeln!(
            s,
            r##"<polygon points="{}" fill="#d9d8d2" stroke="#d9d8d2" stroke-width="0.5"/>"##,
            pts.join(" ")
        );
    }
    let first = p.cells.start;
    for (si, st) in scene.strings[p.strings.clone()].iter().enumerate() {
        let col = STRING_COLOURS[si % STRING_COLOURS.len()];
        for &c in &st.cells {
            let (centre, size) = cells[c - first];
            let (x0, y0) = xy(centre + DVec2::new(-size.x, size.y) / 2.0);
            let _ = writeln!(
                s,
                r##"<rect x="{x0:.1}" y="{y0:.1}" width="{:.1}" height="{:.1}" fill="{col}" fill-opacity="0.35" stroke="{col}" stroke-width="0.8"/>"##,
                size.x * k,
                size.y * k
            );
        }
        let path: Vec<String> = st
            .cells
            .iter()
            .map(|&c| {
                let (x, y) = xy(cells[c - first].0);
                format!("{x:.1},{y:.1}")
            })
            .collect();
        let _ = writeln!(
            s,
            r##"<polyline points="{}" fill="none" stroke="{col}" stroke-width="1.5"/>"##,
            path.join(" ")
        );
        if let Some(&c0) = st.cells.first() {
            let (x, y) = xy(cells[c0 - first].0);
            let _ = writeln!(
                s,
                r##"<circle cx="{x:.1}" cy="{y:.1}" r="3.5" fill="{col}"/><text x="{:.1}" y="{:.1}" fill="#0b0b0b" font-size="10">{}</text>"##,
                x + 5.0,
                y - 4.0,
                si + 1
            );
        }
    }
    // Axes from the panel centroid.
    let (ox, oy) = xy(DVec2::ZERO);
    let len = 0.12 * ext.max_element() * k;
    let _ = writeln!(
        s,
        r##"<g stroke="#0b0b0b" stroke-width="2"><line x1="{ox:.1}" y1="{oy:.1}" x2="{:.1}" y2="{oy:.1}"/><line x1="{ox:.1}" y1="{oy:.1}" x2="{ox:.1}" y2="{:.1}"/></g><text x="{:.1}" y="{:.1}" fill="#0b0b0b">u ({:.2}, {:.2}, {:.2})</text><text x="{:.1}" y="{:.1}" fill="#0b0b0b">v ({:.2}, {:.2}, {:.2})</text>"##,
        ox + len,
        oy - len,
        ox + len + 4.0,
        oy + 4.0,
        p.u.x,
        p.u.y,
        p.u.z,
        ox + 4.0,
        oy - len - 6.0,
        p.v.x,
        p.v.y,
        p.v.z
    );
    let _ = writeln!(
        s,
        r##"<text x="{pad}" y="{:.0}" fill="#52514e">Grey: panel face. Coloured cells: one colour per string; the line walks each string from its first cell (dot, numbered) to its last. Rows run along v, columns along u.</text>"##,
        h - 16.0
    );
    s.push_str("</svg>\n");
    s
}

fn xml(t: &str) -> String {
    t.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::shapes::ObjWriter;
    use crate::geometry::{Mesh, PanelLayout, SceneConfig, StringLayout, WalkOrder};

    fn scene() -> Scene {
        let mut w = ObjWriter::new();
        w.rect("P", DVec3::ZERO, DVec3::X * 0.3, DVec3::Y * 0.2)
            .cuboid("BOX", DVec3::new(0.0, 0.0, -0.3), DVec3::splat(0.1));
        let mesh = Mesh::from_obj_reader(&mut w.finish().as_bytes(), 1.0).unwrap();
        let panel = PanelLayout {
            name: Some("wing".into()),
            object: "P".into(),
            normal: Some([0.0, 0.0, 1.0]),
            rows: 4,
            cols: 6,
            cell: [0.08, 0.04],
            gap: [0.01, 0.01],
            u_axis: Some([1.0, 0.0, 0.0]),
            offset: [0.0, 0.0],
            strings: Some(StringLayout::Chunked {
                series: 8,
                order: WalkOrder::Rows,
            }),
        };
        let cfg = SceneConfig {
            obj: String::new(),
            scale: 1.0,
            samples: 1,
            panels: vec![panel],
        };
        Scene::build(mesh, &cfg).unwrap()
    }

    #[test]
    fn reports_face_grid_and_strings() {
        let s = scene();
        let g = groups(&s);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].panels, vec![0]);
        assert!(g[1].panels.is_empty() && g[1].triangles == 12);
        let i = panel_info(&s, 0);
        assert!((i.face_m - DVec2::new(0.6, 0.4)).length() < 1e-12);
        // 6 × 0.08 + 5 × 0.01 = 0.53; 4 × 0.04 + 3 × 0.01 = 0.19.
        assert!((i.grid_m - DVec2::new(0.53, 0.19)).length() < 1e-12);
        assert!(i.grid_offset_m.length() < 1e-12);
        assert!((i.margin_m - 0.035).abs() < 1e-12);
        assert_eq!(i.string_lengths, vec![8, 8, 8]);
        let svg = layout_svg(&s, 0);
        assert!(svg.starts_with("<svg") && svg.trim_end().ends_with("</svg>"));
        assert_eq!(svg.matches("<polyline").count(), 3);
        assert_eq!(svg.matches("<rect x=").count(), 24);
    }
}
