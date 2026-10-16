//! Controlled geometry, physical material variations and actual renderer-menu transitions.
use crate::engine::render::methods::{Method, Quality};
use crate::{
    App, engine, engine_player,
    engine_settings::{Page, Settings},
    scene::{Client, Result, Scene},
};
use mcbe_core::motion::Vec3;
use std::{collections::HashMap, path::Path};
use winit::keyboard::KeyCode;
fn require(ok: bool, m: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(m.into()) }
}
fn open_graphics(app: &mut App) -> Result<()> {
    app.engine_key(KeyCode::Escape)?;
    app.engine_key(KeyCode::ArrowDown)?;
    app.engine_key(KeyCode::Enter)?;
    for _ in 0..7 {
        app.engine_key(KeyCode::ArrowDown)?;
    }
    app.engine_key(KeyCode::Enter)?;
    require(
        app.menu.page == Page::Graphics,
        "Rendering page did not open",
    )
}
fn close_graphics(app: &mut App) -> Result<()> {
    for _ in 0..3 {
        app.engine_key(KeyCode::Escape)?;
    }
    require(!app.menu.open(), "Rendering page did not close")
}
pub struct Live {
    stage: u8,
    out: std::path::PathBuf,
    trace: Vec<serde_json::Value>,
    since: usize,
    ray: bool,
}
pub fn prepare(app: &mut App, out: &Path, case: &str) -> Result<()> {
    let requested_case = case.to_owned();
    let ray = case.starts_with("ray_");
    let flavor = case.strip_prefix("ray_").unwrap_or(case);
    let case = match flavor {
        "sun" => "lit",
        "mirror" | "no_reflections" => "metal",
        "indirect" | "no_indirect" => "emissive",
        _ => flavor,
    };
    require(!out.exists(), "Fresh lighting proof directory required")?;
    require(
        matches!(
            case,
            "forward"
                | "lit"
                | "unshadowed"
                | "noao"
                | "metal"
                | "emissive"
                | "low"
                | "high"
                | "wire"
                | "menu"
                | "toggle"
                | "resize"
                | "glass"
                | "water"
                | "cutout"
        ),
        "Unknown lighting case",
    )?;
    std::fs::create_dir_all(out)?;
    app.root = out.to_path_buf();
    let mut r = app.client.scene.engine.as_ref().unwrap().registry.clone();
    for (name, color) in [
        ("slate", [160, 150, 135]),
        ("cobalt", [200, 150, 70]),
        ("ember", [80, 160, 245]),
    ] {
        let i = r.find(name)? as usize;
        r.blocks[i].minecraft = None;
        r.blocks[i].face_textures = None;
        r.blocks[i].texture_files = None;
        r.blocks[i].color = color;
        r.blocks[i].pattern = crate::engine_blocks::Pattern::Solid;
    }
    if case == "metal" {
        let i = r.find("cobalt")? as usize;
        r.blocks[i].material.metallic = 1.;
        r.blocks[i].material.roughness = 0.18;
    }
    if case == "emissive" {
        let i = r.find("ember")? as usize;
        r.blocks[i].material.emission = 4.;
    }
    let mirror = ray && matches!(flavor, "mirror" | "no_reflections");
    if mirror {
        let i = r.find("ember")? as usize;
        r.blocks[i].material.emission = 4.;
    }
    r.validate()?;
    let basalt = r.find("basalt")?;
    let stone = r.find("slate")?;
    let metal = r.find("cobalt")?;
    let emissive = r.find("ember")?;
    let slab = r.find("oak_slab")?;
    let window = r.find("oak_window")?;
    let mut cells = HashMap::new();
    for x in -40..40 {
        for z in -40..40 {
            cells.insert([x, 0, z], basalt);
        }
    }
    for x in -8..=8 {
        for z in 0..=18 {
            cells.insert([x, 3, z], stone);
        }
    }
    for x in -1..=1 {
        for y in 4..=8 {
            for z in 9..=11 {
                cells.insert([x, y, z], stone);
            }
        }
    }
    for x in 3..=6 {
        for z in 3..=8 {
            cells.insert([x, 8, z], stone);
        }
    }
    for y in 4..=7 {
        cells.insert([3, y, 10], metal);
        cells.insert([-4, y, 9], emissive);
    }
    cells.insert([-3, 4, 6], slab);
    cells.insert([-3, 5, 6], window);
    if case == "glass" {
        let g = r.find("blue_glass")?;
        for y in 4..=6 {
            for z in 1..=4 {
                cells.insert([3, y, z], g);
            }
        }
    }
    if case == "cutout" {
        let g = r.find("oak_leaves")?;
        for x in -6..=-3 {
            for z in 3..=6 {
                cells.insert([x, 8, z], g);
            }
        }
    }
    if case == "water" {
        let w = r.find("water")?;
        for x in -6..=-3 {
            for z in 1..=4 {
                cells.insert([x, 4, z], w);
            }
        }
    }
    if mirror {
        // A mirror facing an emissive wall behind the camera. The wall is outside
        // primary visibility, so a screen-space reflection cannot produce it.
        for x in -8..=8 {
            for y in 4..=12 {
                cells.insert([x, y, 14], metal);
            }
        }
        for x in -20..=20 {
            for y in 4..=25 {
                cells.insert([x, y, -18], emissive);
            }
        }
    }
    let boxes=cells.iter().filter(|(p,_)|p[1]>3).map(|(p,id)|serde_json::json!({"position":p,"id":id,"layer":r.get(*id).render_layer,"boxes":r.get(*id).geometry.as_ref().map_or_else(||vec![[0.,0.,0.,1.,1.,1.]],|g|g.boxes.clone())})).collect::<Vec<_>>();
    let state = engine::WorldState::new(92, r, stone, None);
    let appearance = app.client.appearance.clone();
    let assets = app.client.engine_assets.take();
    app.client = Client::new(Scene::from_engine(cells, state)?)?;
    app.client.appearance = appearance;
    app.client.engine_assets = assets;
    engine_player::mode(&mut app.client, engine_player::Mode::Noclip)?;
    app.client.set_spawn(Vec3::new(7.5, 9., -7.5))?;
    let target = Vec3::new(0., 5., 8.);
    let eye = app.client.eye();
    let delta = glam::Vec3::new(
        target.x - eye[0] as f32,
        target.y - eye[1] as f32,
        target.z - eye[2] as f32,
    );
    app.client.yaw = delta.x.atan2(delta.z);
    app.client.pitch = delta
        .y
        .atan2((delta.x * delta.x + delta.z * delta.z).sqrt());
    app.simulation_previous = app.client.actor.position;
    app.show_character = false;
    app.debug = false;
    app.frame_limit = 90;
    app.settings.rendering.method = if matches!(case, "forward" | "menu" | "toggle") {
        Method::Forward
    } else if ray {
        Method::RayTraced
    } else {
        Method::Deferred
    };
    app.settings.rendering.shadows = case != "unshadowed";
    app.settings.rendering.ray_reflections = flavor != "no_reflections";
    app.settings.rendering.ray_indirect = flavor != "no_indirect";
    if ray && matches!(flavor, "sun" | "unshadowed") {
        app.settings.rendering.ray_reflections = false;
        app.settings.rendering.ray_indirect = false;
    }
    app.settings.rendering.ambient_occlusion =
        case != "noao" && !(ray && matches!(flavor, "sun" | "unshadowed"));
    app.settings.rendering.quality = match case {
        "low" => Quality::Low,
        "high" => Quality::High,
        _ => Quality::Balanced,
    };
    if case == "wire" {
        app.engine_key(KeyCode::F4)?;
    }
    if case == "menu" {
        open_graphics(app)?;
        app.engine_key(KeyCode::Enter)?;
        if ray {
            app.engine_key(KeyCode::Enter)?;
        }
        require(
            app.settings.rendering.method
                == if ray {
                    Method::RayTraced
                } else {
                    Method::Deferred
                },
            "Renderer menu did not switch",
        )?;
    }
    if case == "toggle" {
        app.frame_limit = 1000;
        app.lighting_live = Some(Live {
            stage: 0,
            out: out.to_path_buf(),
            trace: Vec::new(),
            since: 0,
            ray,
        });
    }
    if case == "resize" {
        app.window_test = true;
        app.frame_limit = 8;
    }
    app.settings.write(out)?;
    require(
        Settings::read(out)? == app.settings,
        "Lighting settings did not roundtrip",
    )?;
    let save = engine::save(&mut app.client, &out.join("saves"))?;
    let loaded = engine::load(&save)?;
    let current = app.client.scene.engine.as_ref().unwrap();
    let copy = loaded.scene.engine.as_ref().unwrap();
    require(
        serde_json::to_vec(&current.registry)? == serde_json::to_vec(&copy.registry)?,
        "Material save did not roundtrip",
    )?;
    std::fs::write(
        out.join("lighting-scene.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"complete":true,"case":requested_case,"ray":ray,"mirror_offscreen_wall":mirror,"settings_persisted":true,"materials_saved":true,"saved_world":save,"floor_id":stone,"floor_height":4.,"occluders":boxes,"scope":"Controlled opaque/partial/cutout geometry, lighting controls, material storage and native renderer transitions."}),
        )?,
    )?;
    Ok(())
}
pub fn advance(app: &mut App) -> Result<()> {
    let Some(mut live) = app.lighting_live.take() else {
        return Ok(());
    };
    let Some(g) = &app.gpu else {
        app.lighting_live = Some(live);
        return Ok(());
    };
    if !g.stats.meshes_ready || g.stats.texture_uploads != 1 || app.frames < live.since + 10 {
        app.lighting_live = Some(live);
        return Ok(());
    }
    if g.stats.render_method == Method::RayTraced {
        let observation = g.rendering_observation();
        if observation["ray_scene"]["ready"] != true || observation["ray_scene"]["pending"] != 0 {
            app.lighting_live = Some(live);
            return Ok(());
        }
    }
    match live.stage {
        0 => {
            require(
                g.stats.graphics_bytes == 0,
                "Forward renderer allocated deferred targets",
            )?;
            live.trace
                .push(serde_json::json!({"stage":"forward","renderer":g.rendering_observation()}));
            open_graphics(app)?;
            app.engine_key(KeyCode::Enter)?;
            close_graphics(app)?;
            live.stage = 1;
            live.since = app.frames;
        }
        1 => {
            require(
                g.stats.graphics_bytes > 0 && g.stats.deferred_passes > 0,
                "Deferred switch did not draw",
            )?;
            live.trace
                .push(serde_json::json!({"stage":"deferred","renderer":g.rendering_observation()}));
            open_graphics(app)?;
            app.engine_key(if live.ray {
                KeyCode::Enter
            } else {
                KeyCode::ArrowLeft
            })?;
            close_graphics(app)?;
            live.stage = 2;
            live.since = app.frames;
        }
        2 if live.ray => {
            require(
                g.rendering_observation()["genuine_ray_tracing"] == true,
                "Ray renderer did not become ready",
            )?;
            live.trace.push(
                serde_json::json!({"stage":"ray_traced","renderer":g.rendering_observation()}),
            );
            open_graphics(app)?;
            for _ in 0..4 {
                app.engine_key(KeyCode::ArrowDown)?;
            }
            app.engine_key(KeyCode::Enter)?;
            close_graphics(app)?;
            live.stage = 3;
            live.since = app.frames;
        }
        3 if live.ray => {
            require(
                app.settings.rendering.quality == Quality::High
                    && g.rendering_observation()["ray_scene"]["parameters"]["options"][2] == 4.,
                "Live ray quality change was not applied",
            )?;
            live.trace.push(
                serde_json::json!({"stage":"ray_quality","renderer":g.rendering_observation()}),
            );
            open_graphics(app)?;
            app.engine_key(KeyCode::Enter)?;
            close_graphics(app)?;
            live.stage = 4;
            live.since = app.frames;
        }
        _ => {
            require(
                g.stats.graphics_bytes == 0 && g.stats.render_method == Method::Forward,
                "Forward switch retained deferred targets",
            )?;
            live.trace
                .push(serde_json::json!({"stage":"released","renderer":g.rendering_observation()}));
            std::fs::write(
                live.out.join("lighting-switch.json"),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"complete":true,"trace":live.trace,"scope":"Actual settings keys switch between live renderers and release deferred GPU allocations."}),
                )?,
            )?;
            app.frame_limit = app.frames + 10;
            return Ok(());
        }
    }
    app.lighting_live = Some(live);
    Ok(())
}
