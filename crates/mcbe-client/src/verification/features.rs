//! Checkpoint 48 playable gallery and real client settings controls.
use crate::{
    App, engine,
    engine_blocks::Registry,
    engine_player::{self, Mode},
    scene::{Input, Result},
};
use mcbe_core::motion::Vec3;
use serde_json::json;
use std::{fs, path::Path};
use winit::keyboard::KeyCode;
pub fn prepare(app: &mut App, out: &Path, settings: bool) -> Result<()> {
    if out.exists() {
        return Err("Fresh feature proof directory required".into());
    }
    fs::create_dir_all(out)?;
    app.root = out.to_path_buf();
    let registry = Registry::built_in();
    let textures = app.client.engine_assets.take();
    app.client = crate::engine_stream::generate(360036, registry.clone())?;
    app.client.engine_assets = textures;
    // A short course: lower/upper/double slabs and all eight stair shapes.
    let names = [
        "oak_slab",
        "oak_slab_upper",
        "oak_slab_double",
        "stone_slab",
        "stone_slab_upper",
        "stone_slab_double",
        "oak_stairs_north",
        "oak_stairs_north_upper",
        "oak_stairs_east",
        "oak_stairs_east_upper",
        "oak_stairs_south",
        "oak_stairs_south_upper",
        "oak_stairs_west",
        "oak_stairs_west_upper",
    ];
    for (i, name) in names.iter().enumerate() {
        app.client.scene.change_engine(
            [-10 + (i % 7) as i32 * 3, 4, -4 + (i / 7) as i32 * 2],
            Some(registry.find(name)?),
        )?;
    }
    // Clear glass in front of contrasting opaque blocks, blue blended glass beside it.
    for x in -5..=5 {
        for y in 4..=6 {
            app.client.scene.change_engine(
                [x, y, 2],
                Some(registry.find(if x < 1 { "glass" } else { "blue_glass" })?),
            )?;
            app.client.scene.change_engine(
                [x, y, 4],
                Some(registry.find(if (x + y) % 2 == 0 { "ivory" } else { "ember" })?),
            )?;
        }
    }
    for (i, name) in [
        "glass",
        "blue_glass",
        "oak_slab",
        "stone_slab",
        "oak_stairs_north",
        "oak_planks",
        "stone_bricks",
        "oak_window",
        "oak_log",
    ]
    .iter()
    .enumerate()
    {
        app.client
            .scene
            .engine
            .as_mut()
            .unwrap()
            .player
            .assign(registry.find(name)?, i)?;
    }
    app.client.set_spawn(Vec3::new(0.5, 4., -10.5))?;
    app.client.yaw = 0.;
    app.client.pitch = -0.02;
    // Exercise the production Space-press controller before the native gallery.
    let now = std::time::Instant::now();
    for ms in [0, 100, 150, 200] {
        app.jump_event(true, false, now + std::time::Duration::from_millis(ms))?;
        app.jump_event(
            false,
            false,
            now + std::time::Duration::from_millis(ms + 10),
        )?;
        let expected = if (100..200).contains(&ms) {
            Mode::Flying
        } else {
            Mode::Walking
        };
        if app.client.movement.mode != expected {
            return Err("Double-tap Space flight toggle failed".into());
        }
    }
    app.engine_key(KeyCode::KeyF)?;
    let old = app.client.actor.position;
    app.client.tick(Input {
        forward: 1.,
        sprint: true,
        ..Default::default()
    })?;
    if (app.client.actor.position.z - old.z - 0.8).abs() > 0.001
        || app.client.movement.mode != Mode::Flying
    {
        return Err("Sprint flight control failed".into());
    }
    engine_player::mode(&mut app.client, Mode::Walking)?;
    app.client.set_spawn(Vec3::new(0.5, 4., -10.5))?;
    let old = app.client.actor.position;
    app.client.tick(Input {
        forward: 1.,
        sprint: true,
        ..Default::default()
    })?;
    if (app.client.actor.position.z - old.z - 0.28).abs() > 0.001 {
        return Err("Walking sprint failed".into());
    }
    app.client.set_spawn(Vec3::new(0.5, 4., -10.5))?;
    app.client.pitch = -0.02;
    app.simulation_previous = app.client.actor.position;
    app.engine_key(KeyCode::F5)?;
    app.finish_work()?;
    let saved = app.client.last_save.clone().ok_or("Gallery save missing")?;
    let loaded = engine::load(&saved)?;
    if loaded.scene.engine.as_ref().unwrap().blocks
        != app.client.scene.engine.as_ref().unwrap().blocks
    {
        return Err("Variant/material save roundtrip failed".into());
    }
    if settings {
        app.engine_key(KeyCode::Escape)?;
        app.engine_key(KeyCode::ArrowDown)?;
        app.engine_key(KeyCode::Enter)?;
        // Exercise actual menu keyboard actions, not direct generator mutation.
        for _ in 0..5 {
            app.engine_key(KeyCode::ArrowRight)?;
        }
        app.settings.write(out)?;
        if crate::engine_settings::Settings::read(out)? != app.settings {
            return Err("Settings persistence failed".into());
        }
        app.settings_test = true;
        app.frame_limit = 1000;
    } else {
        app.frame_limit = 100;
    }
    fs::write(
        out.join("features.json"),
        serde_json::to_vec_pretty(
            &json!({"complete":true,"checkpoint":engine::CHECKPOINT,"registry_blocks":registry.blocks.len(),"models":names,"saved_world":saved,"walk_sprint":0.28,"flight_sprint":0.8,"double_jump_flight_toggle":true,"variant_save_roundtrip":true,"settings_persisted":settings}),
        )?,
    )?;
    app.notice = None;
    Ok(())
}
/// Maximum-range fixture uses real settings controls and the save/load pipeline.
pub fn distance(app: &mut App, out: &Path, menu: bool, shrink: bool) -> Result<()> {
    prepare(app, out, false)?;
    app.engine_key(KeyCode::Escape)?;
    app.engine_key(KeyCode::ArrowDown)?;
    app.engine_key(KeyCode::Enter)?;
    app.engine_key(KeyCode::End)?;
    app.settings.write(out)?;
    if crate::engine_settings::Settings::read(out)?.render_distance != 255 {
        return Err("Maximum distance preference did not persist".into());
    }
    engine_player::mode(&mut app.client, Mode::Flying)?;
    app.client.set_spawn(Vec3::new(0.5, 50., -10.5))?;
    app.client.pitch = -0.08;
    app.simulation_previous = app.client.actor.position;
    app.request_save()?;
    app.finish_work()?;
    let saved = app
        .client
        .last_save
        .clone()
        .ok_or("Maximum distance save missing")?;
    let loaded = engine::load(&saved)?;
    let stream = loaded
        .scene
        .engine
        .as_ref()
        .unwrap()
        .stream
        .as_ref()
        .unwrap();
    if stream.generator.config.radius != 255 || stream.residents.len() != 289 {
        return Err("Maximum distance save/load exceeded detailed residency budget".into());
    }
    fs::write(
        out.join("distance.json"),
        serde_json::to_vec_pretty(
            &json!({"complete":true,"radius":255,"maximum_detail_columns":289,"saved_world":saved,"saved_radius":stream.generator.config.radius,"saved_residents":stream.residents.len(),"menu":menu,"shrink":shrink}),
        )?,
    )?;
    if !menu && !shrink {
        app.engine_key(KeyCode::Escape)?;
        app.engine_key(KeyCode::Escape)?;
    }
    app.settings_test = shrink;
    app.frame_limit = 700;
    app.notice = None;
    Ok(())
}
