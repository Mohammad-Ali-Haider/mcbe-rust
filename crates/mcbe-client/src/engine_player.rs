//! Engine-owned creative movement, posture, collision and edge-safe sneaking.
use crate::scene::{Client, Input, Result};
use mcbe_core::motion::{Body, SolidScene, Vec3};
use serde::{Deserialize, Serialize};
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Walking,
    Flying,
    Noclip,
}
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Movement {
    pub mode: Mode,
    pub crouched: bool,
}
impl Movement {
    pub fn body(self) -> Body {
        Body {
            width: 0.6,
            height: if self.crouched { 1.5 } else { 1.8 },
        }
    }
    pub fn eye_height(self) -> f32 {
        if self.crouched { 1.27 } else { 1.62 }
    }
}
pub fn clear(client: &Client, body: Body) -> Result<bool> {
    let mut actor = client.actor;
    actor.body = body;
    actor.velocity = Vec3::default();
    let scene = client.scene.motion_scene(&actor)?;
    let p = actor.position;
    let half = body.width * 0.5;
    let low = [p.x - half, p.y, p.z - half];
    let high = [p.x + half, p.y + body.height, p.z + half];
    Ok(!scene
        .boxes
        .iter()
        .any(|b| (0..3).all(|a| low[a] < b.max[a] && high[a] > b.min[a])))
}
pub fn mode(client: &mut Client, next: Mode) -> Result<()> {
    if next != Mode::Noclip && !clear(client, client.movement.body())? {
        return Err("Move into clear space before disabling noclip".into());
    }
    client.movement.mode = next;
    client.actor.velocity = Vec3::default();
    client.actor.on_ground = false;
    Ok(())
}
fn supported(scene: &SolidScene, p: Vec3, body: Body) -> bool {
    let half = body.width * 0.5;
    scene.boxes.iter().any(|b| {
        p.x + half > b.min.x
            && p.x - half < b.max.x
            && p.z + half > b.min.z
            && p.z - half < b.max.z
            && b.max.y <= p.y + 0.0001
            && b.max.y >= p.y - 0.051
    })
}
pub fn tick(client: &mut Client, input: Input) -> Result<()> {
    client.sync_region()?;
    if client.movement.mode == Mode::Walking {
        if input.sneak {
            client.movement.crouched = true;
        } else if client.movement.crouched && clear(client, Body::PROBE)? {
            client.movement.crouched = false;
        }
    }
    client.actor.body = client.movement.body();
    let mut actor = client.actor;
    let (sin, cos) = client.yaw.sin_cos();
    let vertical = if client.movement.mode == Mode::Walking {
        0.
    } else {
        f32::from(input.jump) - f32::from(input.sneak)
    };
    let norm = (input.forward * input.forward + input.side * input.side + vertical * vertical)
        .sqrt()
        .max(1.);
    let speed = if client.movement.mode != Mode::Walking {
        0.4
    } else if client.movement.crouched {
        0.054
    } else {
        0.18
    };
    // Right-handed camera: screen-right is forward cross up, (-cos,0,sin).
    actor.velocity.x = (sin * input.forward - cos * input.side) * speed / norm;
    actor.velocity.z = (cos * input.forward + sin * input.side) * speed / norm;
    if client.movement.mode != Mode::Walking {
        actor.velocity.y = vertical * speed / norm;
        let old = actor.position;
        let mut position = if client.movement.mode == Mode::Noclip {
            Vec3::new(
                old.x + actor.velocity.x,
                old.y + actor.velocity.y,
                old.z + actor.velocity.z,
            )
        } else {
            let scene = client.scene.motion_scene(&actor)?;
            scene.move_actor(old, actor.body, actor.velocity, false)?.0
        };
        let bounds = client.scene.bounds;
        let half = actor.body.width * 0.5;
        position.x = position
            .x
            .clamp(bounds.min[0] as f32 + half, bounds.max[0] as f32 - half);
        position.y = position.y.clamp(
            bounds.min[1] as f32,
            bounds.max[1] as f32 - actor.body.height,
        );
        position.z = position
            .z
            .clamp(bounds.min[2] as f32 + half, bounds.max[2] as f32 - half);
        actor.position = position;
        actor.displacement = Vec3::new(position.x - old.x, position.y - old.y, position.z - old.z);
        actor.velocity = Vec3::default();
        actor.on_ground = false;
        actor.falling = false;
        client.actor = actor;
    } else {
        if input.jump && actor.on_ground {
            actor.velocity.y = 0.42;
        }
        let scene = client.scene.motion_scene(&actor)?;
        if client.movement.crouched && actor.on_ground && !input.jump {
            // Reduce each horizontal component until some support remains.
            for axis in [0, 2] {
                while actor.velocity[axis] != 0. {
                    let mut next = actor.position;
                    next[axis] += actor.velocity[axis];
                    if supported(&scene, next, actor.body) {
                        break;
                    }
                    let v = actor.velocity[axis];
                    actor.velocity[axis] = if v.abs() <= 0.01 {
                        0.
                    } else {
                        v - v.signum() * 0.01
                    };
                }
            }
            while actor.velocity.x != 0. && actor.velocity.z != 0. {
                let next = Vec3::new(
                    actor.position.x + actor.velocity.x,
                    actor.position.y,
                    actor.position.z + actor.velocity.z,
                );
                if supported(&scene, next, actor.body) {
                    break;
                }
                for axis in [0, 2] {
                    let v = actor.velocity[axis];
                    actor.velocity[axis] = if v.abs() <= 0.01 {
                        0.
                    } else {
                        v - v.signum() * 0.01
                    };
                }
            }
        }
        client.actor = actor.tick_solid(&scene)?;
    }
    client.sync_region()?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn client() -> Client {
        let mut c = crate::engine::generate(360036).unwrap();
        c.set_spawn(Vec3::new(0.5, 4., -9.5)).unwrap();
        c
    }
    #[test]
    fn strafe_and_mouse_follow_screen_right_at_every_heading() {
        for yaw in [
            0.,
            std::f32::consts::FRAC_PI_2,
            std::f32::consts::PI,
            -std::f32::consts::FRAC_PI_2,
        ] {
            let mut c = client();
            c.yaw = yaw;
            let (sin, cos) = yaw.sin_cos();
            let old = c.actor.position;
            c.tick(Input {
                side: 1.,
                ..Default::default()
            })
            .unwrap();
            assert!((c.actor.position.x - old.x) * -cos + (c.actor.position.z - old.z) * sin > 0.);
            c.look(0.2, 0.);
            let direction = c.direction();
            assert!(direction[0] * f64::from(-cos) + direction[2] * f64::from(sin) > 0.);
        }
    }
    #[test]
    fn sneaking_slows_lowers_and_stays_under_a_low_ceiling() {
        let mut c = client();
        let initial = c.eye()[1];
        c.tick(Input {
            sneak: true,
            forward: 1.,
            ..Default::default()
        })
        .unwrap();
        assert!(c.eye()[1] < initial - 0.3);
        assert!((c.actor.displacement.z - 0.054).abs() < 0.001);
        let block = c
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .find("slate")
            .unwrap();
        c.scene.change_engine([0, 6, -10], Some(block)).unwrap();
        c.tick(Input::default()).unwrap();
        assert!(!c.movement.crouched); // Standing still fits beneath y6.
        c.scene.change_engine([0, 5, -10], Some(block)).unwrap(); // Body overlap is deliberately checked by clear().
        assert!(!clear(&c, Body::PROBE).unwrap());
    }
    #[test]
    fn flight_collides_and_noclip_crosses_then_refuses_unsafe_exit() {
        let mut c = client();
        let block = c
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .find("slate")
            .unwrap();
        for y in 4..7 {
            c.scene.change_engine([0, y, -8], Some(block)).unwrap();
        }
        mode(&mut c, Mode::Flying).unwrap();
        for _ in 0..8 {
            c.tick(Input {
                forward: 1.,
                ..Default::default()
            })
            .unwrap();
        }
        assert!(c.actor.position.z <= -8.3 + 0.001);
        mode(&mut c, Mode::Noclip).unwrap();
        c.tick(Input {
            forward: 1.,
            ..Default::default()
        })
        .unwrap();
        assert!(c.actor.position.z > -8.3);
        assert!(mode(&mut c, Mode::Flying).is_err());
        for _ in 0..5 {
            c.tick(Input {
                forward: 1.,
                ..Default::default()
            })
            .unwrap();
        }
        mode(&mut c, Mode::Flying).unwrap();
        let y = c.actor.position.y;
        c.tick(Input {
            jump: true,
            ..Default::default()
        })
        .unwrap();
        assert!(c.actor.position.y > y);
    }
    #[test]
    fn sneak_stays_on_a_ledge_and_keeps_low_posture_under_slab_ceiling() {
        let mut c = client();
        let block = c
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .find("slate")
            .unwrap();
        c.scene.change_engine([0, 5, 0], Some(block)).unwrap();
        c.set_spawn(Vec3::new(0.5, 6., 0.5)).unwrap();
        c.yaw = 0.;
        for _ in 0..50 {
            c.tick(Input {
                forward: 1.,
                sneak: true,
                ..Default::default()
            })
            .unwrap();
        }
        assert!(c.actor.position.z < 1.3 && c.actor.position.y >= 5.999);
        c.movement = Movement::default();
        c.set_spawn(Vec3::new(0.5, 4., -9.5)).unwrap();
        c.tick(Input {
            sneak: true,
            ..Default::default()
        })
        .unwrap();
        let mut registry = c.scene.engine.as_ref().unwrap().registry.clone();
        let slab = registry.find("stone_slab").unwrap();
        registry.blocks[usize::from(slab)]
            .geometry
            .as_mut()
            .unwrap()
            .boxes = vec![[0., 0.5, 0., 1., 1., 1.]];
        crate::engine::apply_registry(&mut c, registry).unwrap();
        c.scene.change_engine([0, 5, -10], Some(slab)).unwrap();
        c.tick(Input::default()).unwrap();
        assert!(c.movement.crouched);
        assert_eq!(c.actor.body.height, 1.5);
        c.scene.change_engine([0, 5, -10], None).unwrap();
        c.tick(Input::default()).unwrap();
        assert!(!c.movement.crouched);
    }
    #[test]
    fn slabs_and_stairs_have_actual_collision_and_window_hole_is_selectable_through() {
        let mut c = client();
        let r = &c.scene.engine.as_ref().unwrap().registry;
        let stair = r.find("oak_stairs_north").unwrap();
        let window = r.find("oak_window").unwrap();
        let stone = r.find("slate").unwrap();
        c.scene.change_engine([2, 4, 0], Some(stair)).unwrap();
        c.set_spawn(Vec3::new(2.5, 4., -0.5)).unwrap();
        c.yaw = 0.;
        for _ in 0..8 {
            c.tick(Input {
                forward: 1.,
                ..Default::default()
            })
            .unwrap();
        }
        assert!(c.actor.position.y > 4.9);
        c.scene.change_engine([0, 4, 0], Some(window)).unwrap();
        c.scene.change_engine([0, 4, 1], Some(stone)).unwrap();
        let through = c
            .scene
            .target([0.5, 4.5, -1.], [0., 0., 1.])
            .unwrap()
            .unwrap();
        assert_eq!(through.position.z, 1);
        let frame = c
            .scene
            .target([0.1, 4.5, -1.], [0., 0., 1.])
            .unwrap()
            .unwrap();
        assert_eq!(frame.position.z, 0);
    }
}
