//! Swept body motion shared by climbing, running, jumping and staged ledge pull-up.
use super::*;
pub fn move_by(c: &mut Client, delta: Vec3) -> Result<()> {
    let mut actor = c.actor;
    actor.velocity = delta;
    let scene = c.scene.motion_scene(&actor)?;
    let old = actor.position;
    let (p, blocked) = scene.move_actor(old, actor.body, delta, false)?;
    actor.position = p;
    actor.displacement = Vec3::new(p.x - old.x, p.y - old.y, p.z - old.z);
    actor.velocity = delta;
    for (axis, blocked) in blocked.into_iter().enumerate() {
        if blocked {
            actor.velocity[axis] = 0.;
        }
    }
    actor.on_ground = blocked[1] && delta.y < 0.;
    actor.falling = actor.velocity.y < 0.;
    c.actor = actor;
    Ok(())
}
pub fn climb(c: &mut Client, input: Input, wall: Wall) -> Result<bool> {
    c.parkour.action = Action::Climbing;
    c.parkour.wall = Some(wall);
    c.parkour.ticks = c.parkour.ticks.saturating_add(1);
    let mut delta = Vec3::new(0., input.forward.clamp(-1., 1.) * 0.14, 0.);
    delta[wall.axis] = -wall.normal * 0.06;
    let tangent = 2 - wall.axis;
    let (sin, cos) = c.yaw.sin_cos();
    let side = if tangent == 0 { -cos } else { sin };
    delta[tangent] = side * input.side * 0.07;
    move_by(c, delta)?;
    Ok(true)
}
pub fn run(c: &mut Client, _input: Input, wall: Wall, wish: [f32; 2]) -> Result<bool> {
    if c.parkour.action != Action::WallRunning {
        c.parkour.ticks = 0;
        c.actor.velocity.y = c.actor.velocity.y.max(0.06);
    }
    if c.parkour.ticks >= 30 {
        c.parkour.release();
        return Ok(false);
    }
    c.parkour.action = Action::WallRunning;
    c.parkour.wall = Some(wall);
    c.parkour.ticks += 1;
    let tangent = 2 - wall.axis;
    let index = if tangent == 0 { 0 } else { 1 };
    let mut delta = Vec3::new(0., (c.actor.velocity.y - 0.006).max(-0.08), 0.);
    delta[tangent] = wish[index].signum() * 0.30;
    delta[wall.axis] = -wall.normal * 0.06;
    move_by(c, delta)?;
    Ok(true)
}
pub fn wall_jump(c: &mut Client, input: Input) -> Result<bool> {
    let wall = c.parkour.wall.unwrap();
    c.parkour.action = Action::WallJump;
    c.parkour.ticks = 0;
    c.parkour.cooldown = 10;
    let (sin, cos) = c.yaw.sin_cos();
    c.actor.velocity = Vec3::new(
        (sin * input.forward - cos * input.side) * 0.12,
        0.42,
        (cos * input.forward + sin * input.side) * 0.12,
    );
    c.actor.velocity[wall.axis] = wall.normal * 0.28;
    wall_jump_step(c, input)
}
pub fn wall_jump_step(c: &mut Client, input: Input) -> Result<bool> {
    let (sin, cos) = c.yaw.sin_cos();
    let mut actor = c.actor;
    actor.velocity.x = actor.velocity.x * 0.96 + (sin * input.forward - cos * input.side) * 0.008;
    actor.velocity.z = actor.velocity.z * 0.96 + (cos * input.forward + sin * input.side) * 0.008;
    let scene = c.scene.motion_scene(&actor)?;
    c.actor = actor.tick_solid(&scene)?;
    c.parkour.ticks += 1;
    if c.parkour.ticks >= 6 || c.actor.on_ground {
        c.parkour.release();
    }
    Ok(true)
}
pub fn mantle(c: &mut Client) -> Result<bool> {
    let wall = c.parkour.wall.unwrap();
    let Some(target) = super::probes::landing(c, wall)? else {
        c.parkour.release();
        return Ok(false);
    };

    c.parkour.ticks += 1;
    if c.parkour.ticks > 40 {
        c.parkour.release();
        return Ok(false);
    }
    let mut delta = Vec3::default();
    if c.parkour.phase == 0 {
        delta.y = (target.y - c.actor.position.y).clamp(0., 0.14);
        if delta.y < 0.0005 {
            c.parkour.phase = 1;
        }
    }
    if c.parkour.phase == 1 {
        delta[wall.axis] = (target[wall.axis] - c.actor.position[wall.axis]).clamp(-0.14, 0.14);
        if delta[wall.axis].abs() < 0.0005 {
            c.parkour.phase = 2;
        }
    }
    if c.parkour.phase == 2 {
        delta.y = -0.02;
    }
    let before = c.actor.position;
    move_by(c, delta)?;
    if c.parkour.phase == 2 && c.actor.on_ground {
        c.parkour.release();
        return Ok(true);
    }
    if (delta.y.abs() > 0.001 && (c.actor.position.y - before.y).abs() < 0.0001)
        || (delta[wall.axis].abs() > 0.001
            && (c.actor.position[wall.axis] - before[wall.axis]).abs() < 0.0001)
    {
        c.parkour.release();
    }
    Ok(true)
}
