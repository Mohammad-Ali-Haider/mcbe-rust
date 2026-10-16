use super::*;
use crate::{
    engine::fluids,
    engine_inventory::{Button, Slot},
    scene::Client,
};
use mcbe_core::{
    motion::Vec3,
    raycast::{Face, Hit},
};
fn setup() -> (Client, BlockId) {
    let mut c = crate::engine::generate(77).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    s.blocks.clear();
    let w = s.registry.find("water").unwrap();
    let b = s.registry.find("bucket").unwrap();
    s.player
        .assign_item(s.registry.catalog_stack(b), 0)
        .unwrap();
    s.player.select(0).unwrap();
    (c, w)
}
fn aim(p: [i32; 3], fluid: bool) -> bucket::Target {
    bucket::Target {
        hit: Hit {
            id: "test".into(),
            position: p.into(),
            face: Face::Up,
            face_location: [0.5, 1., 0.5].into(),
        },
        fluid,
    }
}
fn amount(s: Stack) -> u16 {
    match s.data {
        Some(ItemData::Fluid { amount, .. }) => amount,
        None => 0,
    }
}
fn mass(c: &Client) -> u64 {
    let s = c.scene.engine.as_ref().unwrap();
    s.blocks
        .keys()
        .map(|p| u64::from(fluids::amount(s, p)))
        .sum::<u64>()
        + s.player
            .slots
            .iter()
            .chain(&s.player.crafting)
            .chain(&s.player.table)
            .chain(std::iter::once(&s.player.cursor))
            .flatten()
            .map(|s| u64::from(amount(*s)))
            .sum::<u64>()
}
#[test]
fn shallow_collect_and_partial_pour_conserve_world_plus_vessel() {
    let (mut c, w) = setup();
    c.scene.set_fluid([0, 5, 0], w, 333).unwrap();
    assert!(bucket::apply(&mut c, &aim([0, 5, 0], true), false).unwrap());
    assert_eq!(mass(&c), 333);
    assert_eq!(
        fluids::amount(c.scene.engine.as_ref().unwrap(), [0, 5, 0]),
        0
    );
    c.scene.set_fluid([1, 5, 0], w, 900).unwrap();
    let initial = mass(&c);
    assert!(bucket::apply(&mut c, &aim([1, 5, 0], true), true).unwrap());
    assert_eq!(mass(&c), initial);
    let s = c.scene.engine.as_ref().unwrap();
    assert_eq!(amount(s.player.slots[0].unwrap()), 209);
    assert_eq!(fluids::amount(s, [1, 5, 0]), 1024);
    let stone = c
        .scene
        .engine
        .as_ref()
        .unwrap()
        .registry
        .find("slate")
        .unwrap();
    c.scene.change_engine([2, 4, 0], Some(stone)).unwrap();
    assert!(bucket::apply(&mut c, &aim([2, 4, 0], false), false).unwrap());
    assert_eq!(mass(&c), initial);
    assert_eq!(
        amount(c.scene.engine.as_ref().unwrap().player.slots[0].unwrap()),
        0
    );
}
#[test]
fn filled_bucket_blocked_world_write_is_atomic_and_never_places_bucket() {
    let (mut c, w) = setup();
    c.scene.set_fluid([0, 5, 0], w, fluids::FULL).unwrap();
    bucket::apply(&mut c, &aim([0, 5, 0], true), false).unwrap();
    let s = c.scene.engine.as_ref().unwrap();
    let stone = s.registry.find("slate").unwrap();
    let b = s.registry.find("bucket").unwrap();
    let prior = s.player.clone();
    c.scene.change_engine([1, 4, 0], Some(stone)).unwrap();
    c.scene.change_engine([1, 5, 0], Some(stone)).unwrap();
    assert!(!bucket::apply(&mut c, &aim([1, 4, 0], false), false).unwrap());
    assert_eq!(
        c.scene
            .engine
            .as_ref()
            .unwrap()
            .player
            .changed_slots(&prior),
        0
    );
    assert_eq!(mass(&c), 1024);
    assert!(c.scene.change_engine([3, 5, 0], Some(b)).is_err());
}
#[test]
fn ray_hits_fractional_water_stops_at_wall_and_traverses_slab_gaps() {
    let (mut c, w) = setup();
    c.set_spawn(Vec3::new(0.5, 4., 0.5)).unwrap();
    c.pitch = 0.;
    c.yaw = 0.;
    let y = c.eye()[1].floor() as i32;
    c.scene.set_fluid([0, y, 2], w, 256).unwrap();
    assert!(bucket::target(&c).unwrap().is_none());
    c.scene.set_fluid([0, y, 2], w, 1024).unwrap();
    assert!(bucket::target(&c).unwrap().unwrap().fluid);
    let stone = c
        .scene
        .engine
        .as_ref()
        .unwrap()
        .registry
        .find("slate")
        .unwrap();
    let slab = c
        .scene
        .engine
        .as_ref()
        .unwrap()
        .registry
        .find("oak_slab")
        .unwrap();
    c.scene.change_engine([0, y, 1], Some(stone)).unwrap();
    assert!(!bucket::target(&c).unwrap().unwrap().fluid);
    c.scene.change_engine([0, y, 1], Some(slab)).unwrap();
    assert!(bucket::target(&c).unwrap().unwrap().fluid);
    assert!(c.edit(true).unwrap());
    assert_eq!(mass(&c), 1024);
    assert_eq!(
        amount(c.scene.engine.as_ref().unwrap().player.slots[0].unwrap()),
        1024
    );
}
#[test]
fn container_moves_never_merge_or_drop_contents() {
    let (mut c, w) = setup();
    c.scene.set_fluid([0, 5, 0], w, 731).unwrap();
    bucket::apply(&mut c, &aim([0, 5, 0], true), false).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    let b = s.registry.find("bucket").unwrap();
    s.player.slots[1] = Some(s.registry.catalog_stack(b));
    let full = s.player.slots[0];
    let mut seed = 91u64;
    for _ in 0..400 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let i = (seed % 40) as usize;
        let slot = if i < 36 {
            Slot::Player(i)
        } else {
            Slot::Craft(i - 36)
        };
        s.player
            .click(
                slot,
                if seed & 64 == 0 {
                    Button::Left
                } else {
                    Button::Right
                },
                seed & 128 != 0,
            )
            .unwrap();
        s.player.validate_items(&s.registry).unwrap();
        assert_eq!(
            s.player
                .slots
                .iter()
                .chain(&s.player.crafting)
                .chain(std::iter::once(&s.player.cursor))
                .flatten()
                .map(|v| u64::from(amount(*v)))
                .sum::<u64>(),
            731
        );
    }
    assert!(s.player.close_inventory());
    assert!(s.player.slots.contains(&full));
    assert_eq!(s.player.total(), 7 * 64 + 2);
}
#[test]
fn custom_capacity_validation_and_held_fill_geometry() {
    let (mut c, _) = setup();
    let s = c.scene.engine.as_mut().unwrap();
    let id = s.registry.find("bucket").unwrap();
    let mut r = s.registry.clone();
    r.blocks[id as usize].container.as_mut().unwrap().capacity = 400;
    r.validate().unwrap();
    let mut st = r.catalog_stack(id);
    assert_eq!(st.limit(), 1);
    st.data = Some(ItemData::Fluid {
        kind: FluidKind::Water,
        amount: 401,
    });
    assert!(st.validate_item(&r).is_err());
    st.count = 2;
    st.data = Some(ItemData::Fluid {
        kind: FluidKind::Water,
        amount: 300,
    });
    assert!(st.validate_item(&r).is_err());
    st.count = 1;
    assert!(st.validate_item(&r).is_ok());
    let empty = mesh(s, s.registry.catalog_stack(id));
    let filled = mesh(s, st);
    assert_eq!(filled.len(), empty.len() + 6);
    assert!(
        filled
            .iter()
            .all(|v| v.position.iter().all(|v| v.is_finite()))
    );
    r.blocks[id as usize].container.as_mut().unwrap().capacity = 0;
    assert!(r.validate().is_err());
}
#[test]
fn save_reload_and_registry_reorder_preserve_all_item_data() {
    let mut c = crate::engine::generate(78).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    let b = s.registry.find("bucket").unwrap();
    let mut stack = s.registry.catalog_stack(b);
    stack.data = Some(ItemData::Fluid {
        kind: FluidKind::Water,
        amount: 713,
    });
    s.player.slots[0] = Some(stack);
    s.player.active = 0;
    s.player.crafting[3] = Some(stack);
    s.player.table[8] = Some(stack);
    s.player.cursor = Some(stack);
    let dir = std::env::temp_dir().join(format!(
        "mcbe-bucket-{}-{}",
        std::process::id(),
        crate::engine::noise(1, 2, 78)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let saved = crate::engine::save(&mut c, &dir).unwrap();
    let d: serde_json::Value = serde_json::from_slice(&std::fs::read(&saved).unwrap()).unwrap();
    assert_eq!(d["version"], 11);
    let mut loaded = crate::engine::load(&saved).unwrap();
    let p = &loaded.scene.engine.as_ref().unwrap().player;
    assert_eq!(p.changed_slots(&c.scene.engine.as_ref().unwrap().player), 0);
    assert_eq!(amount(p.cursor.unwrap()), 713);
    let mut r = loaded.scene.engine.as_ref().unwrap().registry.clone();
    r.blocks.reverse();
    crate::engine::apply_registry(&mut loaded, r).unwrap();
    let s = loaded.scene.engine.as_ref().unwrap();
    assert_eq!(s.registry.get(s.player.cursor.unwrap().id).id, "bucket");
    assert_eq!(amount(s.player.cursor.unwrap()), 713);
    let mut r = s.registry.clone();
    let b = r.find("bucket").unwrap();
    r.blocks[b as usize].container = None;
    assert!(crate::engine::apply_registry(&mut loaded, r).is_err());
    assert_eq!(
        amount(loaded.scene.engine.as_ref().unwrap().player.cursor.unwrap()),
        713
    );
}

#[test]
fn malformed_item_saves_reject_payload_loss_and_legacy_data() {
    let mut c = crate::engine::generate(79).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    let b = s.registry.find("bucket").unwrap();
    let mut stack = s.registry.catalog_stack(b);
    stack.data = Some(ItemData::Fluid {
        kind: FluidKind::Water,
        amount: 400,
    });
    s.player.slots[0] = Some(stack);
    s.player.active = 0;
    let doc = serde_json::to_value(crate::engine::document(&c).unwrap()).unwrap();
    for (field, value) in [
        ("amount", serde_json::json!(1025)),
        ("amount", serde_json::json!(-1)),
        ("kind", serde_json::json!("lava")),
    ] {
        let mut d = doc.clone();
        d["inventory"]["slots"][0]["data"][field] = value;
        assert!(crate::engine::decode(&serde_json::to_vec(&d).unwrap(), None).is_err());
    }
    for count in [0, 2, 64] {
        let mut d = doc.clone();
        d["inventory"]["slots"][0]["count"] = serde_json::json!(count);
        assert!(crate::engine::decode(&serde_json::to_vec(&d).unwrap(), None).is_err());
    }
    let mut d = doc.clone();
    d["version"] = serde_json::json!(8);
    d.as_object_mut().unwrap().remove("entities");
    assert!(crate::engine::decode(&serde_json::to_vec(&d).unwrap(), None).is_err());
    let mut d = doc.clone();
    d["inventory"]["slots"][0]
        .as_object_mut()
        .unwrap()
        .remove("data");
    assert!(crate::engine::decode(&serde_json::to_vec(&d).unwrap(), None).is_err());
    let mut d = doc.clone();
    d["inventory"]["slots"][0]["block"] = serde_json::json!("slate");
    assert!(crate::engine::decode(&serde_json::to_vec(&d).unwrap(), None).is_err());
    let plain = crate::engine::generate(80).unwrap();
    let mut d = serde_json::to_value(crate::engine::document(&plain).unwrap()).unwrap();
    d["version"] = serde_json::json!(8);
    d.as_object_mut().unwrap().remove("entities");
    assert!(crate::engine::decode(&serde_json::to_vec(&d).unwrap(), None).is_ok());
}
#[test]
fn bucket_contents_persist_in_a_machine_and_cannot_be_consumed_by_crafting() {
    let mut c = crate::engine::generate(81).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    let b = s.registry.find("bucket").unwrap();
    let furnace = s.registry.find("furnace").unwrap();
    let mut stack = s.registry.catalog_stack(b);
    stack.data = Some(ItemData::Fluid {
        kind: FluidKind::Water,
        amount: 477,
    });
    c.scene.change_engine([0, 8, 0], Some(furnace)).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    s.machines.ensure([0, 8, 0]).unwrap();
    s.machines.furnaces.get_mut(&[0, 8, 0]).unwrap().slots[0] = Some(stack);
    let doc = crate::engine::document(&c).unwrap();
    let loaded = crate::engine::decode(&serde_json::to_vec(&doc).unwrap(), None).unwrap();
    assert_eq!(
        loaded.scene.engine.as_ref().unwrap().machines.furnaces[&[0, 8, 0]].slots[0],
        Some(stack)
    );
    let s = c.scene.engine.as_mut().unwrap();
    s.recipes.recipes.push(crate::engine_crafting::Recipe {
        id: "bucket_test".into(),
        pattern: vec![vec!["bucket".into()]],
        result: "sand".into(),
        count: 1,
    });
    s.player.crafting[0] = Some(stack);
    assert!(s.recipes.find(&s.registry, &s.player).is_none());
    s.player.crafting[0] = Some(s.registry.catalog_stack(b));
    assert!(s.recipes.find(&s.registry, &s.player).is_some());
    let mut defs = s.smelting.definitions.clone();
    defs.fuels.push(crate::engine::machines::recipes::Fuel {
        item: "bucket".into(),
        burn_ms: 1000,
    });
    assert!(defs.compile(&s.registry).is_err());
}
#[test]
fn partial_capacity_collection_at_volume_budget_is_atomic() {
    let (mut c, w) = setup();
    let s = c.scene.engine.as_mut().unwrap();
    let b = s.registry.find("bucket").unwrap();
    s.registry.blocks[b as usize]
        .container
        .as_mut()
        .unwrap()
        .capacity = 400;
    c.scene.set_fluid([0, 5, 0], w, 1024).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    for i in 0..fluids::MAX_AMOUNTS {
        s.fluids.amounts.insert([1000 + i as i32, 5, 0], 1);
    }
    let prior = s.player.clone();
    assert!(bucket::apply(&mut c, &aim([0, 5, 0], true), false).is_err());
    assert_eq!(
        c.scene
            .engine
            .as_ref()
            .unwrap()
            .player
            .changed_slots(&prior),
        0
    );
    assert_eq!(
        fluids::amount(c.scene.engine.as_ref().unwrap(), [0, 5, 0]),
        1024
    );
    c.scene.engine.as_mut().unwrap().fluids.amounts.clear();
    assert!(bucket::apply(&mut c, &aim([0, 5, 0], true), false).unwrap());
    assert_eq!(
        amount(c.scene.engine.as_ref().unwrap().player.slots[0].unwrap()),
        400
    );
    assert_eq!(
        fluids::amount(c.scene.engine.as_ref().unwrap(), [0, 5, 0]),
        624
    );
}

#[test]
fn definition_reload_cannot_convert_a_placed_block_to_an_item() {
    let mut c = crate::engine::generate(82).unwrap();
    let s = c.scene.engine.as_ref().unwrap();
    let stone = s.registry.find("slate").unwrap();
    let b = s.registry.find("bucket").unwrap();
    let mut r = s.registry.clone();
    let mut item = r.get(b).clone();
    item.id = "slate".into();
    item.state = r.get(stone).state.clone();
    r.blocks[stone as usize] = item;
    r.validate().unwrap();
    assert!(crate::engine::apply_registry(&mut c, r).is_err());
    assert!(
        c.scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .get(stone)
            .container
            .is_none()
    );
}

#[test]
fn newly_generated_custom_catalog_initializes_container_metadata() {
    let mut r = crate::engine_blocks::Registry::built_in();
    let b = r.find("bucket").unwrap() as usize;
    r.blocks.swap(0, b);
    let c = crate::engine_stream::generate(83, r).unwrap();
    let s = c.scene.engine.as_ref().unwrap();
    s.player.validate_items(&s.registry).unwrap();
    assert_eq!(s.player.slots[0].unwrap().count, 1);
    assert_eq!(amount(s.player.slots[0].unwrap()), 0);
}
