//! Native-resolution GPU terrain: persistent exposed-face meshes and batched HUD.
use crate::{
    engine_gpu_ui::{Atlas, Context},
    engine_mesh::{self, Vertex},
    engine_ui::Palette,
    scene::{Client, Result},
};
use glam::{Mat4, Vec3};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
    time::Instant,
};
use wgpu::util::DeviceExt;
use winit::window::Window;
struct Mesh {
    buffer: Option<wgpu::Buffer>,
    vertices: u32,
    exposed_faces: usize,
    occluded_faces: usize,
    block_count: usize,
    fingerprint: Option<[u8; 32]>,
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Camera {
    matrix: [f32; 16],
    eye: [f32; 4],
    settings: [f32; 4],
    focus: [f32; 4],
}
#[derive(PartialEq, Eq)]
struct UiKey {
    startup: bool,
    movement: crate::engine_player::Movement,
    size: [u32; 2],
    scale: u32,
    active: usize,
    player_revision: u64,
    palette_revision: u64,
    cursor: usize,
    palette: bool,
    wire: bool,
    debug: bool,
    notice: u64,
    bucket: u128,
    greedy: bool,
    loading: bool,
}
#[derive(Serialize)]
pub struct Statistics {
    pub preview_uploads: usize,
    pub preview_vertices: usize,
    pub preview_draws: usize,
    pub texture_stage_frames: usize,
    pub texture_stage_bytes: usize,
    pub texture_stage_writes: usize,
    pub max_texture_stage_bytes: usize,
    pub max_texture_stage_writes: usize,
    pub max_texture_stage_ms: f64,
    pub texture_stages_completed: usize,
    pub texture_stages_cancelled: usize,
    pub ui_uploads: usize,
    pub texture_uploads: usize,
    pub range_culled_chunks: usize,
    pub peak_chunks: usize,
    pub peak_mesh_bytes: usize,
    pub adapter: String,
    pub backend: String,
    pub present_mode: String,
    pub width: u32,
    pub height: u32,
    pub frames: usize,
    pub warmup_frames: usize,
    pub chunks: usize,
    pub visible_chunks: usize,
    pub frustum_culled_chunks: usize,
    pub exposed_faces: usize,
    pub omitted_internal_faces: usize,
    pub submitted_vertices: usize,
    pub greedy_meshing: bool,
    pub mesh_quads: usize,
    pub merged_faces: usize,
    pub mesh_bytes: usize,
    pub async_meshing: bool,
    pub mesh_jobs_in_flight: usize,
    pub mesh_pending_chunks: usize,
    pub mesh_peak_jobs: usize,
    pub mesh_discarded_jobs: usize,
    pub mesh_worker_jobs: usize,
    pub mesh_main_builds: usize,
    pub mesh_main_ms: f64,
    pub mesh_snapshot_ms: f64,
    pub max_mesh_snapshot_ms: f64,
    pub meshes_ready: bool,
    pub mesh_covered_blocks: usize,
    pub mesh_build_ms: f64,
    pub max_chunk_build_ms: f64,
    pub mesh_rebuilds: usize,
    pub native_resolution: bool,
    pub gui_scale: f32,
    pub last_frame_ms: f64,
}
pub struct Frame<'a> {
    pub startup: bool,
    pub wire: bool,
    pub debug: bool,
    pub notice: Option<&'a str>,
    pub eye: [f32; 3],
    pub capture: Option<&'a Path>,
}
struct Upload {
    prepared: crate::engine_texture::Prepared,
    terrain: wgpu::Texture,
    ui: wgpu::Texture,
    cursor: usize,
    staged: bool,
}
pub struct Renderer {
    preview_pipeline: wgpu::RenderPipeline,
    preview_buffer: wgpu::Buffer,
    preview_mesh: Option<crate::engine_preview::Mesh>,
    ui_preview_at: Option<u32>,
    uploading: Option<Upload>,
    mesher: crate::engine_meshing::Scheduler,
    async_requested: bool,
    pub collect_timings: bool,
    mesh_mode_dirty: bool,
    visible_keys: Vec<[i32; 3]>,
    ui_key: Option<UiKey>,
    ui_vertices: u32,
    ui_epoch: Instant,
    _instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    world_pipeline: wgpu::RenderPipeline,
    ui_pipeline: wgpu::RenderPipeline,
    depth: wgpu::TextureView,
    uniform: wgpu::Buffer,
    world_bind: Option<wgpu::BindGroup>,
    ui_bind: Option<wgpu::BindGroup>,
    ui_buffer: wgpu::Buffer,
    atlas: Option<Atlas>,
    meshes: BTreeMap<[i32; 3], Mesh>,
    world_instance: u64,
    revision: u64,
    registry_revision: u64,
    pub stats: Statistics,
    pub frame_times: Vec<f64>,
    previous: Instant,
}
fn depth(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}
impl Renderer {
    pub fn new(window: Arc<Window>) -> Result<Self> {
        pollster::block_on(Self::initialize(window))
    }
    async fn initialize(window: Arc<Window>) -> Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            ..Default::default()
        });
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await?;
        let info = adapter.get_info();
        let limits = wgpu::Limits {
            max_texture_array_layers: adapter.limits().max_texture_array_layers.min(1536),
            ..Default::default()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Blockwork GPU"),
                required_features: wgpu::Features::empty(),
                required_limits: limits,
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("No compatible GPU surface")?;
        let caps = surface.get_capabilities(&adapter);
        config.format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        config.present_mode = wgpu::PresentMode::AutoNoVsync;
        config.desired_maximum_frame_latency = 2;
        if caps.usages.contains(wgpu::TextureUsages::COPY_SRC) {
            config.usage |= wgpu::TextureUsages::COPY_SRC;
        }
        surface.configure(&device, &config);
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let world_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terrain shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("engine_world.wgsl").into()),
        });
        let ui_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("HUD shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("engine_overlay.wgsl").into()),
        });
        let world_pipeline=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{
            label:Some("culled terrain"),layout:None,vertex:wgpu::VertexState{module:&world_shader,entry_point:Some("vs"),compilation_options:Default::default(),buffers:&[wgpu::VertexBufferLayout{array_stride:std::mem::size_of::<Vertex>()as u64,step_mode:wgpu::VertexStepMode::Vertex,attributes:&wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Float32,4=>Float32x3]}]},
            primitive:wgpu::PrimitiveState{cull_mode:Some(wgpu::Face::Back),..Default::default()},depth_stencil:Some(wgpu::DepthStencilState{format:wgpu::TextureFormat::Depth32Float,depth_write_enabled:true,depth_compare:wgpu::CompareFunction::Less,stencil:Default::default(),bias:Default::default()}),multisample:Default::default(),fragment:Some(wgpu::FragmentState{module:&world_shader,entry_point:Some("fs"),compilation_options:Default::default(),targets:&[Some(wgpu::ColorTargetState{format:config.format,blend:None,write_mask:wgpu::ColorWrites::ALL})]}),multiview:None,cache:None,
        });
        let ui_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shared HUD and preview atlas"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let ui_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shared HUD and preview layout"),
            bind_group_layouts: &[&ui_layout],
            push_constant_ranges: &[],
        });
        let ui_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("batched native-size HUD"),
            layout: Some(&ui_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &ui_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<crate::engine_gpu_ui::Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x2,1=>Float32x2,2=>Float32x4],
                }],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &ui_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let preview_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("inventory player shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("engine_preview.wgsl").into()),
        });
        let preview_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("depth-tested inventory player"),
            layout: Some(&ui_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &preview_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<crate::engine_preview::Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Float32x4],
                }],
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
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &preview_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let preview_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bounded inventory player mesh"),
            size: (32 * 36 * std::mem::size_of::<crate::engine_preview::Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if let Some(error) = device.pop_error_scope().await {
            return Err(format!("GPU pipeline validation: {error}").into());
        }
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: 112,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ui_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("HUD batch"),
            size: 65536 * std::mem::size_of::<crate::engine_gpu_ui::Vertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let depth = depth(&device, config.width, config.height);
        let stats = Statistics {
            preview_uploads: 0,
            preview_vertices: 0,
            preview_draws: 0,
            texture_stage_frames: 0,
            texture_stage_bytes: 0,
            texture_stage_writes: 0,
            max_texture_stage_bytes: 0,
            max_texture_stage_writes: 0,
            max_texture_stage_ms: 0.,
            texture_stages_completed: 0,
            texture_stages_cancelled: 0,
            ui_uploads: 0,
            texture_uploads: 0,
            range_culled_chunks: 0,
            peak_chunks: 0,
            peak_mesh_bytes: 0,
            adapter: info.name,
            backend: format!("{:?}", info.backend),
            present_mode: format!("{:?}", config.present_mode),
            width: config.width,
            height: config.height,
            frames: 0,
            warmup_frames: 10,
            chunks: 0,
            visible_chunks: 0,
            frustum_culled_chunks: 0,
            exposed_faces: 0,
            omitted_internal_faces: 0,
            submitted_vertices: 0,
            greedy_meshing: true,
            mesh_quads: 0,
            merged_faces: 0,
            mesh_bytes: 0,
            async_meshing: false,
            mesh_jobs_in_flight: 0,
            mesh_pending_chunks: 0,
            mesh_peak_jobs: 0,
            mesh_discarded_jobs: 0,
            mesh_worker_jobs: 0,
            mesh_main_builds: 0,
            mesh_main_ms: 0.,
            mesh_snapshot_ms: 0.,
            max_mesh_snapshot_ms: 0.,
            meshes_ready: false,
            mesh_covered_blocks: 0,
            mesh_build_ms: 0.,
            max_chunk_build_ms: 0.,
            mesh_rebuilds: 0,
            native_resolution: true,
            gui_scale: 2.,
            last_frame_ms: 0.,
        };
        Ok(Self {
            preview_pipeline,
            preview_buffer,
            preview_mesh: None,
            ui_preview_at: None,
            uploading: None,
            mesher: crate::engine_meshing::Scheduler::new()?,
            async_requested: true,
            collect_timings: false,
            mesh_mode_dirty: false,
            visible_keys: Vec::new(),
            ui_key: None,
            ui_vertices: 0,
            ui_epoch: Instant::now(),
            _instance: instance,
            surface,
            device,
            queue,
            config,
            world_pipeline,
            ui_pipeline,
            depth,
            uniform,
            world_bind: None,
            ui_bind: None,
            ui_buffer,
            atlas: None,
            meshes: BTreeMap::new(),
            world_instance: 0,
            revision: 0,
            registry_revision: 0,
            stats,
            frame_times: Vec::new(),
            previous: Instant::now(),
        })
    }
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth = depth(&self.device, width, height);
        self.stats.width = width;
        self.stats.height = height;
    }
    pub fn set_async(&mut self, enabled: bool) {
        if self.async_requested != enabled {
            self.async_requested = enabled;
            self.mesh_mode_dirty = true;
        }
    }
    pub fn set_greedy(&mut self, greedy: bool) {
        if self.stats.greedy_meshing != greedy {
            self.stats.greedy_meshing = greedy;
            self.mesh_mode_dirty = true;
        }
    }
    pub fn logical_size(&self) -> (f64, f64) {
        (
            f64::from(self.config.width) / f64::from(self.stats.gui_scale),
            f64::from(self.config.height) / f64::from(self.stats.gui_scale),
        )
    }
    pub fn texture_pending(&self) -> bool {
        self.uploading.is_some()
    }
    pub fn preview_pose(&self) -> Option<[f32; 2]> {
        self.preview_mesh.as_ref().map(|m| m.head_angles)
    }
    pub fn preview_observation(&self) -> serde_json::Value {
        self.preview_mesh.as_ref().map_or(serde_json::Value::Null,|m|serde_json::json!({"rectangle":m.rectangle,"head_angles":m.head_angles,"vertices":m.vertices,"skin":self.atlas.as_ref().and_then(|a|a.preview_skin()),"depth_test":true,"alpha_test":true,"scissored":true,"foreground_after_preview":true}))
    }
    pub fn cancel_texture_upload(&mut self) {
        if self.uploading.take().is_some_and(|p| p.staged) {
            self.stats.texture_stages_cancelled += 1;
        }
    }
    fn textures(&mut self, client: &Client) -> Result<bool> {
        use crate::engine_texture::{Prepared, Step};
        let started = Instant::now();
        if self
            .uploading
            .as_ref()
            .is_some_and(|p| !p.prepared.matches(client))
        {
            self.cancel_texture_upload();
        }
        if self.uploading.is_none() {
            let owned = client.engine_gpu_prepared.borrow_mut().take();
            let staged = owned.as_ref().is_some_and(|p| p.matches(client));
            let prepared = if let Some(p) = owned.filter(|p| p.matches(client)) {
                p
            } else {
                Prepared::build(client)?
            };
            if prepared.layers > self.device.limits().max_texture_array_layers {
                return Err("GPU texture-array limit exceeded by registry".into());
            }
            let terrain = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Minecraft terrain layers"),
                size: wgpu::Extent3d {
                    width: 32,
                    height: 32,
                    depth_or_array_layers: prepared.layers,
                },
                mip_level_count: 6,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let ui = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("local HUD sprite atlas"),
                size: wgpu::Extent3d {
                    width: prepared.atlas.size,
                    height: prepared.atlas.size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.uploading = Some(Upload {
                prepared,
                terrain,
                ui,
                cursor: 0,
                staged,
            });
        }
        let pending = self.uploading.as_mut().unwrap();
        let staged = pending.staged;
        let batch = if staged {
            pending.prepared.batch(pending.cursor)
        } else {
            pending.cursor..pending.prepared.steps.len()
        };
        if batch.is_empty() {
            return Err("Texture copy cannot fit the upload budget".into());
        }
        let writes = batch.len();
        let mut bytes = 0;
        for i in batch.clone() {
            let step = &pending.prepared.steps[i];
            let payload = step.bytes(&pending.prepared.atlas);
            bytes += payload.len();
            let (texture, mip, origin, width, height) = match step {
                Step::Terrain {
                    layer, mip, width, ..
                } => (
                    &pending.terrain,
                    *mip,
                    wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: *layer,
                    },
                    *width,
                    *width,
                ),
                Step::Ui { row, rows } => (
                    &pending.ui,
                    0,
                    wgpu::Origin3d {
                        x: 0,
                        y: *row,
                        z: 0,
                    },
                    pending.prepared.atlas.size,
                    *rows,
                ),
            };
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: mip,
                    origin,
                    aspect: wgpu::TextureAspect::All,
                },
                payload,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
        pending.cursor = batch.end;
        if staged {
            self.stats.texture_stage_frames += 1;
            self.stats.texture_stage_writes += writes;
            self.stats.texture_stage_bytes += bytes;
            self.stats.max_texture_stage_bytes = self.stats.max_texture_stage_bytes.max(bytes);
            self.stats.max_texture_stage_writes = self.stats.max_texture_stage_writes.max(writes);
        }
        if pending.cursor < pending.prepared.steps.len() {
            self.stats.max_texture_stage_ms = self
                .stats
                .max_texture_stage_ms
                .max(started.elapsed().as_secs_f64() * 1000.);
            return Ok(false);
        }
        let pending = self.uploading.take().unwrap();
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pixel textures with mipmaps"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        self.world_bind = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain binding"),
            layout: &self.world_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&pending.terrain.create_view(
                        &wgpu::TextureViewDescriptor {
                            dimension: Some(wgpu::TextureViewDimension::D2Array),
                            ..Default::default()
                        },
                    )),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.uniform.as_entire_binding(),
                },
            ],
        }));
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        self.ui_bind = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("HUD binding"),
            layout: &self.ui_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &pending.ui.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        }));
        self.atlas = Some(pending.prepared.atlas);
        self.stats.texture_uploads += 1;
        self.ui_key = None;
        if staged {
            self.stats.texture_stages_completed += 1;
            self.stats.max_texture_stage_ms = self
                .stats
                .max_texture_stage_ms
                .max(started.elapsed().as_secs_f64() * 1000.);
        }
        Ok(true)
    }
    fn upload(&mut self, key: [i32; 3], mesh: engine_mesh::ChunkMesh) {
        self.stats.mesh_rebuilds += 1;
        if mesh.block_count == 0 {
            self.meshes.remove(&key);
            return;
        }
        let buffer = (!mesh.vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("exposed chunk faces"),
                    contents: bytemuck::cast_slice(&mesh.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        self.meshes.insert(
            key,
            Mesh {
                buffer,
                vertices: mesh.vertices.len() as u32,
                exposed_faces: mesh.exposed_faces,
                occluded_faces: mesh.occluded_faces,
                block_count: mesh.block_count,
                fingerprint: self
                    .collect_timings
                    .then(|| Sha256::digest(bytemuck::cast_slice(&mesh.vertices)).into()),
            },
        );
    }
    fn sync(&mut self, client: &Client, eye: [f32; 3]) -> Result<()> {
        let state = client
            .scene
            .engine
            .as_ref()
            .ok_or("Engine GPU world missing")?;
        let reset = self.world_instance != state.instance;
        let remap = reset || self.registry_revision != state.registry_revision;
        let all = remap || self.mesh_mode_dirty;
        self.stats.async_meshing = state.stream.is_some() && self.async_requested;
        if remap {
            if !self.textures(client)? {
                self.stats.meshes_ready = false;
                return Ok(());
            }
            self.meshes.clear();
        }
        if all {
            self.mesher.invalidate();
        }
        if let Some(stream) = &state.stream {
            self.meshes
                .retain(|k, _| stream.residents.contains_key(&[k[0], k[2]]));
            self.mesher
                .retain(|k| stream.residents.contains_key(&[k[0], k[2]]));
        }
        if all || self.revision != state.revision {
            let keys: BTreeSet<_> = if all {
                state
                    .blocks
                    .keys()
                    .copied()
                    .map(engine_mesh::chunk)
                    .chain(self.meshes.keys().copied())
                    .collect()
            } else {
                state
                    .dirty_chunks
                    .iter()
                    .filter(|(_, r)| **r > self.revision)
                    .map(|(k, _)| *k)
                    .collect()
            };
            for key in keys {
                if self.stats.async_meshing {
                    self.mesher.mark(key, state.revision);
                } else {
                    let started = Instant::now();
                    let mesh = if state.stream.is_some() {
                        engine_mesh::Snapshot::capture(state, key).build(self.stats.greedy_meshing)
                    } else if self.stats.greedy_meshing {
                        engine_mesh::build(state, key)
                    } else {
                        engine_mesh::build_naive(state, key)
                    };
                    let elapsed = started.elapsed().as_secs_f64() * 1000.;
                    self.stats.mesh_main_builds += 1;
                    self.stats.mesh_main_ms += elapsed;
                    self.stats.max_chunk_build_ms = self.stats.max_chunk_build_ms.max(elapsed);
                    self.upload(key, mesh);
                }
            }
            self.world_instance = state.instance;
            self.registry_revision = state.registry_revision;
            self.revision = state.revision;
            self.mesh_mode_dirty = false;
        }
        if let Some(completed) = self.mesher.poll()?
            && self.stats.async_meshing
        {
            self.upload(completed.key, completed.mesh);
        }
        if self.stats.async_meshing {
            self.mesher.submit(state, eye, self.stats.greedy_meshing)?;
        }
        let c = &self.mesher.counters;
        self.stats.mesh_jobs_in_flight = self.mesher.jobs();
        self.stats.mesh_pending_chunks = self.mesher.remaining();
        self.stats.mesh_peak_jobs = c.peak_jobs;
        self.stats.mesh_discarded_jobs = c.rejected;
        self.stats.mesh_worker_jobs = c.off_thread;
        self.stats.mesh_snapshot_ms = c.snapshot_ms;
        self.stats.max_mesh_snapshot_ms = c.max_snapshot_ms;
        self.stats.mesh_build_ms = self.stats.mesh_main_ms + c.worker_ms;
        self.stats.max_chunk_build_ms = self.stats.max_chunk_build_ms.max(c.max_worker_ms);
        self.stats.meshes_ready = self.mesher.ready();
        self.stats.chunks = self.meshes.len();
        self.stats.peak_chunks = self.stats.peak_chunks.max(self.stats.chunks);
        self.stats.exposed_faces = self.meshes.values().map(|m| m.exposed_faces).sum();
        self.stats.mesh_covered_blocks = self.meshes.values().map(|m| m.block_count).sum();
        self.stats.mesh_quads = self.meshes.values().map(|m| m.vertices as usize / 6).sum();
        self.stats.merged_faces = self.stats.exposed_faces - self.stats.mesh_quads;
        self.stats.mesh_bytes = self.stats.mesh_quads * 6 * std::mem::size_of::<Vertex>();
        self.stats.peak_mesh_bytes = self.stats.peak_mesh_bytes.max(self.stats.mesh_bytes);
        self.stats.omitted_internal_faces = self.meshes.values().map(|m| m.occluded_faces).sum();
        Ok(())
    }
    pub fn verify_geometry(&self, client: &Client) -> Result<serde_json::Value> {
        let state = client.scene.engine.as_ref().unwrap();
        let keys: BTreeSet<_> = state
            .blocks
            .keys()
            .copied()
            .map(engine_mesh::chunk)
            .collect();
        let mut equal = keys.len() == self.meshes.len();
        let mut faces = 0;
        let mut quads = 0;
        for key in keys {
            let expected =
                engine_mesh::Snapshot::capture(state, key).build(self.stats.greedy_meshing);
            let hash: [u8; 32] = Sha256::digest(bytemuck::cast_slice(&expected.vertices)).into();
            equal &= self.meshes.get(&key).is_some_and(|m| {
                m.fingerprint == Some(hash)
                    && m.block_count == expected.block_count
                    && m.exposed_faces == expected.exposed_faces
                    && m.occluded_faces == expected.occluded_faces
            });
            faces += expected.exposed_faces;
            quads += expected.vertices.len() / 6;
        }
        Ok(
            serde_json::json!({"complete":equal,"expected_faces":faces,"expected_quads":quads,"uploaded_faces":self.stats.exposed_faces,"uploaded_quads":self.stats.mesh_quads,"all_vertex_bytes_match":equal}),
        )
    }
    pub fn has_mesh(&self, key: [i32; 3]) -> bool {
        self.meshes.contains_key(&key)
    }
    pub fn draw(&mut self, client: &Client, palette: &Palette, frame: Frame<'_>) -> Result<bool> {
        self.sync(client, frame.eye)?;
        // A requested verification capture waits for complete, current geometry.
        // No surface is presented or counted while this final capture is pending.
        if frame.capture.is_some() && !self.stats.meshes_ready && !frame.startup {
            return Ok(false);
        }
        let surface = match self.surface.get_current_texture() {
            Ok(s) => s,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return Ok(false);
            }
            Err(wgpu::SurfaceError::Timeout) => return Ok(false),
            Err(e) => return Err(e.into()),
        };
        let dir = client.direction().map(|v| v as f32);
        let eye = Vec3::from(frame.eye);
        let streamed = client.scene.engine.as_ref().unwrap().stream.as_ref();
        let flying = client.movement.mode != crate::engine_player::Mode::Walking;
        let fog_end = streamed.map_or(95., |s| s.generator.config.radius as f32 * 16. - 2.);
        let fog_start = if streamed.is_some() {
            fog_end - 14.
        } else {
            55.
        };
        let matrix = Mat4::perspective_rh(
            70f32.to_radians(),
            self.config.width as f32 / self.config.height as f32,
            0.05,
            100.,
        ) * Mat4::look_to_rh(eye, Vec3::from(dir), Vec3::Y);
        let target = client
            .scene
            .target(frame.eye.map(f64::from), client.direction())?;
        let uniform = Camera {
            matrix: matrix.to_cols_array(),
            eye: [eye.x, eye.y, eye.z, 1.],
            settings: [
                if frame.wire { 1. } else { 0. },
                fog_start,
                fog_end,
                if flying { 1. } else { 0. },
            ],
            focus: target.map_or([0.; 4], |h| {
                [
                    h.position.x as f32,
                    h.position.y as f32,
                    h.position.z as f32,
                    1.,
                ]
            }),
        };
        self.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        self.visible_keys.clear();
        self.stats.frustum_culled_chunks = 0;
        self.stats.range_culled_chunks = 0;
        for key in self.meshes.keys().copied() {
            if !engine_mesh::visible(key, matrix) {
                self.stats.frustum_culled_chunks += 1;
            } else if streamed.is_some()
                && !(if flying {
                    engine_mesh::visible_range_flat(key, frame.eye, fog_end)
                } else {
                    engine_mesh::visible_range(key, frame.eye, fog_end)
                })
            {
                self.stats.range_culled_chunks += 1;
            } else {
                self.visible_keys.push(key);
            }
        }
        let keys = &self.visible_keys;
        self.stats.visible_chunks = keys.len();
        self.stats.submitted_vertices = keys.iter().map(|p| self.meshes[p].vertices as usize).sum();
        let state = client.scene.engine.as_ref().unwrap();
        let key = UiKey {
            startup: frame.startup,
            movement: client.movement,
            size: [self.config.width, self.config.height],
            scale: self.stats.gui_scale.to_bits(),
            active: state.player.active,
            player_revision: state.player.revision,
            palette_revision: palette.revision,
            cursor: palette.cursor,
            greedy: self.stats.greedy_meshing,
            loading: !self.stats.meshes_ready,
            palette: palette.open,
            wire: frame.wire,
            debug: frame.debug,
            notice: frame
                .notice
                .unwrap_or("")
                .bytes()
                .fold(0u64, |a, b| a.wrapping_mul(31) + u64::from(b)),
            bucket: if frame.debug || frame.wire {
                self.ui_epoch.elapsed().as_millis() / 250
            } else {
                0
            },
        };
        if self.ui_key.as_ref() != Some(&key) {
            let label = format!(
                "{:0.0} FPS | {}x{} | chunks {}/{} | {} quads | {} merged | F8 {} | F2 {} | jobs {} pending {}",
                if self.stats.last_frame_ms > 0. {
                    1000. / self.stats.last_frame_ms
                } else {
                    0.
                },
                self.config.width,
                self.config.height,
                keys.len(),
                self.meshes.len(),
                self.stats.submitted_vertices / 6,
                self.stats.merged_faces,
                if self.stats.greedy_meshing {
                    "GREEDY"
                } else {
                    "REFERENCE"
                },
                if self.stats.async_meshing {
                    "ASYNC"
                } else {
                    "SYNC"
                },
                self.stats.mesh_jobs_in_flight,
                self.stats.mesh_pending_chunks
            );
            let composition = self.atlas.as_ref().unwrap().compose(
                client,
                palette,
                Context {
                    startup: frame.startup,
                    loading: !self.stats.meshes_ready,
                    width: self.config.width,
                    height: self.config.height,
                    scale: self.stats.gui_scale,
                    wire: frame.wire,
                    debug: frame.debug,
                    statistics: &label,
                    notice: frame.notice,
                },
            );
            self.ui_preview_at = composition.preview_at;
            self.preview_mesh = if !frame.startup {
                self.atlas.as_ref().unwrap().preview(
                    client,
                    palette,
                    [
                        self.config.width as f32 / self.stats.gui_scale,
                        self.config.height as f32 / self.stats.gui_scale,
                    ],
                )
            } else {
                None
            };
            self.stats.preview_vertices =
                self.preview_mesh.as_ref().map_or(0, |m| m.vertices.len());
            if let Some(mesh) = &self.preview_mesh {
                self.queue.write_buffer(
                    &self.preview_buffer,
                    0,
                    bytemuck::cast_slice(&mesh.vertices),
                );
                self.stats.preview_uploads += 1;
            }
            let vertices = composition.vertices;
            if vertices.len() > 65536 {
                return Err("HUD batch overflow".into());
            }
            self.queue
                .write_buffer(&self.ui_buffer, 0, bytemuck::cast_slice(&vertices));
            self.ui_vertices = vertices.len() as u32;
            self.ui_key = Some(key);
            self.stats.ui_uploads += 1;
        }
        let view = surface.texture.create_view(&Default::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("native-resolution frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("depth-tested terrain"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.62,
                            g: 0.77,
                            b: 0.89,
                            a: 1.,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.world_pipeline);
            pass.set_bind_group(0, self.world_bind.as_ref().unwrap(), &[]);
            for key in keys {
                let mesh = &self.meshes[key];
                if let Some(buffer) = &mesh.buffer {
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..mesh.vertices, 0..1);
                }
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("compact native HUD"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.ui_pipeline);
            pass.set_bind_group(0, self.ui_bind.as_ref().unwrap(), &[]);
            pass.set_vertex_buffer(0, self.ui_buffer.slice(..));
            pass.draw(
                0..if self.preview_mesh.is_some() {
                    self.ui_preview_at.unwrap_or(self.ui_vertices)
                } else {
                    self.ui_vertices
                },
                0..1,
            );
        }
        if let Some(mesh) = &self.preview_mesh {
            let [x, y, w, h] = mesh.rectangle.map(|v| v * self.stats.gui_scale);
            let left = (x.floor().max(0.) as u32).min(self.config.width);
            let top = (y.floor().max(0.) as u32).min(self.config.height);
            let right = ((x + w).ceil().max(0.) as u32).min(self.config.width);
            let bottom = ((y + h).ceil().max(0.) as u32).min(self.config.height);
            if right > left && bottom > top {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("inventory player depth and alpha"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.preview_pipeline);
                pass.set_bind_group(0, self.ui_bind.as_ref().unwrap(), &[]);
                pass.set_scissor_rect(left, top, right - left, bottom - top);
                pass.set_vertex_buffer(0, self.preview_buffer.slice(..));
                pass.draw(0..mesh.vertices.len() as u32, 0..1);
                self.stats.preview_draws += 1;
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("inventory foreground and cursor"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.ui_pipeline);
            pass.set_bind_group(0, self.ui_bind.as_ref().unwrap(), &[]);
            pass.set_vertex_buffer(0, self.ui_buffer.slice(..));
            pass.draw(self.ui_preview_at.unwrap_or(0)..self.ui_vertices, 0..1);
        }
        let readback = if frame.capture.is_some() {
            if !self.config.usage.contains(wgpu::TextureUsages::COPY_SRC) {
                return Err("Surface readback unsupported".into());
            }
            let stride = (self.config.width * 4).div_ceil(256) * 256;
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("checkpoint GPU capture"),
                size: u64::from(stride) * u64::from(self.config.height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &surface.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(stride),
                        rows_per_image: Some(self.config.height),
                    },
                },
                wgpu::Extent3d {
                    width: self.config.width,
                    height: self.config.height,
                    depth_or_array_layers: 1,
                },
            );
            Some((buffer, stride))
        } else {
            None
        };
        self.queue.submit(Some(encoder.finish()));
        if let (Some(path), Some((buffer, stride))) = (frame.capture, readback) {
            let (tx, rx) = std::sync::mpsc::channel();
            buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            self.device.poll(wgpu::PollType::Wait)?;
            rx.recv()??;
            let bytes = buffer.slice(..).get_mapped_range();
            let mut pixels = Vec::with_capacity((self.config.width * self.config.height) as usize);
            let bgra = matches!(
                self.config.format,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
            );
            for y in 0..self.config.height {
                for x in 0..self.config.width {
                    let i = (y * stride + x * 4) as usize;
                    let (r, b) = if bgra {
                        (bytes[i + 2], bytes[i])
                    } else {
                        (bytes[i], bytes[i + 2])
                    };
                    pixels
                        .push((u32::from(r) << 16) | (u32::from(bytes[i + 1]) << 8) | u32::from(b));
                }
            }
            crate::render::png(
                path,
                &pixels,
                self.config.width as usize,
                self.config.height as usize,
            )?;
            drop(bytes);
            buffer.unmap();
        }
        surface.present();
        self.stats.frames += 1;
        let now = Instant::now();
        self.stats.last_frame_ms = (now - self.previous).as_secs_f64() * 1000.;
        self.previous = now;
        if self.collect_timings
            && self.stats.frames > self.stats.warmup_frames
            && frame.capture.is_none()
        {
            self.frame_times.push(self.stats.last_frame_ms);
        }
        Ok(true)
    }
}
