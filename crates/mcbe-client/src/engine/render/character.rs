//! Bounded reusable player geometry with shared skin UVs and held union models.
use crate::engine::player::camera::Mode;
use crate::engine_inventory::Hand;
use crate::{
    engine_mesh::Vertex as ItemVertex,
    engine_preview::{Cube, Vertex as SkinVertex},
    scene::Client,
};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;
fn pivot(c: &Cube, angle: f32) -> Mat4 {
    let p = Vec3::from(c.pivot);
    Mat4::from_translation(p) * Mat4::from_rotation_x(angle) * Mat4::from_translation(-p)
}
fn root(position: [f32; 3], yaw: f32, crouched: bool) -> Mat4 {
    Mat4::from_translation(Vec3::from(position) - Vec3::Y * if crouched { 0.15 } else { 0. })
        * Mat4::from_rotation_y(std::f32::consts::PI + yaw)
        * Mat4::from_scale(Vec3::splat(1.8 / 32.))
}
fn swing(client: &Client, hand: Hand) -> f32 {
    if client.view.swing_hand == hand {
        client.animations.wave("player_use", 1. - client.view.swing)
    } else {
        0.
    }
}
fn walk(client: &Client, position: [f32; 3]) -> f32 {
    let remaining = ((position[0] - client.actor.position.x).powi(2)
        + (position[2] - client.actor.position.z).powi(2))
    .sqrt();
    client.animations.wave(
        "player_walk",
        (client.view.gait - remaining * 8.) / std::f32::consts::TAU,
    )
}
fn arm(client: &Client, hand: Hand, position: [f32; 3]) -> f32 {
    let sign = if hand == Hand::Main { -1. } else { 1. };
    sign * walk(client, position) * client.view.walk * 0.55 - swing(client, hand) * 0.8
}
pub fn body(client: &Client, position: [f32; 3], skin: [f32; 4]) -> Vec<SkinVertex> {
    let mut out = Vec::with_capacity(12 * 36);
    let base = root(position, client.yaw, client.movement.crouched);
    let swing = walk(client, position) * client.view.walk * 0.55;
    for c in &client.appearance.model.cubes {
        let angle = match c.bone.as_str() {
            "head" | "hat" => client.pitch,
            "rightLeg" | "rightPants" => swing,
            "leftArm" | "leftSleeve" => arm(client, Hand::Off, position),
            "leftLeg" | "leftPants" => -swing,
            "rightArm" | "rightSleeve" => arm(client, Hand::Main, position),
            _ => 0.,
        };
        // Imported cuboid X positions oppose the avatar's anatomical right axis.
        // Move bounds across the center without reflecting the skin UVs.
        let side = Mat4::from_translation(Vec3::new(-2. * c.origin[0] - c.size[0], 0., 0.));
        c.emit(&mut out, base * side * pivot(c, angle), skin);
    }
    out
}
#[cfg(test)]
pub fn hand(client: &Client, skin: [f32; 4]) -> Vec<SkinVertex> {
    hand_at(
        client,
        skin,
        [
            client.actor.position.x,
            client.actor.position.y,
            client.actor.position.z,
        ],
    )
}
fn hand_at(client: &Client, skin: [f32; 4], position: [f32; 3]) -> Vec<SkinVertex> {
    let mut out = hand_geometry(client, skin, Hand::Main, position);
    if client
        .scene
        .engine
        .as_ref()
        .is_some_and(|s| s.player.offhand.is_some())
    {
        out.extend(hand_geometry(client, skin, Hand::Off, position));
    }
    out
}
fn hand_geometry(
    client: &Client,
    skin: [f32; 4],
    hand: Hand,
    position: [f32; 3],
) -> Vec<SkinVertex> {
    let sign = if hand == Hand::Main { 1. } else { -1. };
    let (bone, sleeve) = if hand == Hand::Main {
        ("rightArm", "rightSleeve")
    } else {
        ("leftArm", "leftSleeve")
    };
    let mut out = Vec::with_capacity(72);
    let center = client
        .appearance
        .model
        .cubes
        .iter()
        .find(|c| c.bone == bone)
        .map_or(Vec3::new(-6. * sign, 18., 0.), |c| {
            Vec3::from(c.origin) + Vec3::from(c.size) * 0.5
        });
    let swing = swing(client, hand);
    let bob = walk(client, position) * client.view.walk * 0.012;
    let base = Mat4::from_translation(Vec3::new(
        sign * (0.55 - swing * 0.12),
        -0.40 + bob,
        -0.72 + swing * 0.12,
    )) * Mat4::from_rotation_x(-0.9 - swing * 0.4)
        * Mat4::from_rotation_y(sign * 0.2)
        * Mat4::from_scale(Vec3::splat(1.8 / 32. * 0.9))
        * Mat4::from_translation(-center);
    for c in &client.appearance.model.cubes {
        if c.bone == bone || c.bone == sleeve {
            c.emit(&mut out, base, skin);
        }
    }
    out
}
pub fn item(client: &Client, position: [f32; 3], first: bool) -> Vec<ItemVertex> {
    let mut out = item_hand(client, position, first, Hand::Main);
    out.extend(item_hand(client, position, first, Hand::Off));
    out
}
pub fn item_hand(client: &Client, position: [f32; 3], first: bool, hand: Hand) -> Vec<ItemVertex> {
    let sign = if hand == Hand::Main { 1. } else { -1. };
    let bone = if hand == Hand::Main {
        "rightArm"
    } else {
        "leftArm"
    };
    let state = client.scene.engine.as_ref().unwrap();
    let Some(stack) = state.player.hand(hand) else {
        return Vec::new();
    };
    let mut out = crate::engine::items::mesh(state, stack);
    let swing = swing(client, hand);
    let bob = walk(client, position) * client.view.walk * 0.012;
    let container = state.registry.get(stack.id).container.is_some();
    let transform = if first {
        Mat4::from_translation(Vec3::new(
            sign * ((if container { 0.32 } else { 0.43 }) - swing * 0.1),
            (if container { -0.28 } else { -0.25 }) + bob,
            (if container { -0.85 } else { -0.75 }) + swing * 0.15,
        )) * Mat4::from_rotation_x((if container { 0.7 } else { 0.3 }) - swing * 0.4)
            * Mat4::from_rotation_y(sign * -0.55)
            * Mat4::from_scale(Vec3::splat(0.30))
    } else {
        let anchor = client
            .appearance
            .model
            .cubes
            .iter()
            .find(|c| c.bone == bone)
            .map_or(Vec3::new(6. * sign, 10., -3.), |c| {
                Vec3::new(
                    -c.origin[0] - c.size[0] * 0.5,
                    c.origin[1] - 2.,
                    c.origin[2] - 1.,
                )
            });
        root(position, client.yaw, client.movement.crouched)
            * client
                .appearance
                .model
                .cubes
                .iter()
                .find(|c| c.bone == bone)
                .map_or(Mat4::IDENTITY, |c| pivot(c, arm(client, hand, position)))
            * Mat4::from_translation(anchor)
            * Mat4::from_scale(Vec3::splat(5.))
    };
    for v in &mut out {
        v.position = transform
            .transform_point3(Vec3::from(v.position) - Vec3::splat(0.5))
            .to_array();
    }
    out
}
pub struct Frame {
    pub position: [f32; 3],
    pub skin: [f32; 4],
    pub world: Mat4,
    pub aspect: f32,
    pub hidden: bool,
}
#[derive(PartialEq, Eq)]
struct Key {
    pose: [u32; 12],
    swing_hand: Hand,
    offhand: Option<crate::engine_inventory::Stack>,
    mode: Mode,
    hidden: bool,
    crouched: bool,
    instance: u64,
    registry: u64,
    selected: Option<crate::engine_inventory::Stack>,
    appearance: usize,
    animations: u64,
}
pub struct Renderer {
    skin_pipeline: wgpu::RenderPipeline,
    item_pipeline: wgpu::RenderPipeline,
    world_uniform: wgpu::Buffer,
    view_uniform: wgpu::Buffer,
    world_bind: wgpu::BindGroup,
    view_bind: wgpu::BindGroup,
    body_buffer: wgpu::Buffer,
    hand_buffer: wgpu::Buffer,
    item_buffer: wgpu::Buffer,
    key: Option<Key>,
    pub body: Vec<SkinVertex>,
    pub hand: Vec<SkinVertex>,
    pub item: Vec<ItemVertex>,
    pub first: bool,
    pub uploads: usize,
    pub draws: usize,
}
impl Renderer {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        ui: &wgpu::BindGroupLayout,
        terrain: &wgpu::BindGroupLayout,
    ) -> Self {
        let matrix_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("player projection layout"),
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
        let create = |name| {
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(name),
                contents: bytemuck::cast_slice(&Mat4::IDENTITY.to_cols_array()),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(name),
                layout: &matrix_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            (buffer, bind)
        };
        let (world_uniform, world_bind) = create("player world projection");
        let (view_uniform, view_bind) = create("first-person projection");
        let build = |name,
                     code: &str,
                     layout: &wgpu::BindGroupLayout,
                     stride,
                     attributes: &[wgpu::VertexAttribute]| {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(name),
                source: wgpu::ShaderSource::Wgsl(code.into()),
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(name),
                bind_group_layouts: &[layout, &matrix_layout],
                push_constant_ranges: &[],
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(name),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: stride,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes,
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
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview: None,
                cache: None,
            })
        };
        let skin_pipeline = build(
            "skinned player",
            include_str!("shaders/character.wgsl"),
            ui,
            std::mem::size_of::<SkinVertex>() as u64,
            &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Float32x4],
        );
        let item_pipeline = build(
            "held block",
            include_str!("shaders/held.wgsl"),
            terrain,
            std::mem::size_of::<ItemVertex>() as u64,
            &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Float32,4=>Float32x3],
        );
        let buffer = |name, size| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(name),
                size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Self {
            skin_pipeline,
            item_pipeline,
            world_uniform,
            view_uniform,
            world_bind,
            view_bind,
            body_buffer: buffer(
                "world body",
                (32 * 36 * std::mem::size_of::<SkinVertex>()) as u64,
            ),
            hand_buffer: buffer(
                "first person hand",
                (4 * 36 * std::mem::size_of::<SkinVertex>()) as u64,
            ),
            item_buffer: buffer(
                "held union model",
                (2 * (256 + 6) * 6 * std::mem::size_of::<ItemVertex>()) as u64,
            ),
            key: None,
            body: Vec::new(),
            hand: Vec::new(),
            item: Vec::new(),
            first: true,
            uploads: 0,
            draws: 0,
        }
    }
    pub fn prepare(&mut self, queue: &wgpu::Queue, client: &Client, frame: Frame) {
        let Frame {
            position,
            skin,
            world,
            aspect,
            hidden,
        } = frame;
        queue.write_buffer(
            &self.world_uniform,
            0,
            bytemuck::cast_slice(&world.to_cols_array()),
        );
        let view = Mat4::perspective_rh(70_f32.to_radians(), aspect, 0.025, 10.);
        queue.write_buffer(
            &self.view_uniform,
            0,
            bytemuck::cast_slice(&view.to_cols_array()),
        );
        let state = client.scene.engine.as_ref().unwrap();
        let values = [
            position[0],
            position[1],
            position[2],
            client.yaw,
            client.pitch,
            client.view.gait,
            client.view.walk,
            client.view.swing,
            skin[0],
            skin[1],
            skin[2],
            skin[3],
        ];
        let key = Key {
            pose: values.map(f32::to_bits),
            swing_hand: client.view.swing_hand,
            offhand: state.player.offhand,
            mode: client.view.mode,
            hidden,
            crouched: client.movement.crouched,
            instance: state.instance,
            registry: state.registry_revision,
            selected: state.player.slots[state.player.active],
            appearance: std::sync::Arc::as_ptr(&client.appearance) as usize,
            animations: client.animations.id,
        };
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.key = Some(key);
        self.first = client.view.mode == Mode::FirstPerson;
        self.body = if self.first || hidden {
            Vec::new()
        } else {
            body(client, position, skin)
        };
        self.hand = if self.first && !hidden {
            hand_at(client, skin, position)
        } else {
            Vec::new()
        };
        self.item = if hidden {
            Vec::new()
        } else {
            item(client, position, self.first)
        };
        for (buffer, bytes) in [
            (&self.body_buffer, bytemuck::cast_slice(&self.body)),
            (&self.hand_buffer, bytemuck::cast_slice(&self.hand)),
            (&self.item_buffer, bytemuck::cast_slice(&self.item)),
        ] {
            if !bytes.is_empty() {
                queue.write_buffer(buffer, 0, bytes);
                self.uploads += 1;
            }
        }
    }
    pub fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        ui: &'a wgpu::BindGroup,
        terrain: &'a wgpu::BindGroup,
        first: bool,
    ) {
        if first != self.first {
            return;
        }
        let (vertices, buffer) = if first {
            (&self.hand, &self.hand_buffer)
        } else {
            (&self.body, &self.body_buffer)
        };
        let projection = if first {
            &self.view_bind
        } else {
            &self.world_bind
        };
        if !vertices.is_empty() {
            pass.set_pipeline(&self.skin_pipeline);
            pass.set_bind_group(0, ui, &[]);
            pass.set_bind_group(1, projection, &[]);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..vertices.len() as u32, 0..1);
        }
        if !self.item.is_empty() {
            pass.set_pipeline(&self.item_pipeline);
            pass.set_bind_group(0, terrain, &[]);
            pass.set_bind_group(1, projection, &[]);
            pass.set_vertex_buffer(0, self.item_buffer.slice(..));
            pass.draw(0..self.item.len() as u32, 0..1);
        }
    }
    pub fn observation(&self, client: &Client) -> serde_json::Value {
        serde_json::json!({"offhand_data":client.scene.engine.as_ref().and_then(|s|s.player.offhand.and_then(|v|v.data)),"main_item_vertices":item_hand(client,[0.;3],self.first,Hand::Main).len(),"offhand_item_vertices":item_hand(client,[0.;3],self.first,Hand::Off).len(),"offhand":client.scene.engine.as_ref().and_then(|s|s.player.offhand.map(|v|s.registry.get(v.id).id.clone())),"swing_hand":client.view.swing_hand,"mode":client.view.mode,"skin":client.appearance.provenance,"body_vertices":self.body.len(),"hand_vertices":self.hand.len(),"item_vertices":self.item.len(),"selected":client.scene.engine.as_ref().and_then(|s|s.player.selected().map(|id|s.registry.get(id).id.clone())),"mesh_uploads":self.uploads,"draws":self.draws,"item_geometry":self.item.iter().map(|v|serde_json::json!({"position":v.position,"uv":v.uv,"layer":v.layer,"shade":v.shade})).collect::<Vec<_>>(),"item_data":client.scene.engine.as_ref().and_then(|s|s.player.slots[s.player.active].and_then(|s|s.data)),"hand_geometry":self.hand,"body_geometry":self.body,"shared_skin_uvs":true,"world_depth_test":true,"separate_first_person_depth":true})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anatomical_arms_and_items_follow_camera_right_in_rear_view() {
        let mut c = crate::engine::generate(37).unwrap();
        let state = c.scene.engine.as_mut().unwrap();
        state.player.offhand = Some(
            state
                .registry
                .catalog_stack(state.registry.find("blue_glass").unwrap()),
        );
        for yaw in [0., 0.7, 2.5] {
            c.yaw = yaw;
            let right = Vec3::new(-yaw.cos(), 0., yaw.sin());
            let vertices = body(&c, [0.; 3], [0., 0., 64., 64.]);
            for (cube, mesh) in c
                .appearance
                .model
                .cubes
                .iter()
                .zip(vertices.chunks_exact(36))
            {
                if matches!(cube.bone.as_str(), "rightArm" | "leftArm") {
                    let center = mesh.iter().map(|v| Vec3::from(v.position)).sum::<Vec3>() / 36.;
                    let expected = if cube.bone == "rightArm" { 1. } else { -1. };
                    assert!(center.dot(right) * expected > 0.25);
                }
            }
            for (hand, sign) in [(Hand::Main, 1.), (Hand::Off, -1.)] {
                let mesh = item_hand(&c, [0.; 3], false, hand);
                let center =
                    mesh.iter().map(|v| Vec3::from(v.position)).sum::<Vec3>() / mesh.len() as f32;
                assert!(center.dot(right) * sign > 0.25);
            }
        }
    }
    #[test]
    fn shared_skin_mesh_and_partial_items_are_bounded_and_selected_by_owned_id() {
        let mut c = crate::engine::generate(37).unwrap();
        let p = [0.5, 4., -9.5];
        let skin = [100., 200., 64., 64.];
        let mesh = body(&c, p, skin);
        assert_eq!(mesh.len(), 12 * 36);
        assert_eq!(hand(&c, skin).len(), 72);
        for v in mesh {
            assert!(v.position.iter().all(|v| v.is_finite()));
            assert!((100. / 1024. ..=164. / 1024.).contains(&v.uv[0]));
            assert!((200. / 1024. ..=264. / 1024.).contains(&v.uv[1]));
        }
        let state = c.scene.engine.as_mut().unwrap();
        let slab = state.registry.find("oak_slab").unwrap();
        state.player.assign(slab, 0).unwrap();
        state.player.select(0).unwrap();
        let vertices = crate::engine_mesh::item(state, slab);
        assert_eq!(vertices.len(), 36);
        assert!(vertices.iter().all(|v| v.position[1] <= 0.5));
        assert!(
            vertices
                .iter()
                .all(|v| v.layer & 0x007fffff >= u32::from(slab) * 6
                    && v.layer & 0x007fffff < u32::from(slab) * 6 + 6)
        );
        assert!(!item(&c, p, true).is_empty());
        c.scene.engine.as_mut().unwrap().player.slots[0] = None;
        assert!(item(&c, p, true).is_empty());
    }
}
