//! Exercise streamed edits, evictions, worker crossings and real save controls.
use crate::{App, engine, engine_stream as stream, scene::Result};
use mcbe_core::motion::Vec3;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};
use winit::keyboard::KeyCode;
fn require(value: bool, message: &str) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}
fn snap(app: &App, name: &str) -> Value {
    json!({"step":name,"stream":stream::observation(&app.client),"player":[app.client.actor.position.x,app.client.actor.position.y,app.client.actor.position.z]})
}
fn settle(app: &mut App) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.client.sync_region()?;
        let s = stream::observation(&app.client);
        require(
            s["resident_columns"].as_u64().unwrap() <= 49,
            "Resident column budget exceeded",
        )?;
        require(
            s["dirty_keys"].as_u64().unwrap() <= 196,
            "Dirty chunk map grew beyond resident budget",
        )?;
        if s["resident_columns"] == s["wanted_columns"] && s["queued_jobs"] == 0 {
            break;
        }
        require(Instant::now() < deadline, "Terrain worker did not settle")?;
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}
pub fn verify(app: &mut App, out: &Path) -> Result<()> {
    require(
        !out.exists(),
        "Fresh streamed-world verification folder required",
    )?;
    require(
        app.client
            .scene
            .engine
            .as_ref()
            .and_then(|s| s.stream.as_ref())
            .is_some(),
        "Streamed world missing",
    )?;
    fs::create_dir_all(out)?;
    app.root = out.to_path_buf();
    if let Some(assets) = &app.client.engine_assets {
        fs::write(
            out.join("assets.json"),
            serde_json::to_vec_pretty(&assets.provenance)?,
        )?;
    }
    let mut trace = vec![snap(app, "initial")];
    let oak = app
        .client
        .scene
        .engine
        .as_ref()
        .unwrap()
        .registry
        .find("oak_planks")?;
    app.client.scene.change_engine([2, 3, -2], Some(oak))?;
    app.client.scene.change_engine([3, 3, -2], None)?;
    app.client.scene.change_engine([4, 40, -2], Some(oak))?;
    require(
        app.client.scene.change_engine([0, 0, 0], None).is_err(),
        "Foundation edit accepted",
    )?;
    {
        let p = &mut app.client.scene.engine.as_mut().unwrap().player;
        p.assign(oak, p.active)?;
    }
    trace.push(snap(app, "edited"));
    app.save_shortcut()?;
    app.finish_work()?;
    let first = app
        .client
        .last_save
        .clone()
        .ok_or("First streamed save missing")?;
    let first_bytes = fs::read(&first)?;
    app.engine_key(KeyCode::F10)?;
    app.finish_work()?;
    require(
        !app.client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .blocks
            .contains_key(&[4, 40, -2]),
        "Distant travel retained old resident blocks",
    )?;
    require(
        app.client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .stream
            .as_ref()
            .unwrap()
            .edits
            .len()
            == 3,
        "Eviction lost sparse edits",
    )?;
    trace.push(snap(app, "edited-column-evicted"));
    app.save_shortcut()?;
    app.finish_work()?;
    let second = app
        .client
        .last_save
        .clone()
        .ok_or("Unloaded-edit save missing")?;
    let second_bytes = fs::read(&second)?;
    let document: Value = serde_json::from_slice(&second_bytes)?;
    require(
        document["version"] == 6
            && document.get("cells").is_none()
            && document["edits"].as_array().is_some_and(|e| e.len() == 3)
            && second_bytes.len() < 16_384,
        "Save is not a sparse version 6 document",
    )?;
    for _ in 0..3 {
        app.engine_key(KeyCode::F10)?;
        app.finish_work()?;
    }
    let s = app.client.scene.engine.as_ref().unwrap();
    require(
        s.blocks.get(&[2, 3, -2]) == Some(&oak)
            && !s.blocks.contains_key(&[3, 3, -2])
            && s.blocks.get(&[4, 40, -2]) == Some(&oak),
        "Returning lost replaced, removed or added blocks",
    )?;
    trace.push(snap(app, "returned-edits"));
    app.engine_key(KeyCode::F9)?;
    app.finish_work()?;
    require(
        app.client.pending_edits() == 0,
        "Reloaded streamed save has spurious pending edits",
    )?;
    require(
        app.client.scene.engine.as_ref().unwrap().player.selected() == Some(oak),
        "Streamed save lost inventory",
    )?;
    stream::jump(&mut app.client, 0)?;
    require(
        app.client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .blocks
            .get(&[4, 40, -2])
            == Some(&oak),
        "Reload lost unloaded high block",
    )?;
    trace.push(snap(app, "reloaded-and-returned"));
    // Cross a column boundary without teleport preparation: the live worker fills it.
    app.client.set_spawn(Vec3::new(15.5, 4., -9.5))?;
    app.client.yaw = std::f32::consts::FRAC_PI_2;
    let start = app.client.actor.position;
    for _ in 0..10 {
        app.client.tick(crate::scene::Input {
            forward: 1.,
            ..Default::default()
        })?;
        if stream::center(app.client.actor.position)[0] == 1 {
            break;
        }
    }
    require(
        stream::center(app.client.actor.position)[0] == 1 && app.client.actor.position.x > start.x,
        "Player input did not cross the streamed column boundary",
    )?;
    let waiting = stream::observation(&app.client);
    require(
        waiting["resident_columns"].as_u64().unwrap() < 49
            && waiting["queued_jobs"].as_u64().unwrap() > 0,
        "Crossing did not queue background terrain",
    )?;
    trace.push(snap(app, "worker-crossing-queued"));
    settle(app)?;
    trace.push(snap(app, "worker-crossing-ready"));
    require(
        app.client.pending_edits() == 0,
        "Streaming changed the save baseline",
    )?;
    app.client.set_spawn(engine::SPAWN)?;
    settle(app)?;
    app.client.yaw = 0.;
    app.client.pitch = -0.08;
    app.notice = None;
    require(
        fs::read(&first)? == first_bytes && fs::read(&second)? == second_bytes,
        "Earlier saves changed",
    )?;
    let report = json!({"complete":true,"checkpoint":engine::CHECKPOINT,"generator_version":1,"save_version":6,"saved_world":second,"saved_sha256":format!("{:x}",Sha256::digest(&second_bytes)),"saved_bytes":second_bytes.len(),"sparse_edits":3,"evicted_edits_saved":true,"added_removed_replaced_returned":true,"inventory_preserved":true,"previous_saves_unchanged":true,"foundation_protected":true,"background_crossing_verified":true,"player_input_crossing_verified":true,"streaming_not_pending_edits":true,"resident_column_budget":49,"dirty_key_budget":196,"trace":trace,"final_stream":stream::observation(&app.client),"scope":"Generator version/seed plus sparse edits; bounded resident columns, F10 travel, unloaded-edit save, F9 restore, protected foundation and worker-driven crossing. Native scripted camera tour and GPU cache bounds checked separately; live collision/input tests retained."});
    fs::write(out.join("stream.json"), serde_json::to_vec_pretty(&report)?)?;
    app.frames = 0;
    println!("Streamed engine checks passed");
    Ok(())
}
