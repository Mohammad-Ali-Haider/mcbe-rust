//! Transient, collision-driven wall/ledge controllers. No world or inventory writes.
mod probes;
#[cfg(test)]
mod tests;
mod traversal;
use crate::scene::{Client, Input, Result};
use mcbe_core::motion::{SolidBox, Vec3};
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    #[default]
    Idle,
    Climbing,
    WallRunning,
    Hanging,
    Mantling,
    WallJump,
}
#[derive(Clone, Copy, Debug)]
pub struct Wall {
    pub bounds: SolidBox,
    pub axis: usize,
    pub normal: f32,
    pub plane: f32,
}
#[derive(Clone, Copy, Default, Debug)]
pub struct State {
    pub action: Action,
    pub wall: Option<Wall>,
    pub ticks: u16,
    pub cooldown: u8,
    pub jump_held: bool,
    pub phase: u8,
    pub world: Option<(u64, u64)>,
}
impl State {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn release(&mut self) {
        self.action = Action::Idle;
        self.wall = None;
        self.ticks = 0;
        self.phase = 0;
        self.cooldown = 6;
    }
    pub fn label(self) -> &'static str {
        match self.action {
            Action::Idle => "",
            Action::Climbing => "CLIMB | W/S MOVE | SPACE JUMP | SHIFT DROP",
            Action::WallRunning => "WALL RUN | CTRL MOVE | SPACE JUMP | SHIFT DROP",
            Action::Hanging => "HANGING | SPACE PULL UP | SHIFT DROP",
            Action::Mantling => "PULLING UP | SHIFT DROP",
            Action::WallJump => "WALL JUMP",
        }
    }
}
pub struct Parkour;
impl super::Controller for Parkour {
    fn eligible(&self, e: &super::Environment) -> bool {
        e.immersion <= 0.2
    }
    fn step(&self, c: &mut Client, input: Input, _e: &super::Environment) -> Result<bool> {
        let key = c
            .scene
            .engine
            .as_ref()
            .map(|s| (s.instance, s.registry_revision));
        if c.parkour.world != key {
            c.parkour.reset();
            c.parkour.world = key;
        }
        let jump = input.jump && !c.parkour.jump_held;
        c.parkour.jump_held = input.jump;
        if c.parkour.cooldown > 0 {
            c.parkour.cooldown -= 1;
        }
        if c.parkour.action == Action::WallJump {
            return traversal::wall_jump_step(c, input);
        }
        if !input.grab || input.sneak {
            if c.parkour.action != Action::Idle {
                c.parkour.release();
            }
            return Ok(false);
        }
        if c.movement.crouched {
            if !crate::engine_player::clear(c, mcbe_core::motion::Body::PROBE)? {
                return Ok(false);
            }
            c.movement.crouched = false;
        }
        c.actor.body = c.movement.body();
        if c.parkour.action == Action::Mantling {
            return traversal::mantle(c);
        }
        let walls = probes::nearby(c)?;
        if c.parkour.action == Action::Hanging {
            let wall = c.parkour.wall.unwrap();
            if !walls.iter().any(|w| probes::same_ledge(w, &wall)) {
                c.parkour.release();
                return Ok(false);
            }
            if jump && probes::landing(c, wall)?.is_some() {
                c.parkour.action = Action::Mantling;
                c.parkour.phase = 0;
                c.parkour.ticks = 0;
                return traversal::mantle(c);
            }
            traversal::move_by(c, Vec3::default())?;
            return Ok(true);
        }
        if c.parkour.cooldown > 0 {
            return Ok(false);
        }
        if jump && matches!(c.parkour.action, Action::Climbing | Action::WallRunning) {
            return traversal::wall_jump(c, input);
        }
        let (sin, cos) = c.yaw.sin_cos();
        let forward = [sin, cos];
        let wish = [
            sin * input.forward - cos * input.side,
            cos * input.forward + sin * input.side,
        ];
        for wall in walls {
            let a = if wall.axis == 0 { 0 } else { 1 };
            let facing = -forward[a] * wall.normal;
            if facing > 0.35
                && !c.actor.on_ground
                && ((wall.bounds.max.y - c.actor.position.y - 1.4).abs() <= 0.18
                    || (c.actor.velocity.y < 0.
                        && c.actor.displacement.y >= -0.8
                        && c.actor.displacement.y < 0.
                        && c.actor.position.y + 1.4 <= wall.bounds.max.y
                        && c.actor.position.y - c.actor.displacement.y + 1.4 >= wall.bounds.max.y))
                && probes::landing(c, wall)?.is_some()
            {
                c.parkour.action = Action::Hanging;
                c.parkour.wall = Some(wall);
                c.parkour.ticks = 0;
                traversal::move_by(
                    c,
                    Vec3::new(0., wall.bounds.max.y - 1.4 - c.actor.position.y, 0.),
                )?;
                return Ok(true);
            }
            let length = (wish[0] * wish[0] + wish[1] * wish[1]).sqrt();
            if input.sprint && !c.actor.on_ground && length > 0.5 && (wish[a] / length).abs() < 0.45
            {
                return traversal::run(c, input, wall, wish);
            }
            if facing > 0.5 && (input.forward != 0. || c.parkour.action == Action::Climbing) {
                return traversal::climb(c, input, wall);
            }
        }
        if c.parkour.action != Action::Idle {
            c.parkour.release();
        }
        Ok(false)
    }
}
