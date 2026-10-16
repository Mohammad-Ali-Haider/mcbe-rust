use super::*;
use crate::{
    engine,
    engine_blocks::Registry,
    scene::{Client, Scene},
};
use mcbe_core::motion::Vec3;
use std::collections::HashMap;
pub(super) fn flat() -> Client {
    let r = Registry::built_in();
    let floor = r.find("basalt").unwrap();
    let mut cells = HashMap::new();
    for x in -40..40 {
        for z in -40..40 {
            cells.insert([x, 0, z], floor);
        }
    }
    let mut c = Client::new(
        Scene::from_engine(cells, engine::WorldState::new(44, r, floor, None)).unwrap(),
    )
    .unwrap();
    c.set_spawn(Vec3::new(0.5, 1., -8.5)).unwrap();
    c
}
fn world(c: &mut Client) -> &mut World {
    &mut c.scene.engine.as_mut().unwrap().entities
}
#[test]
fn identity_damage_and_optional_component_lifecycle() {
    let mut c = flat();
    let w = world(&mut c);
    let id = w.spawn("grazer", [3., 1., 0.], true).unwrap();
    assert!(w.damage(id, 3));
    assert_eq!(w.records[&id].health.as_ref().unwrap().current, 9);
    assert!(!w.damage(id, 3));
    w.clock = 8;
    assert!(w.damage(id, 100));
    assert!(!w.records.contains_key(&id));
    let next = w.spawn("grazer", [3., 1., 0.], true).unwrap();
    assert!(next.0 > id.0);
    assert!(!w.damage(id, 1));
    assert!(w.interact(next));
    assert!(w.records[&next].persistent);
    assert!(w.records[&next].brain.as_ref().unwrap().following);
    let mut definitions = w.library.definitions.clone();
    let d = &mut definitions.kinds[0];
    d.id = "decoration".into();
    d.body = None;
    d.behavior = None;
    d.health = None;
    d.ambient_weight = 0;
    let r = Registry::built_in();
    assert!(
        w.reconfigured(definitions.clone().compile(&r).unwrap())
            .is_err()
    );
    let mut empty = World::new(&r);
    empty.library = std::sync::Arc::new(definitions.compile(&r).unwrap());
    let id = empty.spawn("decoration", [0., 1., 0.], true).unwrap();
    let e = &empty.records[&id];
    assert!(e.physics.is_none() && e.brain.is_none() && e.health.is_none() && e.visual.is_some());
}
#[test]
fn strict_definitions_and_saved_records_reject_malformed_state() {
    let r = Registry::built_in();
    let mut d = Definitions::built_in();
    d.kinds[0].model[0].bounds[0] = f32::NAN;
    assert!(d.compile(&r).is_err());
    let mut d = Definitions::built_in();
    d.kinds[0].model[0].material = "glass".into();
    assert!(d.compile(&r).is_err());
    let mut w = World::new(&r);
    w.spawn("companion", [1., 1., 1.], true).unwrap();
    let saved = w.saved();
    assert!(World::restore(saved.clone(), &r).is_ok());
    let mut bad = saved.clone();
    bad.records.push(bad.records[0].clone());
    assert!(World::restore(bad, &r).is_err());
    let mut bad = saved.clone();
    bad.records[0].physics.as_mut().unwrap().velocity[0] = 100.;
    assert!(World::restore(bad, &r).is_err());
    let mut bad = saved;
    bad.records[0].health.as_mut().unwrap().maximum = 1000;
    assert!(World::restore(bad, &r).is_err());
    let mut w = World::new(&r);
    w.next_id = u64::MAX;
    assert!(w.spawn("grazer", [0., 1., 0.], false).is_err());
}
#[test]
fn navigation_routes_around_wall_and_uses_registered_slab_heights() {
    let mut c = flat();
    let r = &c.scene.engine.as_ref().unwrap().registry;
    let stone = r.find("slate").unwrap();
    let slab = r.find("oak_slab").unwrap();
    for z in -4..=4 {
        for y in 1..=3 {
            c.scene.change_engine([0, y, z], Some(stone)).unwrap();
        }
    }
    let body = Physics {
        width: 0.6,
        height: 1.5,
        speed: 2.,
        velocity: [0.; 3],
        grounded: true,
    };
    let p = navigation::plan(&c, [-3.5, 1., 0.5], [3.5, 1., 0.5], &body);
    assert!(p.complete, "visited {} points {:?}", p.visited, p.points);
    assert!(p.points.iter().any(|p| p[2].abs() > 4.));
    assert!(p.visited <= navigation::NODE_BUDGET);
    c.scene.change_engine([0, 1, 8], Some(slab)).unwrap();
    let p = navigation::surface(&c, 0.5, 8.5, 1., &body).unwrap();
    assert_eq!(p[1], 1.5);
    assert!(navigation::clear(&c, p, &body));
}
#[test]
fn follow_and_flee_use_physics_without_dirtying_terrain() {
    let mut c = flat();
    let id = world(&mut c)
        .spawn("companion", [0.5, 1., 1.5], true)
        .unwrap();
    world(&mut c).interact(id);
    let revision = c.scene.engine.as_ref().unwrap().edit_revision;
    let player = position(c.actor.position);
    let before = distance(world(&mut c).records[&id].transform.position, player);
    for _ in 0..80 {
        simulation::tick(&mut c).unwrap();
    }
    let after = distance(world(&mut c).records[&id].transform.position, player);
    assert!(after < before - 5., "{before} -> {after}");
    assert!(after > 1.);
    assert_eq!(c.scene.engine.as_ref().unwrap().edit_revision, revision);
    let id = world(&mut c)
        .spawn("grazer", [3.5, 1., -5.5], true)
        .unwrap();
    let before = distance(world(&mut c).records[&id].transform.position, player);
    for _ in 0..20 {
        simulation::tick(&mut c).unwrap();
    }
    let after = distance(world(&mut c).records[&id].transform.position, player);
    assert!(after > before + 0.5, "{before} -> {after}");
}
#[test]
fn scheduling_is_bounded_fair_and_distant_entities_sleep() {
    let mut c = flat();
    c.set_spawn(Vec3::new(0.5, 1., -25.5)).unwrap();
    let r = c.scene.engine.as_ref().unwrap().registry.clone();
    let mut d = world(&mut c).library.definitions.clone();
    for d in &mut d.kinds {
        d.behavior = Some(Behavior::Idle);
    }
    let next = world(&mut c).reconfigured(d.compile(&r).unwrap()).unwrap();
    *world(&mut c) = next;
    for i in 0..200 {
        world(&mut c)
            .spawn(
                "companion",
                [
                    -19.5 + (i % 20) as f32 * 2.,
                    1.,
                    -9.5 + (i / 20) as f32 * 2.,
                ],
                true,
            )
            .unwrap();
    }
    let far = world(&mut c).spawn("grazer", [200., 1., 0.], true).unwrap();
    for _ in 0..12 {
        simulation::tick(&mut c).unwrap();
        let stats = &world(&mut c).stats;
        assert!(
            stats.physics_steps <= ACTIVE_LIMIT
                && stats.decisions <= AI_BUDGET
                && stats.paths <= PATH_BUDGET
                && stats.path_nodes <= PATH_BUDGET * navigation::NODE_BUDGET
        );
    }
    assert!(
        world(&mut c)
            .records
            .values()
            .filter(|e| e.id != far)
            .all(|e| e.brain.as_ref().unwrap().decide_at > 0)
    );
    assert_eq!(
        world(&mut c).records[&far].transform.position,
        [200., 1., 0.]
    );
    assert_eq!(
        world(&mut c).records[&far].brain.as_ref().unwrap().activity,
        Activity::Sleeping
    );
}
#[test]
fn entity_save_roundtrip_and_snapshot_baselines_preserve_later_mutations() {
    let mut c = flat();
    let id = world(&mut c)
        .spawn("companion", [3., 1., 3.], true)
        .unwrap();
    world(&mut c).interact(id);
    let snapshot = engine::SaveSnapshot::capture(&c).unwrap();
    let document = snapshot.document().unwrap();
    let json = serde_json::to_vec(&document).unwrap();
    let loaded = engine::decode(&json, None).unwrap();
    assert_eq!(
        loaded.scene.engine.as_ref().unwrap().entities.saved(),
        world(&mut c).saved()
    );
    let mut legacy = serde_json::to_value(&document).unwrap();
    legacy["version"] = serde_json::json!(10);
    assert!(engine::decode(&serde_json::to_vec(&legacy).unwrap(), None).is_err());
    legacy.as_object_mut().unwrap().remove("entities");
    assert!(engine::decode(&serde_json::to_vec(&legacy).unwrap(), None).is_ok());
    let path = std::env::temp_dir().join(format!(
        "mcbe-entities-{}-{}-{}",
        std::process::id(),
        id.0,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    assert!(!path.exists());
    std::fs::create_dir_all(&path).unwrap();
    let committed = snapshot.write(&path).unwrap();
    world(&mut c).damage(id, 3);
    committed.adopt(&mut c).unwrap();
    assert!(c.pending_edits() > 0);
    assert_eq!(
        world(&mut c).records[&id].health.as_ref().unwrap().current,
        17
    );
    let target = path.canonicalize().unwrap();
    assert!(target.starts_with(std::env::temp_dir().canonicalize().unwrap()));
    assert_eq!(target.file_name(), path.file_name());
    std::fs::remove_dir_all(target).unwrap();
}

#[test]
fn checked_spawn_ray_occlusion_and_shader_entry_points() {
    let mut c = flat();
    assert!(interaction::spawn(&mut c, "companion", [0.5, 1., -8.5], true).is_err());
    let id = interaction::spawn(&mut c, "companion", [0.5, 1., -4.5], true).unwrap();
    c.yaw = 0.;
    c.pitch = -0.16;
    assert_eq!(interaction::target(&c).unwrap(), Some(id));
    let stone = c
        .scene
        .engine
        .as_ref()
        .unwrap()
        .registry
        .find("slate")
        .unwrap();
    c.scene.change_engine([0, 2, -6], Some(stone)).unwrap();
    assert_eq!(interaction::target(&c).unwrap(), None);
    assert!(!interaction::attack_target(&mut c).unwrap());
    assert_eq!(
        world(&mut c).records[&id].health.as_ref().unwrap().current,
        20
    );
    for code in [
        mesh::shader(),
        format!(
            "{}\n{}\n{}",
            include_str!("../render/shaders/lighting_frame.wgsl"),
            include_str!("../render/shaders/shadow.wgsl"),
            include_str!("../render/shaders/entities_shadow.wgsl")
        ),
    ] {
        let module = wgpu::naga::front::wgsl::parse_str(&code).unwrap();
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}

#[test]
fn unavailable_regions_sleep_and_registry_rebind_preserves_saved_brain_state() {
    let mut c = flat();
    let id = interaction::spawn(&mut c, "companion", [3.5, 1., -5.5], true).unwrap();
    world(&mut c).interact(id);
    simulation::tick(&mut c).unwrap();
    let before = world(&mut c).saved();
    let old_bounds = c.scene.bounds;
    c.scene.bounds = mcbe_core::streaming::Bounds {
        min: [80, 0, 80],
        max: [120, 40, 120],
    };
    let p = world(&mut c).records[&id].transform.position;
    simulation::tick(&mut c).unwrap();
    assert_eq!(world(&mut c).records[&id].transform.position, p);
    assert_eq!(world(&mut c).stats.physics_steps, 0);
    c.scene.bounds = old_bounds;
    let r = c.scene.engine.as_ref().unwrap().registry.clone();
    let definition = world(&mut c).library.definitions.clone();
    let next = world(&mut c)
        .reconfigured(definition.compile(&r).unwrap())
        .unwrap();
    assert_eq!(
        next.records[&id].brain.as_ref().unwrap().goal,
        world(&mut c).records[&id].brain.as_ref().unwrap().goal
    );
    assert!(next.records[&id].brain.as_ref().unwrap().following);
    assert_eq!(
        next.records[&id].brain.as_ref().unwrap().random,
        before.records[0].brain.as_ref().unwrap().random
    );
}

#[test]
fn visible_head_and_nonphysical_decoration_are_pickable() {
    let mut c = flat();
    let id = interaction::spawn(&mut c, "grazer", [0.5, 1., -4.5], true).unwrap();
    world(&mut c).records.get_mut(&id).unwrap().transform.yaw = std::f32::consts::FRAC_PI_2;
    c.yaw = 0.67f32.atan2(4.);
    c.pitch = (-0.62f32).atan2((4f32 * 4. + 0.67 * 0.67).sqrt());
    assert_eq!(interaction::target(&c).unwrap(), Some(id));
    let mut c = flat();
    let registry = c.scene.engine.as_ref().unwrap().registry.clone();
    let mut defs = world(&mut c).library.definitions.clone();
    let mut decoration = defs.kinds[1].clone();
    decoration.id = "marker".into();
    decoration.body = None;
    decoration.behavior = None;
    decoration.ambient_weight = 0;
    decoration.model = vec![definition::Part {
        uv: None,
        transform: None,
        bounds: [-0.25, 0., -0.25, 0.25, 1., 0.25],
        material: "cobalt".into(),
        swing: 0.,
        pivot: [0.; 3],
    }];
    defs.kinds.push(decoration);
    world(&mut c).library = std::sync::Arc::new(defs.compile(&registry).unwrap());
    let id = interaction::spawn(&mut c, "marker", [0.5, 2., -4.5], true).unwrap();
    c.yaw = 0.;
    c.pitch = -0.03;
    assert_eq!(interaction::target(&c).unwrap(), Some(id));
    assert!(world(&mut c).records[&id].physics.is_none());
}
