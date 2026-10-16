//! Optional offscreen GPU verification: no window, surface or event loop.
use wgpu::util::DeviceExt;
#[test]
#[ignore = "Explicit offscreen GPU check; never opens a game window"]
fn offscreen_material_mirror_response_keeps_rough_surfaces_matte() {
    pollster::block_on(async {
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
                label: Some("offscreen material regression"),
                ..Default::default()
            })
            .await
            .unwrap();
        let source = include_str!("../../shaders/ray_lighting.wgsl");
        let helper = source.split("// Hybrid primary").next().unwrap();
        assert!(source.contains("mirror_response(material.x)"));
        let code = format!(
            "{}\n@group(0) @binding(0) var<storage,read> input:array<f32>; @group(0) @binding(1) var<storage,read_write> output:array<f32>; @compute @workgroup_size(1) fn cs(@builtin(global_invocation_id) id:vec3<u32>) {{ output[id.x]=mirror_response(input[id.x]); }}",
            helper
        );
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("actual mirror response"),
            source: wgpu::ShaderSource::Wgsl(code.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &module,
            entry_point: Some("cs"),
            compilation_options: Default::default(),
            cache: None,
        });
        let roughness = [
            crate::engine::blocks::components::material::Material::default().roughness,
            1.,
            0.12,
            0.04,
        ];
        let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&roughness),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(4, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &staging, 0, 16);
        queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                tx.send(result).unwrap();
            });
        device.poll(wgpu::PollType::Wait).unwrap();
        rx.recv().unwrap().unwrap();
        let bytes = staging.slice(..).get_mapped_range();
        let response: &[f32] = bytemuck::cast_slice(&bytes);
        assert!(
            response[0] < 0.002,
            "Default terrain must not launch a sharp mirror ray"
        );
        assert_eq!(response[1], 0.);
        assert!(response[2] > 0.5 && response[2] < 1.);
        assert!(response[3] > 0.8 && response[3] <= 1.);
        eprintln!("Offscreen GPU mirror responses: {response:?}");
    });
}
