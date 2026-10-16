use super::*;
#[test]
fn authored_tracks_interpolate_loop_reverse_and_bound_time() {
    let c:Clip=serde_json::from_value(serde_json::json!({"duration":2.,"playback":"ping_pong","tracks":{"height":[{"time":0.,"value":10.},{"time":2.,"value":30.}]}})).unwrap();
    c.validate().unwrap();
    assert_eq!(c.sample("height", 0.), Some(10.));
    assert_eq!(c.sample("height", 1.), Some(20.));
    assert_eq!(c.sample("height", 3.), Some(20.));
    assert_eq!(c.sample("height", -1.), Some(20.));
    assert_eq!(c.sample("missing", 1.), None);
    assert_eq!(c.sample("height", f64::NAN), None);
    let lib = Library::shared();
    for i in -128..128 {
        let phase = i as f32 / 127.;
        assert!(
            (lib.wave("player_walk", phase) - (phase * std::f32::consts::TAU).sin()).abs()
                < 0.00001
        );
    }
}
#[test]
fn interrupted_transitions_are_continuous_and_settle_without_more_work() {
    let lib = Library::shared();
    let mut t = Tween::new(0.);
    t.retarget(10., 1., &lib, "ui_progress");
    let at = 1.09;
    let halfway = t.value(at, &lib, "ui_progress");
    assert!(halfway > 0. && halfway < 10.);
    t.retarget(-3., at, &lib, "ui_progress");
    assert_eq!(t.value(at, &lib, "ui_progress"), halfway);
    assert_eq!(t.value(2., &lib, "ui_progress"), -3.);
    assert!(!t.active(2., &lib, "ui_progress"));
    t.retarget(f32::NAN, 2., &lib, "ui_progress");
    assert_eq!(t.value(3., &lib, "ui_progress"), -3.);
    let mut disabled = (*lib).clone();
    disabled.enabled = false;
    t.retarget(8., 3., &disabled, "ui_progress");
    assert_eq!(t.value(3., &disabled, "ui_progress"), 8.);
}
#[test]
fn authoring_rejects_nonfinite_duplicate_unbounded_and_missing_engine_tracks() {
    let good = Library::shared();
    let mut bad = (*good).clone();
    bad.clips
        .get_mut("ui_focus")
        .unwrap()
        .tracks
        .get_mut("value")
        .unwrap()[1]
        .time = 0.;
    assert!(bad.validate().is_err());
    let mut bad = (*good).clone();
    bad.clips.remove("player_walk");
    assert!(bad.validate().is_err());
    let mut bad = (*good).clone();
    bad.clips
        .get_mut("ui_focus")
        .unwrap()
        .tracks
        .get_mut("value")
        .unwrap()[0]
        .value = f32::INFINITY;
    assert!(bad.validate().is_err());
    let mut bad = (*good).clone();
    bad.clips.get_mut("ui_focus").unwrap().playback = Playback::Loop;
    assert!(bad.validate().is_err());
    let mut bad = (*good).clone();
    bad.clips.get_mut("loading").unwrap().duration = 61.;
    assert!(bad.validate().is_err());
}

#[test]
fn bad_animation_reload_is_transactional() {
    use crate::engine;
    let mut c = engine::generate(713).unwrap();
    let old = c.animations.clone();
    let blocks = c.scene.engine.as_ref().unwrap().blocks.clone();
    let player = c.scene.engine.as_ref().unwrap().player.clone();
    let home = std::env::temp_dir().join(format!("mcbe-animation-{}", std::process::id()));
    std::fs::create_dir_all(home.join("content/base")).unwrap();
    std::fs::write(
        home.join("content/base/animations.json"),
        b"{\"version\":1,\"clips\":{}}",
    )
    .unwrap();
    let definitions = home.join("blocks.json");
    std::fs::write(
        &definitions,
        serde_json::to_vec(&c.scene.engine.as_ref().unwrap().registry).unwrap(),
    )
    .unwrap();
    c.owned_materials = Some(
        crate::engine_materials::load(&c.scene.engine.as_ref().unwrap().registry, &home).unwrap(),
    );
    let result = engine::reload_definitions(&mut c, &definitions);
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Required animation clip missing")
    );
    assert!(std::sync::Arc::ptr_eq(&old, &c.animations));
    assert_eq!(c.scene.engine.as_ref().unwrap().blocks, blocks);
    assert_eq!(
        c.scene.engine.as_ref().unwrap().player.revision,
        player.revision
    );
    std::fs::remove_file(home.join("content/base/animations.json")).unwrap();
    std::fs::remove_file(definitions).unwrap();
    std::fs::remove_dir(home.join("content/base")).unwrap();
    std::fs::remove_dir(home.join("content")).unwrap();
    std::fs::remove_dir(home).unwrap();
}

#[test]
fn custom_named_tracks_compile_with_unique_cache_generations() {
    let original = Library::shared();
    let mut custom = (*original).clone();
    custom.clips.insert("door_open".into(),serde_json::from_value(serde_json::json!({"duration":1.,"tracks":{"angle":[{"time":0.,"value":0.},{"time":1.,"value":90.}],"lift":[{"time":0.,"value":0.},{"time":1.,"value":2.}]}})).unwrap());
    let custom = custom.compile().unwrap();
    assert_ne!(custom.id, original.id);
    assert_eq!(custom.sample("door_open", "angle", 0.5), Some(45.));
    assert_eq!(custom.sample("door_open", "lift", 0.5), Some(1.));
    assert_eq!(custom.sample("unknown", "angle", 0.5), None);
    let encoded = serde_json::to_string(&custom).unwrap();
    let restored: Library = serde_json::from_str(&encoded).unwrap();
    assert_ne!(restored.id, custom.id);
    assert_eq!(restored.sample("door_open", "angle", 0.5), Some(45.));
    restored.validate().unwrap();
}
