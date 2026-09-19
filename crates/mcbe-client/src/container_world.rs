//! Integration through the actual target/open/save/reload handlers.
use crate::{
    App,
    inventory_ui::Panel,
    render,
    scene::{Input, Result},
};
use mcbe_core::{
    container_geometry::ContainerBlock,
    container_storage::Store,
    items::Registry,
    motion::{Actor, Body, Vec3},
    world_edit,
};
use serde_json::{Value, json};
use std::{fs, path::Path};
use winit::keyboard::KeyCode;

fn pose(app: &mut App, p: [i32; 3]) -> Result<()> {
    app.client
        .set_spawn(Vec3::new(p[0] as f32 + 0.5, 64., p[2] as f32 - 1.5))?;
    app.client.yaw = 0.;
    app.client.pitch = -0.5;
    for _ in 0..20 {
        app.client.tick(Input::default())?;
    }
    if (app.client.actor.position.y - 64.).abs() > 2e-5 {
        return Err("Container fixture walking floor differs".into());
    }
    Ok(())
}
pub fn verify(app: &mut App, out: &Path) -> Result<()> {
    if out.exists() {
        return Err("Fresh container-world output required".into());
    }
    fs::create_dir_all(out)?;
    let source = app.client.scene.source.clone();
    let before = world_edit::tree_hashes(&source)?;
    let registry = Registry::from_bytes(&fs::read(
        app.root
            .join("evidence/bds-1.26.52.3/registry-probe-02/registry.json"),
    )?)?;
    let store = Store::read(&app.client.scene.snapshot, &registry)?;
    let records = store.record_hashes()?;
    if app.client.scene.containers.len() != 42 {
        return Err("Expected 42 resident native container blocks".into());
    }
    let mut targets = Vec::new();
    let mut rays = Vec::new();
    for (name, p, kind) in [
        ("barrel", [-14, 64, -14], ContainerBlock::BARREL),
        ("hopper", [-10, 64, -14], ContainerBlock::HOPPER),
        ("dispenser", [-6, 64, -14], ContainerBlock::DISPENSER),
        ("chest", [14, 64, 14], ContainerBlock::CHEST),
    ] {
        pose(app, p)?;
        if app.client.scene.containers.get(&p) != Some(&kind) {
            return Err("Container column classification differs".into());
        }
        let hit = app.client.target()?.ok_or("Container target missing")?;
        if [hit.position.x, hit.position.y, hit.position.z] != p {
            return Err("Walking camera targets another block".into());
        }
        rays.push(json!({"origin":app.client.eye(),"direction":app.client.direction(),"distance":6,"expected":hit}));
        render::png(
            &out.join(format!("{name}.png")),
            &render::render(&app.client, 960, 540),
            960,
            540,
        )?;
        if kind != ContainerBlock::CHEST {
            if !app.open_target_inventory()? {
                return Err("Targeted container did not open".into());
            }
            let panel = app.inventory.as_mut().ok_or("Container panel missing")?;
            let expected = store.load_group(&[p], &registry)?.observation();
            if panel.positions() != [p]
                || panel.source() != source
                || panel.containers.observation() != expected
            {
                return Err("Panel did not load the targeted saved record".into());
            }
            panel.key(KeyCode::Tab)?;
            panel.key(KeyCode::Digit2)?;
            panel.key(KeyCode::Digit3)?;
            fs::write(
                out.join(format!("{name}-panel.txt")),
                panel.lines()?.join("\n"),
            )?;
            targets.push(json!({"position":p,"contents":expected}));
            app.inventory_open = false;
        } else if Panel::load_target(&app.root, &source.join("db"), p).is_ok() {
            return Err("Chest mutation unexpectedly supported".into());
        }
        if app
            .client
            .scene
            .change(p, Some(mcbe_core::cubes::Cube::Stone))
            .is_ok()
        {
            return Err("Container replacement bypassed block entity lifecycle".into());
        }
    }
    if Panel::load_target(&app.root, &source.join("db"), [10, 64, 14]).is_ok() {
        return Err("Named/damaged container mutation unexpectedly supported".into());
    }
    let barrel = [-14, 64, -14];
    pose(app, barrel)?;
    app.open_target_inventory()?;
    app.inventory_key(KeyCode::KeyS)?;
    let changed_cube = [-15, 64, -15];
    app.client
        .scene
        .change(changed_cube, Some(mcbe_core::cubes::Cube::Stone))?;
    if app.client.pending_edits() != 1
        || app
            .inventory
            .as_ref()
            .ok_or("Panel missing")?
            .pending_slots()?
            != 1
    {
        return Err("Combined save inputs missing".into());
    }
    let pending = app.inventory.as_ref().unwrap().prepared_updates()?;
    if app
        .client
        .save_unique_checked(&app.root, &pending, |_| {
            Err("Intentional preflight rejection".into())
        })
        .is_ok()
        || app.client.scene.source != source
        || app.client.pending_edits() != 1
        || app.inventory.as_ref().unwrap().pending_slots()? != 1
    {
        return Err("Rejected combined save adopted or discarded pending edits".into());
    }
    app.inventory_key(KeyCode::F5)?;
    let first = app
        .client
        .last_save
        .clone()
        .ok_or("F5 world copy missing")?;
    if app.client.scene.source != first
        || app.inventory.as_ref().ok_or("Panel missing")?.source() != first
        || app.client.pending_edits() != 0
        || app.inventory.as_ref().unwrap().pending_slots()? != 0
    {
        return Err("Combined save did not adopt one shared world copy".into());
    }
    let saved_contents = app.inventory.as_ref().unwrap().containers.observation();
    app.inventory_key(KeyCode::Delete)?;
    app.client.scene.change(changed_cube, None)?;
    app.client.set_spawn(Vec3::new(-14.5, 64., -14.5))?;
    if app.reload_current().is_ok()
        || app.client.pending_edits() != 1
        || app.inventory.as_ref().unwrap().pending_slots()? != 1
    {
        return Err("Blocked reload discarded a world or inventory draft".into());
    }
    pose(app, barrel)?;
    app.inventory_key(KeyCode::F9)?;
    if app.inventory.as_ref().unwrap().containers.observation() != saved_contents
        || app.client.scene.kind_at(changed_cube) != Some(mcbe_core::cubes::Cube::Stone)
        || app.client.pending_edits() != 0
    {
        return Err("F9 failed to restore both container and block edits".into());
    }
    app.inventory_open = false;
    pose(app, [-10, 64, -14])?;
    app.open_target_inventory()?;
    app.inventory_key(KeyCode::KeyK)?;
    app.inventory_key(KeyCode::KeyS)?;
    if app.inventory.as_ref().unwrap().pending_slots()? != 1 {
        return Err("Second targeted container edit missing".into());
    }
    pose(app, barrel)?;
    if app.open_target_inventory().is_ok() {
        return Err("Opening another container discarded an unsaved draft".into());
    }
    app.inventory_open = false;
    let second = app.save_current()?;
    if first == second || app.inventory.as_ref().unwrap().source() != second {
        return Err("Closed panel save failed to keep one shared world copy".into());
    }
    let saved = Store::read(&app.client.scene.snapshot, &registry)?;
    let after_records = saved.record_hashes()?;
    let changed_records: Vec<_> = records
        .iter()
        .filter(|(p, h)| after_records.get(*p) != Some(*h))
        .map(|(p, _)| *p)
        .collect();
    if changed_records != vec![barrel, [-10, 64, -14]] {
        return Err("Combined saves changed unrelated container records".into());
    }
    if world_edit::tree_hashes(&source)? != before {
        return Err("Container integration changed source world".into());
    }
    let mut cases = Vec::new();
    let mut frames = Vec::new();
    for p in [barrel, [-10, 64, -14], [-6, 64, -14], [14, 64, 14]] {
        for mode in ["center-fall", "rim-fall", "north-wall"] {
            let index = cases.len();
            let origin = Vec3::new(
                p[0] as f32 + if mode == "rim-fall" { 0.05 } else { 0.5 },
                if mode == "north-wall" { 64. } else { 68. },
                p[2] as f32 + if mode == "north-wall" { -1.6 } else { 0.5 },
            );
            let impulse = Vec3::new(0., 0., if mode == "north-wall" { 0.9 } else { 0. });
            cases.push(json!({"index":index,"entityType":"reconstruction:motion_probe","mode":mode,"origin":origin,"impulse":impulse}));
            let mut actor = Actor::new(origin, Body::PROBE)?.impulse(impulse)?;
            for step in 0..=80 {
                frames.push(json!({"case":index,"step":step,"observation":actor.observation()}));
                if step < 80 {
                    actor = actor.tick_solid(&app.client.scene.motion_scene(&actor)?)?;
                }
            }
        }
    }
    pose(app, barrel)?;
    app.open_target_inventory()?;
    app.inventory_open = false;
    render::png(
        &out.join("saved-world.png"),
        &render::render(&app.client, 960, 540),
        960,
        540,
    )?;
    let report: Value = json!({"complete":true,"source":source,"source_hashes":before,"source_unchanged":true,"targets":targets,"first_save":first,"final_world":second,"saved_world_hashes":world_edit::tree_hashes(&second)?,"changed_cube":changed_cube,"changed_container_records":changed_records,"preserved_container_records":records.len()-changed_records.len(),"motion_cases":cases,"motion_frames":frames,"rays":rays,"final_inventory":app.inventory.as_ref().unwrap().containers.observation(),"scope":"Targeted saved containers and shared copied-world block/inventory persistence through development controls; container visuals provisional and native player inventory/gestures absent"});
    fs::write(
        out.join("container-world.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    app.frames = 0;
    app.inventory_frames = 0;
    app.notice = None;
    Ok(())
}
