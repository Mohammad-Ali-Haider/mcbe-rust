//! Instanced cuboid rendering with distance/frustum culling and reusable buffers.
use super::{World, components::*};
use crate::{engine_mesh::Vertex, scene::Client};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;
pub const VISIBLE_LIMIT: usize = 128;
pub const PART_LIMIT: usize = VISIBLE_LIMIT * 32;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable, serde::Serialize)]
pub struct Instance {
    pub model: [[f32; 4]; 4],
    pub metadata: [u32; 4],
}
#[derive(Default, serde::Serialize)]
pub struct Statistics {
    pub visible: usize,
    pub range_culled: usize,
    pub frustum_culled: usize,
    pub budget_culled: usize,
    pub instances: usize,
    pub visible_parts: usize,
    pub uploads: usize,
    pub draws: usize,
    pub shadow_draws: usize,
    pub bytes: u64,
}
pub struct Frame {
    pub matrix: Mat4,
    pub eye: [f32; 3],
    pub blend: f32,
    pub selected: Option<Id>,
    pub hidden: bool,
}
#[derive(PartialEq)]
struct Key {
    instance: u64,
    registry: u64,
    terrain: u64,
    revision: u64,
    matrix: [f32; 16],
    blend: f32,
    selected: Option<Id>,
    hidden: bool,
}
pub struct Renderer {
    key: Option<Key>,
    forward: wgpu::RenderPipeline,
    geometry: wgpu::RenderPipeline,
    shadow: Option<wgpu::RenderPipeline>,
    world_layout: wgpu::BindGroupLayout,
    cube: wgpu::Buffer,
    buffer: wgpu::Buffer,
    pub instances: Vec<Instance>,
    pending: Vec<Instance>,
    pub stats: Statistics,
}
const FACES: [[[f32; 3]; 4]; 6] = [
    [[0., 1., 0.], [0., 1., 1.], [1., 1., 1.], [1., 1., 0.]],
    [[0., 0., 1.], [0., 0., 0.], [1., 0., 0.], [1., 0., 1.]],
    [[1., 0., 0.], [0., 0., 0.], [0., 1., 0.], [1., 1., 0.]],
    [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
    [[1., 0., 1.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1.]],
    [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
];
pub fn cube() -> Vec<Vertex> {
    let mut out = Vec::new();
    for (face, corners) in FACES.into_iter().enumerate() {
        for (n, i) in [0, 1, 2, 0, 2, 3].into_iter().enumerate() {
            let mut bary = [0.; 3];
            bary[n % 3] = 1.;
            out.push(Vertex {
                position: corners[i],
                uv: [[0., 1.], [1., 1.], [1., 0.], [0., 0.]][i],
                layer: face as u32,
                shade: 1.,
                barycentric: bary,
            });
        }
    }
    out
}
pub fn visible(center: [f32; 3], radius: f32, m: Mat4) -> bool {
    let rows = [m.row(0), m.row(1), m.row(2), m.row(3)];
    [
        rows[3] + rows[0],
        rows[3] - rows[0],
        rows[3] + rows[1],
        rows[3] - rows[1],
        rows[2],
        rows[3] - rows[2],
    ]
    .into_iter()
    .all(|p| p.truncate().dot(Vec3::from(center)) + p.w >= -radius * p.truncate().length())
}
const CUBE_ATTRIBUTES: [wgpu::VertexAttribute; 5] =
    wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Float32,4=>Float32x3];
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 5] =
    wgpu::vertex_attr_array![5=>Float32x4,6=>Float32x4,7=>Float32x4,8=>Float32x4,9=>Uint32x4];
fn layouts() -> [wgpu::VertexBufferLayout<'static>; 2] {
    [
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &CUBE_ATTRIBUTES,
        },
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRIBUTES,
        },
    ]
}
fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    targets: &[Option<wgpu::ColorTargetState>],
    shadow: bool,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("instanced entity parts"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(if shadow {
                "vs_entity_shadow"
            } else {
                "vs_entity"
            }),
            compilation_options: Default::default(),
            buffers: &layouts(),
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: Default::default(),
            bias: if shadow {
                wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.,
                    clamp: 0.,
                }
            } else {
                Default::default()
            },
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(if shadow {
                "fs"
            } else if targets.len() == 2 {
                "fs_gbuffer"
            } else {
                "fs"
            }),
            compilation_options: Default::default(),
            targets,
        }),
        multiview: None,
        cache: None,
    })
}
pub fn shader() -> String {
    format!(
        "{}\n{}",
        include_str!("../render/shaders/world.wgsl"),
        include_str!("../render/shaders/entities.wgsl")
    )
}
impl Renderer {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        world_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("entity instance vertices"),
            source: wgpu::ShaderSource::Wgsl(shader().into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("entity terrain atlas"),
            bind_group_layouts: &[world_layout],
            push_constant_ranges: &[],
        });
        let color = |format| {
            Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })
        };
        let forward = pipeline(device, &layout, &shader, &[color(format)], false);
        let geometry = pipeline(
            device,
            &layout,
            &shader,
            &[
                color(wgpu::TextureFormat::Rgba8Unorm),
                color(wgpu::TextureFormat::Rgba16Float),
            ],
            false,
        );
        let bytes = (PART_LIMIT * std::mem::size_of::<Instance>()) as u64;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bounded entity instance transforms"),
            size: bytes,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cube = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("shared entity unit cube"),
            contents: bytemuck::cast_slice(&cube()),
            usage: wgpu::BufferUsages::VERTEX,
        });
        Self {
            key: None,
            forward,
            geometry,
            shadow: None,
            world_layout: world_layout.clone(),
            cube,
            buffer,
            instances: Vec::new(),
            pending: Vec::new(),
            stats: Statistics {
                bytes: bytes + (36 * std::mem::size_of::<Vertex>()) as u64,
                ..Default::default()
            },
        }
    }
    pub fn ensure_shadow(&mut self, device: &wgpu::Device, frame: &wgpu::BindGroupLayout) {
        if self.shadow.is_some() || self.instances.is_empty() {
            return;
        }
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("entity directional shadows"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}",
                    include_str!("../render/shaders/lighting_frame.wgsl"),
                    include_str!("../render/shaders/shadow.wgsl"),
                    include_str!("../render/shaders/entities_shadow.wgsl")
                )
                .into(),
            ),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("entity shadow frame"),
            bind_group_layouts: &[&self.world_layout, frame],
            push_constant_ranges: &[],
        });
        self.shadow = Some(pipeline(device, &layout, &shader, &[], true));
    }
    pub fn prepare(&mut self, queue: &wgpu::Queue, c: &Client, frame: Frame) {
        let Frame {
            matrix,
            eye,
            blend,
            selected,
            hidden,
        } = frame;
        let state = c.scene.engine.as_ref().unwrap();
        let world = &state.entities;
        let key = Key {
            instance: state.instance,
            registry: state.registry_revision,
            terrain: state.revision,
            revision: world.stats.revision,
            matrix: matrix.to_cols_array(),
            blend: if world.stats.moving > 0 { blend } else { 0. },
            selected,
            hidden,
        };
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.key = Some(key);
        self.pending.clear();
        self.stats.visible = 0;
        self.stats.range_culled = 0;
        self.stats.frustum_culled = 0;
        self.stats.budget_culled = 0;
        self.stats.visible_parts = 0;
        let w = &c.scene.engine.as_ref().unwrap().entities;
        if !hidden {
            let mut candidates = w
                .records
                .values()
                .filter(|e| e.visual.is_some())
                .filter_map(|e| {
                    let d = distance(e.transform.position, eye);
                    if d > 96.
                        || !c
                            .scene
                            .inside(e.transform.position.map(|v| v.floor() as i32))
                    {
                        self.stats.range_culled += 1;
                        None
                    } else {
                        Some((d, e.id))
                    }
                })
                .collect::<Vec<_>>();
            candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            self.stats.budget_culled = candidates.len().saturating_sub(VISIBLE_LIMIT);
            candidates.truncate(VISIBLE_LIMIT);
            let mut views = Vec::new();
            for (_, id) in candidates {
                let e = &w.records[&id];
                let kind = w.library.kinds[&e.kind];
                let radius = w.library.radii[kind];
                let view = visible(e.transform.position, radius, matrix);
                if view {
                    self.stats.visible += 1
                } else {
                    self.stats.frustum_culled += 1
                }
                views.push((!view, id));
            }
            views.sort_by_key(|v| v.0);
            for (hidden, id) in views {
                let e = &w.records[&id];
                let kind = w.library.kinds[&e.kind];
                let d = &w.library.definitions.kinds[kind];
                let position = std::array::from_fn::<_, 3, _>(|a| {
                    e.transform.previous[a]
                        + (e.transform.position[a] - e.transform.previous[a]) * blend.clamp(0., 1.)
                });
                let pose = super::model::Pose::new(e, position);
                for (part, material) in d.model.iter().zip(&w.library.materials[kind]) {
                    let model = pose.part(part);
                    let flags =
                        u32::from(e.health.as_ref().is_some_and(|h| w.clock < h.hurt_until))
                            | (u32::from(selected == Some(id)) * 2);
                    self.pending.push(Instance {
                        model: model.to_cols_array_2d(),
                        metadata: [
                            u32::from(*material) * 6,
                            flags,
                            id.0 as u32,
                            (id.0 >> 32) as u32,
                        ],
                    });
                }
                if !hidden {
                    self.stats.visible_parts = self.pending.len();
                }
            }
        }
        if self.pending != self.instances {
            std::mem::swap(&mut self.instances, &mut self.pending);
            if !self.instances.is_empty() {
                queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&self.instances));
            }
            self.stats.uploads += 1;
        }
        self.stats.instances = self.instances.len();
    }
    pub fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        world: &'a wgpu::BindGroup,
        deferred: bool,
    ) {
        if self.stats.visible_parts == 0 {
            return;
        }
        pass.set_pipeline(if deferred {
            &self.geometry
        } else {
            &self.forward
        });
        pass.set_bind_group(0, world, &[]);
        pass.set_vertex_buffer(0, self.cube.slice(..));
        pass.set_vertex_buffer(1, self.buffer.slice(..));
        pass.draw(0..36, 0..self.stats.visible_parts as u32);
    }
    pub fn draw_shadow<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        world: &'a wgpu::BindGroup,
        frame: &'a wgpu::BindGroup,
    ) {
        if self.instances.is_empty() {
            return;
        }
        let Some(pipeline) = &self.shadow else { return };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, world, &[]);
        pass.set_bind_group(1, frame, &[]);
        pass.set_vertex_buffer(0, self.cube.slice(..));
        pass.set_vertex_buffer(1, self.buffer.slice(..));
        pass.draw(0..36, 0..self.instances.len() as u32);
    }
    pub fn observation(&self, w: &World) -> serde_json::Value {
        serde_json::json!({"world":w.observation(),"render":self.stats,"instances":self.instances,"unit_cube_vertices":36,"back_face_culling":true,"depth_testing":true,"shared_cube":true})
    }
}
