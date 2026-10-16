//! Native modern-terrain scenes, arbitrary vertical construction and sparse restore.
use crate::{App, engine_player, scene::Result};
use mcbe_core::motion::Vec3;
use std::path::Path;
fn require(ok: bool, s: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(s.into()) }
}
pub fn prepare(app: &mut App, out: &Path, case: &str) -> Result<()> {
    if out.exists() {
        return Err("Fresh terrain proof path required".into());
    }
    std::fs::create_dir_all(out)?;
    app.root = out.to_path_buf();
    require(
        app.client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .stream
            .as_ref()
            .unwrap()
            .generator
            .config
            .version
            == 2,
        "Modern terrain not selected",
    )?;
    let initial = app.client.actor.position;
    let generator = app
        .client
        .scene
        .engine
        .as_ref()
        .unwrap()
        .stream
        .as_ref()
        .unwrap()
        .generator
        .clone();
    let terrain = generator.terrain.as_ref().unwrap();
    engine_player::mode(&mut app.client, engine_player::Mode::Noclip)?;
    let mut marker = None;
    if matches!(case, "high" | "deep") {
        let y = if case == "high" { 10000 } else { -10000 };
        app.client.set_spawn(Vec3::new(0.5, y as f32 + 2., 0.5))?;
        let stone = app
            .client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .find("slate")?;
        for x in -4..=4 {
            for z in -4..=4 {
                app.client.scene.change_engine([x, y, z], Some(stone))?;
                if case == "deep" {
                    for dy in 1..=6 {
                        app.client.scene.change_engine([x, y + dy, z], None)?;
                    }
                }
            }
        }
        marker = Some([0, y, 0]);
        app.save_shortcut()?;
        app.finish_work()?;
        let path = app
            .client
            .last_save
            .clone()
            .ok_or("Vertical save missing")?;
        let bytes = std::fs::read(&path)?;
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        require(
            value["version"] == 10 && value["generator"]["version"] == 2,
            "Vertical save schema differs",
        )?;
        app.client.set_spawn(initial)?;
        require(
            !app.client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .blocks
                .contains_key(&[0, y, 0]),
            "Height chunk did not evict",
        )?;
        app.engine_key(winit::keyboard::KeyCode::F9)?;
        app.finish_work()?;
        require(
            app.client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .blocks
                .get(&[0, y, 0])
                == Some(&stone),
            "Vertical edit lost on F9",
        )?;
        app.client.pitch = if case == "high" { -0.8 } else { -0.1 };
        app.client.yaw = 0.4;
    } else if case == "cave" {
        let mut point = None;
        'search: for x in (-96..96).step_by(4) {
            for z in (-96..96).step_by(4) {
                let sample = terrain.sample(x, z);
                for y in (sample.height - 100..sample.height - 12).step_by(3) {
                    if (0..3).all(|dy| terrain.block([x, y + dy, z], sample, None).is_none())
                        && (1..=3).any(|dy| terrain.block([x, y - dy, z], sample, None).is_some())
                    {
                        point = Some([x, y, z]);
                        break 'search;
                    }
                }
            }
        }
        let [x, y, z] = point.ok_or("Cave with air and nearby floor not found")?;
        app.client
            .set_spawn(Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5))?;
        app.client.yaw = 0.8;
        app.client.pitch = -0.15;
    } else {
        let mut point = None;
        'search: for radius in 0..=64i32 {
            for x in -radius..=radius {
                for z in -radius..=radius {
                    if x.abs() != radius && z.abs() != radius {
                        continue;
                    }
                    let (x, z) = (x * 32, z * 32);
                    let s = terrain.sample(x, z);
                    let id = &terrain.settings.biomes[s.biome].id;
                    if (case == "ravine" && s.ravine)
                        || (case == "ocean" && id == "ocean" && s.water.is_some())
                        || (case == "river" && id == "river" && s.water.is_some())
                        || (!matches!(case, "ocean" | "river" | "ravine") && id == case)
                    {
                        point = Some((x, z, s));
                        break 'search;
                    }
                }
            }
        }
        let (x, z, s) = point.ok_or("Terrain case not found within bounded tour search")?;
        let height = if case == "ravine" {
            s.height + 6
        } else {
            s.water.unwrap_or(s.height) + 14
        };
        app.client
            .set_spawn(Vec3::new(x as f32 + 0.5, height as f32, z as f32 + 0.5))?;
        app.client.yaw = if case == "ravine" { 1.57 } else { 0.8 };
        app.client.pitch = if case == "ravine" { -0.1 } else { -0.4 };
    }
    let state = app.client.scene.engine.as_ref().unwrap();
    let stream = state.stream.as_ref().unwrap();
    let p = app.client.actor.position;
    let sample = terrain.sample(p.x.floor() as i32, p.z.floor() as i32);
    require(
        app.client.scene.cells.is_empty(),
        "Modern chunks duplicated into legacy cell hash set",
    )?;
    std::fs::write(
        out.join("terrain.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"complete":true,"case":case,"sample":sample,"biome":terrain.settings.biomes[sample.biome].id,"marker":marker,"saved_world":app.client.last_save,"storage_bytes":state.blocks.allocated_bytes(),"voxel_count":state.blocks.len(),"resident_chunks":stream.volume.as_ref().unwrap().residents.len(),"wanted_chunks":stream.volume.as_ref().unwrap().wanted.len(),"dynamic_height":true,"legacy_cell_copies":app.client.scene.cells.len(),"simulation_advanced":false,"scope":"Modern terrain, density/biome sampling, actual 3D residency, high/deep construction and F9 sparse save restoration. These terrain captures keep simulation paused; fluid transport and swimming are verified by the fluid suite."}),
        )?,
    )?;
    app.notice = None;
    app.debug = true;
    app.frame_limit = 120;
    Ok(())
}
