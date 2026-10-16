use super::*;
use crate::{engine, engine_blocks::Registry};
use mcbe_core::motion::Vec3;
#[test]
fn sparse_amounts_survive_vertical_eviction_reload_and_registry_reorder() {
    let mut t = engine::world::terrain::Settings::built_in();
    t.caves = false;
    let mut c = crate::engine_stream::generate_modern(3, Registry::built_in(), t).unwrap();
    let initial = c.actor.position;
    crate::engine_player::mode(&mut c, crate::engine_player::Mode::Noclip).unwrap();
    c.set_spawn(Vec3::new(-0.5, 10002., -0.5)).unwrap();
    let w = c
        .scene
        .engine
        .as_ref()
        .unwrap()
        .registry
        .find("water")
        .unwrap();
    let p = [-1, 10000, -1];
    c.scene.set_fluid(p, w, 333).unwrap();
    c.set_spawn(initial).unwrap();
    let bytes = serde_json::to_vec(&engine::document(&c).unwrap()).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["version"], 10);
    assert_eq!(v["fluids"][0]["amount"], 333);
    let mut copy = engine::decode(&bytes, None).unwrap();
    assert_eq!(copy.pending_edits(), 0);
    let mut r = copy.scene.engine.as_ref().unwrap().registry.clone();
    r.blocks.reverse();
    engine::apply_registry(&mut copy, r).unwrap();
    copy.set_spawn(Vec3::new(-0.5, 10002., -0.5)).unwrap();
    assert_eq!(amount(copy.scene.engine.as_ref().unwrap(), p), 333);
    assert_eq!(
        kind(
            &copy.scene.engine.as_ref().unwrap().registry,
            block(copy.scene.engine.as_ref().unwrap(), p)
        ),
        Some(crate::engine::blocks::components::FluidKind::Water)
    );
    for bad in [0, FULL, FULL + 1] {
        let mut invalid = v.clone();
        invalid["fluids"][0]["amount"] = serde_json::json!(bad);
        assert!(engine::decode(&serde_json::to_vec(&invalid).unwrap(), None).is_err());
    }
    let mut duplicate = v.clone();
    let first = duplicate["fluids"][0].clone();
    duplicate["fluids"].as_array_mut().unwrap().push(first);
    assert!(engine::decode(&serde_json::to_vec(&duplicate).unwrap(), None).is_err());
}
#[test]
fn pending_amounts_and_failed_solid_replacement_are_atomic() {
    let mut c = engine::generate(4).unwrap();
    let w = c
        .scene
        .engine
        .as_ref()
        .unwrap()
        .registry
        .find("water")
        .unwrap();
    let p = [0, 8, 0];
    c.scene.set_fluid(p, w, 250).unwrap();
    c.scene.change_engine(p, Some(w)).unwrap();
    assert_eq!(amount(c.scene.engine.as_ref().unwrap(), p), 250);
    let snapshot = engine::SaveSnapshot::capture(&c).unwrap();
    c.scene.set_fluid(p, w, 700).unwrap();
    let directory = std::env::temp_dir().join(format!(
        "mc-fluid-save-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let committed = snapshot.write(&directory).unwrap();
    let path = committed.path.clone();
    assert!(committed.adopt(&mut c).unwrap());
    let s = c.scene.engine.as_ref().unwrap();
    assert_eq!(s.fluids.pending(), 1);
    assert_eq!(amount(s, p), 700);
    let copy = engine::load(&path).unwrap();
    assert_eq!(amount(copy.scene.engine.as_ref().unwrap(), p), 250);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(directory).unwrap();
    let before = c.scene.engine.as_ref().unwrap().blocks.clone();
    assert!(c.scene.set_fluid([0, 3, 0], w, FULL).is_err());
    assert_eq!(c.scene.engine.as_ref().unwrap().blocks, before);
    let mut r = c.scene.engine.as_ref().unwrap().registry.clone();
    r.blocks[w as usize].fluid = None;
    assert!(engine::apply_registry(&mut c, r).is_err());
    assert_eq!(amount(c.scene.engine.as_ref().unwrap(), p), 700);
}

#[test]
fn ordinary_player_water_placement_enters_the_same_conservative_system() {
    let mut c = engine::generate(44).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    let water = s.registry.find("water").unwrap();
    let stone = s.registry.find("slate").unwrap();
    s.player.assign(water, 0).unwrap();
    s.player.select(0).unwrap();
    for x in 2..=6 {
        for z in 2..=6 {
            c.scene.change_engine([x, 19, z], Some(stone)).unwrap();
        }
    }
    let hit = mcbe_core::raycast::Hit {
        id: "slate".into(),
        position: [4, 19, 4].into(),
        face: mcbe_core::raycast::Face::Up,
        face_location: [0.5, 1., 0.5].into(),
    };
    assert!(crate::engine_placement::place(&mut c, &hit).unwrap());
    assert_eq!(amount(c.scene.engine.as_ref().unwrap(), [4, 20, 4]), FULL);
    for _ in 0..100 {
        transport::tick(&mut c).unwrap();
    }
    let s = c.scene.engine.as_ref().unwrap();
    let mass: u64 = s.blocks.keys().map(|p| u64::from(amount(s, p))).sum();
    assert_eq!(mass, u64::from(FULL));
    assert!(s.fluids.counters.transfers > 0);
}

#[test]
fn transferred_water_creates_currents_that_decay_without_changing_mass() {
    let mut c = engine::generate(4).unwrap();
    let w = c
        .scene
        .engine
        .as_ref()
        .unwrap()
        .registry
        .find("water")
        .unwrap();
    c.scene.set_fluid([0, 8, 0], w, FULL).unwrap();
    transport::tick(&mut c).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    assert!(s.fluids.current([0, 8, 0])[1] < 0.);
    let masses = s.fluids.amounts.clone();
    let blocks = s.blocks.clone();
    for _ in 0..4 {
        s.fluids.begin_step();
    }
    assert_eq!(s.fluids.current([0, 8, 0]), [0.; 3]);
    assert_eq!(s.blocks, blocks);
    assert_eq!(s.fluids.amounts, masses);
}

#[test]
fn protected_cells_are_preflighted_before_either_transfer_write() {
    let mut c = engine::generate(4).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    let w = s.registry.find("water").unwrap();
    // An engine extension may construct an empty finite cell at the protected plane.
    s.blocks.remove(&[0, 0, 0]);
    s.blocks.remove(&[0, 1, 0]);
    c.scene.set_fluid([0, 1, 0], w, FULL).unwrap();
    assert!(c.scene.set_fluid([0, 0, 0], w, 333).is_err());
    transport::tick(&mut c).unwrap();
    assert_eq!(amount(c.scene.engine.as_ref().unwrap(), [0, 1, 0]), FULL);
    assert_eq!(amount(c.scene.engine.as_ref().unwrap(), [0, 0, 0]), 0);
}

#[test]
fn container_guards_are_preflighted_without_losing_water_or_inventory() {
    let mut c = engine::generate(5).unwrap();
    let s = c.scene.engine.as_mut().unwrap();
    s.blocks.clear();
    let w = s.registry.find("water").unwrap();
    let sand = s.registry.find("sand").unwrap();
    s.machines.ensure([0, 7, 0]).unwrap();
    s.machines.furnaces.get_mut(&[0, 7, 0]).unwrap().slots[0] =
        Some(crate::engine_inventory::Stack {
            data: None,
            id: sand,
            count: 1,
        });
    c.scene.set_fluid([0, 8, 0], w, FULL).unwrap();
    transport::tick(&mut c).unwrap();
    let s = c.scene.engine.as_ref().unwrap();
    assert_eq!(
        s.blocks
            .keys()
            .map(|p| u64::from(amount(s, p)))
            .sum::<u64>(),
        u64::from(FULL)
    );
    assert_eq!(amount(s, [0, 7, 0]), 0);
    assert_eq!(s.machines.furnaces[&[0, 7, 0]].slots[0].unwrap().count, 1);
}
