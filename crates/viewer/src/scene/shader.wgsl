// sunlyt viewer: structure (lit), cells (flat data colour), lines.

struct Uniforms {
    view_proj: mat4x4<f32>,
    // xyz: unit Sun direction (body frame); w: visible solar disk 0..1.
    sun: vec4<f32>,
    // xyz: camera position.
    eye: vec4<f32>,
    // Structure colour, linear RGB.
    structure: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) world: vec3<f32>,
    @location(2) colour: vec4<f32>,
};

@vertex
fn vs_structure(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>) -> VOut {
    var o: VOut;
    o.clip = u.view_proj * vec4<f32>(pos, 1.0);
    o.normal = normal;
    o.world = pos;
    o.colour = u.structure;
    return o;
}

// Two-sided: a headlight for shape plus the real Sun, dimmed by the
// eclipse, so umbra and penumbra also show on the structure.
@fragment
fn fs_structure(i: VOut) -> @location(0) vec4<f32> {
    let v = normalize(u.eye.xyz - i.world);
    var n = normalize(i.normal);
    if (dot(n, v) < 0.0) {
        n = -n;
    }
    let head = max(dot(n, v), 0.0);
    let sun = max(dot(n, u.sun.xyz), 0.0) * u.sun.w;
    let k = 0.18 + 0.32 * head + 0.5 * sun;
    return vec4<f32>(i.colour.rgb * k, 1.0);
}

@vertex
fn vs_cell(
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) colour: vec4<f32>,
) -> VOut {
    var o: VOut;
    o.clip = u.view_proj * vec4<f32>(pos, 1.0);
    o.normal = normal;
    o.world = pos;
    o.colour = colour;
    return o;
}

@vertex
fn vs_line(@location(0) pos: vec3<f32>, @location(1) colour: vec4<f32>) -> VOut {
    var o: VOut;
    o.clip = u.view_proj * vec4<f32>(pos, 1.0);
    o.normal = vec3<f32>(0.0, 0.0, 1.0);
    o.world = pos;
    o.colour = colour;
    return o;
}

// Cells and lines: the colour is data (it must match the legend), so no
// lighting.
@fragment
fn fs_flat(i: VOut) -> @location(0) vec4<f32> {
    return i.colour;
}
