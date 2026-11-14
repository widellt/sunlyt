//! Array power from cells: at a constant efficiency (slice 3), or through
//! the cell I-V model and string circuit (slice 4). The illumination is the
//! same for both.
//!
//! Effective irradiance on a cell = irradiance · visible disk fraction ·
//! max(0, cos θ) · lit fraction. The eclipse (visible disk) and the
//! self-shadow (lit fraction) are independent and multiply. A partly lit
//! cell makes photocurrent in proportion to its lit area (design D4).
//!
//! Evaluation is split in two so the thermal model (slice 5) can solve the
//! circuit at each panel's temperature without casting rays again:
//! [`ArrayModel::illuminate`] (rays; stateless, parallel over rows) and
//! [`ArrayModel::panel_output`] (electrical, at a given temperature).

use crate::env::{EnvConfig, Sunlight, sunlight};
use crate::geometry::Scene;
use crate::shadow::Illuminator;
use crate::trajectory::Epoch;

use super::circuit::{Circuit, group_irradiance};

#[derive(Debug, Clone, PartialEq)]
pub enum ArrayModel {
    /// Power = absorbed sunlight · efficiency (0–1), cell by cell.
    Constant { efficiency: f64 },
    /// Cells in series strings with bypass and blocking diodes.
    Circuit(Circuit),
}

/// Array output at one instant.
#[derive(Debug, Clone, PartialEq)]
pub struct ArraySample {
    pub sun: Sunlight,
    /// Electrical power per panel, W.
    pub panel_power_w: Vec<f64>,
    /// Fraction of each panel's cell area in sunlight: facing the Sun, not
    /// self-shadowed, and scaled by the visible disk (0 in umbra).
    pub panel_sunlyt: Vec<f64>,
    /// Cell temperature per panel, K: the thermal model's value, or the
    /// circuit's fixed temperature. Empty for constant efficiency without a
    /// thermal model.
    pub panel_temperature_k: Vec<f64>,
    /// Per string (as [`Scene::strings`]), circuit model only: current, A.
    pub string_current_a: Vec<f64>,
    /// Per string, circuit model only: terminal voltage before the
    /// blocking diode, V.
    pub string_voltage_v: Vec<f64>,
    pub power_w: f64,
}

/// Everything the electrical model needs from one instant's geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayIllum {
    pub sun: Sunlight,
    /// max(0, cos θ) per panel.
    pub panel_cos: Vec<f64>,
    /// Geometrically lit cell area per panel (ignoring the eclipse), m².
    pub panel_lit_area_m2: Vec<f64>,
    /// As [`ArraySample::panel_sunlyt`].
    pub panel_sunlyt: Vec<f64>,
    /// Circuit only: cell irradiance groups (W/m², count) of string `s` are
    /// `groups[group_start[s]..group_start[s + 1]]`.
    pub groups: Vec<(f64, u32)>,
    pub group_start: Vec<u32>,
}

impl ArrayModel {
    /// Evaluates the array at `epoch` at the circuit's fixed temperature.
    /// `lit` is scratch space of one entry per cell; on return it holds the
    /// cells' geometric lit fractions, or zeros if the Sun is fully eclipsed
    /// (rays are skipped then).
    pub fn sample(
        &self,
        scene: &Scene,
        illum: &dyn Illuminator,
        epoch: &Epoch,
        env: &EnvConfig,
        lit: &mut [f64],
    ) -> ArraySample {
        let il = self.illuminate(scene, illum, epoch, env, lit);
        self.output(scene, il, None)
    }

    /// The geometric half: sunlight, rays, and per-string irradiance groups.
    pub fn illuminate(
        &self,
        scene: &Scene,
        illum: &dyn Illuminator,
        epoch: &Epoch,
        env: &EnvConfig,
        lit: &mut [f64],
    ) -> ArrayIllum {
        let sun = sunlight(epoch, env);
        if sun.visible > 0.0 {
            illum.lit_fractions(scene, sun.sun_dir_b, lit);
        } else {
            lit.fill(0.0);
        }
        let np = scene.panels.len();
        let mut il = ArrayIllum {
            panel_cos: vec![0.0; np],
            panel_lit_area_m2: vec![0.0; np],
            panel_sunlyt: vec![0.0; np],
            groups: Vec::new(),
            group_start: Vec::new(),
            sun,
        };
        let flux = il.sun.irradiance_w_m2 * il.sun.visible;
        let circuit = matches!(self, ArrayModel::Circuit(_));
        if circuit {
            il.group_start.reserve(scene.strings.len() + 1);
            il.group_start.push(0);
        }
        let (mut g, mut groups) = (Vec::new(), Vec::new());
        for (p, panel) in scene.panels.iter().enumerate() {
            let cos = panel.normal.dot(il.sun.sun_dir_b).max(0.0);
            let (mut lit_area, mut area) = (0.0, 0.0);
            for c in panel.cells.clone() {
                lit_area += lit[c] * scene.cells[c].area_m2;
                area += scene.cells[c].area_m2;
            }
            il.panel_cos[p] = cos;
            il.panel_lit_area_m2[p] = lit_area;
            il.panel_sunlyt[p] = if area > 0.0 {
                lit_area / area * il.sun.visible
            } else {
                0.0
            };
            if circuit {
                for s in panel.strings.clone() {
                    g.clear();
                    g.extend(scene.strings[s].cells.iter().map(|&c| flux * cos * lit[c]));
                    group_irradiance(&mut g, &mut groups);
                    il.groups.extend_from_slice(&groups);
                    il.group_start.push(il.groups.len() as u32);
                }
            }
        }
        il
    }

    /// Electrical output of panel `p`, with its cells at `temperature_k`
    /// (`None`: the circuit's fixed temperature). Writes the panel's string
    /// currents and voltages into `i_str`/`v_str` (indexed as
    /// [`Scene::strings`]; untouched for constant efficiency) and returns
    /// the panel's power, W.
    pub fn panel_output(
        &self,
        scene: &Scene,
        il: &ArrayIllum,
        p: usize,
        temperature_k: Option<f64>,
        i_str: &mut [f64],
        v_str: &mut [f64],
    ) -> f64 {
        match self {
            ArrayModel::Constant { efficiency } => {
                il.sun.irradiance_w_m2
                    * il.sun.visible
                    * efficiency
                    * il.panel_cos[p]
                    * il.panel_lit_area_m2[p]
            }
            ArrayModel::Circuit(c) => {
                let c = match temperature_k {
                    Some(t) => c.with_temperature(t),
                    None => *c,
                };
                // All cells of a panel share the layout's cell size.
                let panel = &scene.panels[p];
                let c = match scene.cells.get(panel.cells.start) {
                    Some(cell) if !panel.cells.is_empty() => c.for_cell_area(cell.area_m2),
                    _ => c,
                };
                let mut power = 0.0;
                for s in scene.panels[p].strings.clone() {
                    let (a, b) = (il.group_start[s] as usize, il.group_start[s + 1] as usize);
                    let op = c.solve(&il.groups[a..b]);
                    i_str[s] = op.current_a;
                    v_str[s] = op.voltage_v;
                    power += op.power_w;
                }
                power
            }
        }
    }

    /// Assembles a sample from an illumination and optional per-panel
    /// temperatures.
    pub fn output(
        &self,
        scene: &Scene,
        il: ArrayIllum,
        panel_temp_k: Option<&[f64]>,
    ) -> ArraySample {
        let ns = match self {
            ArrayModel::Circuit(_) => scene.strings.len(),
            ArrayModel::Constant { .. } => 0,
        };
        let (mut i_str, mut v_str) = (vec![0.0; ns], vec![0.0; ns]);
        let power: Vec<f64> = (0..scene.panels.len())
            .map(|p| {
                let t = panel_temp_k.map(|t| t[p]);
                self.panel_output(scene, &il, p, t, &mut i_str, &mut v_str)
            })
            .collect();
        let panel_temperature_k = match (panel_temp_k, self) {
            (Some(t), _) => t.to_vec(),
            (None, ArrayModel::Circuit(c)) => vec![c.cell.temperature_k; scene.panels.len()],
            (None, ArrayModel::Constant { .. }) => Vec::new(),
        };
        ArraySample {
            power_w: power.iter().sum(),
            sun: il.sun,
            panel_power_w: power,
            panel_sunlyt: il.panel_sunlyt,
            panel_temperature_k,
            string_current_a: i_str,
            string_voltage_v: v_str,
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::{DQuat, DVec3};

    use super::*;
    use crate::geometry::shapes::ObjWriter;
    use crate::geometry::{Mesh, PanelLayout, SceneConfig};
    use crate::math::AU_M;
    use crate::power::FlatPlate;
    use crate::shadow::RayCaster;
    use crate::trajectory::BodyState;

    fn one_panel(normal: [f64; 3]) -> Scene {
        let mut w = ObjWriter::new();
        // 0.6 × 0.4 m panel facing +Z, plus a slab behind it (never in view).
        w.rect("P", DVec3::ZERO, DVec3::X * 0.3, DVec3::Y * 0.2)
            .cuboid(
                "BUS",
                DVec3::new(-0.3, -0.2, -1.0),
                DVec3::new(0.3, 0.2, -0.1),
            );
        let mesh = Mesh::from_obj_reader(&mut w.finish().as_bytes(), 1.0).unwrap();
        let panel = PanelLayout {
            name: None,
            object: "P".into(),
            normal: Some(normal),
            rows: 4,
            cols: 6,
            cell: [0.1, 0.1],
            gap: [0.0, 0.0],
            u_axis: None,
            offset: [0.0, 0.0],
            strings: None,
        };
        let cfg = SceneConfig {
            obj: String::new(),
            scale: 1.0,
            samples: 2,
            panels: vec![panel],
        };
        Scene::build(mesh, &cfg).unwrap()
    }

    fn epoch(sun_b: DVec3, bodies: Vec<BodyState>) -> Epoch {
        Epoch {
            t: 0.0,
            sun_b,
            bodies,
            q_bi: DQuat::IDENTITY,
        }
    }

    #[test]
    fn unshaded_array_equals_flat_plate() {
        let s = one_panel([0.0, 0.0, 1.0]);
        let rc = RayCaster::new(&s);
        let model = ArrayModel::Constant { efficiency: 0.3 };
        let plate = FlatPlate::new(DVec3::Z, 0.24, 0.3);
        let env = EnvConfig::default();
        let mut lit = vec![0.0; s.cells.len()];
        for deg in [0.0, 30.0, 60.0, 85.0] {
            let th = f64::to_radians(deg);
            let e = epoch(DVec3::new(th.sin(), 0.0, th.cos()) * AU_M, vec![]);
            let a = model.sample(&s, &rc, &e, &env, &mut lit);
            let p = plate.sample(&e, &env);
            assert!((a.power_w - p.power_w).abs() <= 1e-12 * p.power_w, "{deg}°");
            assert_eq!(a.panel_sunlyt, vec![1.0]);
        }
    }

    #[test]
    fn eclipse_and_self_shadow_multiply() {
        let s = one_panel([0.0, 0.0, 1.0]);
        let rc = RayCaster::new(&s);
        let model = ArrayModel::Constant { efficiency: 0.3 };
        let env = EnvConfig::default();
        let mut lit = vec![0.0; s.cells.len()];
        let sun_b = DVec3::Z * AU_M;
        // A Moon-sized body whose limb crosses the Sun centre (a curved
        // limb hides a little under half).
        let d = 1.0e8;
        let b = (1_737_400.0f64 / d).asin();
        let dir = DVec3::new(b.sin(), 0.0, b.cos());
        let moon = BodyState {
            index: 0,
            equatorial_radius_m: 1_737_400.0,
            polar_radius_m: 1_737_400.0,
            pole_b: DVec3::Z,
            vec_b: dir * d,
        };
        let half = model.sample(&s, &rc, &epoch(sun_b, vec![moon.clone()]), &env, &mut lit);
        let full = model.sample(&s, &rc, &epoch(sun_b, vec![]), &env, &mut lit);
        assert!(
            half.sun.visible > 0.45 && half.sun.visible < 0.55,
            "{}",
            half.sun.visible
        );
        assert!(
            (half.power_w - full.power_w * half.sun.visible).abs() < 1e-9 * full.power_w,
            "eclipse scales power"
        );
        // Total eclipse: rays are skipped and every output is zero.
        let mut umbra = moon;
        umbra.vec_b = DVec3::Z * d;
        let z = model.sample(&s, &rc, &epoch(sun_b, vec![umbra]), &env, &mut lit);
        assert_eq!((z.power_w, z.panel_sunlyt[0]), (0.0, 0.0));
        assert!(lit.iter().all(|&f| f == 0.0));
    }

    #[test]
    fn panel_facing_away_gives_zero_not_negative() {
        let s = one_panel([0.0, 0.0, 1.0]);
        let rc = RayCaster::new(&s);
        let mut lit = vec![0.0; s.cells.len()];
        let a = ArrayModel::Constant { efficiency: 0.3 }.sample(
            &s,
            &rc,
            &epoch(-DVec3::Z * AU_M, vec![]),
            &EnvConfig::default(),
            &mut lit,
        );
        assert_eq!(a.power_w, 0.0);
    }
}

#[cfg(test)]
mod circuit_tests {
    use std::path::Path;

    use glam::{DQuat, DVec3};

    use super::*;
    use crate::geometry::shapes::ObjWriter;
    use crate::geometry::{Mesh, PanelLayout, SceneConfig};
    use crate::math::AU_M;
    use crate::power::{ElectricalConfig, Regulation};
    use crate::shadow::RayCaster;

    fn circuit(reg: Regulation) -> Circuit {
        let cfg =
            ElectricalConfig::from_scene_str("[electrical]\ncell = \"azur-3g30a-advanced-4x8\"")
                .unwrap()
                .unwrap();
        let (_, mut c) = cfg.build(Path::new(".")).unwrap();
        c.regulation = reg;
        c
    }

    /// 0.8 × 0.4 m panel facing +Z with 10 × 4 cells; a mast-like box can
    /// shade it from the side.
    fn scene(with_post: bool) -> Scene {
        let mut w = ObjWriter::new();
        w.rect("P", DVec3::ZERO, DVec3::X * 0.4, DVec3::Y * 0.2);
        if with_post {
            w.cuboid(
                "POST",
                DVec3::new(0.45, -0.3, 0.0),
                DVec3::new(0.55, 0.3, 0.15),
            );
        }
        let mesh = Mesh::from_obj_reader(&mut w.finish().as_bytes(), 1.0).unwrap();
        let panel = PanelLayout {
            name: None,
            object: "P".into(),
            normal: None,
            rows: 4,
            cols: 10,
            cell: [0.08, 0.1],
            gap: [0.0, 0.0],
            u_axis: Some([1.0, 0.0, 0.0]),
            offset: [0.0, 0.0],
            strings: None,
        };
        let cfg = SceneConfig {
            obj: String::new(),
            scale: 1.0,
            samples: 2,
            panels: vec![panel],
        };
        Scene::build(mesh, &cfg).unwrap()
    }

    fn epoch(sun_b: DVec3) -> Epoch {
        Epoch {
            t: 0.0,
            sun_b,
            bodies: vec![],
            q_bi: DQuat::IDENTITY,
        }
    }

    /// Unshaded: every string sees the same light, so the array is
    /// (strings) × one string solved on its own, bit for bit.
    #[test]
    fn unshaded_array_is_strings_times_one_string() {
        let s = scene(false);
        let rc = RayCaster::new(&s);
        let env = EnvConfig::default();
        let mut lit = vec![0.0; s.cells.len()];
        for reg in [Regulation::Mppt, Regulation::FixedBus { bus_v: 20.0 }] {
            let c = circuit(reg);
            let th = 30f64.to_radians();
            let e = epoch(DVec3::new(th.sin(), 0.0, th.cos()) * AU_M);
            let a = ArrayModel::Circuit(c).sample(&s, &rc, &e, &env, &mut lit);
            let g = a.sun.irradiance_w_m2 * a.sun.visible * th.cos();
            // 8 × 10 cm layout cells: the string is the datasheet cell scaled up.
            let one = c.for_cell_area(s.cells[0].area_m2).solve(&[(g, 10)]);
            assert_eq!(a.string_current_a, vec![one.current_a; 4]);
            assert!((a.power_w - 4.0 * one.power_w).abs() <= 1e-12 * a.power_w);
            assert!(a.power_w > 0.0);
        }
    }

    /// A post shades the end of every row-string. With bypass diodes the
    /// fixed-bus array loses more than the shaded area (the string current
    /// is set by the shaded cells or they are bypassed), but never more
    /// than the whole string, and never goes negative.
    #[test]
    fn partial_shade_costs_at_least_its_area() {
        let (lit_s, open) = (scene(true), scene(false));
        let env = EnvConfig::default();
        let c = ArrayModel::Circuit(circuit(Regulation::Mppt));
        // Sun low from +X: the post's shadow falls across the +X end.
        let e = epoch(DVec3::new(1.0, 0.0, 0.8).normalize() * AU_M);
        let mut lit = vec![0.0; lit_s.cells.len()];
        let shaded = c.sample(&lit_s, &RayCaster::new(&lit_s), &e, &env, &mut lit);
        let frac = 1.0 - shaded.panel_sunlyt[0];
        let clear = c.sample(&open, &RayCaster::new(&open), &e, &env, &mut lit);
        let loss = 1.0 - shaded.power_w / clear.power_w;
        println!(
            "shaded area {:.1} %, power loss {:.1} % (ideal MPPT)",
            100.0 * frac,
            100.0 * loss
        );
        assert!(frac > 0.05 && frac < 0.5, "{frac}");
        assert!(loss >= frac - 1e-9 && loss < 1.0, "{loss} vs {frac}");
        assert!(shaded.string_current_a.iter().all(|&i| i >= 0.0));
    }

    #[test]
    fn umbra_gives_zero_current() {
        let s = scene(false);
        let rc = RayCaster::new(&s);
        let mut lit = vec![0.0; s.cells.len()];
        let moon = crate::trajectory::BodyState {
            index: 0,
            equatorial_radius_m: 1_737_400.0,
            polar_radius_m: 1_737_400.0,
            pole_b: DVec3::Z,
            vec_b: DVec3::Z * 1e7,
        };
        let e = Epoch {
            bodies: vec![moon],
            ..epoch(DVec3::Z * AU_M)
        };
        let a = ArrayModel::Circuit(circuit(Regulation::FixedBus { bus_v: 28.0 })).sample(
            &s,
            &rc,
            &e,
            &EnvConfig::default(),
            &mut lit,
        );
        assert_eq!(a.power_w, 0.0);
        assert!(a.string_current_a.iter().all(|&i| i == 0.0));
    }
}
