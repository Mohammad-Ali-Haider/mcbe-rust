//! Player interaction adapter; shared block behaviors resolve placement.
use crate::engine::blocks::{
    behavior::{self, PlacementContext},
    systems::collision,
};
use crate::{
    engine_blocks::{BlockId, Registry},
    scene::{Client, Result},
};
use behavior::placement::neighbor;
#[cfg(test)]
use mcbe_core::raycast::Face;
use mcbe_core::raycast::Hit;
pub fn resolve(
    registry: &Registry,
    selected: BlockId,
    hit: &Hit,
    yaw: f32,
    target: Option<BlockId>,
    adjacent: Option<BlockId>,
) -> Result<Option<([i32; 3], BlockId)>> {
    behavior::get(registry, selected).place(&PlacementContext {
        registry,
        selected,
        hit,
        yaw,
        target,
        adjacent,
    })
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
    if collision::intersects_actor(&state.geometry, id, p, &client.actor) {
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
