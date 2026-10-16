//! Nearby registered collision faces, loaded-cell guards and full-body landing checks.
use super::*;
use mcbe_core::motion::{Actor, Body};
pub fn clear_at(c: &Client, p: Vec3) -> Result<bool> {
    let actor = Actor::new(p, Body::PROBE)?;
    let scene = c.scene.motion_scene(&actor)?;
    let h = actor.body.width * 0.5;
    let lo = [p.x - h, p.y, p.z - h];
    let hi = [p.x + h, p.y + actor.body.height, p.z + h];
    Ok(!scene
        .boxes
        .iter()
        .any(|b| (0..3).all(|a| lo[a] < b.max[a] && hi[a] > b.min[a])))
}
pub fn nearby(c: &Client) -> Result<Vec<Wall>> {
    let mut actor = c.actor;
    actor.velocity = Vec3::default();
    let scene = c.scene.motion_scene(&actor)?;
    let p = actor.position;
    let h = actor.body.width * 0.5;
    let mut walls = Vec::new();
    for b in scene.boxes {
        let center = std::array::from_fn(|i| ((b.min[i] + b.max[i]) * 0.5).floor() as i32);
        if !c.scene.inside(center) || b.max.y <= p.y + 0.1 || b.min.y >= p.y + actor.body.height {
            continue;
        }
        for axis in [0, 2] {
            let tangent = 2 - axis;
            if p[tangent] + h <= b.min[tangent] + 0.05 || p[tangent] - h >= b.max[tangent] - 0.05 {
                continue;
            }
            for normal in [-1., 1.] {
                let plane = if normal < 0. {
                    b.min[axis]
                } else {
                    b.max[axis]
                };
                let gap = (p[axis] - plane) * normal - h;
                if (-0.002..=0.14).contains(&gap) {
                    walls.push(Wall {
                        bounds: b,
                        axis,
                        normal,
                        plane,
                    });
                }
            }
        }
    }
    walls.sort_by(|a, b| {
        b.bounds
            .max
            .y
            .total_cmp(&a.bounds.max.y)
            .then(a.axis.cmp(&b.axis))
    });
    Ok(walls)
}
pub fn same_ledge(a: &Wall, b: &Wall) -> bool {
    a.axis == b.axis
        && a.normal == b.normal
        && (a.plane - b.plane).abs() < 0.002
        && (a.bounds.max.y - b.bounds.max.y).abs() < 0.002
}
pub fn landing(c: &Client, wall: Wall) -> Result<Option<Vec3>> {
    let mut p = c.actor.position;
    p.y = wall.bounds.max.y + 0.002;
    p[wall.axis] = wall.plane - wall.normal * (c.actor.body.width * 0.5 + 0.08);
    let cell = [
        p.x.floor() as i32,
        (p.y - 0.01).floor() as i32,
        p.z.floor() as i32,
    ];
    if !c.scene.inside(cell) || !clear_at(c, p)? {
        return Ok(None);
    }
    let actor = Actor::new(p, Body::PROBE)?;
    let scene = c.scene.motion_scene(&actor)?;
    let h = actor.body.width * 0.5;
    let supported = scene.boxes.iter().any(|b| {
        (b.max.y - wall.bounds.max.y).abs() < 0.002
            && p.x + h > b.min.x
            && p.x - h < b.max.x
            && p.z + h > b.min.z
            && p.z - h < b.max.z
    });
    Ok(supported.then_some(p))
}
