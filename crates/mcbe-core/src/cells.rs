//! Collision over finite, explicitly supplied saved-cell observations.
//! Database scenes share the same bounded broad phase; missing observations are never air.
use crate::{
    Result,
    motion::{Actor, SolidBox, SolidScene, Vec3},
};
use std::collections::HashMap;
pub struct CellScene {
    pub cells: HashMap<[i32; 3], bool>,
}
impl CellScene {
    pub fn scene_for(&self, actor: &Actor) -> Result<SolidScene> {
        scene_from(actor, |position| {
            self.cells
                .get(&position)
                .copied()
                .ok_or_else(|| format!("Unrecorded cell: {position:?}"))
        })
    }
}
pub fn scene_from(
    actor: &Actor,
    mut lookup: impl FnMut([i32; 3]) -> Result<bool>,
) -> Result<SolidScene> {
    const UNIT: [[f32; 6]; 1] = [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]];
    scene_from_shapes(actor, |p| Ok(if lookup(p)? { &UNIT[..] } else { &[] }))
}

/// Query exact local collision boxes without treating missing cells as air.
pub fn scene_from_shapes<B: AsRef<[[f32; 6]]>>(
    actor: &Actor,
    mut lookup: impl FnMut([i32; 3]) -> Result<B>,
) -> Result<SolidScene> {
    let p = actor.position;
    let v = actor.velocity;
    let half = actor.body.width * 0.5;
    let low = Vec3::new(p.x - half, p.y, p.z - half);
    let high = Vec3::new(p.x + half, p.y + actor.body.height, p.z + half);
    let mut starts = [0; 3];
    let mut ends = [0; 3];
    let mut volume = 1_u64;
    for a in 0..3 {
        let start = f64::from((low[a] + v[a].min(0.0)).floor()) - 1.0;
        let end = f64::from((high[a] + v[a].max(if a == 1 { 0.5625 } else { 0.0 })).ceil()) + 1.0;
        if !start.is_finite()
            || !end.is_finite()
            || start < f64::from(i32::MIN)
            || end > f64::from(i32::MAX)
            || end <= start
        {
            return Err("Collision query exceeds integer coordinate bounds".into());
        }
        starts[a] = start as i32;
        ends[a] = end as i32;
        volume = volume
            .checked_mul((i64::from(ends[a]) - i64::from(starts[a])) as u64)
            .ok_or("Collision query volume overflow")?;
        if volume > 1_000_000 {
            return Err("Collision query exceeds 1000000 cells".into());
        }
    }
    let mut boxes = Vec::new();
    for x in starts[0]..ends[0] {
        for y in starts[1]..ends[1] {
            for z in starts[2]..ends[2] {
                for bounds in lookup([x, y, z])?.as_ref() {
                    if bounds.iter().any(|v| !v.is_finite())
                        || (0..3).any(|a| {
                            bounds[a] < 0.0 || bounds[a] >= bounds[a + 3] || bounds[a + 3] > 1.0
                        })
                    {
                        return Err("Invalid local collision box".into());
                    }
                    boxes.push(SolidBox::new(
                        Vec3::new(
                            x as f32 + bounds[0],
                            y as f32 + bounds[1],
                            z as f32 + bounds[2],
                        ),
                        Vec3::new(
                            x as f32 + bounds[3],
                            y as f32 + bounds[4],
                            z as f32 + bounds[5],
                        ),
                    )?);
                }
            }
        }
    }
    Ok(SolidScene { boxes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::Body;

    #[test]
    fn excessive_queries_fail_before_lookup() {
        for (position, velocity) in [
            (Vec3::new(f32::MAX, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.0)),
            (Vec3::new(0.0, 0.0, 0.0), Vec3::new(1_000_000.0, 0.0, 0.0)),
        ] {
            let actor = Actor::new(position, Body::PROBE)
                .unwrap()
                .impulse(velocity)
                .unwrap();
            assert!(scene_from(&actor, |_| panic!("Lookup before bounds check")).is_err());
        }
    }

    #[test]
    fn partial_boxes_stop_a_fall_at_their_actual_top() {
        let slab = [[0.0, 0.0, 0.0, 1.0, 0.6875, 1.0]];
        let mut actor = Actor::new(Vec3::new(0.5, 2.0, 0.5), Body::PROBE).unwrap();
        for _ in 0..30 {
            let scene =
                scene_from_shapes(&actor, |p| Ok(if p == [0, 0, 0] { &slab[..] } else { &[] }))
                    .unwrap();
            actor = actor.tick_solid(&scene).unwrap();
        }
        assert_eq!(actor.position.y, 0.6875);
        assert!(actor.on_ground);
        assert!(
            scene_from_shapes(&actor, |_| Err::<&[[f32; 6]], _>("Unobserved cell".into())).is_err()
        );
    }

    #[test]
    fn local_shapes_cannot_escape_their_cells() {
        let actor = Actor::new(Vec3::new(0.5, 2.0, 0.5), Body::PROBE).unwrap();
        for invalid in [
            [0.0, 0.0, 0.0, 1.1, 1.0, 1.0],
            [0.0, 0.0, 0.0, f32::NAN, 1.0, 1.0],
            [0.5, 0.0, 0.0, 0.5, 1.0, 1.0],
        ] {
            let boxes = [invalid];
            assert!(scene_from_shapes(&actor, |_| Ok(&boxes)).is_err());
        }
    }
}
