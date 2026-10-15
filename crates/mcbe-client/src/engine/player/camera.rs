//! Perspective rig: collision-shortened orbit and detached free-camera movement.
use crate::scene::{Client, Input, Result};
use glam::Vec3;
use serde::Serialize;
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    FirstPerson,
    ThirdBack,
    ThirdFront,
    Free,
}
#[derive(Clone, Copy, Serialize)]
pub struct Pose {
    pub eye: [f32; 3],
    pub direction: [f32; 3],
    pub mode: Mode,
}
type PoseKey = ([u32; 5], Mode, u64, u64, u64);
#[derive(Default)]
pub struct Rig {
    pub mode: Mode,
    pub free: [f32; 3],
    pub previous_free: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    cache: std::cell::RefCell<Option<(PoseKey, Pose)>>,
    pub walk: f32,
    pub gait: f32,
    pub swing: f32,
}
impl Rig {
    pub fn is_free(&self) -> bool {
        self.mode == Mode::Free
    }
    pub fn look(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx;
        self.pitch = (self.pitch - dy).clamp(-1.5, 1.5);
    }
    pub fn free_tick(&mut self, input: Input, seconds: f32) {
        self.previous_free = self.free;
        let (s, c) = self.yaw.sin_cos();
        let (p, q) = self.pitch.sin_cos();
        let forward = Vec3::new(s * q, p, c * q);
        let right = Vec3::new(-c, 0., s);
        let motion = forward * input.forward
            + right * input.side
            + Vec3::Y * (f32::from(input.jump) - f32::from(input.sneak));
        let motion = motion / motion.length().max(1.);
        let speed = if input.sprint { 28. } else { 8. };
        let next = Vec3::from(self.free) + motion * seconds * speed;
        self.free = [
            next.x.clamp(-8191.5, 8191.5),
            next.y.clamp(-1_000_000., 1_000_000.),
            next.z.clamp(-8191.5, 8191.5),
        ];
    }
    pub fn advance_animation(&mut self, distance: f32, seconds: f32) {
        let target = (distance / 0.22).min(1.);
        self.walk += (target - self.walk) * (seconds * 12.).min(1.);
        if self.walk < 0.001 {
            self.walk = 0.;
        }
        self.gait = (self.gait + distance * 8.) % std::f32::consts::TAU;
        self.swing = (self.swing - seconds * 4.).max(0.);
    }
}
pub fn pose(client: &Client, eye: [f32; 3]) -> Result<Pose> {
    let rig = &client.view;
    let mode = rig.mode;
    if mode == Mode::Free {
        let (s, c) = rig.yaw.sin_cos();
        let (p, q) = rig.pitch.sin_cos();
        return Ok(Pose {
            eye: rig.free,
            direction: [s * q, p, c * q],
            mode,
        });
    }
    let direction = Vec3::from(client.direction().map(|v| v as f32));
    if mode == Mode::FirstPerson {
        return Ok(Pose {
            eye,
            direction: direction.to_array(),
            mode,
        });
    }
    let state = client.scene.engine.as_ref().unwrap();
    let key = (
        [
            eye[0].to_bits(),
            eye[1].to_bits(),
            eye[2].to_bits(),
            client.yaw.to_bits(),
            client.pitch.to_bits(),
        ],
        mode,
        state.instance,
        state.registry_revision,
        state.revision,
    );
    if let Some((old, pose)) = *rig.cache.borrow()
        && old == key
    {
        return Ok(pose);
    }
    let sign = if mode == Mode::ThirdBack { -1. } else { 1. };
    let ray = direction * sign;
    let origin = Vec3::from(eye);
    let right = Vec3::new(-client.yaw.cos(), 0., client.yaw.sin());
    let mut distance: f32 = 4.;
    for offset in [
        Vec3::ZERO,
        right * 0.14,
        -right * 0.14,
        Vec3::Y * 0.14,
        -Vec3::Y * 0.14,
    ] {
        let start = origin + offset;
        let mut cell = start.floor().to_array().map(|v| v as i32);
        let components = ray.to_array();
        let origin = start.to_array();
        let step = components.map(|v| if v > 0. { 1 } else { -1 });
        let delta = components.map(|v| if v == 0. { f32::INFINITY } else { 1. / v.abs() });
        let mut boundary = std::array::from_fn::<_, 3, _>(|axis| {
            if components[axis] == 0. {
                f32::INFINITY
            } else {
                (cell[axis] as f32 + if step[axis] > 0 { 1. } else { 0. } - origin[axis])
                    / components[axis]
            }
        });
        let mut entered = 0.;
        for _ in 0..32 {
            if !client.scene.inside(cell) {
                distance = distance.min((entered - 0.2_f32).max(0.));
                break;
            }
            if let Some(id) = state.blocks.get(&cell) {
                for cube in state.geometry.boxes(*id) {
                    if let Some((t, _, _)) = crate::render::box_hit(
                        cell,
                        *cube,
                        origin.map(f64::from),
                        components.map(f64::from),
                    ) {
                        distance = distance.min((t as f32 - 0.2).max(0.));
                    }
                }
            }
            let axis = if boundary[0] <= boundary[1] && boundary[0] <= boundary[2] {
                0
            } else if boundary[1] <= boundary[2] {
                1
            } else {
                2
            };
            entered = boundary[axis];
            if entered > distance + 0.2 {
                break;
            }
            cell[axis] += step[axis];
            boundary[axis] += delta[axis];
        }
    }
    let pose = Pose {
        eye: (origin + ray * distance).to_array(),
        direction: (-ray).to_array(),
        mode,
    };
    *rig.cache.borrow_mut() = Some((key, pose));
    Ok(pose)
}
pub fn cycle(client: &mut Client) -> Result<Mode> {
    let previous = pose(client, client.eye().map(|v| v as f32))?;
    let next = match client.view.mode {
        Mode::FirstPerson => Mode::ThirdBack,
        Mode::ThirdBack => Mode::ThirdFront,
        Mode::ThirdFront => Mode::Free,
        Mode::Free => Mode::FirstPerson,
    };
    if next == Mode::Free {
        client.view.free = previous.eye;
        client.view.previous_free = previous.eye;
        let d = previous.direction;
        client.view.yaw = d[0].atan2(d[2]);
        client.view.pitch = d[1].asin().clamp(-1.5, 1.5);
    }
    client.view.mode = next;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modes_cycle_and_detached_motion_look_and_edits_leave_actor_unchanged() {
        let mut c = crate::engine::generate(37).unwrap();
        let actor = c.actor;
        let yaw = c.yaw;
        let pitch = c.pitch;
        for expected in [Mode::ThirdBack, Mode::ThirdFront, Mode::Free] {
            assert_eq!(cycle(&mut c).unwrap(), expected);
        }
        let camera = c.view.free;
        c.look(-0.4, -0.2);
        for _ in 0..20 {
            c.tick(Input {
                forward: 1.,
                side: 1.,
                jump: true,
                ..Default::default()
            })
            .unwrap();
        }
        assert_ne!(c.view.free, camera);
        assert_eq!(c.actor.position, actor.position);
        assert_eq!(c.actor.velocity, actor.velocity);
        assert_eq!(c.yaw, yaw);
        assert_eq!(c.pitch, pitch);
        let edits = c.pending_edits();
        assert!(!c.edit(true).unwrap());
        assert_eq!(c.pending_edits(), edits);
        assert_eq!(cycle(&mut c).unwrap(), Mode::FirstPerson);
        assert_eq!(
            pose(&c, c.eye().map(|v| v as f32)).unwrap().eye,
            c.eye().map(|v| v as f32)
        );
    }
    #[test]
    fn orbit_retracts_before_wall_and_invalidation_restores_distance() {
        let mut c = crate::engine::generate(37).unwrap();
        c.yaw = 0.;
        c.pitch = 0.;
        cycle(&mut c).unwrap();
        let eye = c.eye().map(|v| v as f32);
        let clear = pose(&c, eye).unwrap();
        assert!((Vec3::from(clear.eye).distance(Vec3::from(eye)) - 4.).abs() < 0.001);
        let id = c
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .find("basalt")
            .unwrap();
        for y in 4..7 {
            c.scene.change_engine([0, y, -11], Some(id)).unwrap();
        }
        let blocked = pose(&c, eye).unwrap();
        assert!(Vec3::from(blocked.eye).distance(Vec3::from(eye)) < 0.5);
        assert!(blocked.eye[2] > -10.);
        for y in 4..7 {
            c.scene.change_engine([0, y, -11], None).unwrap();
        }
        assert!(
            (Vec3::from(pose(&c, eye).unwrap().eye).distance(Vec3::from(eye)) - 4.).abs() < 0.001
        );
    }
    #[test]
    fn free_camera_drives_bounded_residency_and_returns_to_actor_region() {
        let mut c =
            crate::engine_stream::generate(360036, crate::engine_blocks::Registry::built_in())
                .unwrap();
        let actor = c.actor.position;
        for _ in 0..3 {
            cycle(&mut c).unwrap();
        }
        c.view.free = [128.5, 50., 128.5];
        crate::engine_stream::update(&mut c).unwrap();
        let s = c.scene.engine.as_ref().unwrap().stream.as_ref().unwrap();
        assert_eq!(s.center, [8, 8]);
        assert!(s.residents.len() <= 49);
        assert_eq!(c.actor.position, actor);
        cycle(&mut c).unwrap();
        crate::engine_stream::update(&mut c).unwrap();
        let s = c.scene.engine.as_ref().unwrap().stream.as_ref().unwrap();
        assert_eq!(s.center, crate::engine_stream::center(actor));
        assert!(s.residents.len() <= 49);
    }
    #[test]
    fn free_camera_strafe_and_up_match_projected_screen_axes() {
        for yaw in [0., 1., 2., 3.] {
            let mut rig = Rig {
                mode: Mode::Free,
                yaw,
                ..Default::default()
            };
            rig.free_tick(
                Input {
                    side: 1.,
                    ..Default::default()
                },
                1.,
            );
            assert!(Vec3::from(rig.free).dot(Vec3::new(-yaw.cos(), 0., yaw.sin())) > 7.9);
            rig.free_tick(
                Input {
                    jump: true,
                    ..Default::default()
                },
                1.,
            );
            assert_eq!(rig.free[1], 8.);
        }
    }
}
