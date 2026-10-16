//! Native-resolution G-buffer, material lighting, SSAO and bounded directional shadows.
use super::Rendering;
use crate::{engine_blocks::Registry, engine_mesh::Vertex, scene::Result};
use glam::{Mat4, Vec3};
use serde::Serialize;
use wgpu::util::DeviceExt;
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Serialize)]
pub struct Frame {
    pub inverse: [f32; 16],
    pub light: [f32; 16],
    pub eye: [f32; 4],
    pub sun: [f32; 4],
    pub parameters: [f32; 4],
    pub sampling: [f32; 4],
    pub fog: [f32; 4],
    pub medium: [f32; 4],
}
impl Frame {
    pub fn new(
        matrix: Mat4,
        eye: [f32; 3],
        settings: &Rendering,
        fov: u32,
        wire: bool,
        fog: [f32; 4],
        medium: [f32; 4],
    ) -> Self {
        let sun = Vec3::new(0.45, 0.82, 0.35).normalize();
        let radius = settings.quality.radius();
        let center = Vec3::from(eye);
        let right = sun.cross(Vec3::Y).normalize();
        let up = right.cross(sun).normalize();
        let texel = radius * 2. / settings.quality.shadow_size() as f32;
        let center = center
            + right * ((center.dot(right) / texel).round() * texel - center.dot(right))
            + up * ((center.dot(up) / texel).round() * texel - center.dot(up));
        let light = Mat4::orthographic_rh(-radius, radius, -radius, radius, 0.1, 400.)
            * Mat4::look_at_rh(center + sun * 160., center, Vec3::Y);
        Self {
            inverse: matrix.inverse().to_cols_array(),
            light: light.to_cols_array(),
            eye: [eye[0], eye[1], eye[2], 1.],
            sun: [sun.x, sun.y, sun.z, 3.2],
            parameters: [
                settings.exposure as f32 / 100.,
                f32::from(settings.ambient_occlusion),
                f32::from(settings.shadows),
                f32::from(wire),
            ],
            sampling: [
                radius,
                settings.quality.shadow_size() as f32,
                settings.quality.samples() as f32,
                (fov as f32).to_radians().mul_add(0.5, 0.).tan(),
            ],
            fog,
            medium,
        }
    }
}
fn texture(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
struct Targets {
    albedo: wgpu::Texture,
    normal: wgpu::Texture,
    depth: wgpu::Texture,
    shadow: wgpu::Texture,
    pub albedo_view: wgpu::TextureView,
    pub normal_view: wgpu::TextureView,
    pub depth_view: wgpu::TextureView,
    pub shadow_view: wgpu::TextureView,
    size: [u32; 2],
    shadow_size: u32,
}
impl Targets {
    fn new(device: &wgpu::Device, size: [u32; 2], shadow_size: u32) -> Self {
        let albedo = texture(
            device,
            "linear surface color",
            size[0],
            size[1],
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let normal = texture(
            device,
            "world normal and material ID",
            size[0],
            size[1],
            wgpu::TextureFormat::Rgba16Float,
        );
        let depth = texture(
            device,
            "G-buffer primary depth",
            size[0],
            size[1],
            wgpu::TextureFormat::Depth32Float,
        );
        let shadow = texture(
            device,
            "directional shadow depth",
            shadow_size,
            shadow_size,
            wgpu::TextureFormat::Depth32Float,
        );
        let albedo_view = albedo.create_view(&Default::default());
        let normal_view = normal.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let shadow_view = shadow.create_view(&Default::default());
        Self {
            albedo,
            normal,
            depth,
            shadow,
            albedo_view,
            normal_view,
            depth_view,
            shadow_view,
            size,
            shadow_size,
        }
    }
}
pub struct Renderer {
    pub geometry: wgpu::RenderPipeline,
    pub previous: wgpu::RenderPipeline,
    pub shadow_pipeline: wgpu::RenderPipeline,
    pub frame_bind: wgpu::BindGroup,
    pub frame: Frame,
    uniform: wgpu::Buffer,
    materials: wgpu::Buffer,
    material_key: Option<(u64, u64)>,
    pub material_uploads: usize,
    lighting: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    targets: Targets,
    pub allocations: usize,
    pub frames: usize,
}
fn geometry_pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    previous: bool,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{label:Some("deferred surface geometry"),layout:Some(layout),vertex:wgpu::VertexState{module,entry_point:Some(if previous{"vs_previous"}else{"vs"}),compilation_options:Default::default(),buffers:&[wgpu::VertexBufferLayout{array_stride:std::mem::size_of::<Vertex>()as u64,step_mode:wgpu::VertexStepMode::Vertex,attributes:&wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Float32,4=>Float32x3]}]},primitive:wgpu::PrimitiveState{cull_mode:Some(wgpu::Face::Back),..Default::default()},depth_stencil:Some(wgpu::DepthStencilState{format:wgpu::TextureFormat::Depth32Float,depth_write_enabled:true,depth_compare:wgpu::CompareFunction::Less,stencil:Default::default(),bias:Default::default()}),multisample:Default::default(),fragment:Some(wgpu::FragmentState{module,entry_point:Some("fs_gbuffer"),compilation_options:Default::default(),targets:&[Some(wgpu::ColorTargetState{format:wgpu::TextureFormat::Rgba8Unorm,blend:None,write_mask:wgpu::ColorWrites::ALL}),Some(wgpu::ColorTargetState{format:wgpu::TextureFormat::Rgba16Float,blend:None,write_mask:wgpu::ColorWrites::ALL})]}),multiview:None,cache:None})
}
fn bind(
    device: &wgpu::Device,
    pipeline: &wgpu::RenderPipeline,
    t: &Targets,
    uniform: &wgpu::Buffer,
    materials: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("deferred inputs"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&t.albedo_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&t.normal_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&t.depth_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&t.shadow_view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: materials.as_entire_binding(),
            },
        ],
    })
}
impl Renderer {
    pub fn new(
        device: &wgpu::Device,
        world_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        settings: &Rendering,
    ) -> Result<Self> {
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("deferred G-buffer"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/world.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shared G-buffer terrain inputs"),
            bind_group_layouts: &[world_layout],
            push_constant_ranges: &[],
        });
        let geometry = geometry_pipeline(device, &shader, &layout, false);
        let previous = geometry_pipeline(device, &shader, &layout, true);
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lighting frame"),
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
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("deferred frame parameters"),
            contents: bytemuck::bytes_of(&Frame::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let frame_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow frame"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let materials = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("registered PBR materials"),
            contents: bytemuck::cast_slice(&[[0.82f32, 0., 0., 0.]; 256]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cutout directional shadows"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}",
                    include_str!("../shaders/lighting_frame.wgsl"),
                    include_str!("../shaders/shadow.wgsl")
                )
                .into(),
            ),
        });
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow terrain and frame"),
            bind_group_layouts: &[world_layout, &frame_layout],
            push_constant_ranges: &[],
        });
        let shadow_pipeline=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{label:Some("sun shadow map"),layout:Some(&shadow_layout),vertex:wgpu::VertexState{module:&shadow_shader,entry_point:Some("vs"),compilation_options:Default::default(),buffers:&[wgpu::VertexBufferLayout{array_stride:std::mem::size_of::<Vertex>()as u64,step_mode:wgpu::VertexStepMode::Vertex,attributes:&wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Float32,4=>Float32x3]}]},primitive:wgpu::PrimitiveState{cull_mode:Some(wgpu::Face::Back),..Default::default()},depth_stencil:Some(wgpu::DepthStencilState{format:wgpu::TextureFormat::Depth32Float,depth_write_enabled:true,depth_compare:wgpu::CompareFunction::Less,stencil:Default::default(),bias:wgpu::DepthBiasState{constant:2,slope_scale:2.,clamp:0.}}),multisample:Default::default(),fragment:Some(wgpu::FragmentState{module:&shadow_shader,entry_point:Some("fs"),compilation_options:Default::default(),targets:&[]}),multiview:None,cache:None});
        let lighting_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("deferred metallic roughness lighting"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}",
                    include_str!("../shaders/lighting_frame.wgsl"),
                    include_str!("../shaders/deferred.wgsl")
                )
                .into(),
            ),
        });
        let lighting = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("deferred full-screen lighting"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &lighting_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &lighting_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let targets = Targets::new(
            device,
            size,
            if settings.shadows {
                settings.quality.shadow_size()
            } else {
                1
            },
        );
        let bind = bind(device, &lighting, &targets, &uniform, &materials);
        if let Some(e) = pollster::block_on(device.pop_error_scope()) {
            return Err(format!("Deferred renderer validation: {e}").into());
        }
        Ok(Self {
            geometry,
            previous,
            shadow_pipeline,
            frame_bind,
            frame: Frame::zeroed(),
            uniform,
            materials,
            material_key: None,
            material_uploads: 0,
            lighting,
            bind,
            targets,
            allocations: 1,
            frames: 0,
        })
    }
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: [u32; 2],
        settings: &Rendering,
        frame: Frame,
        state: &crate::engine::WorldState,
    ) {
        let registry = &state.registry;
        let key = (state.instance, state.registry_revision);
        let shadow_size = if settings.shadows {
            settings.quality.shadow_size()
        } else {
            1
        };
        if self.targets.size != size || self.targets.shadow_size != shadow_size {
            self.targets = Targets::new(device, size, shadow_size);
            self.bind = bind(
                device,
                &self.lighting,
                &self.targets,
                &self.uniform,
                &self.materials,
            );
            self.allocations += 1;
        }
        if self.material_key != Some(key) {
            let mut rows = [[0.82f32, 0., 0., 0.]; 256];
            for (i, b) in registry.blocks.iter().enumerate() {
                rows[i] = b.material.row();
            }
            queue.write_buffer(&self.materials, 0, bytemuck::cast_slice(&rows));
            self.material_key = Some(key);
            self.material_uploads += 1;
        }
        self.frame = frame;
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&self.frame));
        self.frames += 1;
    }
    pub fn depth(&self) -> &wgpu::TextureView {
        &self.targets.depth_view
    }
    pub fn begin_geometry<'a>(
        &'a self,
        encoder: &'a mut wgpu::CommandEncoder,
    ) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("deferred color normal depth G-buffer"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: &self.targets.albedo_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &self.targets.normal_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                }),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.targets.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }
    pub fn begin_shadow<'a>(
        &'a self,
        encoder: &'a mut wgpu::CommandEncoder,
    ) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("directional occluders"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.targets.shadow_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }
    pub fn light(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("deferred lighting and ambient occlusion"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.lighting);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.draw(0..3, 0..1);
    }
    pub fn bytes(&self) -> u64 {
        u64::from(self.targets.size[0]) * u64::from(self.targets.size[1]) * 16
            + u64::from(self.targets.shadow_size).pow(2) * 4
            + 4096
            + std::mem::size_of::<Frame>() as u64
    }
    pub fn observation(&self) -> serde_json::Value {
        serde_json::json!({"method":"deferred","formats":["rgba8unorm","rgba16float","depth32float"],"size":self.targets.size,"shadow_size":self.targets.shadow_size,"extra_bytes":self.bytes(),"allocations":self.allocations,"material_uploads":self.material_uploads,"frames":self.frames,"frame":self.frame,"genuine_ray_tracing":false})
    }
}
use bytemuck::Zeroable;

impl Renderer {
    pub fn capture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        path: &std::path::Path,
        registry: &Registry,
    ) -> Result<()> {
        for (name, texture, size, bytes, aspect) in [
            (
                "gbuffer-albedo.bin",
                &self.targets.albedo,
                self.targets.size,
                4,
                wgpu::TextureAspect::All,
            ),
            (
                "gbuffer-normal.bin",
                &self.targets.normal,
                self.targets.size,
                8,
                wgpu::TextureAspect::All,
            ),
            (
                "gbuffer-depth.bin",
                &self.targets.depth,
                self.targets.size,
                4,
                wgpu::TextureAspect::DepthOnly,
            ),
            (
                "shadow-depth.bin",
                &self.targets.shadow,
                [self.targets.shadow_size; 2],
                4,
                wgpu::TextureAspect::DepthOnly,
            ),
        ] {
            let stride = (size[0] * bytes).div_ceil(256) * 256;
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("lighting proof readback"),
                size: u64::from(stride) * u64::from(size[1]),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("G-buffer proof capture"),
            });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(stride),
                        rows_per_image: Some(size[1]),
                    },
                },
                wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
            );
            queue.submit(Some(encoder.finish()));
            let (tx, rx) = std::sync::mpsc::channel();
            buffer.slice(..).map_async(wgpu::MapMode::Read, move |v| {
                let _ = tx.send(v);
            });
            device.poll(wgpu::PollType::Wait)?;
            rx.recv()??;
            let mapped = buffer.slice(..).get_mapped_range();
            let mut packed = Vec::with_capacity((size[0] * size[1] * bytes) as usize);
            for row in mapped.chunks_exact(stride as usize) {
                packed.extend_from_slice(&row[..(size[0] * bytes) as usize]);
            }
            std::fs::write(path.join(name), packed)?;
            drop(mapped);
            buffer.unmap();
        }
        std::fs::write(
            path.join("lighting.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"frame":self.frame,"materials":registry.blocks.iter().map(|b|b.material.row()).collect::<Vec<_>>(),"size":self.targets.size,"shadow_size":self.targets.shadow_size}),
            )?,
        )?;
        Ok(())
    }
}
