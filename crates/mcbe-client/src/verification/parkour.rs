//! Native keyboard-map/controller/HUD fixtures with a simulated fixed-tick clock.
use crate::engine::player::locomotion::parkour::Action;
use crate::{
    App, engine, engine_player,
    scene::{Client, Result, Scene},
};
use mcbe_core::motion::Vec3;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Instant,
};
use winit::keyboard::KeyCode;
pub struct Live {
    case: String,
    out: PathBuf,
    tick: u16,
    trace: Vec<serde_json::Value>,
    done: bool,
}
fn require(ok: bool, m: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(m.into()) }
}
pub fn prepare(app: &mut App, out: &Path, case: &str) -> Result<()> {
    require(!out.exists(), "Fresh parkour proof directory required")?;
    require(
        matches!(
            case,
            "climb" | "hang" | "mantle" | "run" | "jump" | "drop" | "ceiling" | "removed" | "slab"
        ),
        "Unknown parkour case",
    )?;
    std::fs::create_dir_all(out)?;
    app.root = out.to_path_buf();
    let r = app.client.scene.engine.as_ref().unwrap().registry.clone();
    let stone = r.find("slate")?;
    let basalt = r.find("basalt")?;
    let slab = r.find("oak_slab")?;
    let axis = if matches!(case, "run" | "jump") { 0 } else { 2 };
    let mut cells = HashMap::new();
    for x in -40..40 {
        for z in -40..40 {
            cells.insert([x, 0, z], basalt);
        }
    }
    for t in -20..=20 {
        for y in 1..6 {
            let mut p = [0, y, 0];
            p[axis] = 2;
            p[2 - axis] = t;
            cells.insert(
                p,
                if case == "slab" && y == 5 {
                    slab
                } else {
                    stone
                },
            );
        }
    }
    let assets = app.client.engine_assets.take();
    let appearance = app.client.appearance.clone();
    app.client = Client::new(Scene::from_engine(
        cells,
        engine::WorldState::new(44, r, stone, None),
    )?)?;
    app.client.engine_assets = assets;
    app.client.appearance = appearance;
    app.client.set_spawn(if axis == 2 {
        Vec3::new(0.5, 1., 1.7)
    } else {
        Vec3::new(1.7, 3., 0.5)
    })?;
    app.client.yaw = 0.;
    app.client.pitch = -0.08;
    if axis == 0 {
        app.client.actor.on_ground = false;
    }
    app.simulation_previous = app.client.actor.position;
    app.debug = false;
    app.show_character = true;
    app.frame_limit = 1000;
    app.parkour_live = Some(Live {
        case: case.into(),
        out: out.into(),
        tick: 0,
        trace: Vec::new(),
        done: false,
    });
    Ok(())
}
pub fn advance(app: &mut App) -> Result<()> {
    let Some(mut live) = app.parkour_live.take() else {
        return Ok(());
    };
    if live.done || app.gpu.as_ref().is_none_or(|g| !g.stats.meshes_ready) {
        app.parkour_live = Some(live);
        return Ok(());
    }
    app.keys.clear();
    app.keys.insert(KeyCode::KeyC);
    match live.case.as_str() {
        "run" | "jump" => {
            app.keys.insert(KeyCode::KeyW);
            app.keys.insert(KeyCode::ControlLeft);
        }
        _ => {
            if live.tick < 35 {
                app.keys.insert(KeyCode::KeyW);
            }
        }
    }
    if (live.case == "mantle" && live.tick == 35) || (live.case == "jump" && live.tick == 10) {
        app.keys.insert(KeyCode::Space);
        app.jump_event(true, false, Instant::now())?;
    }
    if live.case == "drop" && live.tick >= 35 {
        app.keys.insert(KeyCode::ShiftLeft);
    }
    if live.case == "ceiling" && live.tick == 35 {
        let stone = app
            .client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .find("slate")?;
        app.client.scene.change_engine([0, 7, 2], Some(stone))?;
        app.keys.insert(KeyCode::Space);
        app.jump_event(true, false, Instant::now())?;
    }
    if live.case == "removed" && live.tick == 35 {
        let cells = app
            .client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .blocks
            .keys()
            .filter(|p| p[2] == 2 && p[1] > 0)
            .collect::<Vec<_>>();
        for p in cells {
            app.client.scene.change_engine(p, None)?;
        }
    }
    let input = app.movement_input();
    app.simulation_previous = app.client.actor.position;
    app.client.tick_motion(input)?;
    live.tick += 1;
    require(
        engine_player::clear(&app.client, app.client.movement.body())?,
        "Parkour penetrated solid geometry",
    )?;
    live.trace.push(serde_json::json!({"tick":live.tick,"action":app.client.parkour.action,"position":app.client.actor.position,"velocity":app.client.actor.velocity,"on_ground":app.client.actor.on_ground,"grab":input.grab}));
    let limit = match live.case.as_str() {
        "climb" | "run" => 10,
        "jump" => 15,
        "drop" => 43,
        "mantle" => 56,
        "ceiling" | "removed" => 36,
        _ => 35,
    };
    if live.tick >= limit {
        let expected = match live.case.as_str() {
            "climb" => Action::Climbing,
            "run" => Action::WallRunning,
            "jump" => Action::WallJump,
            "drop" | "mantle" | "removed" => Action::Idle,
            _ => Action::Hanging,
        };
        require(
            app.client.parkour.action == expected,
            "Native parkour state mismatch",
        )?;
        require(
            app.client.movement.mode == engine_player::Mode::Walking,
            "Parkour Space triggered flight",
        )?;
        let save = engine::save(&mut app.client, &live.out.join("saves"))?;
        let loaded = engine::load(&save)?;
        require(
            loaded.parkour.action == Action::Idle,
            "Saved world retained transient attachment",
        )?;
        std::fs::write(
            live.out.join("parkour.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"complete":true,"case":live.case,"action":app.client.parkour.action,"position":app.client.actor.position,"on_ground":app.client.actor.on_ground,"trace":live.trace,"saved_world":save,"transient_attachment_cleared_on_load":true,"collision_clear":true,"scope":"Native frame loop with ordinary keyboard mapping, fixed-tick wall/ledge controllers, collision checks, HUD and save compatibility."}),
            )?,
        )?;
        live.done = true;
        app.frame_limit = app.frames + 5;
    }
    app.parkour_live = Some(live);
    Ok(())
}
