//! Reusable hit-height, heading and slab-combination placement policies.
use crate::{
    engine_blocks::{BlockId, Placement, Registry},
    scene::{Client, Result},
};
use mcbe_core::raycast::{Face, Hit};
fn upper(hit: &Hit) -> bool {
    hit.face == Face::Down || (hit.face != Face::Up && hit.face_location.y > 0.5)
}
fn neighbor(hit: &Hit) -> [i32; 3] {
    let mut p = [hit.position.x, hit.position.y, hit.position.z];
    let (axis, step) = match hit.face {
        Face::Up => (1, 1),
        Face::Down => (1, -1),
        Face::East => (0, 1),
        Face::West => (0, -1),
        Face::North => (2, -1),
        Face::South => (2, 1),
    };
    p[axis] += step;
    p
}
/// Pure resolver shared by interaction tests and the live client.
pub fn resolve(
    registry: &Registry,
    selected: BlockId,
    hit: &Hit,
    yaw: f32,
    target: Option<BlockId>,
    adjacent: Option<BlockId>,
) -> Result<Option<([i32; 3], BlockId)>> {
    let mut p = neighbor(hit);
    let id = match &registry.get(selected).placement {
        Some(Placement::Slab {
            lower,
            upper: top,
            double,
        }) => {
            let lo = registry.find(lower)?;
            let hi = registry.find(top)?;
            let both = registry.find(double)?;
            if (target == Some(lo) && hit.face == Face::Up)
                || (target == Some(hi) && hit.face == Face::Down)
            {
                p = [hit.position.x, hit.position.y, hit.position.z];
                both
            } else {
                let id = if upper(hit) { hi } else { lo };
                match adjacent {
                    None => id,
                    Some(old) if (old == lo && id == hi) || (old == hi && id == lo) => both,
                    _ => return Ok(None),
                }
            }
        }
        Some(Placement::Stairs { lower, upper: top }) => {
            if adjacent.is_some() {
                return Ok(None);
            }
            // North, east, south, west are the directions of the upper step.
            let heading =
                ((yaw / std::f32::consts::FRAC_PI_2).round() as i32).rem_euclid(4) as usize;
            let index = [0, 3, 2, 1][heading];
            registry.find(&(if upper(hit) { top } else { lower })[index])?
        }
        None => {
            if adjacent.is_some() {
                return Ok(None);
            }
            selected
        }
    };
    Ok(Some((p, id)))
}
pub fn place(client: &mut Client, hit: &Hit) -> Result<bool> {
    let state = client.scene.engine.as_ref().unwrap();
    let Some(selected) = state.player.selected() else {
        return Ok(false);
    };
    let hit_p = [hit.position.x, hit.position.y, hit.position.z];
    let Some((p, id)) = resolve(
        &state.registry,
        selected,
        hit,
        client.yaw,
        state.blocks.get(&hit_p).copied(),
        state.blocks.get(&neighbor(hit)).copied(),
    )?
    else {
        return Ok(false);
    };
    if !client.scene.inside(p) {
        return Ok(false);
    }
    let a = client.actor.position;
    let half = client.actor.body.width * 0.5;
    let low = [a.x - half, a.y, a.z - half];
    let high = [a.x + half, a.y + client.actor.body.height, a.z + half];
    if state
        .geometry
        .collision(id)
        .iter()
        .any(|b| (0..3).all(|n| low[n] < p[n] as f32 + b[n + 3] && high[n] > p[n] as f32 + b[n]))
    {
        return Ok(false);
    }
    client.scene.change_engine(p, Some(id))?;
    client.changed += 1;
    Ok(true)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_slab_merge_checks_expanded_collision_before_mutating() {
        let mut c = crate::engine::generate(360036).unwrap();
        let r = &c.scene.engine.as_ref().unwrap().registry;
        let lo = r.find("oak_slab").unwrap();
        let double = r.find("oak_slab_double").unwrap();
        c.scene
            .engine
            .as_mut()
            .unwrap()
            .player
            .assign(lo, 0)
            .unwrap();
        c.scene.engine.as_mut().unwrap().player.select(0).unwrap();
        c.scene.change_engine([2, 4, 2], Some(lo)).unwrap();
        c.set_spawn(mcbe_core::motion::Vec3::new(2.5, 4.5, 2.5))
            .unwrap();
        let h = hit(Face::Up, 0.5);
        assert!(!place(&mut c, &h).unwrap());
        assert_eq!(c.scene.engine.as_ref().unwrap().blocks[&[2, 4, 2]], lo);
        c.set_spawn(crate::engine::SPAWN).unwrap();
        assert!(place(&mut c, &h).unwrap());
        assert_eq!(c.scene.engine.as_ref().unwrap().blocks[&[2, 4, 2]], double);
    }
    fn hit(face: Face, y: f32) -> Hit {
        Hit {
            id: "test".into(),
            position: [2, 4, 2].into(),
            face,
            face_location: [0.5, y, 0.5].into(),
        }
    }
    #[test]
    fn slabs_place_by_height_combine_only_matching_halves() {
        let r = Registry::built_in();
        let lo = r.find("oak_slab").unwrap();
        let hi = r.find("oak_slab_upper").unwrap();
        let double = r.find("oak_slab_double").unwrap();
        for (h, want) in [
            (hit(Face::Up, 1.), lo),
            (hit(Face::Down, 0.), hi),
            (hit(Face::East, 0.75), hi),
            (hit(Face::East, 0.25), lo),
        ] {
            assert_eq!(
                resolve(&r, lo, &h, 0., None, None).unwrap().unwrap().1,
                want
            );
        }
        assert_eq!(
            resolve(&r, lo, &hit(Face::Up, 0.5), 0., Some(lo), None).unwrap(),
            Some(([2, 4, 2], double))
        );
        assert_eq!(
            resolve(&r, lo, &hit(Face::Down, 0.5), 0., Some(hi), None).unwrap(),
            Some(([2, 4, 2], double))
        );
        assert_eq!(
            resolve(&r, lo, &hit(Face::East, 0.75), 0., None, Some(lo))
                .unwrap()
                .unwrap()
                .1,
            double
        );
        assert!(
            resolve(&r, lo, &hit(Face::Up, 1.), 0., None, Some(lo))
                .unwrap()
                .is_none()
        );
        assert!(
            resolve(
                &r,
                lo,
                &hit(Face::East, 0.75),
                0.,
                None,
                Some(r.find("stone_slab").unwrap())
            )
            .unwrap()
            .is_none()
        );
    }
    #[test]
    fn stairs_choose_all_eight_registered_shapes_and_reverse_heading() {
        let r = Registry::built_in();
        let selected = r.find("oak_stairs_north").unwrap();
        for (q, dir) in ["north", "west", "south", "east"].iter().enumerate() {
            for (h, suffix) in [(hit(Face::Up, 1.), ""), (hit(Face::Down, 0.), "_upper")] {
                let id = resolve(
                    &r,
                    selected,
                    &h,
                    q as f32 * std::f32::consts::FRAC_PI_2,
                    None,
                    None,
                )
                .unwrap()
                .unwrap()
                .1;
                assert_eq!(r.get(id).id, format!("oak_stairs_{dir}{suffix}"));
            }
        }
    }
}
