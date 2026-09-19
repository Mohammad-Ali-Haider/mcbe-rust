//! Version-one controls, movement persistence, editable models and playable gallery.
use crate::{
    App, engine,
    engine_blocks::Registry,
    engine_player::{self, Mode},
    scene::{Input, Result},
};
use glam::{Mat4, Vec3 as GVec3};
use mcbe_core::motion::Vec3;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use winit::keyboard::KeyCode;
fn require(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}
pub fn verify(app: &mut App, out: &Path) -> Result<()> {
    require(
        !out.exists(),
        "Fresh version-one verification directory required",
    )?;
    fs::create_dir_all(out)?;
    app.root = out.to_path_buf();
    let registry = Registry::built_in();
    let stone = registry.find("slate")?;
    let slab = registry.find("oak_slab")?;
    for yaw in [
        0.,
        std::f32::consts::FRAC_PI_2,
        std::f32::consts::PI,
        -std::f32::consts::FRAC_PI_2,
    ] {
        app.client.set_spawn(engine::SPAWN)?;
        app.client.yaw = yaw;
        app.client.pitch = 0.;
        let old = app.client.actor.position;
        let direction = GVec3::from(app.client.direction().map(|v| v as f32));
        let view = Mat4::look_to_rh(GVec3::from([old.x, old.y, old.z]), direction, GVec3::Y);
        app.client.tick(Input {
            side: 1.,
            ..Default::default()
        })?;
        let p = app.client.actor.position;
        require(
            view.transform_point3(GVec3::new(p.x, p.y, p.z) + direction * 2.)
                .x
                > 0.,
            "D did not move toward screen-right",
        )?;
        app.client.look(0.2, 0.);
        let turned = GVec3::from(app.client.direction().map(|v| v as f32));
        require(
            view.transform_vector3(turned).x > 0.,
            "Rightward mouse movement turned left",
        )?;
    }
    app.client.set_spawn(engine::SPAWN)?;
    app.client.yaw = 0.;
    app.client.pitch = -0.08;
    let eye = app.client.eye()[1];
    app.client.tick(Input {
        sneak: true,
        forward: 1.,
        ..Default::default()
    })?;
    require(
        app.client.movement.crouched
            && app.client.eye()[1] < eye - 0.3
            && (app.client.actor.displacement.z - 0.054).abs() < 0.001,
        "Sneak posture/speed failed",
    )?;
    app.client.tick(Input::default())?;
    app.client.set_spawn(engine::SPAWN)?;
    for y in 4..7 {
        app.client.scene.change_engine([0, y, -7], Some(stone))?;
    }
    app.engine_key(KeyCode::KeyF)?;
    require(
        app.client.movement.mode == Mode::Flying,
        "F did not enable flight",
    )?;
    let y = app.client.actor.position.y;
    app.client.tick(Input {
        jump: true,
        ..Default::default()
    })?;
    require(app.client.actor.position.y > y, "Flight ascent failed")?;
    app.client.tick(Input {
        sneak: true,
        ..Default::default()
    })?;
    for _ in 0..10 {
        app.client.tick(Input {
            forward: 1.,
            ..Default::default()
        })?;
    }
    require(
        app.client.actor.position.z <= -7.299,
        "Flying passed through a wall",
    )?;
    app.engine_key(KeyCode::KeyN)?;
    app.client.tick(Input {
        forward: 1.,
        ..Default::default()
    })?;
    require(
        app.client.actor.position.z > -7.3
            && app.engine_key(KeyCode::KeyN).is_err()
            && app.client.movement.mode == Mode::Noclip,
        "Noclip crossing or safe-exit guard failed",
    )?;
    app.engine_key(KeyCode::F5)?;
    app.finish_work()?;
    let saved = app.client.last_save.clone().ok_or("Noclip save missing")?;
    for _ in 0..6 {
        app.client.tick(Input {
            forward: 1.,
            ..Default::default()
        })?;
    }
    app.engine_key(KeyCode::KeyN)?;
    app.engine_key(KeyCode::F9)?;
    app.finish_work()?;
    require(
        app.client.movement.mode == Mode::Noclip
            && !engine_player::clear(&app.client, app.client.movement.body())?,
        "Reload did not restore the noclip snapshot inside geometry",
    )?;
    require(
        !fs::read(&saved)?.is_empty(),
        "Saved noclip document disappeared",
    )?;
    app.client.set_spawn(engine::SPAWN)?;
    app.engine_key(KeyCode::KeyF)?;
    for y in 4..7 {
        app.client.scene.change_engine([0, y, -7], None)?;
    }
    // Searching for model names must not activate letter-based world shortcuts.
    app.engine_key(KeyCode::KeyE)?;
    app.palette.search = true;
    app.palette.text("stairs");
    let movement = app.client.movement;
    for key in [KeyCode::KeyF, KeyCode::KeyN, KeyCode::KeyR, KeyCode::KeyT] {
        app.engine_key(key)?;
    }
    require(
        app.client.movement == movement && !app.load_busy(),
        "Inventory search triggered a movement shortcut",
    )?;
    app.engine_key(KeyCode::Escape)?;
    let assets = out.join("assets");
    fs::create_dir(&assets)?;
    fs::write(
        assets.join("recipes.json"),
        serde_json::to_vec(&app.client.scene.engine.as_ref().unwrap().recipes)?,
    )?;
    app.client.scene.change_engine([6, 4, -5], Some(slab))?;
    app.client.set_spawn(Vec3::new(6.5, 4.5, -4.5))?;
    let mut changed = registry.clone();
    changed.blocks[usize::from(slab)]
        .geometry
        .as_mut()
        .unwrap()
        .boxes = vec![crate::engine_geometry::UNIT];
    fs::write(assets.join("blocks.json"), serde_json::to_vec(&changed)?)?;
    let revision = app.client.scene.engine.as_ref().unwrap().registry_revision;
    require(
        app.engine_key(KeyCode::F6).is_err()
            && app.client.scene.engine.as_ref().unwrap().registry_revision == revision,
        "Overlapping model reload was partially applied",
    )?;
    app.client.set_spawn(engine::SPAWN)?;
    app.engine_key(KeyCode::F6)?;
    require(
        app.client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .geometry
            .full(slab),
        "Edited cuboid JSON was not applied",
    )?;
    fs::write(
        assets.join("blocks.json"),
        serde_json::to_vec_pretty(&registry)?,
    )?;
    app.engine_key(KeyCode::F6)?;
    require(
        !app.client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .geometry
            .full(slab),
        "Model reload did not restore the slab",
    )?;
    // A fresh, saved gallery is the user-facing starting point for trying the features.
    let textures = app.client.engine_assets.take();
    app.client = crate::engine_stream::generate(360036, registry.clone())?;
    app.client.engine_assets = textures;
    let names = [
        "oak_slab",
        "stone_slab",
        "oak_stairs_north",
        "oak_stairs_east",
        "oak_stairs_south",
        "oak_stairs_west",
        "oak_window",
        "oak_planks",
        "stone_bricks",
    ];
    for (i, name) in names.iter().enumerate() {
        let id = registry.find(name)?;
        app.client
            .scene
            .engine
            .as_mut()
            .unwrap()
            .player
            .assign(id, i)?;
        app.client
            .scene
            .change_engine([-9 + i as i32 * 3, 4, 0], Some(id))?;
    }
    app.client.set_spawn(Vec3::new(0.5, 4., -8.5))?;
    app.client.yaw = 0.;
    app.client.pitch = -0.18;
    app.client.scene.engine.as_mut().unwrap().player.select(2)?;
    app.engine_key(KeyCode::F5)?;
    app.finish_work()?;
    let saved = app
        .client
        .last_save
        .clone()
        .ok_or("Geometry gallery save missing")?;
    let bytes = fs::read(&saved)?;
    let report = json!({"complete":true,"checkpoint":engine::CHECKPOINT,"version":"1.0","controls_follow_projected_camera":true,"sneak_speed_and_eye":true,"flying_collision_and_vertical_controls":true,"noclip_crossing_and_safe_exit":true,"noclip_snapshot_reload":true,"inventory_search_preserves_movement":true,"editable_geometry_F6":true,"overlapping_F6_atomic":true,"models":names,"registry_blocks":registry.blocks.len(),"save_version":5,"saved_world":saved,"saved_sha256":format!("{:x}",Sha256::digest(&bytes)),"saved_bytes":bytes.len(),"pending_edits":app.client.pending_edits(),"saving":app.saving_observation(),"loading":app.loading_observation(),"scope":"Actual F/N/F5/F9/F6/E controllers and camera-projected motion, sneak height/speed, flight collision, noclip traversal and safe exit, saved noclip spawn, search shortcut isolation, editable JSON geometry and rejection under the player. Unit tests separately cover ledges, low ceilings, stairs, holes, union surfaces, halo parity and legacy migration."});
    fs::write(
        out.join("version1.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    app.v1_report = Some(report);
    app.notice = None;
    app.frames = 0;
    Ok(())
}
