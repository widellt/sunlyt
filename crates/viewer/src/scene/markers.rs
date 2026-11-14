//! Sun, Earth and Moon direction markers and the body axes, as 3D lines
//! plus screen-space labels.

use eframe::egui::Color32;
use glam::Vec3;

use super::gpu::linear;
use super::mesh::LineVertex;
use crate::ui::theme::Theme;
use crate::worker::Snapshot;

pub struct Label {
    pub at: Vec3,
    pub text: String,
    pub colour: Color32,
}

pub struct Markers {
    pub lines: Vec<LineVertex>,
    pub labels: Vec<Label>,
}

pub fn markers(
    snap: &Snapshot,
    body_names: &[String],
    centre: Vec3,
    radius: f32,
    theme: &Theme,
) -> Markers {
    let mut m = Markers {
        lines: Vec::new(),
        labels: Vec::new(),
    };
    let line = |m: &mut Markers, from: Vec3, to: Vec3, colour: Color32| {
        let c = linear(colour);
        m.lines.push(LineVertex {
            pos: from.to_array(),
            colour: c,
        });
        m.lines.push(LineVertex {
            pos: to.to_array(),
            colour: c,
        });
    };

    for (k, (axis, name)) in [(Vec3::X, "+X"), (Vec3::Y, "+Y"), (Vec3::Z, "+Z")]
        .into_iter()
        .enumerate()
    {
        let end = axis * radius * 0.5;
        line(&mut m, Vec3::ZERO, end, theme.axes[k]);
        m.labels.push(Label {
            at: end * 1.08,
            text: name.into(),
            colour: theme.axes[k],
        });
    }

    let sun = &snap.sample.sun;
    let dir = sun.sun_dir_b.as_vec3().normalize_or_zero();
    // Faded toward the muted ink by the hidden fraction of the disk: an
    // exact function of the instant's value, not an animation.
    let v = sun.visible as f32;
    let sun_colour = theme.text_muted.lerp_to_gamma(theme.sun, v);
    let end = centre + dir * radius * 1.7;
    line(&mut m, centre, end, sun_colour);
    m.labels.push(Label {
        at: end,
        text: if v < 1.0 {
            format!("Sun ({:.1} % visible)", 100.0 * v)
        } else {
            "Sun".into()
        },
        colour: sun_colour,
    });

    for (k, b) in snap.epoch.bodies.iter().enumerate() {
        let name = body_names.get(k).map_or("body", String::as_str);
        let colour = match name {
            "Earth" => theme.earth,
            "Moon" => theme.moon,
            _ => theme.text_muted,
        };
        let d = b.vec_b.as_vec3().normalize_or_zero();
        let end = centre + d * radius * 1.35;
        line(&mut m, centre, end, colour);
        m.labels.push(Label {
            at: end,
            text: format!("{name} · {:.0} km", b.vec_b.length() / 1000.0),
            colour,
        });
    }
    m
}
