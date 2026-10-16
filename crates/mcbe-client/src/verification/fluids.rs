//! Real conservative transport, swimming, sparse reload and native surface captures.
use crate::engine::fluids::{self, FULL};
use crate::{
    App, engine_player,
    scene::{Input, Result},
};
use mcbe_core::motion::Vec3;
use std::path::Path;
const Y: i32 = 10000;
fn require(ok: bool, s: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(s.into()) }
}
fn mass(app: &App) -> u64 {
    let s = app.client.scene.engine.as_ref().unwrap();
    let mut n = 0;
    for x in -2..=5 {
        for y in Y..Y + 20 {
            for z in 6..=13 {
                n += u64::from(fluids::amount(s, [x, y, z]));
            }
        }
    }
    n
}
pub fn prepare(app: &mut App, out: &Path, case: &str) -> Result<()> {
    require(!out.exists(), "Fresh fluid proof path required")?;
    std::fs::create_dir_all(out)?;
    app.root = out.to_path_buf();
    require(
        matches!(case, "flow" | "underwater" | "pressure" | "lod"),
        "Unknown fluid case",
    )?;
    engine_player::mode(&mut app.client, engine_player::Mode::Noclip)?;
    app.client.set_spawn(Vec3::new(1.5, Y as f32 + 4., 9.5))?;
    let r = &app.client.scene.engine.as_ref().unwrap().registry;
    let water = r.find("water")?;
    let stone = r.find("slate")?;
    let glass = r.find("glass")?;
    for x in -1..=4 {
        for z in 7..=12 {
            for y in Y..=Y + 8 {
                if y == Y || x == -1 || x == 4 || z == 7 || z == 12 {
                    app.client.scene.change_engine(
                        [x, y, z],
                        Some(if y > Y && z == 7 { glass } else { stone }),
                    )?;
                }
            }
        }
    }
    if matches!(case, "flow" | "lod") {
        for y in Y + 11..=Y + 15 {
            for z in 9..=10 {
                app.client.scene.set_fluid([1, y, z], water, FULL)?;
            }
        }
    } else if case == "underwater" {
        for x in 0..=3 {
            for y in Y + 1..=Y + 5 {
                for z in 8..=11 {
                    app.client.scene.set_fluid([x, y, z], water, FULL)?;
                }
            }
        }
    } else {
        // Enclose two shafts and the covered bottom connection.
        for x in 0..=3 {
            for y in Y + 1..=Y + 7 {
                for z in 8..=11 {
                    if z != 9 || !(x == 0 || x == 2 || x == 1 && y == Y + 1) {
                        app.client
                            .scene
                            .change_engine([x, y, z], Some(if z == 8 { glass } else { stone }))?;
                    }
                }
            }
        }
        for y in Y + 1..=Y + 5 {
            app.client.scene.set_fluid([0, y, 9], water, FULL)?;
        }
        app.client.scene.set_fluid([1, Y + 1, 9], water, FULL)?;
        app.client.scene.set_fluid([2, Y + 1, 9], water, FULL)?;
    }
    let initial_mass = mass(app);
    let actor = app.client.actor.position;
    let before = app
        .client
        .scene
        .engine
        .as_ref()
        .unwrap()
        .fluids
        .counters
        .processed;
    app.inventory_open = true;
    app.advance_world(0.05)?;
    app.inventory_open = false;
    require(
        app.client.actor.position == actor
            && app
                .client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .fluids
                .counters
                .processed
                > before,
        "Inventory opening froze world simulation or moved the actor",
    )?;
    require(mass(app) == initial_mass, "UI-open simulation lost water")?;
    let mut steps = 0;
    for _ in 0..1600 {
        fluids::transport::tick(&mut app.client)?;
        steps += 1;
        require(
            mass(app) == initial_mass,
            "Conservative native transport lost mass",
        )?;
        require(
            app.client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .fluids
                .counters
                .last_processed
                <= fluids::STEP_BUDGET,
            "Fluid work exceeded step budget",
        )?;
    }
    if case == "pressure" {
        let s = app.client.scene.engine.as_ref().unwrap();
        let column = |x| {
            (Y + 1..=Y + 8)
                .map(|y| u32::from(fluids::amount(s, [x, y, 9])))
                .sum::<u32>()
        };
        require(
            column(0).abs_diff(column(2)) <= 1,
            "Covered shafts failed to equalize pressure",
        )?;
    }
    let mut movement = serde_json::Value::Null;
    if case == "underwater" {
        engine_player::mode(&mut app.client, engine_player::Mode::Walking)?;
        app.client.set_spawn(Vec3::new(1.5, Y as f32 + 2., 9.5))?;
        let begin = app.client.actor.position;
        for _ in 0..12 {
            engine_player::tick(
                &mut app.client,
                Input {
                    jump: true,
                    ..Default::default()
                },
            )?;
        }
        let risen = app.client.actor.position;
        for _ in 0..20 {
            engine_player::tick(
                &mut app.client,
                Input {
                    sneak: true,
                    ..Default::default()
                },
            )?;
        }
        let dived = app.client.actor.position;
        require(
            risen.y > begin.y + 0.3 && dived.y < risen.y && app.client.movement.swimming,
            "Real swimming controls did not rise/dive",
        )?;
        movement = serde_json::json!({"begin":begin,"risen":risen,"dived":dived});
        app.client.yaw = 0.1;
        app.client.pitch = 0.25;
        require(
            crate::engine::player::locomotion::Environment::sample(&app.client).head_underwater,
            "Underwater camera case is dry",
        )?;
    } else {
        let target = Vec3::new(1.5, Y as f32 + 1.6, 9.5);
        let camera = Vec3::new(6.5, Y as f32 + 5., 3.5);
        app.client.set_spawn(camera)?;
        let dx = target.x - camera.x;
        let dz = target.z - camera.z;
        app.client.yaw = dx.atan2(dz);
        app.client.pitch = (target.y - camera.y - app.client.movement.eye_height())
            .atan2((dx * dx + dz * dz).sqrt());
    }
    let transport_observation = fluids::observation(&app.client);
    let amounts = app
        .client
        .scene
        .engine
        .as_ref()
        .unwrap()
        .fluids
        .amounts
        .clone();
    app.save_shortcut()?;
    app.finish_work()?;
    let saved = app.client.last_save.clone().ok_or("Fluid save missing")?;
    let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(&saved)?)?;
    require(doc["version"] == 8, "Fluid save schema differs")?;
    let old = app.client.actor.position;
    engine_player::mode(&mut app.client, engine_player::Mode::Noclip)?;
    app.client.set_spawn(Vec3::new(0.5, 100., 0.5))?;
    app.engine_key(winit::keyboard::KeyCode::F9)?;
    app.finish_work()?;
    require(
        app.client.actor.position == old
            && app.client.scene.engine.as_ref().unwrap().fluids.amounts == amounts
            && mass(app) == initial_mass,
        "Fluid amounts changed after eviction/F9",
    )?;
    let state = app.client.scene.engine.as_ref().unwrap();
    let heights: Vec<_> = (0..=3)
        .flat_map(|x| (8..=11).map(move |z| (x, z)))
        .map(|(x, z)| serde_json::json!({"x":x,"z":z,"amount":fluids::amount(state,[x,Y+1,z])}))
        .collect();
    if matches!(case, "flow" | "lod") {
        require(
            heights.iter().all(|v| {
                v["amount"]
                    .as_u64()
                    .is_some_and(|a| (638..=642).contains(&a))
            }),
            "Conservative pool did not level",
        )?;
    }
    std::fs::write(
        out.join("fluids.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"complete":true,"case":case,"mass":initial_mass,"steps":steps,"heights":heights,"partial_cells":state.fluids.amounts.len(),"saved_world":saved,"movement":movement,"eviction_reload":true,"world_updates_with_ui_open":true,"pending_after_reload":state.pending(),"transport":transport_observation,"observation":fluids::observation(&app.client),"scope":"Actual conservative transport, covered-passage hydrostatic pressure, swimming controls, fractional surfaces, high vertical eviction, Ctrl+S/F9 and persistent fluid amounts. Bucket items remain separate work."}),
        )?,
    )?;
    app.notice = None;
    app.debug = true;
    app.frame_limit = 140;
    if case == "lod" {
        app.fluid_live = Some(Live {
            start: None,
            published: false,
            mutations: 0,
            done: false,
            out: out.to_path_buf(),
            mass: initial_mass,
        });
        app.frame_limit = 100_000;
    }
    Ok(())
}

pub struct Live {
    start: Option<std::time::Instant>,
    published: bool,
    mutations: u32,
    done: bool,
    out: std::path::PathBuf,
    mass: u64,
}
pub fn advance(app: &mut App) -> Result<()> {
    let Some(mut live) = app.fluid_live.take() else {
        return Ok(());
    };
    if !live.done
        && app
            .gpu
            .as_ref()
            .is_some_and(|g| g.stats.texture_uploads > 0)
    {
        if live.start.is_none() {
            require(
                app.gpu.as_ref().unwrap().stats.distant_vertices == 0,
                "Live LOD scenario must start without a distant mesh",
            )?;
            crate::engine_stream::set_radius(&mut app.client, 16)?;
            app.settings.render_distance = 16;
        }
        let start = live.start.get_or_insert_with(std::time::Instant::now);
        let elapsed = start.elapsed();
        if elapsed < std::time::Duration::from_secs(3)
            || ((!live.published || live.mutations <= 10)
                && elapsed < std::time::Duration::from_secs(15))
        {
            if app.frames.is_multiple_of(4) {
                let w = app
                    .client
                    .scene
                    .engine
                    .as_ref()
                    .unwrap()
                    .registry
                    .find("water")?;
                let state = app.client.scene.engine.as_ref().unwrap();
                let total =
                    fluids::amount(state, [0, Y + 1, 8]) + fluids::amount(state, [1, Y + 1, 8]);
                let mean = total / 2;
                let amount = if live.mutations.is_multiple_of(2) {
                    mean - 1
                } else {
                    mean + 1
                };
                app.client.scene.set_fluid([0, Y + 1, 8], w, amount)?;
                app.client
                    .scene
                    .set_fluid([1, Y + 1, 8], w, total - amount)?;
                live.mutations += 1;
                require(mass(app) == live.mass, "Live LOD mutation lost water")?;
            }
            live.published |= app
                .gpu
                .as_ref()
                .is_some_and(|g| g.stats.distant_vertices > 0);
        } else {
            require(
                live.published && live.mutations > 10,
                "Continuous fluid edits starved the distant mesh worker",
            )?;
            live.done = true;
            app.frame_limit = app.frames + 120;
            std::fs::write(
                live.out.join("fluid-live.json"),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"complete":true,"published_during_edits":live.published,"mutations":live.mutations,"elapsed_seconds":elapsed.as_secs_f64(),"mass":live.mass,"scope":"Actual per-frame fractional fluid edits while the LOD worker builds; a matching view must publish before the edit stream stops, without mass loss."}),
                )?,
            )?;
        }
    }
    app.fluid_live = Some(live);
    Ok(())
}
