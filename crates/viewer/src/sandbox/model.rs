//! The sandbox's state and its evaluation at one instant (no UI).
//!
//! A sandbox is one panel in a [`Scene`] of its own, so its shadows and
//! power go through the same code as a full run (`Scene::build`, the ray
//! caster, `ArrayModel`). Alongside that it computes the independent
//! checks of S2: exact polygon-clipped shadows, and the per-string I-V
//! curves the operating points must lie on.

use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use glam::{DQuat, DVec3};
use sunlyt_core::geometry::shapes::ObjWriter;
use sunlyt_core::geometry::{Mesh, SceneConfig};
use sunlyt_core::power::StringPoint;
use sunlyt_core::shadow::exact::{Poly2, exact_lit_fractions, shadow_polygons};
use sunlyt_core::{
    AU_M, ArrayModel, ArraySample, BodyState, ElectricalConfig, EnvConfig, Epoch, ExactClipper,
    Illuminator, RayCaster, Scene, StateTable,
};

use super::config::{PANEL_OBJECT, SandboxConfig};

/// Where the Sun (and any eclipsing body) comes from.
pub enum Source {
    /// Set by hand: `sandbox.toml` and the UI's sliders.
    Fixed {
        /// Unit Sun direction, sandbox (= body) frame.
        sun_dir: DVec3,
        sun_dist_m: f64,
        bodies: Vec<BodyState>,
        body_names: Vec<String>,
    },
    /// One instant of a trajectory ("from scene" mode).
    Trajectory { table: StateTable, t: f64 },
}

pub struct Sandbox {
    /// One panel (index 0); everything else in the mesh is an occluder.
    pub scene: Scene,
    pub model: ArrayModel,
    pub rays: RayCaster,
    pub env: EnvConfig,
    pub source: Source,
    /// The sandbox TOML text (box mode), for Save.
    pub text: Option<String>,
    /// Shown in the title bar.
    pub label: String,
}

impl Sandbox {
    /// Box mode, from `sandbox.toml` text. `base_dir` resolves a `.toml`
    /// cell file.
    pub fn from_toml(text: &str, base_dir: &Path, label: String) -> Result<Self> {
        let cfg = SandboxConfig::from_str(text).map_err(|e| anyhow!("sandbox: {e}"))?;
        let mut w = ObjWriter::new();
        let [su, sv] = cfg.size;
        w.rect(
            PANEL_OBJECT,
            DVec3::ZERO,
            DVec3::X * (su / 2.0),
            DVec3::Y * (sv / 2.0),
        );
        for (i, b) in cfg.boxes.iter().enumerate() {
            let rot = DQuat::from_rotation_z(b.rotation_deg.to_radians());
            w.oriented_box(
                &format!("BOX_{i}"),
                DVec3::from_array(b.centre),
                DVec3::from_array(b.size) / 2.0,
                [rot * DVec3::X, rot * DVec3::Y, DVec3::Z],
            );
        }
        let mesh = Mesh::from_obj_reader(&mut w.finish().as_bytes(), 1.0)?;
        let scene = Scene::build(
            mesh,
            &SceneConfig {
                obj: String::new(),
                scale: 1.0,
                samples: cfg.samples,
                panels: vec![cfg.panel.clone().expect("parsed")],
            },
        )?;
        let model = build_model(ElectricalConfig::from_scene_str(text)?, base_dir)?;
        let sun_dir = match cfg.sun.vector {
            Some(v) => {
                let v = DVec3::from_array(v);
                if v.length() == 0.0 {
                    bail!("sun.vector must be non-zero");
                }
                v.normalize()
            }
            None => sun_from_az_el(cfg.sun.azimuth_deg, cfg.sun.elevation_deg),
        };
        let bodies = cfg
            .bodies
            .iter()
            .enumerate()
            .map(|(index, b)| {
                let r = b.radius_km * 1e3;
                BodyState {
                    index,
                    equatorial_radius_m: r,
                    polar_radius_m: r,
                    pole_b: DVec3::Z,
                    vec_b: DVec3::from_array(b.direction).normalize() * b.distance_km * 1e3,
                }
            })
            .collect();
        Ok(Self {
            rays: RayCaster::new(&scene),
            scene,
            model,
            env: EnvConfig::default(),
            source: Source::Fixed {
                sun_dir,
                sun_dist_m: cfg.sun.distance_au * AU_M,
                bodies,
                body_names: cfg.bodies.iter().map(|b| b.name.clone()).collect(),
            },
            text: Some(text.to_owned()),
            label,
        })
    }

    /// From-scene mode: panel `panel` (its name or OBJ object) of the scene
    /// at `scene_path`, with every other triangle as an occluder, lit by
    /// `table` at `t`. Cells are at `[electrical]`'s fixed temperature.
    pub fn from_scene(
        scene_path: &Path,
        panel: &str,
        table: StateTable,
        env: EnvConfig,
        t: f64,
    ) -> Result<Self> {
        let (mut cfg, mesh) = Scene::load_config(scene_path)
            .with_context(|| format!("loading scene {}", scene_path.display()))?;
        let names: Vec<String> = cfg
            .panels
            .iter()
            .map(|p| p.name.clone().unwrap_or_else(|| p.object.clone()))
            .collect();
        let k = names
            .iter()
            .position(|n| n == panel)
            .or_else(|| cfg.panels.iter().position(|p| p.object == panel))
            .ok_or_else(|| anyhow!("no panel {panel:?} in the scene (panels: {names:?})"))?;
        cfg.panels = vec![cfg.panels.swap_remove(k)];
        let scene = Scene::build(mesh, &cfg)?;
        let dir = scene_path.parent().unwrap_or(Path::new("."));
        let model = build_model(ElectricalConfig::from_scene_file(scene_path)?, dir)?;
        let label = format!("{} — {}", names[k], scene_path.display());
        Ok(Self {
            rays: RayCaster::new(&scene),
            scene,
            model,
            env,
            source: Source::Trajectory { table, t },
            text: None,
            label,
        })
    }

    /// The geometry at the current instant.
    pub fn epoch(&self) -> Epoch {
        match &self.source {
            Source::Fixed {
                sun_dir,
                sun_dist_m,
                bodies,
                ..
            } => Epoch {
                t: 0.0,
                sun_b: *sun_dir * *sun_dist_m,
                bodies: bodies.clone(),
                q_bi: DQuat::IDENTITY,
            },
            Source::Trajectory { table, t } => {
                table.sample(*t).expect("sandbox time is inside the table")
            }
        }
    }

    pub fn body_names(&self) -> Vec<String> {
        match &self.source {
            Source::Fixed { body_names, .. } => body_names.clone(),
            Source::Trajectory { table, .. } => table.body_names().map(str::to_owned).collect(),
        }
    }

    /// Everything the window shows, at the current instant.
    pub fn evaluate(&self) -> Evaluation {
        let epoch = self.epoch();
        let n = self.scene.cells.len();
        let (mut scratch_ray, mut scratch_exact) = (vec![0.0; n], vec![0.0; n]);
        let il =
            self.model
                .illuminate(&self.scene, &self.rays, &epoch, &self.env, &mut scratch_ray);
        let il_exact = self.model.illuminate(
            &self.scene,
            &ExactClipper,
            &epoch,
            &self.env,
            &mut scratch_exact,
        );
        // Shadows are geometry: show them even inside an eclipse.
        let sun_dir = il.sun.sun_dir_b;
        let shadows = shadow_polygons(&self.scene, 0, sun_dir);
        let geo_exact = exact_lit_fractions(&self.scene, 0, sun_dir);
        let mut geo_ray = vec![0.0; n];
        self.rays.lit_fractions(&self.scene, sun_dir, &mut geo_ray);
        let mut curves: Vec<StringCurve> = Vec::new();
        if let ArrayModel::Circuit(c) = &self.model {
            let c = c.for_cell_area(self.scene.cells[0].area_m2);
            for s in 0..self.scene.strings.len() {
                let (a, b) = (il.group_start[s] as usize, il.group_start[s + 1] as usize);
                let groups = &il.groups[a..b];
                match curves.iter_mut().find(|k| k.groups == groups) {
                    Some(k) => k.strings.push(s),
                    None => curves.push(StringCurve {
                        strings: vec![s],
                        groups: groups.to_vec(),
                        points: c.string_curve(groups, 400),
                        op: c.solve(groups),
                    }),
                }
            }
        }
        let ray = self.model.output(&self.scene, il, None);
        let exact = self.model.output(&self.scene, il_exact, None);
        Evaluation {
            sun_dir,
            epoch,
            lit_ray: geo_ray,
            lit_exact: geo_exact,
            shadows,
            ray,
            exact,
            curves,
        }
    }
}

/// One instant in the sandbox.
pub struct Evaluation {
    pub sun_dir: DVec3,
    pub epoch: Epoch,
    /// Geometric lit fraction per cell, rays (what the power uses), even
    /// inside an eclipse.
    pub lit_ray: Vec<f64>,
    /// Geometric lit fraction per cell by exact clipping (always computed).
    pub lit_exact: Vec<f64>,
    /// Shadow polygons on the panel, panel-plane coordinates.
    pub shadows: Vec<Poly2>,
    /// The power path on ray-cast shadows: the full simulation's numbers.
    pub ray: ArraySample,
    /// The same on exact shadows.
    pub exact: ArraySample,
    /// Distinct string curves (strings under identical light share one).
    pub curves: Vec<StringCurve>,
}

/// The I-V curve shared by one or more identically lit strings.
pub struct StringCurve {
    /// Indices into `Scene::strings`.
    pub strings: Vec<usize>,
    pub groups: Vec<(f64, u32)>,
    /// (current A, string voltage V), current ascending.
    pub points: Vec<(f64, f64)>,
    pub op: StringPoint,
}

/// Unit vector at `az` (from +x towards +y) and `el` (above the xy plane).
pub fn sun_from_az_el(az_deg: f64, el_deg: f64) -> DVec3 {
    let (a, e) = (az_deg.to_radians(), el_deg.to_radians());
    DVec3::new(e.cos() * a.cos(), e.cos() * a.sin(), e.sin())
}

/// Azimuth and elevation, degrees, of a unit vector.
pub fn az_el(d: DVec3) -> (f64, f64) {
    let az = d.y.atan2(d.x).to_degrees().rem_euclid(360.0);
    let el = d.z.atan2(d.x.hypot(d.y)).to_degrees();
    (az, el)
}

/// Angular radius (rad) and unit direction of a body as seen from the
/// spacecraft, for drawing (an oblate body is drawn as its equatorial
/// circle).
pub fn body_disk(b: &BodyState) -> (f64, DVec3) {
    let d = b.vec_b.length();
    let r = if d <= b.equatorial_radius_m {
        std::f64::consts::FRAC_PI_2
    } else {
        (b.equatorial_radius_m / d).asin()
    };
    (r, b.vec_b / d)
}

fn build_model(cfg: Option<ElectricalConfig>, base_dir: &Path) -> Result<ArrayModel> {
    Ok(match cfg {
        Some(cfg) => ArrayModel::Circuit(cfg.build(base_dir).context("[electrical]")?.1),
        None => ArrayModel::Constant { efficiency: 0.3 },
    })
}
