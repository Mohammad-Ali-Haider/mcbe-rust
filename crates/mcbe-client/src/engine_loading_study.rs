//! Production travel/reload controls, latest-request wins, cancellation and retries.
use crate::{App, engine, scene::Result};
use serde_json::json;
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
        "Fresh loading verification directory required",
    )?;
    fs::create_dir_all(out)?;
    app.root = out.to_path_buf();
    let original = app.client.actor.position;
    let instance = app.client.scene.engine.as_ref().unwrap().instance;
    let oak = app
        .client
        .scene
        .engine
        .as_ref()
        .unwrap()
        .registry
        .find("oak_planks")?;
    app.client.scene.change_engine([2, 3, -2], Some(oak))?;
    app.client.scene.change_engine([4, 40, -2], Some(oak))?;
    app.client.scene.engine.as_mut().unwrap().player.slots[10] =
        Some(crate::engine_inventory::Stack { id: oak, count: 17 });
    for _ in 0..3 {
        app.engine_key(KeyCode::F10)?;
    }
    require(
        app.load_busy() && app.client.actor.position == original,
        "F10 changed the scene before preparation completed",
    )?;
    app.finish_work()?;
    require(
        app.loader.as_ref().unwrap().counters.coalesced == 1
            && app.loader.as_ref().unwrap().counters.applied == 1,
        "F10 did not select only the latest queued destination",
    )?;
    require(
        crate::engine_stream::center(app.client.actor.position) == [20, -17]
            && app.client.pending_edits() == 3
            && app.client.scene.engine.as_ref().unwrap().instance == instance,
        "Travel replaced inventory, edits or world identity",
    )?;
    app.engine_key(KeyCode::KeyR)?;
    app.finish_work()?;
    require(
        app.client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .blocks
            .get(&[4, 40, -2])
            == Some(&oak),
        "Return lost unloaded edits",
    )?;
    app.engine_key(KeyCode::F10)?;
    app.engine_key(KeyCode::Escape)?;
    let before = app.client.actor.position;
    app.finish_work()?;
    require(
        app.client.actor.position == before,
        "Cancelled preparation moved the player",
    )?;
    app.client.scene.change_engine([20, 13, 0], Some(oak))?;
    app.engine_key(KeyCode::KeyT)?;
    require(
        app.finish_work().is_err() && app.client.actor.position == before,
        "Obstructed travel was partially applied",
    )?;
    app.client.scene.change_engine([20, 13, 0], None)?;
    app.engine_key(KeyCode::KeyT)?;
    app.finish_work()?;
    require(
        app.client.actor.position.x == 20.5 && app.client.actor.position.y == 13.,
        "Travel retry failed",
    )?;
    app.engine_key(KeyCode::KeyR)?;
    app.engine_key(KeyCode::F5)?;
    require(
        app.deferred == Some(KeyCode::F5),
        "F5 did not wait for preparation",
    )?;
    app.finish_work()?;
    let saved = app
        .client
        .last_save
        .clone()
        .ok_or("Deferred save missing")?;
    let bytes = fs::read(&saved)?;
    let mut external = bytes.clone();
    external.push(b' ');
    fs::write(&saved, &external)?;
    app.engine_key(KeyCode::F9)?;
    require(
        app.finish_work().is_err(),
        "Externally changed valid save was accepted",
    )?;
    require(
        fs::read(&saved)? == external
            && app.client.scene.engine.as_ref().unwrap().instance == instance,
        "Failed reload modified the live world or external file",
    )?;
    fs::write(&saved, &bytes)?;
    app.engine_key(KeyCode::F9)?;
    app.finish_work()?;
    require(
        app.client.pending_edits() == 0
            && app.client.scene.engine.as_ref().unwrap().instance != instance
            && fs::read(&saved)? == bytes,
        "Valid reload failed to restore its snapshot",
    )?;
    let report = json!({"complete":true,"checkpoint":engine::CHECKPOINT,"latest_destination_verified":true,"old_scene_until_ready":true,"travel_preserves_world_identity":true,"unloaded_edits_and_inventory_preserved":true,"cancel_preserves_current_scene":true,"obstruction_failure_atomic":true,"retry_succeeded":true,"F5_deferred_until_ready":true,"valid_external_modification_rejected":true,"successful_reload_verified":true,"saved_world":saved,"loading":app.loading_observation(),"scope":"Actual F10/R/T/F9/F5/Escape handlers with coalescing, unloaded edits, carried stacks, cancelled and obstructed destinations, deferred saving and external-file guard; native rendering checked separately."});
    fs::write(
        out.join("loading.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    app.load_report = Some(report);
    app.notice = None;
    app.frames = 0;
    Ok(())
}
