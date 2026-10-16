//! Actual camera controls, detached movement, skin input and native held-model scenes.
use crate::{App, engine_camera, engine_inventory::Stack, scene::Result};
use std::path::Path;
use winit::keyboard::KeyCode;
pub fn prepare(app: &mut App, out: &Path, case: &str) -> Result<()> {
    if out.exists() {
        return Err("Fresh player verification output required".into());
    }
    std::fs::create_dir_all(out)?;
    app.root = out.to_path_buf();
    app.show_character = true;
    app.client.yaw = 0.;
    app.client.pitch = -0.08;
    let before = app.client.actor;
    let initial_yaw = app.client.yaw;
    let initial_pitch = app.client.pitch;
    let id = app
        .client
        .scene
        .engine
        .as_ref()
        .unwrap()
        .registry
        .find(match case {
            "slab" => "oak_slab",
            "glass" => "blue_glass",
            _ => "oak_planks",
        })?;
    {
        let s = app.client.scene.engine.as_mut().unwrap();
        s.player.slots[s.player.active] = if case == "empty" {
            None
        } else {
            Some(Stack {
                data: None,
                id,
                count: 64,
            })
        };
    }
    let count = match case {
        "back" | "blocked" => 1,
        "front" => 2,
        "free" | "occluded" => 3,
        "first" | "empty" | "slab" | "glass" => 0,
        _ => return Err("Unknown player verification case".into()),
    };
    for _ in 0..count {
        app.engine_key(KeyCode::F5)?;
    }
    if app.saver.is_some() {
        return Err("F5 unexpectedly requested a save".into());
    }
    if case == "blocked" {
        let basalt = app
            .client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .find("basalt")?;
        for y in 4..7 {
            app.client.scene.change_engine([0, y, -11], Some(basalt))?;
        }
    }
    if case == "occluded" {
        app.client.view.free = [0.5, 5.62, -5.5];
        app.client.view.yaw = std::f32::consts::PI;
        app.client.view.pitch = 0.;
        let basalt = app
            .client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .find("basalt")?;
        for x in -1..=1 {
            for y in 4..=6 {
                app.client.scene.change_engine([x, y, -7], Some(basalt))?;
            }
        }
    }
    if case == "free" {
        app.client.look(-0.3, -0.1);
        for _ in 0..8 {
            app.client.tick(crate::scene::Input {
                forward: 1.,
                side: 0.4,
                jump: true,
                sprint: false,
                ..Default::default()
            })?;
        }
        if app.client.actor.position != before.position
            || app.client.yaw != initial_yaw
            || app.client.pitch != initial_pitch
        {
            return Err("Free camera moved the actor".into());
        }
        if app.client.edit(false)? {
            return Err("Free camera edited terrain".into());
        }
        app.modifiers = winit::keyboard::ModifiersState::CONTROL;
        app.engine_key(KeyCode::KeyS)?;
        app.modifiers = winit::keyboard::ModifiersState::empty();
        app.finish_work()?;
        let saved =
            crate::engine::load(app.client.last_save.as_ref().ok_or("Ctrl+S did not save")?)?;
        if saved.actor.position != before.position
            || saved.yaw != initial_yaw
            || saved.pitch != initial_pitch
        {
            return Err("Free camera position leaked into save".into());
        }
    }
    let pose = engine_camera::pose(&app.client, app.client.eye().map(|v| v as f32))?;
    std::fs::write(
        out.join("player.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"complete":true,"case":case,"pose":pose,"actor_position":app.client.actor.position,"skin":app.client.appearance.provenance,"free_actor_unchanged":case=="free","saved_actor_unchanged":case=="free","ctrl_s_save":case=="free","orbit_distance":glam::Vec3::from(pose.eye).distance(glam::Vec3::from(app.client.eye().map(|v|v as f32)))}),
        )?,
    )?;
    app.notice = None;
    app.frame_limit = 80;
    Ok(())
}
