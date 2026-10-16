//! Player interaction adapter; shared block behaviors resolve placement.
use crate::engine::blocks::{
    behavior::placement::{self as policies, PlacementContext, neighbor, target_position},
    systems::collision,
};
use crate::{
    engine_blocks::{BlockId, Registry},
    scene::{Client, Result},
};
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
    policies::get(registry, selected).place(&PlacementContext {
        registry,
        selected,
        hit,
        yaw,
        target,
        adjacent,
    })
}
#[cfg(test)]
pub fn place(client: &mut Client, hit: &Hit) -> Result<bool> {
    place_hand(client, hit, crate::engine_inventory::Hand::Main)
}
pub fn place_hand(
    client: &mut Client,
    hit: &Hit,
    hand: crate::engine_inventory::Hand,
) -> Result<bool> {
    let state = client.scene.engine.as_ref().unwrap();
    let Some(stack) = state.player.hand(hand) else {
        return Ok(false);
    };
    let selected = stack.id;
    if state.registry.get(selected).container.is_some() {
        return Ok(false);
    }
    let hit_p = target_position(hit);
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
    for entity in state.entities.records.values() {
        if let Some(physics) = &entity.physics {
            let actor = physics.actor(entity.transform.position)?;
            if collision::intersects_actor(&state.geometry, id, p, &actor) {
                return Ok(false);
            }
        }
    }
    client.scene.change_engine(p, Some(id))?;
    client.changed += 1;
    Ok(true)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn facing_policy_rotates_furnace_front_and_is_reusable_by_other_families() {
        let mut r = Registry::built_in();
        let variants = r
            .blocks
            .iter()
            .filter(|b| b.state.as_ref().is_some_and(|s| s.block == "furnace"))
            .cloned()
            .collect::<Vec<_>>();
        for mut b in variants {
            b.id = b.id.replacen("furnace", "kiln", 1);
            b.state.as_mut().unwrap().block = "kiln".into();
            if let Some(crate::engine_blocks::Placement::Facing { states }) = &mut b.placement {
                for name in states {
                    *name = name.replacen("furnace", "kiln", 1);
                }
            }
            r.blocks.push(b);
        }
        r.validate().unwrap();
        for family in ["furnace", "kiln"] {
            for (quarter, dir, face) in [
                (0, "north", 2),
                (1, "west", 5),
                (2, "south", 3),
                (3, "east", 4),
            ] {
                let (_, id) = resolve(
                    &r,
                    r.find(family).unwrap(),
                    &hit(Face::Up, 1.),
                    quarter as f32 * std::f32::consts::FRAC_PI_2,
                    None,
                    None,
                )
                .unwrap()
                .unwrap();
                let b = r.get(id);
                assert_eq!(b.state.as_ref().unwrap().properties["facing"], dir);
                let bindings = b.face_textures.as_ref().unwrap();
                assert_eq!(bindings[face].face.as_deref(), Some("south"));
                assert_eq!(
                    bindings
                        .iter()
                        .filter(|v| v.face.as_deref() == Some("south"))
                        .count(),
                    1
                );
                assert!(
                    resolve(&r, id, &hit(Face::Up, 1.), 0., None, Some(id))
                        .unwrap()
                        .is_none()
                );
            }
        }
    }
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
    fn registry_with_stone_stairs() -> Registry {
        let mut r = Registry::built_in();
        let stone = r.get(r.find("slate").unwrap()).clone();
        let stairs: Vec<_> = r
            .blocks
            .iter()
            .filter(|b| {
                b.state
                    .as_ref()
                    .is_some_and(|state| state.block == "oak_stairs")
            })
            .cloned()
            .collect();
        for mut block in stairs {
            block.id = block.id.replace("oak_stairs", "stone_stairs");
            block.label = "Stone Stairs".into();
            block.state.as_mut().unwrap().block = "stone_stairs".into();
            block.color = stone.color;
            block.minecraft = stone.minecraft.clone();
            if let Some(crate::engine_blocks::Placement::Stairs { lower, upper }) =
                &mut block.placement
            {
                for name in lower.iter_mut().chain(upper) {
                    *name = name.replace("oak_stairs", "stone_stairs");
                }
            }
            r.blocks.push(block);
        }
        r.validate().unwrap();
        r
    }
    #[test]
    fn stairs_choose_all_eight_states_for_multiple_material_families() {
        let r = registry_with_stone_stairs();
        for material in ["oak", "stone"] {
            let selected = r.find(&format!("{material}_stairs_north")).unwrap();
            for (q, dir) in ["north", "west", "south", "east"].iter().enumerate() {
                for (h, suffix) in [(hit(Face::Up, 1.), ""), (hit(Face::Down, 0.), "_upper")] {
                    let (_, id) = resolve(
                        &r,
                        selected,
                        &h,
                        q as f32 * std::f32::consts::FRAC_PI_2,
                        None,
                        None,
                    )
                    .unwrap()
                    .unwrap();
                    assert_eq!(r.get(id).id, format!("{material}_stairs_{dir}{suffix}"));
                }
            }
        }
    }
    #[test]
    fn half_selection_is_shared_and_occupied_neighbors_are_protected() {
        let r = registry_with_stone_stairs();
        for (selected, lower, upper) in [
            ("oak_slab", "oak_slab", "oak_slab_upper"),
            ("stone_slab", "stone_slab", "stone_slab_upper"),
            (
                "oak_stairs_north",
                "oak_stairs_north",
                "oak_stairs_north_upper",
            ),
            (
                "stone_stairs_north",
                "stone_stairs_north",
                "stone_stairs_north_upper",
            ),
        ] {
            let selected = r.find(selected).unwrap();
            for (height, expected) in [(0.5, lower), (0.5001, upper)] {
                let (position, id) =
                    resolve(&r, selected, &hit(Face::East, height), 0., None, None)
                        .unwrap()
                        .unwrap();
                assert_eq!(position, [3, 4, 2]);
                assert_eq!(r.get(id).id, expected);
            }
        }
        let occupied = r.find("earth").unwrap();
        for selected in ["earth", "oak_stairs_north", "stone_stairs_north"] {
            assert!(
                resolve(
                    &r,
                    r.find(selected).unwrap(),
                    &hit(Face::Up, 1.),
                    0.,
                    None,
                    Some(occupied)
                )
                .unwrap()
                .is_none()
            );
        }
    }
}
