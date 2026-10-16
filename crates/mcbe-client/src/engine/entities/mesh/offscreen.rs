//! Real entity forward/G-buffer/shadow rendering with no window or surface.
use super::*;
use crate::engine::entities::{assets, definition::Definitions};
use std::sync::Arc;
fn texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    layers: u32,
    width: u32,
    height: u32,
    mips: u32,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen entity attachment"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: layers,
        },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}
fn read(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    bytes: u32,
    aspect: wgpu::TextureAspect,
) -> Vec<u8> {
    let size = (width * height * bytes) as u64;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
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
                bytes_per_row: Some(width * bytes),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    let (tx, rx) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |v| tx.send(v).unwrap());
    device.poll(wgpu::PollType::wait()).unwrap();
    rx.recv().unwrap().unwrap();
    buffer.slice(..).get_mapped_range().to_vec()
}
fn attachment(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color {
                r: 0.025,
                g: 0.035,
                b: 0.05,
                a: 0.,
            }),
            store: wgpu::StoreOp::Store,
        },
    }
}
fn half(bits: u16) -> f32 {
    let sign = if bits & 0x8000 == 0 { 1. } else { -1. };
    let exp = ((bits >> 10) & 31) as i32;
    let mant = (bits & 1023) as f32;
    if exp == 0 {
        sign * mant / 1024. * 2f32.powi(-14)
    } else {
        sign * (1. + mant / 1024.) * 2f32.powi(exp - 15)
    }
}
#[test]
#[ignore = "Explicit offscreen mob render; no window, surface or event loop"]
fn offscreen_vanilla_skins_render_faces_materials_normals_and_shadows() {
    pollster::block_on(async {
        let root = std::env::var_os("MCBE_ENTITY_TEST_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| crate::engine_assets::DEFAULT_ROOT.into());
        let mut c = crate::engine::generate(37).unwrap();
        let r = c.scene.engine.as_ref().unwrap().registry.clone();
        let (definitions, assets) = assets::Assets::read(
            &Definitions::built_in(),
            &r,
            std::path::Path::new("."),
            Some(&root),
        )
        .unwrap();
        assert_eq!(assets.textures.len(), 4);
        for (name, count) in [("cow", 9), ("pig", 7), ("creeper", 6), ("zombie", 6)] {
            assert_eq!(
                definitions
                    .kinds
                    .iter()
                    .find(|d| d.id == name)
                    .unwrap()
                    .model
                    .len(),
                count
            );
        }
        for (name, height) in [("cow", 0.75), ("pig", 0.375)] {
            let d = definitions.kinds.iter().find(|d| d.id == name).unwrap();
            assert!(
                (d.model[0].bounds[1] - height).abs() < 0.0001,
                "{name} body must meet its legs"
            );
        }
        c.scene.engine.as_mut().unwrap().entities.library =
            Arc::new(definitions.compile(&r).unwrap());
        c.entity_assets = Arc::new(assets);
        let kinds = ["cow", "pig", "creeper", "zombie"];
        for (i, kind) in kinds.iter().enumerate() {
            c.scene
                .engine
                .as_mut()
                .unwrap()
                .entities
                .spawn(kind, [-2.4 + i as f32 * 1.6, 0., 0.], true)
                .unwrap();
        }
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: None,
                ..Default::default()
            })
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("window-free mob proof"),
                ..Default::default()
            })
            .await
            .unwrap();
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../render/shaders/world.wgsl").into(),
            ),
        });
        let world = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("actual world bind layout"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[layouts()[0].clone()],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let layout = world.get_bind_group_layout(0);
        let mut renderer = Renderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb, &layout);
        let eye = [0., 2.2, 7.];
        let matrix = Mat4::orthographic_rh(-3.6, 3.6, -1.8, 1.8, 0.1, 40.)
            * Mat4::look_at_rh(Vec3::from(eye), Vec3::new(0., 1., 0.), Vec3::Y);
        let mut camera = [0f32; 44];
        camera[..16].copy_from_slice(&matrix.to_cols_array());
        camera[16..19].copy_from_slice(&eye);
        camera[21] = 100.;
        camera[22] = 200.;
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&camera),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let plan = crate::engine_texture::Prepared::build(&c).unwrap();
        let skins = texture(
            &device,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            plan.skin_layers,
            plan.skin_width,
            plan.skin_width,
            plan.skin_width.ilog2() + 1,
        );
        let terrain = texture(&device, wgpu::TextureFormat::Rgba8UnormSrgb, 1, 1, 1, 1);
        for step in &plan.steps {
            if let crate::engine_texture::Step::Skin {
                layer,
                mip,
                width,
                rgba,
            } = step
            {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &skins,
                        mip_level: *mip,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: 0,
                            z: *layer,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    rgba,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(width * 4),
                        rows_per_image: Some(*width),
                    },
                    wgpu::Extent3d {
                        width: *width,
                        height: *width,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let array_view = |t: &wgpu::Texture| {
            t.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            })
        };
        let coverage = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&vec![
                [u32::MAX; 4];
                crate::engine::render::coverage::SLOTS
            ]),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let world_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&array_view(&terrain)),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: coverage.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&array_view(&skins)),
                },
            ],
        });
        let (w, h) = (1024, 512);
        let color = texture(&device, wgpu::TextureFormat::Rgba8UnormSrgb, 1, w, h, 1);
        let albedo = texture(&device, wgpu::TextureFormat::Rgba8Unorm, 1, w, h, 1);
        let normals = texture(&device, wgpu::TextureFormat::Rgba16Float, 1, w, h, 1);
        let depth = texture(&device, wgpu::TextureFormat::Depth32Float, 1, w, h, 1);
        let shadow = texture(&device, wgpu::TextureFormat::Depth32Float, 1, w, h, 1);
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let mut light = [0f32; 56];
        light[16..32].copy_from_slice(&matrix.to_cols_array());
        let light_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&light),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let light_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: light_buffer.as_entire_binding(),
            }],
        });
        for (view, angle) in [
            0.,
            std::f32::consts::FRAC_PI_2,
            std::f32::consts::PI,
            -std::f32::consts::FRAC_PI_2,
        ]
        .into_iter()
        .enumerate()
        {
            {
                let world = &mut c.scene.engine.as_mut().unwrap().entities;
                for e in world.records.values_mut() {
                    e.transform.yaw = angle;
                }
                world.stats.revision += 1;
            }
            renderer.prepare(
                &queue,
                &c,
                Frame {
                    matrix,
                    eye,
                    blend: 1.,
                    selected: None,
                    hidden: false,
                },
            );
            assert_eq!(renderer.stats.visible, 4);
            assert_eq!(renderer.instances.len(), 28);
            assert!(renderer.instances.iter().all(|i| i.metadata[1] >> 8 > 0));
            renderer.ensure_shadow(&device, &frame_layout);
            for deferred in [false, true] {
                let albedo_view = albedo.create_view(&Default::default());
                let normals_view = normals.create_view(&Default::default());
                let color_view = color.create_view(&Default::default());
                let colors = if deferred {
                    vec![
                        Some(attachment(&albedo_view)),
                        Some(attachment(&normals_view)),
                    ]
                } else {
                    vec![Some(attachment(&color_view))]
                };
                let depth_view = depth.create_view(&Default::default());
                let mut encoder = device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: None,
                        color_attachments: &colors,
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    renderer.draw(&mut pass, &world_bind, deferred);
                }
                queue.submit(Some(encoder.finish()));
                let bytes = read(
                    &device,
                    &queue,
                    if deferred { &albedo } else { &color },
                    w,
                    h,
                    4,
                    wgpu::TextureAspect::All,
                );
                let mut counts = [0usize; 4];
                let mut green = [0usize; 4];
                let mut pink = [0usize; 4];
                for row in bytes.chunks_exact(w as usize * 4) {
                    for (x, p) in row.chunks_exact(4).enumerate() {
                        if p[3] > 128 {
                            let bank = ((x as f32 / w as f32 * 7.2 - 0.4) / 1.6)
                                .floor()
                                .clamp(0., 3.) as usize;
                            counts[bank] += 1;
                            green[bank] +=
                                usize::from(p[1] as u16 > p[0] as u16 * 12 / 10 && p[1] > p[2]);
                            pink[bank] += usize::from(p[0] > p[1] && p[0] > p[2]);
                        }
                    }
                }
                assert!(
                    counts.iter().all(|n| *n > 1200),
                    "missing mob coverage {counts:?}"
                );
                assert!(
                    green[2] > 200 && green[3] > 200,
                    "vanilla green skins missing {green:?}"
                );
                assert!(pink[1] > 200, "pig skin missing {pink:?}");
                if let Ok(out) = std::env::var("MCBE_ENTITY_PREVIEW_OUT") {
                    let out = std::path::PathBuf::from(out);
                    std::fs::create_dir_all(&out).unwrap();
                    let pixels = bytes
                        .chunks_exact(4)
                        .map(|p| (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]))
                        .collect::<Vec<_>>();
                    crate::render::png(
                        &out.join(format!(
                            "mobs-{}-{view}.png",
                            if deferred { "gbuffer" } else { "forward" }
                        )),
                        &pixels,
                        w as usize,
                        h as usize,
                    )
                    .unwrap();
                }
                if deferred {
                    let data = read(&device, &queue, &normals, w, h, 8, wgpu::TextureAspect::All);
                    let mut seen = 0;
                    for (p, n) in bytes.chunks_exact(4).zip(data.chunks_exact(8)) {
                        if p[3] > 128 {
                            let v = std::array::from_fn::<_, 4, _>(|i| {
                                half(u16::from_le_bytes([n[i * 2], n[i * 2 + 1]]))
                            });
                            let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                            assert!((length - 1.).abs() < 0.02);
                            assert_eq!(v[3] as u16, r.find("ivory").unwrap());
                            seen += 1;
                        }
                    }
                    assert!(seen > 6000);
                }
            }
            let shadow_view = shadow.create_view(&Default::default());
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &shadow_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                renderer.draw_shadow(&mut pass, &world_bind, &light_bind);
            }
            queue.submit(Some(encoder.finish()));
            let data = read(
                &device,
                &queue,
                &shadow,
                w,
                h,
                4,
                wgpu::TextureAspect::DepthOnly,
            );
            let covered = data
                .chunks_exact(4)
                .filter(|p| f32::from_le_bytes([p[0], p[1], p[2], p[3]]) < 0.999)
                .count();
            assert!(covered > 6000);
        }
        assert!(device.pop_error_scope().await.is_none());
        eprintln!(
            "Four vanilla skins, 28 instanced cuboids, four headings: forward, G-buffer material/normal and shadow passes verified without a window"
        );
    });
}
