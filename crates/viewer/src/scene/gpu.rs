//! The 3D pass. Draws into our own 4× MSAA colour + depth targets, resolves
//! into a texture that egui shows as an image (design V2): we own depth and
//! anti-aliasing, and the pass stays independent of egui's.

use bytemuck::{Pod, Zeroable};
use eframe::egui::{self, Color32};
use eframe::egui_wgpu::{self, wgpu};
use glam::{Mat4, Vec3};
use sunlyt_core::Scene;
use wgpu::util::DeviceExt;

use super::mesh::{self, CELL_VERTICES, LineVertex, Vertex};

const SAMPLES: u32 = 4;
const COLOUR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const MAX_LINE_VERTICES: usize = 256;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    sun: [f32; 4],
    eye: [f32; 4],
    structure: [f32; 4],
}

/// Linear RGBA of an sRGB colour (the targets are sRGB).
pub fn linear(c: Color32) -> [f32; 4] {
    egui::Rgba::from(c).to_array()
}

struct Targets {
    size: [u32; 2],
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolve: wgpu::TextureView,
}

impl Targets {
    fn new(device: &wgpu::Device, size: [u32; 2]) -> Self {
        let extent = wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let make = |label, format, samples, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        Self {
            size,
            msaa: make(
                "sunlyt msaa",
                COLOUR_FORMAT,
                SAMPLES,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            ),
            depth: make(
                "sunlyt depth",
                DEPTH_FORMAT,
                SAMPLES,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            ),
            resolve: make(
                "sunlyt resolve",
                COLOUR_FORMAT,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
        }
    }
}

pub struct SceneRenderer {
    structure_pipe: wgpu::RenderPipeline,
    cell_pipe: wgpu::RenderPipeline,
    line_pipe: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    structure_vb: wgpu::Buffer,
    structure_n: u32,
    cell_vb: wgpu::Buffer,
    cell_colour_vb: wgpu::Buffer,
    cell_n: u32,
    line_vb: wgpu::Buffer,
    targets: Targets,
    texture: egui::TextureId,
}

/// Per-frame inputs of the pass.
pub struct Frame<'a> {
    pub view_proj: Mat4,
    pub eye: Vec3,
    pub sun_dir: Vec3,
    pub visible: f32,
    pub structure: Color32,
    pub clear: Color32,
    pub lines: &'a [LineVertex],
}

impl SceneRenderer {
    pub fn new(rs: &egui_wgpu::RenderState, scene: &Scene, cell_lift_m: f64) -> Self {
        let device = &rs.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sunlyt scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sunlyt uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sunlyt uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sunlyt uniforms"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sunlyt scene"),
            bind_group_layouts: &[Some(&bgl)],
            ..Default::default()
        });

        let vertex = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
        };
        let colour = wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![2 => Float32x4],
        };
        let line = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
        };
        let pipe = |label: &str,
                    vs: &str,
                    fs: &str,
                    buffers: &[Option<wgpu::VertexBufferLayout>],
                    topology,
                    cull_mode,
                    depth_bias: i32| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: wgpu::DepthBiasState {
                        constant: depth_bias,
                        slope_scale: if depth_bias != 0 { -1.0 } else { 0.0 },
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOUR_FORMAT,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let tri = wgpu::PrimitiveTopology::TriangleList;
        let structure_pipe = pipe(
            "sunlyt structure",
            "vs_structure",
            "fs_structure",
            &[Some(vertex.clone())],
            tri,
            None,
            0,
        );
        // Cells sit `cell_lift_m` off their substrate; a small negative
        // depth bias keeps them in front at grazing angles too.
        let cell_pipe = pipe(
            "sunlyt cells",
            "vs_cell",
            "fs_flat",
            &[Some(vertex), Some(colour)],
            tri,
            Some(wgpu::Face::Back),
            -2,
        );
        let line_pipe = pipe(
            "sunlyt lines",
            "vs_line",
            "fs_flat",
            &[Some(line)],
            wgpu::PrimitiveTopology::LineList,
            None,
            0,
        );

        let init = |label, contents: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage,
            })
        };
        let s = mesh::structure(scene);
        let c = mesh::cells(scene, cell_lift_m);
        let structure_vb = init(
            "sunlyt structure",
            bytemuck::cast_slice(&s),
            wgpu::BufferUsages::VERTEX,
        );
        let cell_vb = init(
            "sunlyt cells",
            bytemuck::cast_slice(&c),
            wgpu::BufferUsages::VERTEX,
        );
        let cell_colour_vb = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sunlyt cell colours"),
            size: (c.len().max(1) * 16) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let line_vb = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sunlyt lines"),
            size: (MAX_LINE_VERTICES * std::mem::size_of::<LineVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let targets = Targets::new(device, [16, 16]);
        let texture = rs.renderer.write().register_native_texture(
            device,
            &targets.resolve,
            wgpu::FilterMode::Linear,
        );
        Self {
            structure_pipe,
            cell_pipe,
            line_pipe,
            uniforms,
            bind_group,
            structure_vb,
            structure_n: s.len() as u32,
            cell_vb,
            cell_colour_vb,
            cell_n: c.len() as u32,
            line_vb,
            targets,
            texture,
        }
    }

    /// Uploads one colour per cell.
    pub fn set_cell_colours(&self, rs: &egui_wgpu::RenderState, colours: &[Color32]) {
        let mut v = Vec::with_capacity(colours.len() * CELL_VERTICES);
        for &c in colours {
            let l = linear(c);
            v.extend(std::iter::repeat_n(l, CELL_VERTICES));
        }
        if !v.is_empty() {
            rs.queue
                .write_buffer(&self.cell_colour_vb, 0, bytemuck::cast_slice(&v));
        }
    }

    /// Renders at `size` physical pixels; returns the texture to show.
    pub fn render(
        &mut self,
        rs: &egui_wgpu::RenderState,
        size: [u32; 2],
        f: &Frame<'_>,
    ) -> egui::TextureId {
        let size = [size[0].max(1), size[1].max(1)];
        if size != self.targets.size {
            self.targets = Targets::new(&rs.device, size);
            rs.renderer.write().update_egui_texture_from_wgpu_texture(
                &rs.device,
                &self.targets.resolve,
                wgpu::FilterMode::Linear,
                self.texture,
            );
        }
        let u = Uniforms {
            view_proj: f.view_proj.to_cols_array_2d(),
            sun: [f.sun_dir.x, f.sun_dir.y, f.sun_dir.z, f.visible],
            eye: [f.eye.x, f.eye.y, f.eye.z, 1.0],
            structure: linear(f.structure),
        };
        rs.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
        let lines = &f.lines[..f.lines.len().min(MAX_LINE_VERTICES)];
        if !lines.is_empty() {
            rs.queue
                .write_buffer(&self.line_vb, 0, bytemuck::cast_slice(lines));
        }
        let clear = egui::Rgba::from(f.clear);
        let mut enc = rs
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("sunlyt scene"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sunlyt scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.targets.msaa,
                    resolve_target: Some(&self.targets.resolve),
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear.r() as f64,
                            g: clear.g() as f64,
                            b: clear.b() as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_pipeline(&self.structure_pipe);
            pass.set_vertex_buffer(0, self.structure_vb.slice(..));
            pass.draw(0..self.structure_n, 0..1);
            if self.cell_n > 0 {
                pass.set_pipeline(&self.cell_pipe);
                pass.set_vertex_buffer(0, self.cell_vb.slice(..));
                pass.set_vertex_buffer(1, self.cell_colour_vb.slice(..));
                pass.draw(0..self.cell_n, 0..1);
            }
            if !lines.is_empty() {
                pass.set_pipeline(&self.line_pipe);
                pass.set_vertex_buffer(0, self.line_vb.slice(..));
                pass.draw(0..lines.len() as u32, 0..1);
            }
        }
        rs.queue.submit([enc.finish()]);
        self.texture
    }
}
