//! Actual F5 worker requests, coalescing, post-capture edits and deferred reloads.
use crate::{
    App, engine,
    engine_inventory::Stack,
    scene::{Input, Result},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use winit::keyboard::KeyCode;
fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}
pub fn verify(app: &mut App, out: &Path) -> Result<()> {
    require(
        !out.exists(),
        "Fresh background-save verification directory required",
    )?;
    fs::create_dir_all(out)?;
    app.root = out.to_path_buf();
    let oak = app
        .client
        .scene
        .engine
        .as_ref()
        .unwrap()
        .registry
        .find("oak_planks")?;
    app.client.scene.change_engine([2, 3, -2], Some(oak))?;
    app.client.scene.engine.as_mut().unwrap().player.slots[9] = Some(Stack { id: oak, count: 10 });
    app.save_shortcut()?;
    app.client.scene.change_engine([3, 3, -2], None)?;
    app.save_shortcut()?;
    app.client.scene.change_engine([4, 40, -2], Some(oak))?;
    app.client.scene.engine.as_mut().unwrap().player.slots[9] = Some(Stack { id: oak, count: 8 });
    app.save_shortcut()?;
    app.client.scene.change_engine([5, 4, -2], Some(oak))?;
    app.client.scene.engine.as_mut().unwrap().player.slots[9] = Some(Stack { id: oak, count: 7 });
    let start = app.client.actor.position;
    for _ in 0..3 {
        app.client.tick(Input {
            forward: 1.,
            ..Default::default()
        })?;
    }
    require(
        app.client.actor.position.z > start.z,
        "Movement stopped during background saving",
    )?;
    let instance = app.client.scene.engine.as_ref().unwrap().instance;
    let revision = app.client.scene.engine.as_ref().unwrap().revision;
    app.finish_work()?;
    require(
        app.save_events.len() == 2 && app.saver.as_ref().unwrap().counters.coalesced == 1,
        "F5 did not coalesce latest pending snapshot",
    )?;
    let first = app.save_events[0]
        .path
        .clone()
        .ok_or("First save missing")?;
    let second = app.save_events[1]
        .path
        .clone()
        .ok_or("Coalesced save missing")?;
    let first_bytes = fs::read(&first)?;
    let second_bytes = fs::read(&second)?;
    let a: Value = serde_json::from_slice(&first_bytes)?;
    let c: Value = serde_json::from_slice(&second_bytes)?;
    require(
        a["edits"].as_array().is_some_and(|v| v.len() == 1)
            && c["edits"].as_array().is_some_and(|v| v.len() == 3)
            && c["inventory"]["slots"][9]["count"] == 8,
        "Saved snapshots differ from their F5 capture points",
    )?;
    require(
        app.client.pending_edits() == 2
            && app.client.scene.engine.as_ref().unwrap().player.slots[9]
                .unwrap()
                .count
                == 7,
        "Post-request edits were falsely marked saved",
    )?;
    require(
        app.client.scene.engine.as_ref().unwrap().instance == instance
            && app.client.scene.engine.as_ref().unwrap().revision == revision,
        "Save completion replaced the live world or mesh revision",
    )?;
    app.save_shortcut()?;
    app.client.scene.change_engine([6, 4, -2], Some(oak))?;
    app.engine_key(KeyCode::F9)?;
    require(
        app.deferred == Some(KeyCode::F9),
        "F9 did not wait for the requested save",
    )?;
    app.finish_work()?;
    require(
        !app.client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .blocks
            .contains_key(&[6, 4, -2])
            && app.client.pending_edits() == 0,
        "Deferred F9 restored the wrong snapshot",
    )?;
    let previous = app.client.last_save.clone();
    app.client.scene.change_engine([7, 4, -2], Some(oak))?;
    let blocker = out.join("blocked-file");
    fs::write(&blocker, b"preserve")?;
    app.root = blocker.clone();
    app.save_shortcut()?;
    require(
        app.finish_work().is_err(),
        "Failed output directory was accepted",
    )?;
    require(
        app.client.pending_edits() == 1
            && app.client.last_save == previous
            && fs::read(&blocker)? == b"preserve",
        "Failed save changed the baseline or existing file",
    )?;
    app.root = out.to_path_buf();
    let definitions = out.join("assets");
    fs::create_dir(&definitions)?;
    let mut registry = app.client.scene.engine.as_ref().unwrap().registry.clone();
    registry.blocks.reverse();
    fs::write(
        definitions.join("blocks.json"),
        serde_json::to_vec(&registry)?,
    )?;
    fs::write(
        definitions.join("recipes.json"),
        serde_json::to_vec(&app.client.scene.engine.as_ref().unwrap().recipes)?,
    )?;
    app.save_shortcut()?;
    app.engine_key(KeyCode::F6)?;
    require(
        app.deferred == Some(KeyCode::F6),
        "F6 did not wait for the save",
    )?;
    app.finish_work()?;
    require(
        app.client.scene.engine.as_ref().unwrap().registry.blocks[0].id == registry.blocks[0].id,
        "Deferred F6 did not reload definitions",
    )?;
    app.engine_key(KeyCode::F9)?;
    app.finish_work()?;
    require(
        app.client.pending_edits() == 0
            && app
                .client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .stream
                .as_ref()
                .unwrap()
                .edits
                .len()
                == 5,
        "Retry/deferred reload lost edits",
    )?;
    require(
        fs::read(&first)? == first_bytes && fs::read(&second)? == second_bytes,
        "Earlier snapshot files changed",
    )?;
    let saved = app.client.last_save.clone().ok_or("Final save missing")?;
    let bytes = fs::read(&saved)?;
    require(
        fs::read_dir(out.join("saves"))?
            .all(|f| f.is_ok_and(|f| f.path().extension().is_some_and(|e| e == "blockworld"))),
        "Incomplete staging files remain",
    )?;
    let report = json!({"complete":true,"checkpoint":engine::CHECKPOINT,"saved_world":saved,"saved_sha256":format!("{:x}",Sha256::digest(&bytes)),"saved_bytes":bytes.len(),"coalesced_snapshots_verified":true,"post_request_edits_remain_pending":true,"movement_continued":true,"world_instance_preserved":true,"revision_preserved":true,"deferred_F9_verified":true,"deferred_F6_verified":true,"failed_save_preserves_baseline":true,"retry_succeeded":true,"previous_saves_unchanged":true,"no_incomplete_files":true,"saving":app.saving_observation(),"scope":"Actual F5 requests and worker adoption, coalesced intermediate request, live world/stack edits and movement after capture, deferred F9/F6, failed-directory handling and retry. Native rendering and pending-save close checked separately."});
    fs::write(out.join("saving.json"), serde_json::to_vec_pretty(&report)?)?;
    app.save_report = Some(report);
    app.notice = None;
    app.frames = 0;
    println!("Background save checks passed");
    Ok(())
}
