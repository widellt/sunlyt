use glam::DVec3;

use super::shapes::ObjWriter;
use super::*;

fn mesh(w: &ObjWriter) -> Mesh {
    Mesh::from_obj_reader(&mut w.clone().finish().as_bytes(), 1.0).unwrap()
}

fn layout(object: &str, rows: u32, cols: u32, cell: [f64; 2]) -> PanelLayout {
    PanelLayout {
        name: None,
        object: object.into(),
        normal: None,
        rows,
        cols,
        cell,
        gap: [0.0, 0.0],
        u_axis: None,
        offset: [0.0, 0.0],
        strings: None,
    }
}

fn config(panels: Vec<PanelLayout>, samples: u32) -> SceneConfig {
    SceneConfig {
        obj: String::new(),
        scale: 1.0,
        samples,
        panels,
    }
}

#[test]
fn obj_objects_and_triangles() {
    let mut w = ObjWriter::new();
    w.cuboid("BUS", DVec3::splat(-0.5), DVec3::splat(0.5)).rect(
        "WING",
        DVec3::new(0.0, 2.0, 0.0),
        DVec3::X,
        DVec3::Y,
    );
    let m = mesh(&w);
    assert_eq!(m.objects, ["BUS", "WING"]);
    assert_eq!(m.triangles.len(), 12 + 2);
    // Cuboid faces point outward: total area-weighted normal is zero and
    // each face normal points away from the centre.
    for t in 0..12 {
        let [a, b, c] = m.tri(t);
        assert!(
            m.tri_cross(t).dot((a + b + c) / 3.0) > 0.0,
            "tri {t} faces inward"
        );
    }
}

#[test]
fn grid_on_a_planar_panel() {
    let mut w = ObjWriter::new();
    // 1 m × 0.5 m panel at z = 1 facing +Z.
    w.rect("P", DVec3::Z, DVec3::X * 0.5, DVec3::Y * 0.25);
    let mut l = layout("P", 4, 8, [0.12, 0.1]);
    l.gap = [0.005, 0.01];
    let s = Scene::build(mesh(&w), &config(vec![l], 3)).unwrap();
    assert_eq!(s.cells.len(), 32);
    assert_eq!(s.sample_points.len(), 32 * 9);
    let p = &s.panels[0];
    assert!((p.normal - DVec3::Z).length() < 1e-12);
    assert!((p.u - DVec3::X).length() < 1e-12 && (p.v - DVec3::Y).length() < 1e-12);
    assert!((s.cell_area_m2() - 32.0 * 0.012).abs() < 1e-12);
    // Grid is centred: mean of cell centres is the face centroid.
    let mean = s.cells.iter().map(|c| c.centre).sum::<DVec3>() / 32.0;
    assert!((mean - DVec3::Z).length() < 1e-12);
    // Samples sit on the cell, just above the surface.
    for (i, c) in s.cells.iter().enumerate() {
        for p in s.samples(i) {
            let d = *p - c.centre;
            assert!(d.x.abs() < 0.06 && d.y.abs() < 0.05);
            assert!((d.z - s.ray_offset_m).abs() < 1e-15);
        }
    }
}

#[test]
fn grid_past_the_edge_is_rejected() {
    let mut w = ObjWriter::new();
    w.rect("P", DVec3::ZERO, DVec3::X * 0.5, DVec3::Y * 0.5);
    let err = Scene::build(mesh(&w), &config(vec![layout("P", 1, 11, [0.1, 0.1])], 1))
        .unwrap_err()
        .to_string();
    assert!(err.contains("past the substrate edge"), "{err}");
    // Exactly filling the face is fine.
    Scene::build(mesh(&w), &config(vec![layout("P", 10, 10, [0.1, 0.1])], 1)).unwrap();
}

#[test]
fn slab_needs_normal_to_pick_its_side() {
    let mut w = ObjWriter::new();
    w.cuboid(
        "SLAB",
        DVec3::new(-0.5, -0.5, 0.0),
        DVec3::new(0.5, 0.5, 0.02),
    );
    let err = Scene::build(mesh(&w), &config(vec![layout("SLAB", 2, 2, [0.4, 0.4])], 1))
        .unwrap_err()
        .to_string();
    assert!(err.contains("not planar"), "{err}");
    let mut l = layout("SLAB", 2, 2, [0.4, 0.4]);
    l.normal = Some([0.0, 0.0, -1.0]);
    let s = Scene::build(mesh(&w), &config(vec![l], 1)).unwrap();
    assert!((s.panels[0].normal + DVec3::Z).length() < 1e-12);
    assert!(s.cells.iter().all(|c| c.centre.z.abs() < 1e-12));
}

#[test]
fn unknown_object_and_bad_layout() {
    let mut w = ObjWriter::new();
    w.rect("P", DVec3::ZERO, DVec3::X, DVec3::Y);
    let e = Scene::build(mesh(&w), &config(vec![layout("Q", 1, 1, [0.1, 0.1])], 1));
    assert!(matches!(e, Err(GeometryError::UnknownObject(_))));
    let e = Scene::build(mesh(&w), &config(vec![layout("P", 0, 1, [0.1, 0.1])], 1));
    assert!(matches!(e, Err(GeometryError::Layout { .. })));
    let mut l = layout("P", 1, 1, [0.1, 0.1]);
    l.u_axis = Some([0.0, 0.0, 1.0]);
    assert!(Scene::build(mesh(&w), &config(vec![l], 1)).is_err());
}

#[test]
fn config_parses_from_toml() {
    let cfg: SceneConfig = toml::from_str(
        r#"
        obj = "craft.obj"
        samples = 3
        [[panel]]
        object = "WING"
        normal = [0, 1, 0]
        rows = 2
        cols = 3
        cell = [0.08, 0.04]
        gap = [0.002, 0.002]
        "#,
    )
    .unwrap();
    assert_eq!(cfg.scale, 1.0);
    assert_eq!(cfg.panels[0].normal, Some([0.0, 1.0, 0.0]));
    assert!(toml::from_str::<SceneConfig>("obj='x'\nbogus=1\npanel=[]").is_err());
}

fn wired(rows: u32, cols: u32, strings: Option<StringLayout>) -> Result<Scene, GeometryError> {
    let mut w = ObjWriter::new();
    w.rect("P", DVec3::ZERO, DVec3::X, DVec3::Y);
    let mut l = layout("P", rows, cols, [0.1, 0.1]);
    l.strings = strings;
    Scene::build(mesh(&w), &config(vec![l], 1))
}

/// Default wiring: one string per row. Chunked wiring walks serpentine and
/// cuts into strings; explicit wiring is taken as given. Either way every
/// cell is in exactly one string.
#[test]
fn strings_cover_every_cell_once() {
    let s = wired(3, 4, None).unwrap();
    assert_eq!(s.panels[0].strings, 0..3);
    assert_eq!(s.strings[1].cells, vec![7, 6, 5, 4]); // serpentine

    let s = wired(
        3,
        4,
        Some(StringLayout::Chunked {
            series: 6,
            order: WalkOrder::Rows,
        }),
    )
    .unwrap();
    assert_eq!(s.strings.len(), 2);
    assert_eq!(s.strings[0].cells, vec![0, 1, 2, 3, 7, 6]); // row 1 runs backwards
    assert_eq!(s.strings[1].cells, vec![5, 4, 8, 9, 10, 11]);

    let s = wired(
        2,
        3,
        Some(StringLayout::Chunked {
            series: 2,
            order: WalkOrder::Columns,
        }),
    )
    .unwrap();
    let cells: Vec<Vec<usize>> = s.strings.iter().map(|x| x.cells.clone()).collect();
    assert_eq!(cells, vec![vec![0, 3], vec![4, 1], vec![2, 5]]);

    let s = wired(
        1,
        3,
        Some(StringLayout::Explicit(vec![
            vec![[0, 2], [0, 0]],
            vec![[0, 1]],
        ])),
    )
    .unwrap();
    assert_eq!(s.strings[0].cells, vec![2, 0]);
    for s in [
        wired(5, 6, None).unwrap(),
        wired(
            5,
            6,
            Some(StringLayout::Chunked {
                series: 10,
                order: WalkOrder::Columns,
            }),
        )
        .unwrap(),
    ] {
        let mut all: Vec<usize> = s.strings.iter().flat_map(|x| x.cells.clone()).collect();
        all.sort();
        assert_eq!(all, (0..30).collect::<Vec<_>>());
    }
}

#[test]
fn bad_wiring_is_rejected_with_the_cell_named() {
    let err = |s| wired(2, 3, Some(s)).unwrap_err().to_string();
    assert!(
        err(StringLayout::Chunked {
            series: 4,
            order: WalkOrder::Rows
        })
        .contains("does not divide")
    );
    assert!(
        err(StringLayout::Explicit(vec![
            vec![[0, 0], [0, 1], [0, 2]],
            vec![[1, 0], [1, 1]]
        ]))
        .contains("[1, 2] is in no string")
    );
    assert!(
        err(StringLayout::Explicit(vec![vec![[0, 0], [0, 0]]])).contains("more than one string")
    );
    assert!(err(StringLayout::Explicit(vec![vec![[2, 0]]])).contains("outside the grid"));
}

#[test]
fn strings_parse_from_toml_and_electrical_is_ignored_by_the_scene() {
    let chunked: PanelLayout = toml::from_str(
        "object = \"P\"\nrows = 2\ncols = 2\ncell = [0.1, 0.1]\nstrings = { series = 2, order = \"columns\" }",
    )
    .unwrap();
    assert_eq!(
        chunked.strings,
        Some(StringLayout::Chunked {
            series: 2,
            order: WalkOrder::Columns
        })
    );
    let explicit: PanelLayout = toml::from_str(
        "object = \"P\"\nrows = 1\ncols = 2\ncell = [0.1, 0.1]\nstrings = [[[0, 1], [0, 0]]]",
    )
    .unwrap();
    assert_eq!(
        explicit.strings,
        Some(StringLayout::Explicit(vec![vec![[0, 1], [0, 0]]]))
    );

    let dir = std::env::temp_dir().join(format!("sunlyt-scene-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut w = ObjWriter::new();
    w.rect("P", DVec3::ZERO, DVec3::X, DVec3::Y);
    std::fs::write(dir.join("p.obj"), w.finish()).unwrap();
    std::fs::write(
        dir.join("scene.toml"),
        "obj = \"p.obj\"\n[[panel]]\nobject = \"P\"\nrows = 2\ncols = 2\ncell = [0.1, 0.1]\n\
         [electrical]\ncell = \"azur-3g30a-advanced-4x8\"\n",
    )
    .unwrap();
    let s = Scene::load(&dir.join("scene.toml")).unwrap();
    assert_eq!(s.strings.len(), 2);
    std::fs::remove_dir_all(&dir).ok();
}
