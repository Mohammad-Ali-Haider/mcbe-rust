//! Buoyancy, drag, directional sprint swimming and solid-world collision.
use super::*;
use mcbe_core::motion::{Body, Vec3};
pub struct Swimming;
impl Controller for Swimming {
    fn eligible(&self, e: &Environment) -> bool {
        e.immersion > 0.2
    }
    fn step(&self, c: &mut Client, input: Input, e: &Environment) -> Result<()> {
        c.movement.swimming = true;
        if c.movement.crouched && crate::engine_player::clear(c, Body::PROBE)? {
            c.movement.crouched = false;
        }
        let mut actor = c.actor;
        actor.body = c.movement.body();
        let norm = (input.forward * input.forward + input.side * input.side)
            .sqrt()
            .max(1.);
        let speed = if input.sprint { 0.14 } else { 0.075 };
        let (sin, cos) = c.yaw.sin_cos();
        let dive = if input.sprint {
            c.pitch.sin() * input.forward * speed
        } else {
            0.
        };
        let horizontal = if input.sprint { c.pitch.cos() } else { 1. };
        let x = (sin * input.forward * horizontal - cos * input.side) * speed / norm;
        let z = (cos * input.forward * horizontal + sin * input.side) * speed / norm;
        actor.velocity.x = actor.velocity.x * 0.65 + x * 0.35 + e.current[0] * 0.04;
        actor.velocity.z = actor.velocity.z * 0.65 + z * 0.35 + e.current[2] * 0.04;
        let thrust = (f32::from(input.jump) - f32::from(input.sneak)) * 0.035;
        actor.velocity.y = (actor.velocity.y
            + 0.04 * (e.immersion - 0.8)
            + thrust
            + dive * 0.25
            + e.current[1] * 0.04)
            * 0.85;
        // Prevent a dry-land fall velocity from crossing the entire lake in one tick.
        actor.velocity.y = actor.velocity.y.clamp(-0.22, 0.18);
        let scene = c.scene.motion_scene(&actor)?;
        let old = actor.position;
        let proposed = actor.velocity;
        let (position, ground) = scene.move_actor(old, actor.body, proposed, actor.on_ground)?;
        actor.position = position;
        actor.displacement = Vec3::new(position.x - old.x, position.y - old.y, position.z - old.z);
        for i in 0..3 {
            if (actor.displacement[i] - proposed[i]).abs() > 0.0001 {
                actor.velocity[i] = 0.;
            }
        }
        actor.on_ground = ground[1] && proposed.y < 0.;
        actor.falling = actor.velocity.y < 0.;
        c.actor = actor;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pool() -> Client {
        let mut c = crate::engine::generate(44).unwrap();
        let s = c.scene.engine.as_mut().unwrap();
        s.blocks.clear();
        let w = s.registry.find("water").unwrap();
        let stone = s.registry.find("slate").unwrap();
        for x in -4i32..=4 {
            for z in -4i32..=4 {
                s.blocks.insert([x, 0, z], stone);
                for y in 1..=6 {
                    if x.abs() == 4 || z.abs() == 4 {
                        s.blocks.insert([x, y, z], stone);
                    } else if y <= 4 {
                        s.blocks.insert([x, y, z], w);
                    }
                }
            }
        }
        c.set_spawn(Vec3::new(0.5, 1.5, 0.5)).unwrap();
        c
    }
    #[test]
    fn automatic_swimming_rises_dives_stops_at_floor_and_exits_to_dry_movement() {
        let mut c = pool();
        let initial = c.actor.position.y;
        for _ in 0..20 {
            crate::engine_player::tick(
                &mut c,
                Input {
                    jump: true,
                    ..Default::default()
                },
            )
            .unwrap();
        }
        assert!(c.movement.swimming && c.actor.position.y > initial + 1.);
        for _ in 0..80 {
            crate::engine_player::tick(
                &mut c,
                Input {
                    sneak: true,
                    ..Default::default()
                },
            )
            .unwrap();
        }
        assert!(c.actor.position.y >= 1. && c.actor.position.y < 1.1);
        assert!(Environment::sample(&c).head_underwater);
        c.set_spawn(Vec3::new(0.5, 7., 0.5)).unwrap();
        crate::engine_player::tick(&mut c, Input::default()).unwrap();
        assert!(!c.movement.swimming);
    }
    #[test]
    fn sprint_swim_follows_look_direction_and_water_respects_wall_collision() {
        let mut c = pool();
        c.yaw = 1.57;
        c.pitch = -0.4;
        let y = c.actor.position.y;
        for _ in 0..40 {
            crate::engine_player::tick(
                &mut c,
                Input {
                    forward: 1.,
                    sprint: true,
                    ..Default::default()
                },
            )
            .unwrap();
        }
        assert!(c.actor.position.x > 2. && c.actor.position.x <= 3.701 && c.actor.position.y < y);
        assert!(c.actor.velocity.x.abs() < 0.001);
    }
}
