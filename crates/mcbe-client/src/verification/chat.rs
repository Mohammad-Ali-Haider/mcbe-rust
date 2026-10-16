//! Native chat text focus, command transactions, background travel and avatar sides.
use crate::{App, engine, engine_camera, engine_inventory::Inventory, scene::Result};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use winit::keyboard::KeyCode as K;
pub struct Live {
    case: String,
    out: PathBuf,
    started: Instant,
    stable: Option<Instant>,
    step: usize,
    waiting: bool,
    cancel_at: Option<Instant>,
    saving: bool,
    done: bool,
    source_instance: u64,
    source_pose: mcbe_core::motion::Vec3,
    trace: Vec<String>,
    idle: Option<(usize, usize)>,
    save_meshes: usize,
    save_instance: u64,
}
fn require(ok: bool, message: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}
fn type_text(app: &mut App, text: &str) -> Result<()> {
    for ch in text.chars() {
        require(
            app.chat_event(K::KeyA, true, false, Some(&ch.to_string()))?,
            "Chat did not consume text key",
        )?;
    }
    Ok(())
}
fn send(app: &mut App, text: &str) -> Result<()> {
    app.engine_key(K::KeyT)?;
    require(app.chat.open, "T did not open chat")?;
    require(
        app.chat.input.is_empty(),
        "Chat opener leaked its T character",
    )?;
    type_text(app, text)?;
    app.chat_event(K::Enter, true, false, Some("\r"))?;
    require(!app.chat.open, "Enter did not close chat")
}
fn count(app: &App, id: &str) -> usize {
    let s = app.client.scene.engine.as_ref().unwrap();
    let id = s.registry.find(id).unwrap();
    s.player
        .slots
        .iter()
        .flatten()
        .filter(|s| s.id == id)
        .map(|s| usize::from(s.count))
        .sum()
}
pub fn prepare(app: &mut App, out: &Path, case: &str) -> Result<()> {
    require(!out.exists(), "Fresh native chat output required")?;
    std::fs::create_dir_all(out)?;
    app.root = out.into();
    app.notice = None;
    app.debug = false;
    app.show_character = true;
    engine::player::movement::mode(&mut app.client, engine::player::movement::Mode::Noclip)?;
    app.client.yaw = 0.7;
    app.client.pitch = -0.08;
    if matches!(case, "back" | "front") {
        let p = app.client.actor.position;
        app.client
            .set_spawn(mcbe_core::motion::Vec3::new(p.x, p.y + 8., p.z))?;
        app.simulation_previous = app.client.actor.position;
    }
    let source_instance = app.client.scene.engine.as_ref().unwrap().instance;
    let source_pose = app.client.actor.position;
    app.client.scene.engine.as_mut().unwrap().player = Inventory::empty();
    match case {
        "free" => {
            for _ in 0..3 {
                app.engine_key(K::F5)?;
            }
            require(app.client.view.is_free(), "F5 did not reach free camera")?;
            send(app, "/travel islands")?;
            require(
                !app.load_busy()
                    && app
                        .chat
                        .messages
                        .back()
                        .is_some_and(|m| m.text.contains("free camera")),
                "Free-camera travel did not produce chat feedback",
            )?;
        }
        "message" => {
            send(app, "Hello islands!")?;
            require(
                app.chat
                    .messages
                    .back()
                    .is_some_and(|m| m.text == "<Player> Hello islands!"),
                "Local chat message missing",
            )?;
        }
        "controller" => {
            use crate::engine::input::gamepad::{Button as B, Pad};
            app.engine_key(K::KeyT)?;
            let edit = app.client.scene.engine.as_ref().unwrap().edit_revision;
            super::controller::sample(
                app,
                Some(Pad {
                    movement: [1., 1.],
                    look: [1., 0.],
                    triggers: [1., 1.],
                    buttons: B::South.bit(),
                }),
            )?;
            require(
                app.chat.open
                    && app.client.actor.position == source_pose
                    && app.client.scene.engine.as_ref().unwrap().edit_revision == edit,
                "Controller gameplay leaked through chat focus",
            )?;
            super::controller::neutral(app)?;
            super::controller::tap(app, B::East)?;
            require(!app.chat.open, "Controller B did not close chat")?;
        }
        "open" => {
            app.engine_key(K::KeyT)?;
        }
        "completion" => {
            app.engine_key(K::Slash)?;
            type_text(app, "tra")?;
            app.chat_event(K::Tab, true, false, None)?;
            require(app.chat.input == "/travel ", "Command completion failed")?;
            type_text(app, "isl")?;
            app.chat_event(K::Tab, true, false, None)?;
            require(
                app.chat.input == "/travel islands ",
                "Dimension completion failed",
            )?;
        }
        "errors" => {
            for line in [
                "/missing",
                "/give glass -1",
                "/give unknown 1",
                "/give glass 1 extra",
                "/travel unknown",
            ] {
                send(app, line)?;
            }
            require(
                app.client.scene.engine.as_ref().unwrap().player.total() == 0,
                "Invalid command mutated inventory",
            )?;
        }
        "editing" => {
            app.engine_key(K::KeyT)?;
            type_text(app, "/give glass 7x")?;
            app.chat_event(K::Backspace, true, false, None)?;
            type_text(app, "0")?;
            app.chat_event(K::Enter, true, false, None)?;
            app.engine_key(K::KeyT)?;
            app.chat_event(K::ArrowUp, true, false, None)?;
            require(
                app.chat.input == "/give glass 70",
                "Up did not recall command",
            )?;
            app.chat_event(K::ArrowDown, true, false, None)?;
            require(app.chat.input.is_empty(), "Down did not restore draft")?;
            type_text(app, "cancelled draft")?;
            app.chat_event(K::Escape, true, false, None)?;
            require(
                count(app, "glass") == 70,
                "Editing or Escape duplicated give",
            )?;
        }
        "give" | "travel" | "cancel" => {
            send(app, "/give glass 70")?;
            send(app, "/give bucket 2")?;
            require(
                count(app, "glass") == 70 && count(app, "bucket") == 2,
                "Give did not merge/split measured items",
            )?;
        }
        "back" | "front" => {
            let s = app.client.scene.engine.as_mut().unwrap();
            s.player.slots[0] = Some(s.registry.catalog_stack(s.registry.find("blue_glass")?));
            let mut off = s.registry.catalog_stack(s.registry.find("bucket")?);
            off.data = Some(engine::items::ItemData::Fluid {
                kind: engine::blocks::components::FluidKind::Water,
                amount: 331,
            });
            s.player.offhand = Some(off);
            app.client.view.mode = if case == "back" {
                engine_camera::Mode::ThirdBack
            } else {
                engine_camera::Mode::ThirdFront
            };
        }
        _ => return Err("Unknown native chat case".into()),
    }
    require(
        app.keys.is_empty() && app.client.actor.position == source_pose,
        "Typing changed movement",
    )?;
    app.chat_live = Some(Live {
        case: case.into(),
        out: out.into(),
        started: Instant::now(),
        stable: None,
        step: 0,
        waiting: false,
        cancel_at: None,
        saving: false,
        done: false,
        source_instance,
        source_pose,
        trace: vec![],
        idle: None,
        save_meshes: 0,
        save_instance: 0,
    });
    app.frame_limit = usize::MAX / 2;
    Ok(())
}
fn ready(app: &App) -> bool {
    if app.load_busy() || app.save_busy() {
        return false;
    }
    let Some(gpu) = &app.gpu else {
        return false;
    };
    let s = app
        .client
        .scene
        .engine
        .as_ref()
        .unwrap()
        .stream
        .as_ref()
        .unwrap();
    s.volume
        .as_ref()
        .is_none_or(|v| v.wanted.len() == v.residents.len())
        && gpu.stats.meshes_ready
        && gpu.stats.distant_ready
        && (gpu.rendering_observation()["requested_method"] != "ray_traced"
            || gpu.rendering_observation()["genuine_ray_tracing"] == true)
}
pub fn advance(app: &mut App) -> Result<()> {
    let Some(mut live) = app.chat_live.take() else {
        return Ok(());
    };
    if live.done {
        app.chat_live = Some(live);
        return Ok(());
    }
    require(
        live.started.elapsed() < Duration::from_secs(120),
        "Native chat fixture failed to settle",
    )?;
    if live
        .cancel_at
        .is_some_and(|at| at.elapsed() >= Duration::from_millis(50))
    {
        app.engine_key(K::Escape)?;
        live.cancel_at = None;
    }
    if live.waiting && !app.load_busy() {
        if live.case == "cancel" {
            require(
                app.client.scene.engine.as_ref().unwrap().instance == live.source_instance,
                "Cancelled chat travel changed world",
            )?;
        } else {
            let active = app
                .client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .dimensions
                .as_ref()
                .unwrap()
                .active
                .clone();
            live.trace.push(active);
            require(
                count(app, "glass") == 70 && count(app, "bucket") == 2,
                "Travel lost given inventory",
            )?;
        }
        live.waiting = false;
        live.stable = None;
    }
    if !ready(app) {
        live.stable = None;
        app.chat_live = Some(live);
        return Ok(());
    }
    if live.stable.get_or_insert_with(Instant::now).elapsed() < Duration::from_millis(150) {
        app.chat_live = Some(live);
        return Ok(());
    }
    let route: &[&str] = match live.case.as_str() {
        "travel" => &["cavern", "islands", "surface"],
        "cancel" => &["cavern"],
        _ => &[],
    };
    if live.step < route.len() {
        send(app, &format!("/travel {}", route[live.step]))?;
        require(app.load_busy(), "Travel command did not schedule worker")?;
        if live.step == 0 {
            app.engine_key(K::KeyT)?;
            require(!app.chat.open, "T opened during travel")?;
            // Exercise the dispatcher guard against a focus request racing a load.
            app.open_chat(false);
            type_text(app, "/give glass 1")?;
            app.chat_event(K::Enter, true, false, None)?;
            require(
                count(app, "glass") == 70,
                "Busy give changed the live snapshot",
            )?;
        }
        live.step += 1;
        live.waiting = true;
        live.stable = None;
        if live.case == "cancel" {
            live.cancel_at = Some(Instant::now());
        }
        app.chat_live = Some(live);
        return Ok(());
    }
    if live.case == "travel" {
        require(
            live.trace == ["cavern", "islands", "surface"]
                && app.client.actor.position == live.source_pose,
            "Command round trip lost pose/dimension order",
        )?;
    }
    if !live.saving {
        if live.idle.is_none() {
            live.idle = Some((app.frames, app.gpu.as_ref().unwrap().stats.ui_uploads));
            app.chat_live = Some(live);
            return Ok(());
        }
        let (frame, uploads) = live.idle.unwrap();
        if app.frames < frame + 20 {
            app.chat_live = Some(live);
            return Ok(());
        }
        require(
            app.gpu.as_ref().unwrap().stats.ui_uploads == uploads,
            "Idle chat rebuilt the cached HUD",
        )?;
        live.save_instance = app.client.scene.engine.as_ref().unwrap().instance;
        live.save_meshes = app.gpu.as_ref().unwrap().stats.mesh_rebuilds;
        app.request_save()?;
        live.saving = true;
        app.chat_live = Some(live);
        return Ok(());
    }
    let path = app
        .client
        .last_save
        .clone()
        .ok_or("Native command save absent")?;
    let loaded = engine::load(&path)?;
    let s = app.client.scene.engine.as_ref().unwrap();
    let copy = loaded.scene.engine.as_ref().unwrap();
    require(
        s.player.changed_slots(&copy.player) == 0,
        "Given items failed save/load",
    )?;
    require(
        serde_json::to_value(s.dimensions.as_ref().unwrap().saved(&s.registry)?)?
            == serde_json::to_value(copy.dimensions.as_ref().unwrap().saved(&copy.registry)?)?,
        "Dimension snapshots differ after command save",
    )?;
    require(
        s.instance == live.save_instance
            && app.gpu.as_ref().unwrap().stats.mesh_rebuilds == live.save_meshes,
        "Chat/save rebuilt or replaced terrain",
    )?;
    if !matches!(live.case.as_str(), "back" | "front" | "controller") && !app.chat.open {
        app.engine_key(K::KeyT)?;
    }
    let bodies: Vec<_> = app
        .client
        .appearance
        .model
        .cubes
        .iter()
        .map(|c| c.bone.clone())
        .collect();
    std::fs::write(
        live.out.join("chat.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"complete":true,"case":live.case,"chat":app.chat.observation(),"route":live.trace,"save":path,"given_items_roundtrip":true,"dimensions_roundtrip":true,"pose_preserved":app.client.actor.position==live.source_pose,"body_bones":bodies,"idle_ui_cached":true,"terrain_unchanged_by_chat_and_save":true,"loading":app.loading_observation(),"scope":"Native shared text handler, local commands and actual background travel; current renderer geometry and screenshot. No multiplayer messaging or physical keyboard/controller automation is claimed."}),
        )?,
    )?;
    app.notice = None;
    live.done = true;
    app.frame_limit = app.frames + 5;
    app.chat_live = Some(live);
    Ok(())
}
