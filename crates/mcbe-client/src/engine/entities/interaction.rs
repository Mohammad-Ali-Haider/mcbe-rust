//! Occlusion-aware entity selection and shared mouse/controller transactions.
use super::components::*;
use crate::scene::{Client, Result};
use mcbe_core::raycast::Face;
fn interval(origin: [f32; 3], direction: [f32; 3], lo: [f32; 3], hi: [f32; 3]) -> Option<f32> {
    let mut near = 0f32;
    let mut far = 6f32;
    for a in 0..3 {
        if direction[a].abs() < 1e-7 {
            if origin[a] < lo[a] || origin[a] > hi[a] {
                return None;
            }
        } else {
            let x = (lo[a] - origin[a]) / direction[a];
            let y = (hi[a] - origin[a]) / direction[a];
            near = near.max(x.min(y));
            far = far.min(x.max(y));
        }
    }
    (near <= far).then_some(near)
}
pub fn target(c: &Client) -> Result<Option<Id>> {
    let Some(s) = &c.scene.engine else {
        return Ok(None);
    };
    if s.entities.records.is_empty() || c.view.is_free() {
        return Ok(None);
    }
    let origin = c.eye().map(|v| v as f32);
    let direction = c.direction().map(|v| v as f32);
    let mut limit = 6.;
    if let Some(h) = c.target()? {
        let p = [
            h.position.x as f32,
            h.position.y as f32,
            h.position.z as f32,
        ];
        let mut local = [h.face_location.x, h.face_location.y, h.face_location.z];
        let (a, positive) = match h.face {
            Face::Up => (1, true),
            Face::Down => (1, false),
            Face::East => (0, true),
            Face::West => (0, false),
            Face::South => (2, true),
            Face::North => (2, false),
        };
        if positive && local[a] < 0.0001 {
            local[a] = 1.;
        }
        limit = distance(origin, std::array::from_fn(|i| p[i] + local[i]));
    }
    let mut selected = None;
    let mut candidates = s
        .entities
        .records
        .values()
        .filter(|e| {
            c.scene
                .inside(e.transform.position.map(|v| v.floor() as i32))
        })
        .filter_map(|e| {
            let index = s.entities.library.kinds[&e.kind];
            let d = distance(origin, e.transform.position);
            (d <= 6. + s.entities.library.radii[index]).then_some((d, e))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    candidates.truncate(super::mesh::VISIBLE_LIMIT);
    for (_, e) in candidates {
        if e.visual.is_some() {
            let d = s.entities.library.get(&e.kind)?;
            let pose = super::model::Pose::new(e, e.transform.position);
            for part in &d.model {
                let inverse = pose.part(part).inverse();
                let p = inverse
                    .transform_point3(glam::Vec3::from(origin))
                    .to_array();
                let direction = inverse
                    .transform_vector3(glam::Vec3::from(direction))
                    .to_array();
                if let Some(t) = interval(p, direction, [0.; 3], [1.; 3])
                    && t < limit - 0.001
                {
                    limit = t;
                    selected = Some(e.id);
                }
            }
        } else if let Some(b) = &e.physics {
            let p = e.transform.position;
            let h = b.width * 0.5;
            let lo = [p[0] - h, p[1], p[2] - h];
            let hi = [p[0] + h, p[1] + b.height, p[2] + h];
            if let Some(t) = interval(origin, direction, lo, hi)
                && t < limit - 0.001
            {
                limit = t;
                selected = Some(e.id);
            }
        }
    }
    Ok(selected)
}
pub fn use_target(c: &mut Client) -> Result<bool> {
    let Some(id) = target(c)? else {
        return Ok(false);
    };
    c.scene.engine.as_mut().unwrap().entities.interact(id);
    Ok(true)
}
pub fn attack_target(c: &mut Client) -> Result<bool> {
    let Some(id) = target(c)? else {
        return Ok(false);
    };
    c.scene.engine.as_mut().unwrap().entities.damage(id, 4);
    c.view.swing = 1.;
    Ok(true)
}
pub fn spawn(c: &mut Client, kind: &str, p: [f32; 3], persistent: bool) -> Result<Id> {
    let s = c.scene.engine.as_ref().ok_or("Entity world missing")?;
    let d = s.entities.library.get(kind)?;
    if !super::valid_position(p) || !c.scene.inside(p.map(|v| v.floor() as i32)) {
        return Err("Spawn position unavailable".into());
    }
    if let Some(b) = &d.body {
        let body = Physics {
            width: b.width,
            height: b.height,
            speed: b.speed,
            velocity: [0.; 3],
            grounded: false,
        };
        if !super::navigation::clear(c, p, &body) {
            return Err("Entity spawn overlaps solid terrain".into());
        }
        let overlap = |q: [f32; 3], w: f32, h: f32| {
            p[0] + b.width / 2. > q[0] - w / 2.
                && p[0] - b.width / 2. < q[0] + w / 2.
                && p[2] + b.width / 2. > q[2] - w / 2.
                && p[2] - b.width / 2. < q[2] + w / 2.
                && p[1] < q[1] + h
                && p[1] + b.height > q[1]
        };
        if overlap(
            position(c.actor.position),
            c.actor.body.width,
            c.actor.body.height,
        ) || s.entities.records.values().any(|e| {
            e.physics
                .as_ref()
                .is_some_and(|b| overlap(e.transform.position, b.width, b.height))
        }) {
            return Err("Entity spawn overlaps another actor".into());
        }
    }
    c.scene
        .engine
        .as_mut()
        .unwrap()
        .entities
        .spawn(kind, p, persistent)
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct TargetInfo {
    pub id: Id,
    pub label: String,
    pub current: u16,
    pub maximum: u16,
    pub following: bool,
}
pub fn label(c: &Client) -> Result<Option<TargetInfo>> {
    let Some(id) = target(c)? else {
        return Ok(None);
    };
    let s = c.scene.engine.as_ref().unwrap();
    let e = &s.entities.records[&id];
    let d = s.entities.library.get(&e.kind)?;
    let h = e.health.as_ref();
    Ok(Some(TargetInfo {
        id,
        label: d.label.clone(),
        current: h.map_or(0, |h| h.current),
        maximum: h.map_or(0, |h| h.maximum),
        following: e.brain.as_ref().is_some_and(|b| b.following),
    }))
}
