//! Native bucket collection/pouring, inventory recovery and held geometry.
use crate::engine::items::{self, ItemData};
use crate::{
    App, engine,
    engine_inventory::{Button, Slot},
    scene::Result,
};
use mcbe_core::motion::Vec3;
use std::path::Path;
use winit::keyboard::KeyCode;
fn require(ok: bool, m: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(m.into()) }
}
fn carried(app: &App) -> u64 {
    let s = app.client.scene.engine.as_ref().unwrap();
    s.player
        .slots
        .iter()
        .chain(&s.player.crafting)
        .chain(&s.player.table)
        .chain(std::iter::once(&s.player.cursor))
        .flatten()
        .map(|s| match s.data {
            Some(ItemData::Fluid { amount, .. }) => u64::from(amount),
            None => 0,
        })
        .sum()
}
fn total(app: &App) -> u64 {
    let s = app.client.scene.engine.as_ref().unwrap();
    let mut mass = carried(app);
    for x in -2..=5 {
        for y in 10000..10020 {
            for z in 6..=13 {
                mass += u64::from(engine::fluids::amount(s, [x, y, z]));
            }
        }
    }
    mass
}
pub fn prepare(app: &mut App, out: &Path, case: &str) -> Result<()> {
    require(
        matches!(case, "empty" | "partial" | "full" | "inventory"),
        "Unknown bucket case",
    )?;
    super::fluids::prepare(app, out, "flow")?;
    app.show_character = true;
    app.client.view = Default::default();
    app.palette.toggle(&mut app.client)?;
    app.palette.filter = "bucket".into();
    app.palette.cursor = 0;
    app.palette.key(&mut app.client, KeyCode::End)?;
    app.palette.key(&mut app.client, KeyCode::Enter)?;
    require(
        items::bucket::selected(&app.client),
        "Catalog did not select bucket",
    )?;
    let before = total(app);
    require(before == 10240, "Initial bucket tank mass differs")?;
    app.client.set_spawn(Vec3::new(1.5, 10003., 9.5))?;
    app.client.yaw = 0.;
    app.client.pitch = -1.2;
    require(app.client.edit(true)?, "Actual bucket collection failed")?;
    let partial = carried(app);
    require(
        partial > 0 && partial < 1024 && total(app) == before,
        "Shallow collection did not conserve mass",
    )?;
    if case == "full" {
        app.client.set_spawn(Vec3::new(0.5, 10003., 8.5))?;
        require(app.client.edit(true)?, "Top-up collection failed")?;
        require(carried(app) == 1024, "Bucket did not fill")?;
    }
    if case == "empty" {
        require(
            items::bucket::use_selected(&mut app.client, true)? == Some(true),
            "Forced pour failed",
        )?;
        require(carried(app) == 0, "Pour left contents")?;
    }
    require(total(app) == before, "World plus buckets changed mass")?;
    if case == "inventory" {
        app.palette.toggle(&mut app.client)?;
        let s = app.client.scene.engine.as_mut().unwrap();
        s.player
            .click(Slot::Player(s.player.active), Button::Right, false)?;
        s.player.click(Slot::Craft(3), Button::Left, false)?;
        require(carried(app) == partial, "Inventory transfer lost contents")?;
    }
    let inventory = app.client.scene.engine.as_ref().unwrap().player.clone();
    app.save_shortcut()?;
    app.finish_work()?;
    let saved = app.client.last_save.clone().ok_or("Bucket save missing")?;
    app.engine_key(KeyCode::F9)?;
    app.finish_work()?;
    require(
        app.client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .player
            .changed_slots(&inventory)
            == 0
            && total(app) == before,
        "Bucket save/reload lost contents",
    )?;
    app.client.pitch = -0.25;
    app.client.yaw = 0.;
    app.simulation_previous = app.client.actor.position;
    app.client.view.swing = 0.;
    app.frame_limit = 140;
    std::fs::write(
        out.join("buckets.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"complete":true,"case":case,"mass":before,"carried":carried(app),"partial_collected":partial,"creative_selection":true,"actual_use_ray":true,"save_reload_equal":true,"saved_world":saved,"inventory":inventory.slots.iter().map(|s|s.map(|s|serde_json::json!({"id":s.id,"count":s.count,"data":s.data}))).collect::<Vec<_>>(),"scope":"Real creative keyboard selection, precise six-block ray, partial collection/top-up/pouring, inventory transfer, Ctrl+S/F9 recovery and native held/UI geometry."}),
        )?,
    )?;
    Ok(())
}
