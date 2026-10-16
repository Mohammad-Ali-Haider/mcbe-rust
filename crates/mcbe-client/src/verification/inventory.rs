//! Check the live inventory controller with real hitboxes and save/reload handlers.
use crate::{
    App, engine,
    engine_inventory::{Button, Slot, Stack},
    engine_ui::{Palette, Pointer, Target},
    scene::Result,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use winit::keyboard::KeyCode;
const VIEW: [f32; 2] = [480., 270.];
fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}
fn stack(app: &App, value: Option<Stack>) -> Value {
    value.map_or(Value::Null, |s| json!({"id":app.client.scene.engine.as_ref().unwrap().registry.get(s.id).id,"count":s.count}))
}
fn observation(app: &App) -> Value {
    let s = app.client.scene.engine.as_ref().unwrap();
    json!({"slots":s.player.slots.iter().map(|v|stack(app,*v)).collect::<Vec<_>>(),"crafting":s.player.crafting.iter().map(|v|stack(app,*v)).collect::<Vec<_>>(),"cursor":stack(app,s.player.cursor),"active":s.player.active,"total":s.player.total(),"output":s.recipes.find(&s.registry,&s.player).map(|r|stack(app,Some(r.stack))),"pending":app.client.pending_edits(),"open":app.palette.open})
}
fn record(trace: &mut Vec<Value>, app: &App, name: &str, total: usize) -> Result<()> {
    let s = app.client.scene.engine.as_ref().unwrap();
    s.player.validate(s.registry.blocks.len())?;
    require(s.player.total() == total, name)?;
    trace.push(json!({"step":name,"inventory":observation(app)}));
    Ok(())
}
fn center(rect: [f32; 4]) -> [f32; 2] {
    [rect[0] + rect[2] / 2., rect[1] + rect[3] / 2.]
}
fn point(app: &App, target: Target) -> Result<[f32; 2]> {
    let [x, y] = Palette::origin(VIEW);
    Ok(match target {
        Target::Slot(slot) => center(Palette::slot_rect(slot, VIEW)),
        Target::Output => [x + 311., y + 35.],
        Target::Search => [x + 20., y + 15.],
        Target::TableOutput => {
            return Err("Table outputs are checked by station verification".into());
        }
        Target::Machine(_) => {
            return Err("Machine slots are checked by station verification".into());
        }
        Target::Button(index) => center(
            app.palette
                .document(&app.client, VIEW)?
                .widgets
                .get(index as usize)
                .ok_or("UI button missing")?
                .rect
                .0,
        ),
        Target::Catalog(id) => {
            let visible = app
                .palette
                .visible(&app.client.scene.engine.as_ref().unwrap().registry);
            let i = visible
                .iter()
                .position(|v| *v == usize::from(id))
                .ok_or("Catalog block not visible")?;
            require(i < 42, "Study catalog entry must be on first page")?;
            [
                x + 16. + (i % 7) as f32 * 18.,
                y + 37. + (i / 7) as f32 * 18.,
            ]
        }
    })
}
fn click(app: &mut App, target: Target, button: Button, shift: bool) -> Result<()> {
    let position = point(app, target)?;
    require(
        app.palette.hit(&app.client, position, VIEW) == Some(target),
        "Inventory hitbox mismatch",
    )?;
    app.palette.click(
        &mut app.client,
        Pointer {
            position,
            viewport: VIEW,
            button,
            shift,
        },
    )
}
fn slot(app: &mut App, slot: Slot, button: Button, shift: bool) -> Result<()> {
    click(app, Target::Slot(slot), button, shift)
}
fn search(app: &mut App, term: &str) -> Result<()> {
    click(app, Target::Search, Button::Left, false)?;
    for _ in 0..32 {
        app.engine_key(KeyCode::Backspace)?;
    }
    app.palette.text(term);
    app.engine_key(KeyCode::Enter)?;
    Ok(())
}
fn expect_slot(app: &App, index: usize, id: &str, count: u8) -> Result<()> {
    require(
        stack(
            app,
            app.client.scene.engine.as_ref().unwrap().player.slots[index],
        ) == json!({"id":id,"count":count}),
        "Unexpected player slot",
    )
}
fn expect_output(app: &App, id: &str, count: u8) -> Result<()> {
    let s = app.client.scene.engine.as_ref().unwrap();
    require(
        s.recipes
            .find(&s.registry, &s.player)
            .map(|m| stack(app, Some(m.stack)))
            == Some(json!({"id":id,"count":count})),
        "Unexpected crafting output",
    )
}
pub fn verify(app: &mut App, out: &Path) -> Result<()> {
    require(
        !out.exists(),
        "Fresh inventory verification directory required",
    )?;
    fs::create_dir_all(out)?;
    app.root = out.to_path_buf();
    let original = app.client.scene.engine.as_ref().unwrap().blocks.clone();
    if let Some(assets) = &app.client.engine_assets {
        fs::write(
            out.join("assets.json"),
            serde_json::to_vec_pretty(&assets.provenance)?,
        )?;
    }
    let mut trace = Vec::new();
    app.engine_key(KeyCode::KeyE)?;
    require(app.palette.open, "E did not open inventory")?;
    record(&mut trace, app, "starter", 576)?;
    slot(app, Slot::Player(0), Button::Right, false)?;
    expect_slot(app, 0, "meadow", 32)?;
    require(
        stack(app, app.client.scene.engine.as_ref().unwrap().player.cursor)
            == json!({"id":"meadow","count":32}),
        "Right click did not split stack",
    )?;
    record(&mut trace, app, "split", 576)?;
    slot(app, Slot::Player(9), Button::Right, false)?;
    expect_slot(app, 9, "meadow", 1)?;
    slot(app, Slot::Player(0), Button::Left, false)?;
    expect_slot(app, 0, "meadow", 63)?;
    record(&mut trace, app, "single-place-and-merge", 576)?;
    slot(app, Slot::Player(0), Button::Left, true)?;
    expect_slot(app, 9, "meadow", 64)?;
    slot(app, Slot::Player(9), Button::Left, true)?;
    expect_slot(app, 0, "meadow", 64)?;
    slot(app, Slot::Player(0), Button::Left, true)?;
    let p = point(app, Target::Slot(Slot::Player(9)))?;
    app.palette.move_pointer(&app.client, p, VIEW);
    app.engine_key(KeyCode::Digit1)?;
    expect_slot(app, 0, "meadow", 64)?;
    record(&mut trace, app, "shift-and-hotbar-swap", 576)?;
    search(app, "oak log")?;
    require(
        app.palette
            .visible(&app.client.scene.engine.as_ref().unwrap().registry)
            .len()
            == 1,
        "Search did not filter catalog",
    )?;
    let log = app
        .client
        .scene
        .engine
        .as_ref()
        .unwrap()
        .registry
        .find("oak_log")?;
    click(app, Target::Catalog(log), Button::Left, true)?;
    expect_slot(app, 9, "oak_log", 64)?;
    record(&mut trace, app, "creative-log", 640)?;
    slot(app, Slot::Player(9), Button::Right, false)?;
    slot(app, Slot::Craft(0), Button::Right, false)?;
    slot(app, Slot::Craft(3), Button::Right, false)?;
    require(
        app.client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .recipes
            .find(
                &app.client.scene.engine.as_ref().unwrap().registry,
                &app.client.scene.engine.as_ref().unwrap().player,
            )
            .is_none(),
        "Extra input must prevent log recipe",
    )?;
    slot(app, Slot::Player(9), Button::Left, false)?;
    slot(app, Slot::Craft(3), Button::Left, true)?;
    expect_slot(app, 9, "oak_log", 63)?;
    expect_output(app, "oak_planks", 4)?;
    click(app, Target::Output, Button::Left, false)?;
    record(&mut trace, app, "craft-four-planks", 643)?;
    slot(app, Slot::Player(10), Button::Left, false)?;
    let before = observation(app);
    slot(app, Slot::Player(10), Button::Left, true)?;
    require(
        before == observation(app),
        "Full hotbar quick move must be atomic",
    )?;
    slot(app, Slot::Player(10), Button::Left, false)?;
    for i in 0..4 {
        slot(app, Slot::Craft(i), Button::Right, false)?;
    }
    expect_output(app, "crafting_table", 1)?;
    click(app, Target::Output, Button::Left, true)?;
    expect_slot(app, 10, "crafting_table", 1)?;
    record(&mut trace, app, "craft-table", 640)?;
    search(app, "sand")?;
    let sand = app
        .client
        .scene
        .engine
        .as_ref()
        .unwrap()
        .registry
        .find("sand")?;
    click(app, Target::Catalog(sand), Button::Left, true)?;
    expect_slot(app, 11, "sand", 64)?;
    record(&mut trace, app, "creative-sand", 704)?;
    slot(app, Slot::Player(11), Button::Right, false)?;
    for i in 0..4 {
        slot(app, Slot::Craft(i), Button::Right, false)?;
    }
    slot(app, Slot::Player(11), Button::Left, false)?;
    expect_output(app, "sandstone", 1)?;
    click(app, Target::Output, Button::Left, true)?;
    expect_slot(app, 11, "sand", 60)?;
    expect_slot(app, 12, "sandstone", 1)?;
    record(&mut trace, app, "craft-sandstone", 701)?;
    // Closing returns all four inputs, with no drop entities or lost items.
    slot(app, Slot::Player(11), Button::Right, false)?;
    slot(app, Slot::Craft(2), Button::Right, false)?;
    app.engine_key(KeyCode::Escape)?;
    require(
        !app.palette.open,
        "Inventory did not close after returning inputs",
    )?;
    expect_slot(app, 11, "sand", 60)?;
    record(&mut trace, app, "close-returns-inputs", 701)?;
    app.engine_key(KeyCode::KeyE)?;
    app.save_shortcut()?;
    app.finish_work()?;
    let first = app
        .client
        .last_save
        .clone()
        .ok_or("First inventory save missing")?;
    let first_bytes = fs::read(&first)?;
    slot(app, Slot::Player(9), Button::Left, false)?;
    slot(app, Slot::Craft(2), Button::Right, false)?;
    expect_output(app, "oak_planks", 4)?;
    app.save_shortcut()?;
    app.finish_work()?;
    let second = app
        .client
        .last_save
        .clone()
        .ok_or("Second inventory save missing")?;
    let second_bytes = fs::read(&second)?;
    let saved = observation(app);
    app.engine_key(KeyCode::Delete)?;
    require(
        app.client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .player
            .cursor
            .is_none(),
        "Explicit creative discard failed",
    )?;
    app.engine_key(KeyCode::F9)?;
    app.finish_work()?;
    require(saved == observation(app), "F9 lost cursor, inputs or slots")?;
    record(&mut trace, app, "reload-held-crafting-state", 701)?;
    require(
        fs::read(&first)? == first_bytes && fs::read(&second)? == second_bytes,
        "Earlier saves changed",
    )?;
    require(
        original == app.client.scene.engine.as_ref().unwrap().blocks,
        "Inventory changed terrain",
    )?;
    let saved_document: Value = serde_json::from_slice(&second_bytes)?;
    require(
        saved_document["version"] == 9
            && saved_document["recipes"]["recipes"]
                .as_array()
                .is_some_and(|r| r.len() == 7),
        "Version 7 recipe snapshot missing",
    )?;
    search(app, "")?;
    // Keep the cursor next to the active crafting grid in the native capture.
    let position = point(app, Target::Slot(Slot::Craft(2)))?;
    app.palette.move_pointer(&app.client, position, VIEW);
    app.notice = None;
    let report = json!({"complete":true,"checkpoint":engine::CHECKPOINT,"terrain_unchanged":true,"blocks":original.len(),"click_hitboxes_verified":true,"ordinary_moves_conserve_items":true,"close_returns_inputs":true,"blocked_moves_atomic":true,"save_reload_equal":true,"previous_saves_unchanged":true,"save_version":9,"saved_world":second,"saved_sha256":format!("{:x}",Sha256::digest(&second_bytes)),"recipe_count":7,"trace":trace,"final_inventory":observation(app),"scope":"Engine-owned 36 player slots, cursor, four inventory crafting inputs and seven shared recipe definitions; creative building remains unlimited. Station interactions and the native player preview are verified separately."});
    fs::write(
        out.join("inventory.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    app.frames = 0;
    println!("Engine inventory integration checks passed");
    Ok(())
}
