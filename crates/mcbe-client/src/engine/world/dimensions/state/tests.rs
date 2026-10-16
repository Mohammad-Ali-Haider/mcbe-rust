use super::*;
use crate::engine_inventory::Hand;
fn client() -> Client {
    let mut c = stream::generate(713, Registry::built_in()).unwrap();
    stream::set_radius(&mut c, 2).unwrap();
    attach_legacy(&mut c, Definitions::built_in()).unwrap();
    c
}
fn travel(c: &mut Client, id: &str) {
    *c = Travel::capture(c, id).unwrap().prepare().unwrap();
}
fn seed_source(c: &mut Client) {
    let s = c.scene.engine.as_mut().unwrap();
    let glass = s.registry.find("blue_glass").unwrap();
    let water = s.registry.find("water").unwrap();
    let furnace = s.registry.find("furnace").unwrap();
    let mut bucket = s.registry.catalog_stack(s.registry.find("bucket").unwrap());
    bucket.data = Some(engine::items::ItemData::Fluid {
        kind: engine::blocks::components::FluidKind::Water,
        amount: 331,
    });
    s.player.offhand = Some(bucket);
    c.scene.change_engine([0, 5, 0], Some(glass)).unwrap();
    c.scene.set_fluid([3, 5, 3], water, 333).unwrap();
    c.scene.change_engine([2, 4, 4], Some(furnace)).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    s.machines.ensure([2, 4, 4]).unwrap();
    s.machines.clock_ms = 1300;
    s.entities.spawn("grazer", [8.5, 4., 8.5], true).unwrap();
}
#[test]
fn all_three_worlds_isolate_edits_water_machines_entities_and_return_pose() {
    let mut c = client();
    seed_source(&mut c);
    c.view.mode = engine::player::camera::Mode::ThirdBack;
    let original_pose = (c.actor.position, c.yaw, c.pitch);
    let original_entities = c.scene.engine.as_ref().unwrap().entities.saved();
    let inventory = c.scene.engine.as_ref().unwrap().player.clone();
    let skin = c.appearance.clone();
    let original_id = c.scene.engine.as_ref().unwrap().instance;
    travel(&mut c, "cavern");
    assert_ne!(c.scene.engine.as_ref().unwrap().instance, original_id);
    assert_eq!(c.view.mode, engine::player::camera::Mode::ThirdBack);
    assert!(Arc::ptr_eq(&c.appearance, &skin));
    let s = c.scene.engine.as_ref().unwrap();
    assert_eq!(s.player.changed_slots(&inventory), 0);
    assert_eq!(s.player.hand(Hand::Off), inventory.hand(Hand::Off));
    assert!(
        s.fluids.amounts.is_empty()
            && s.machines.furnaces.is_empty()
            && s.entities.records.is_empty()
    );
    let height = s.stream.as_ref().unwrap().generator.height(0, 0);
    c.set_spawn(Vec3::new(0.5, height as f32 + 6., 0.5))
        .unwrap();
    let marker = c
        .scene
        .engine
        .as_ref()
        .unwrap()
        .registry
        .find("oak_planks")
        .unwrap();
    c.scene.change_engine([0, 5, 0], Some(marker)).unwrap();
    let cavern_pose = c.actor.position;
    travel(&mut c, "islands");
    assert!(
        c.scene
            .engine
            .as_ref()
            .unwrap()
            .stream
            .as_ref()
            .unwrap()
            .generator
            .terrain
            .as_ref()
            .unwrap()
            .layered()
    );
    assert_eq!(
        c.scene
            .engine
            .as_ref()
            .unwrap()
            .dimensions
            .as_ref()
            .unwrap()
            .inactive
            .len(),
        2
    );
    travel(&mut c, "surface");
    let s = c.scene.engine.as_ref().unwrap();
    assert_eq!((c.actor.position, c.yaw, c.pitch), original_pose);
    assert_eq!(s.registry.get(s.blocks[&[0, 5, 0]]).id, "blue_glass");
    assert_eq!(s.fluids.amounts[&[3, 5, 3]], 333);
    assert_eq!(s.machines.clock_ms, 1300);
    assert!(s.machines.furnaces.contains_key(&[2, 4, 4]));
    assert_eq!(s.entities.saved(), original_entities);
    assert!(s.dimensions.as_ref().unwrap().pending());
    travel(&mut c, "cavern");
    assert_eq!(c.actor.position, cavern_pose);
    assert_eq!(c.scene.engine.as_ref().unwrap().blocks[&[0, 5, 0]], marker);
}
#[test]
fn schema_thirteen_roundtrips_all_visited_worlds_without_resident_chunks() {
    let mut c = client();
    seed_source(&mut c);
    travel(&mut c, "cavern");
    travel(&mut c, "islands");
    let d = engine::SaveSnapshot::capture(&c)
        .unwrap()
        .document()
        .unwrap();
    let value = serde_json::to_value(d).unwrap();
    assert_eq!(value["version"], crate::engine::SAVE_VERSION);
    assert_eq!(value["dimensions"]["inactive"].as_array().unwrap().len(), 2);
    assert!(
        value["dimensions"]["inactive"]
            .as_array()
            .unwrap()
            .iter()
            .all(|w| w.get("cells").is_none())
    );
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(bytes.len() < 256 * 1024);
    let mut restored = engine::decode(&bytes, None).unwrap();
    assert_eq!(restored.scene.engine.as_ref().unwrap().pending(), 0);
    assert_eq!(
        restored
            .scene
            .engine
            .as_ref()
            .unwrap()
            .dimensions
            .as_ref()
            .unwrap()
            .active,
        "islands"
    );
    travel(&mut restored, "surface");
    let s = restored.scene.engine.as_ref().unwrap();
    assert_eq!(s.registry.get(s.blocks[&[0, 5, 0]]).id, "blue_glass");
    assert_eq!(s.fluids.amounts[&[3, 5, 3]], 333);
    assert_eq!(s.machines.clock_ms, 1300);
    assert_eq!(s.entities.records.len(), 1);
    assert!(s.player.offhand.is_some());
}
#[test]
fn snapshots_freeze_dimension_definitions_and_inactive_edits() {
    let mut c = client();
    seed_source(&mut c);
    travel(&mut c, "cavern");
    let snapshot = engine::SaveSnapshot::capture(&c).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    let d = s.dimensions.as_mut().unwrap();
    let mut custom = d.definitions.dimensions[2].clone();
    custom.id = "sky_gardens".into();
    d.definitions.dimensions.push(custom);
    d.revision += 1;
    d.inactive
        .get_mut("surface")
        .unwrap()
        .edits
        .remove(&[0, 5, 0]);
    let value = serde_json::to_value(snapshot.document().unwrap()).unwrap();
    assert_eq!(
        value["dimensions"]["definitions"]["dimensions"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let mut restored = engine::decode(&serde_json::to_vec(&value).unwrap(), None).unwrap();
    travel(&mut restored, "surface");
    assert_eq!(
        restored
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .get(restored.scene.engine.as_ref().unwrap().blocks[&[0, 5, 0]])
            .id,
        "blue_glass"
    );
}
#[test]
fn malformed_or_legacy_tagged_dimension_records_are_rejected() {
    let mut c = client();
    seed_source(&mut c);
    travel(&mut c, "cavern");
    let value = serde_json::to_value(
        engine::SaveSnapshot::capture(&c)
            .unwrap()
            .document()
            .unwrap(),
    )
    .unwrap();
    for (field, v) in [
        ("id", serde_json::json!("missing")),
        ("pitch", serde_json::json!(2.)),
        (
            "position",
            serde_json::json!({"x":0.5,"y":-10000000.,"z":0.5}),
        ),
    ] {
        let mut bad = value.clone();
        bad["dimensions"]["inactive"][0][field] = v;
        assert!(
            engine::decode(&serde_json::to_vec(&bad).unwrap(), None).is_err(),
            "{field}"
        );
    }
    let mut bad = value.clone();
    bad["version"] = serde_json::json!(12);
    assert!(engine::decode(&serde_json::to_vec(&bad).unwrap(), None).is_err());
    let mut bad = value.clone();
    bad["dimensions"]["inactive"][0]["id"] = serde_json::json!("cavern");
    assert!(engine::decode(&serde_json::to_vec(&bad).unwrap(), None).is_err());
    let mut bad = value.clone();
    bad["dimensions"]["inactive"][0]["fluids"][0]["amount"] = serde_json::json!(1024);
    assert!(engine::decode(&serde_json::to_vec(&bad).unwrap(), None).is_err());
}
#[test]
fn legacy_world_attachment_keeps_original_generator_and_seed() {
    let c = stream::generate(99, Registry::built_in()).unwrap();
    let before = serde_json::to_value(
        &c.scene
            .engine
            .as_ref()
            .unwrap()
            .stream
            .as_ref()
            .unwrap()
            .generator
            .config,
    )
    .unwrap();
    let mut value = serde_json::to_value(
        engine::SaveSnapshot::capture(&c)
            .unwrap()
            .document()
            .unwrap(),
    )
    .unwrap();
    value["version"] = serde_json::json!(12);
    let mut c = engine::decode(&serde_json::to_vec(&value).unwrap(), None).unwrap();
    assert!(c.scene.engine.as_ref().unwrap().dimensions.is_none());
    attach_legacy(&mut c, Definitions::built_in()).unwrap();
    assert_eq!(c.scene.engine.as_ref().unwrap().seed, 99);
    assert_eq!(
        serde_json::to_value(
            &c.scene
                .engine
                .as_ref()
                .unwrap()
                .stream
                .as_ref()
                .unwrap()
                .generator
                .config
        )
        .unwrap(),
        before
    );
    travel(&mut c, "islands");
    travel(&mut c, "surface");
    assert_eq!(c.scene.engine.as_ref().unwrap().seed, 99);
    assert_eq!(
        serde_json::to_value(
            &c.scene
                .engine
                .as_ref()
                .unwrap()
                .stream
                .as_ref()
                .unwrap()
                .generator
                .config
        )
        .unwrap(),
        before
    );
}
#[test]
fn registry_remapping_covers_inactive_states_and_rejects_losing_visited_worlds() {
    let mut c = client();
    seed_source(&mut c);
    travel(&mut c, "cavern");
    let mut registry = c.scene.engine.as_ref().unwrap().registry.clone();
    registry.blocks.reverse();
    engine::apply_registry(&mut c, registry).unwrap();
    travel(&mut c, "surface");
    let s = c.scene.engine.as_ref().unwrap();
    assert_eq!(s.registry.get(s.blocks[&[0, 5, 0]]).id, "blue_glass");
    assert_eq!(s.fluids.amounts[&[3, 5, 3]], 333);
    let d = s.dimensions.as_ref().unwrap();
    let mut definitions = d.definitions.clone();
    definitions.dimensions.retain(|v| v.id != "cavern");
    assert!(
        d.rebind(
            &s.registry,
            &s.registry,
            &s.smelting,
            &s.smelting,
            None,
            Some(definitions)
        )
        .is_err()
    );
    assert!(
        s.dimensions
            .as_ref()
            .unwrap()
            .inactive
            .contains_key("cavern")
    );
}

#[test]
fn reload_updates_inactive_entities_and_resets_changed_furnace_recipes_atomically() {
    let mut c = client();
    seed_source(&mut c);
    let s = c.scene.engine.as_mut().unwrap();
    let f = s.machines.furnaces.get_mut(&[2, 4, 4]).unwrap();
    let sand = s.registry.find("sand").unwrap();
    f.slots[0] = Some(s.registry.catalog_stack(sand));
    f.cooking = Some(sand);
    f.cook_ms = 1200;
    travel(&mut c, "cavern");
    let s = c.scene.engine.as_ref().unwrap();
    let registry = s.registry.clone();
    let mut entities = s.entities.library.definitions.clone();
    entities
        .kinds
        .iter_mut()
        .find(|e| e.id == "grazer")
        .unwrap()
        .body
        .as_mut()
        .unwrap()
        .speed = 2.5;
    let mut recipes = s.smelting.definitions.clone();
    recipes
        .smelting
        .iter_mut()
        .find(|r| r.input == "sand")
        .unwrap()
        .cook_ms = 100;
    let recipes = recipes.compile(&registry).unwrap();
    engine::apply_registry_complete(&mut c, registry, Some(entities), None, Some(recipes)).unwrap();
    let s = c.scene.engine.as_ref().unwrap();
    let world = &s.dimensions.as_ref().unwrap().inactive["surface"];
    assert_eq!(world.machines.furnaces[&[2, 4, 4]].cook_ms, 0);
    assert_eq!(
        world
            .entities
            .records
            .values()
            .next()
            .unwrap()
            .physics
            .as_ref()
            .unwrap()
            .speed,
        2.5
    );
    let before =
        serde_json::to_value(s.dimensions.as_ref().unwrap().saved(&s.registry).unwrap()).unwrap();
    let mut bad = s.entities.library.definitions.clone();
    bad.kinds.retain(|e| e.id != "grazer");
    let registry = s.registry.clone();
    assert!(engine::apply_registry_complete(&mut c, registry, Some(bad), None, None).is_err());
    let s = c.scene.engine.as_ref().unwrap();
    assert_eq!(
        before,
        serde_json::to_value(s.dimensions.as_ref().unwrap().saved(&s.registry).unwrap()).unwrap()
    );
    travel(&mut c, "surface");
    assert_eq!(
        c.scene.engine.as_ref().unwrap().machines.furnaces[&[2, 4, 4]].cook_ms,
        0
    );
}
