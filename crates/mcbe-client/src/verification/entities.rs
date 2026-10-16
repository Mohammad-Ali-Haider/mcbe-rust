//! Native fixed-tick entity, AI, interaction, persistence and renderer fixtures.
use crate::engine::entities::{self, World, components::*, interaction, navigation};
use crate::{
    App, engine,
    scene::{Client, Result, Scene},
};
use mcbe_core::motion::Vec3;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Instant,
};
pub struct Live {
    case: String,
    out: PathBuf,
    tick: u16,
    limit: u16,
    ids: Vec<Id>,
    initial: Vec<[f32; 3]>,
    trace: Vec<serde_json::Value>,
    done: bool,
}
fn require(ok: bool, m: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(m.into()) }
}
fn w(app: &mut App) -> &mut World {
    &mut app.client.scene.engine.as_mut().unwrap().entities
}
fn idle(app: &mut App) -> Result<()> {
    let r = app.client.scene.engine.as_ref().unwrap().registry.clone();
    let mut defs = w(app).library.definitions.clone();
    for d in &mut defs.kinds {
        if d.body.is_some() {
            d.behavior = Some(Behavior::Idle);
        }
    }
    let world = w(app).reconfigured(defs.compile(&r)?)?;
    *w(app) = world;
    Ok(())
}
pub fn prepare(app: &mut App, out: &Path, case: &str) -> Result<()> {
    require(!out.exists(), "Fresh entity proof directory required")?;
    std::fs::create_dir_all(out)?;
    require(
        matches!(
            case,
            "showcase"
                | "wire"
                | "shadow_off"
                | "follow"
                | "flee"
                | "path"
                | "interaction"
                | "occluded"
                | "occluded_clear"
                | "budget"
                | "save"
                | "natural"
                | "high"
                | "custom"
        ),
        "Unknown entity case",
    )?;
    let state = app.client.scene.engine.as_ref().unwrap();
    let r = state.registry.clone();
    let floor = r.find("basalt")?;
    let library = state.entities.library.clone();
    let mut cells = HashMap::new();
    for x in -40..40 {
        for z in -40..40 {
            cells.insert([x, 0, z], floor);
        }
    }
    let assets = app.client.engine_assets.take();
    let appearance = app.client.appearance.clone();
    let owned = app.client.owned_materials.take();
    app.client = Client::new(Scene::from_engine(
        cells,
        engine::WorldState::new(44, r, floor, None),
    )?)?;
    app.client.engine_assets = assets;
    app.client.appearance = appearance;
    app.client.owned_materials = owned;
    w(app).library = library;
    app.client.set_spawn(Vec3::new(0.5, 1., -8.5))?;
    app.client.yaw = 0.;
    app.client.pitch = -0.1;
    app.simulation_previous = app.client.actor.position;
    app.wireframe = case == "wire";
    if case == "shadow_off" {
        app.settings.rendering.shadows = false;
    }
    app.debug = false;
    app.show_character = false;
    app.frame_limit = 2000;
    app.root = out.into();
    let mut ids = Vec::new();
    let limit = match case {
        "follow" => 80,
        "flee" => 25,
        "path" => 150,
        "budget" => 20,
        "save" => 30,
        "natural" => 120,
        _ => 1,
    };
    match case {
        "showcase" | "save" | "wire" | "shadow_off" => {
            idle(app)?;
            for (kind, p) in [
                ("grazer", [-2.5, 1., 0.5]),
                ("companion", [0.5, 1., 2.5]),
                ("stalker", [3.5, 1., 0.5]),
            ] {
                ids.push(interaction::spawn(&mut app.client, kind, p, true)?);
            }
        }
        "custom" => {
            idle(app)?;
            ids.push(interaction::spawn(
                &mut app.client,
                "companion",
                [0.5, 1., -3.5],
                true,
            )?);
        }
        "follow" => {
            let id = interaction::spawn(&mut app.client, "companion", [0.5, 1., 1.5], true)?;
            w(app).interact(id);
            ids.push(id);
        }
        "flee" => {
            ids.push(interaction::spawn(
                &mut app.client,
                "grazer",
                [3.5, 1., -5.5],
                true,
            )?);
        }
        "path" => {
            app.client.set_spawn(Vec3::new(4.5, 1., 0.5))?;
            let stone = app
                .client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .registry
                .find("slate")?;
            for z in -3..=3 {
                for y in 1..=3 {
                    app.client.scene.change_engine([0, y, z], Some(stone))?;
                }
            }
            let id = interaction::spawn(&mut app.client, "companion", [-4.5, 1., 0.5], true)?;
            w(app).interact(id);
            ids.push(id);
            app.client.view.mode = crate::engine_camera::Mode::Free;
            app.client.view.free = [10., 9., -12.];
            app.client.view.previous_free = app.client.view.free;
            app.client.view.yaw = -0.55;
            app.client.view.pitch = -0.4;
        }
        "interaction" | "occluded" | "occluded_clear" => {
            idle(app)?;
            let id = interaction::spawn(&mut app.client, "companion", [0.5, 1., -4.5], true)?;
            ids.push(id);
            app.client.pitch = -0.16;
            if case != "interaction" {
                let stone = app
                    .client
                    .scene
                    .engine
                    .as_ref()
                    .unwrap()
                    .registry
                    .find("slate")?;
                for x in -1..=1 {
                    for y in 1..=3 {
                        app.client.scene.change_engine([x, y, -6], Some(stone))?;
                    }
                }
                require(
                    interaction::target(&app.client)?.is_none(),
                    "Entity selected through an opaque wall",
                )?;
                if case == "occluded_clear" {
                    for x in -1..=1 {
                        for y in 1..=3 {
                            app.client.scene.change_engine([x, y, -6], None)?;
                        }
                    }
                    require(
                        interaction::target(&app.client)? == Some(id),
                        "Cleared wall did not expose entity",
                    )?;
                }
            } else {
                require(
                    interaction::target(&app.client)? == Some(id),
                    "Entity selection missed",
                )?;
                let revision = app.client.scene.engine.as_ref().unwrap().edit_revision;
                app.client.edit(false)?;
                app.world_use(false)?;
                require(
                    w(app).records[&id].health.as_ref().unwrap().current == 16
                        && w(app).records[&id].brain.as_ref().unwrap().following,
                    "Shared entity hit/use failed",
                )?;
                require(
                    app.client.scene.engine.as_ref().unwrap().edit_revision == revision,
                    "Entity actions dirtied terrain",
                )?;
            }
        }
        "budget" => {
            idle(app)?;
            app.client.set_spawn(Vec3::new(0.5, 1., -25.5))?;
            app.client.pitch = -0.12;
            for i in 0..200 {
                ids.push(interaction::spawn(
                    &mut app.client,
                    "companion",
                    [
                        -19.5 + (i % 20) as f32 * 2.,
                        1.,
                        -9.5 + (i / 20) as f32 * 2.,
                    ],
                    true,
                )?);
            }
            let far = w(app).spawn("grazer", [200., 1., 0.], true)?;
            ids.push(far);
        }
        "natural" => {
            w(app).natural = true;
        }
        "high" => {
            let r = app.client.scene.engine.as_ref().unwrap().registry.clone();
            let library = w(app).library.clone();
            let mut high = crate::engine_stream::generate_modern(
                44,
                r,
                engine::world::terrain::Settings::built_in(),
            )?;
            high.engine_assets = app.client.engine_assets.take();
            high.owned_materials = app.client.owned_materials.take();
            high.appearance = app.client.appearance.clone();
            app.client = high;
            w(app).library = library;
            app.client.set_spawn(Vec3::new(-0.5, 4097., -8.5))?;
            let floor = app
                .client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .registry
                .find("basalt")?;
            for x in -6..=6 {
                for z in -14..=6 {
                    app.client.scene.change_engine([x, 4096, z], Some(floor))?;
                }
            }
            idle(app)?;
            ids.push(interaction::spawn(
                &mut app.client,
                "companion",
                [-0.5, 4097., -3.5],
                true,
            )?);
        }
        _ => unreachable!(),
    }
    if matches!(case, "showcase" | "wire" | "save" | "shadow_off") {
        for (id, yaw) in ids.iter().zip([0.4, -0.6, 0.9]) {
            w(app).records.get_mut(id).unwrap().transform.yaw = yaw;
        }
    }
    app.simulation_previous = app.client.actor.position;
    let initial = ids
        .iter()
        .map(|id| w(app).records[id].transform.position)
        .collect();
    app.entity_live = Some(Live {
        case: case.into(),
        out: out.into(),
        tick: 0,
        limit,
        ids,
        initial,
        trace: Vec::new(),
        done: false,
    });
    Ok(())
}
pub fn advance(app: &mut App) -> Result<()> {
    let Some(mut live) = app.entity_live.take() else {
        return Ok(());
    };
    if live.done || app.gpu.as_ref().is_none_or(|g| !g.stats.meshes_ready) {
        app.entity_live = Some(live);
        return Ok(());
    }
    let before = Instant::now();
    app.client.tick_world()?;
    let elapsed = before.elapsed().as_secs_f64() * 1000.;
    live.tick += 1;
    let world = &app.client.scene.engine.as_ref().unwrap().entities;
    require(
        world.stats.physics_steps <= entities::ACTIVE_LIMIT
            && world.stats.decisions <= entities::AI_BUDGET
            && world.stats.paths <= entities::PATH_BUDGET
            && world.stats.path_nodes <= entities::PATH_BUDGET * navigation::NODE_BUDGET,
        "Entity tick budget exceeded",
    )?;
    live.trace.push(serde_json::json!({"tick":live.tick,"ms":elapsed,"stats":world.stats,"positions":world.records.values().map(|e|(e.id,e.transform.position)).collect::<Vec<_>>()}));
    for e in world.records.values() {
        if let Some(b) = &e.physics
            && distance(e.transform.position, position(app.client.actor.position))
                < entities::ACTIVE_RADIUS
        {
            require(
                navigation::clear(&app.client, e.transform.position, b),
                "Entity penetrated registered terrain",
            )?;
        }
    }
    if live.tick >= live.limit {
        let player = position(app.client.actor.position);
        let world = w(app);
        if live.case == "follow" {
            require(
                distance(world.records[&live.ids[0]].transform.position, player)
                    < distance(live.initial[0], player) - 5.,
                "Follow AI made no progress",
            )?;
        }
        if live.case == "flee" {
            require(
                distance(world.records[&live.ids[0]].transform.position, player)
                    > distance(live.initial[0], player) + 0.5,
                "Flee AI made no progress",
            )?;
        }
        if live.case == "path" {
            require(
                world.records[&live.ids[0]].transform.position[0] > 1.5,
                "Navigation did not route around wall",
            )?;
        }
        if live.case == "budget" {
            require(
                world.records[&live.ids[200]].transform.position == [200., 1., 0.]
                    && world.records[&live.ids[200]]
                        .brain
                        .as_ref()
                        .unwrap()
                        .activity
                        == Activity::Sleeping,
                "Distant mob did not sleep",
            )?;
            require(
                world
                    .records
                    .values()
                    .take(200)
                    .all(|e| e.brain.as_ref().unwrap().decide_at > 0),
                "AI scheduler starved an entity",
            )?;
        }
        if live.case == "natural" {
            require(
                world.records.len() >= 3 && world.records.len() <= 24,
                "Ambient population failed",
            )?;
        }
        let snapshot = world.saved();
        let save = engine::save(&mut app.client, &live.out.join("saves"))?;
        let loaded = engine::load(&save)?;
        require(
            loaded.scene.engine.as_ref().unwrap().entities.saved() == snapshot,
            "Entities changed in save roundtrip",
        )?;
        let clock = w(app).clock;
        app.menu.page = crate::engine_settings::Page::Pause;
        app.advance_world(0.2)?;
        require(w(app).clock == clock, "Pause advanced mob simulation")?;
        app.menu.page = crate::engine_settings::Page::Closed;
        std::fs::write(
            live.out.join("entities.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"complete":true,"case":live.case,"world":w(app).observation(),"trace":live.trace,"initial":live.initial,"saved_world":save,"saved_entities_equal":true,"collision_clear":true,"paused_clock_preserved":true,"scope":"Native fixed-tick typed entities, bounded AI/pathfinding, registered collision, persistent IDs/components, authored instanced models and shared interaction."}),
            )?,
        )?;
        app.notice = None;
        live.done = true;
        app.frame_limit = app.frames
            + if app.settings.rendering.method == engine::render::methods::Method::RayTraced {
                90
            } else {
                8
            };
    }
    app.entity_live = Some(live);
    Ok(())
}
