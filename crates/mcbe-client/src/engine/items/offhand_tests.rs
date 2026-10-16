//! Owned offhand state across transfers, world actions, migration and rendering.
use super::{ItemData, bucket};
use crate::{
    engine,
    engine_inventory::{Button, Hand, Inventory, Slot, Stack},
};
use mcbe_core::raycast::{Face, Hit};
fn bucket_stack(c: &crate::scene::Client, amount: u16) -> Stack {
    let s = c.scene.engine.as_ref().unwrap();
    let mut stack = s.registry.catalog_stack(s.registry.find("bucket").unwrap());
    stack.data = Some(ItemData::Fluid {
        kind: engine::blocks::components::FluidKind::Water,
        amount,
    });
    stack
}
#[test]
fn offhand_transfers_swaps_and_generated_sequences_preserve_owned_contents() {
    let c = engine::generate(210).unwrap();
    let bucket = bucket_stack(&c, 417);
    let mut p = Inventory::starter(8, 0);
    p.offhand = Some(bucket);
    let count = p.total();
    let mut random = 21u64;
    for _ in 0..5000 {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        let i = (random % 50) as usize;
        let slot = match i {
            0..=35 => Slot::Player(i),
            36..=39 => Slot::Craft(i - 36),
            40..=48 => Slot::Table(i - 40),
            _ => Slot::Offhand,
        };
        match random % 5 {
            0 => {
                p.swap_hands().unwrap();
            }
            1 => {
                p.swap_hotbar(slot, (random % 9) as usize).unwrap();
            }
            _ => {
                p.click(
                    slot,
                    if random & 64 == 0 {
                        Button::Left
                    } else {
                        Button::Right
                    },
                    random & 256 != 0,
                )
                .unwrap();
            }
        }
        p.validate_items(&c.scene.engine.as_ref().unwrap().registry)
            .unwrap();
        assert_eq!(p.total(), count);
        assert_eq!(
            p.slots
                .iter()
                .chain(&p.crafting)
                .chain(&p.table)
                .chain([&p.cursor, &p.offhand])
                .flatten()
                .filter(|v| v.data == bucket.data)
                .copied()
                .collect::<Vec<_>>(),
            vec![bucket]
        );
    }
}
#[test]
fn full_inventory_rejects_offhand_quick_move_without_losing_contents() {
    let c = engine::generate(211).unwrap();
    let mut p = Inventory::empty();
    p.slots = [Some(Stack {
        id: 0,
        count: 64,
        data: None,
    }); 36];
    p.offhand = Some(bucket_stack(&c, 901));
    let before = p.clone();
    assert!(!p.quick_move(Slot::Offhand).unwrap());
    assert_eq!(p.changed_slots(&before), 0);
    assert_eq!(p.revision, before.revision);
    p.slots[35] = None;
    assert!(p.quick_move(Slot::Offhand).unwrap());
    assert!(p.offhand.is_none());
    assert_eq!(p.slots[35], before.offhand);
}
#[test]
fn explicit_and_empty_main_hand_selection_are_predictable() {
    let mut p = Inventory::empty();
    assert_eq!(p.use_hand(false), Hand::Off);
    p.slots[0] = Some(Stack {
        id: 0,
        count: 64,
        data: None,
    });
    assert_eq!(p.use_hand(false), Hand::Main);
    assert_eq!(p.use_hand(true), Hand::Off);
    let before = p.clone();
    p.swap_hands().unwrap();
    assert_eq!(p.hand(Hand::Off), before.hand(Hand::Main));
    assert_eq!(p.changed_slots(&before), 2);
    p.swap_hands().unwrap();
    assert_eq!(p.changed_slots(&before), 0);
}
#[test]
fn offhand_bucket_actions_conserve_fluid_and_never_replace_main_stack() {
    let mut c = engine::generate(212).unwrap();
    let bucket = bucket_stack(&c, 0);
    let s = c.scene.engine.as_mut().unwrap();
    s.player.offhand = Some(bucket);
    let main = s.player.hand(Hand::Main);
    let water = s.registry.find("water").unwrap();
    c.scene.set_fluid([2, 6, 2], water, 333).unwrap();
    let hit = Hit {
        id: "water".into(),
        position: [2, 6, 2].into(),
        face: Face::Up,
        face_location: [0.5, 1., 0.5].into(),
    };
    let target = bucket::Target { hit, fluid: true };
    assert!(bucket::apply_hand(&mut c, &target, false, Hand::Off).unwrap());
    let s = c.scene.engine.as_ref().unwrap();
    assert_eq!(s.player.hand(Hand::Main), main);
    assert_eq!(
        s.player.offhand.unwrap().data,
        Some(ItemData::Fluid {
            kind: engine::blocks::components::FluidKind::Water,
            amount: 333
        })
    );
    assert_eq!(engine::fluids::amount(s, [2, 6, 2]), 0);
    c.scene.set_fluid([2, 6, 2], water, 900).unwrap();
    assert!(bucket::apply_hand(&mut c, &target, true, Hand::Off).unwrap());
    let s = c.scene.engine.as_ref().unwrap();
    assert_eq!(engine::fluids::amount(s, [2, 6, 2]), 1024);
    assert_eq!(
        s.player.offhand.unwrap().data,
        Some(ItemData::Fluid {
            kind: engine::blocks::components::FluidKind::Water,
            amount: 209
        })
    );
    assert_eq!(s.player.hand(Hand::Main), main);
    assert_eq!(c.view.swing_hand, Hand::Off);
}
#[test]
fn offhand_uses_shared_stateful_slab_placement() {
    let mut c = engine::generate(213).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    let slab = s.registry.find("oak_slab").unwrap();
    let double = s.registry.find("oak_slab_double").unwrap();
    s.player.offhand = Some(s.registry.catalog_stack(slab));
    let main = s.player.hand(Hand::Main);
    c.scene.change_engine([2, 4, 2], Some(slab)).unwrap();
    let hit = Hit {
        id: "oak_slab".into(),
        position: [2, 4, 2].into(),
        face: Face::Up,
        face_location: [0.5, 0.5, 0.5].into(),
    };
    assert!(crate::engine_placement::place_hand(&mut c, &hit, Hand::Off).unwrap());
    let s = c.scene.engine.as_ref().unwrap();
    assert_eq!(s.blocks[&[2, 4, 2]], double);
    assert_eq!(s.player.hand(Hand::Main), main);
}
#[test]
fn schema_twelve_preserves_contents_rejects_invalid_offhands_and_migrates_eleven() {
    let mut c = engine::generate(214).unwrap();
    c.scene.engine.as_mut().unwrap().player.offhand = Some(bucket_stack(&c, 731));
    let snapshot = engine::SaveSnapshot::capture(&c).unwrap();
    c.scene
        .engine
        .as_mut()
        .unwrap()
        .player
        .swap_hands()
        .unwrap();
    let value = serde_json::to_value(snapshot.document().unwrap()).unwrap();
    assert_eq!(value["version"], 12);
    let restored = engine::decode(&serde_json::to_vec(&value).unwrap(), None).unwrap();
    assert_eq!(
        restored.scene.engine.as_ref().unwrap().player.offhand,
        Some(bucket_stack(&restored, 731))
    );
    assert_ne!(
        c.scene.engine.as_ref().unwrap().player.offhand,
        restored.scene.engine.as_ref().unwrap().player.offhand
    );
    for (field, value) in [
        ("count", serde_json::json!(2)),
        ("block", serde_json::json!("missing")),
    ] {
        let mut invalid = serde_json::to_value(snapshot.document().unwrap()).unwrap();
        invalid["inventory"]["offhand"][field] = value;
        assert!(engine::decode(&serde_json::to_vec(&invalid).unwrap(), None).is_err());
    }
    let mut legacy = value.clone();
    legacy["version"] = serde_json::json!(11);
    assert!(engine::decode(&serde_json::to_vec(&legacy).unwrap(), None).is_err());
    legacy["inventory"]
        .as_object_mut()
        .unwrap()
        .remove("offhand");
    let restored = engine::decode(&serde_json::to_vec(&legacy).unwrap(), None).unwrap();
    assert!(
        restored
            .scene
            .engine
            .as_ref()
            .unwrap()
            .player
            .offhand
            .is_none()
    );
}
#[test]
fn definition_reload_remaps_offhand_by_name_and_rejects_removing_its_type_atomically() {
    let mut c = engine::generate(215).unwrap();
    let held = bucket_stack(&c, 251);
    c.scene.engine.as_mut().unwrap().player.offhand = Some(held);
    let mut r = c.scene.engine.as_ref().unwrap().registry.clone();
    r.blocks.reverse();
    engine::apply_registry(&mut c, r.clone()).unwrap();
    let s = c.scene.engine.as_ref().unwrap();
    assert_eq!(s.registry.get(s.player.offhand.unwrap().id).id, "bucket");
    assert_eq!(s.player.offhand.unwrap().data, held.data);
    let before = s.player.clone();
    r.blocks.retain(|b| b.id != "bucket");
    assert!(engine::apply_registry(&mut c, r).is_err());
    assert_eq!(
        c.scene
            .engine
            .as_ref()
            .unwrap()
            .player
            .changed_slots(&before),
        0
    );
}
#[test]
fn dual_hand_geometry_is_separate_and_only_used_arm_animates() {
    let mut c = engine::generate(216).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    s.player.offhand = Some(
        s.registry
            .catalog_stack(s.registry.find("blue_glass").unwrap()),
    );
    let main = engine::render::character::item_hand(&c, [0.; 3], true, Hand::Main);
    let off = engine::render::character::item_hand(&c, [0.; 3], true, Hand::Off);
    assert!(main.iter().all(|v| v.position[0] > 0.));
    assert!(off.iter().all(|v| v.position[0] < 0.));
    assert_eq!(
        engine::render::character::item(&c, [0.; 3], true).len(),
        main.len() + off.len()
    );
    assert_eq!(
        engine::render::character::hand(&c, [0., 0., 1., 1.]).len(),
        144
    );
    c.view.swing = 0.5;
    c.view.swing_hand = Hand::Off;
    let still = engine::render::character::item_hand(&c, [0.; 3], true, Hand::Main);
    let moving = engine::render::character::item_hand(&c, [0.; 3], true, Hand::Off);
    assert!(
        still
            .iter()
            .zip(main)
            .all(|(a, b)| a.position == b.position)
    );
    assert!(
        moving
            .iter()
            .zip(off)
            .any(|(a, b)| a.position != b.position)
    );
}
