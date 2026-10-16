//! Native station interactions, crafting, smelting and saved processing state.
use crate::engine::ui::framework::widgets::Binding;
use crate::{
    App, engine,
    engine_inventory::{Button, Stack},
    engine_ui::{Pointer, Target},
    scene::Result,
};
use std::path::Path;
use winit::keyboard::KeyCode;
fn require(ok: bool, message: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}
fn click(app: &mut App, binding: Binding, shift: bool) -> Result<()> {
    let document = app.palette.document(&app.client, [480., 270.])?;
    let r = document
        .binding(binding)
        .ok_or("Station binding missing")?
        .0;
    app.palette.click(
        &mut app.client,
        Pointer {
            position: [r[0] + r[2] / 2., r[1] + r[3] / 2.],
            viewport: [480., 270.],
            button: Button::Left,
            shift,
        },
    )
}
pub fn prepare(app: &mut App, out: &Path, furnace: bool) -> Result<()> {
    if out.exists() {
        return Err("Fresh station verification directory required".into());
    }
    std::fs::create_dir_all(out)?;
    let p = [0, 4, -5];
    let name = if furnace { "furnace" } else { "crafting_table" };
    let r = &app.client.scene.engine.as_ref().unwrap().registry;
    let id = r.find(name)?;
    let sand = r.find("sand")?;
    let oak = r.find("oak_planks")?;
    let cobble = r.find("cobblestone")?;
    app.client.scene.change_engine(p, Some(id))?;
    app.client.set_spawn(engine::SPAWN)?;
    app.client.yaw = 0.;
    app.client.pitch = -0.28;
    require(
        app.palette.open_target(&mut app.client)?,
        "Raycast did not open placed station",
    )?;
    require(app.palette.station == Some(p), "Wrong station opened")?;
    app.client.scene.engine.as_mut().unwrap().player = crate::engine_inventory::Inventory::empty();
    if furnace {
        {
            let s = app.client.scene.engine.as_mut().unwrap();
            s.player.slots[0] = Some(Stack {
                data: None,
                id: sand,
                count: 3,
            });
            s.player.slots[1] = Some(Stack {
                data: None,
                id: oak,
                count: 2,
            });
        }
        click(app, Binding::Player(0), true)?;
        click(app, Binding::Player(1), true)?;
        let edit_revision = app.client.scene.engine.as_ref().unwrap().edit_revision;
        app.advance_machines(12.5);
        let bytes = serde_json::to_vec(&engine::SaveSnapshot::capture(&app.client)?.document()?)?;
        std::fs::write(out.join("processing.blockworld"), bytes)?;
        let copy = engine::load(&out.join("processing.blockworld"))?;
        let f = &copy.scene.engine.as_ref().unwrap().machines.furnaces[&p];
        require(
            f.cook_ms == 2500 && f.slots[2].is_some_and(|s| s.count == 1),
            "Furnace processing save failed",
        )?;
        click(app, Binding::Machine(2), true)?;
        require(
            app.client.scene.engine.as_ref().unwrap().player.slots[0].is_some_and(|s| s.count == 1),
            "Output shift withdrawal failed",
        )?;
        // Leave a second finished item and quarter-progress third item visible.
        app.advance_machines(10.);
        require(
            app.client.scene.engine.as_ref().unwrap().edit_revision == edit_revision,
            "Furnace timer dirtied terrain meshes",
        )?;
        let s = app.client.scene.engine.as_ref().unwrap();
        let f = &s.machines.furnaces[&p];
        require(
            f.slots[2].is_some_and(|s| s.count == 1) && f.cook_ms == 2500,
            "Furnace did not resume with UI open",
        )?;
        std::fs::write(
            out.join("stations.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"complete":true,"station":name,"raycast_open":true,"shift_input_fuel":true,"output_withdrawal":true,"saved_processing":true,"runtime_ui_open_updates":true,"timer_preserves_terrain_revision":true,"cook_progress":f.cook_fraction(&s.smelting),"burn_progress":f.burn_fraction(),"input_count":f.slots[0].unwrap().count,"output_count":f.slots[2].unwrap().count,"terrain_edit_revision":s.edit_revision}),
            )?,
        )?;
    } else {
        // Use cursor deposits through all eight ring slots, then take an actual 3x3 recipe.
        app.client.scene.engine.as_mut().unwrap().player.cursor = Some(Stack {
            data: None,
            id: cobble,
            count: 8,
        });
        for i in [0, 1, 2, 3, 5, 6, 7, 8] {
            let document = app.palette.document(&app.client, [480., 270.])?;
            let r = document.binding(Binding::Table(i)).unwrap().0;
            app.palette.click(
                &mut app.client,
                Pointer {
                    position: [r[0] + 9., r[1] + 9.],
                    viewport: [480., 270.],
                    button: Button::Right,
                    shift: false,
                },
            )?;
            require(
                app.palette.hover == Some(Target::Slot(crate::engine_inventory::Slot::Table(i))),
                "3x3 slot hit mismatch",
            )?;
        }
        require(
            app.palette
                .crafting_result(&app.client, true)
                .is_some_and(|m| {
                    m.stack.id
                        == app
                            .client
                            .scene
                            .engine
                            .as_ref()
                            .unwrap()
                            .registry
                            .find("furnace")
                            .unwrap()
                }),
            "3x3 furnace recipe missing",
        )?;
        click(app, Binding::TableResult, true)?;
        require(
            app.client.scene.engine.as_ref().unwrap().player.slots[0].is_some_and(|s| {
                s.id == app
                    .client
                    .scene
                    .engine
                    .as_ref()
                    .unwrap()
                    .registry
                    .find("furnace")
                    .unwrap()
            }),
            "Table output was not taken",
        )?;
        app.client.scene.engine.as_mut().unwrap().player.table[8] = Some(Stack {
            data: None,
            id: oak,
            count: 3,
        });
        app.palette.key(&mut app.client, KeyCode::Escape)?;
        require(
            app.client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .player
                .table
                .iter()
                .all(Option::is_none),
            "Closing did not return table inputs",
        )?;
        require(
            app.palette.open_target(&mut app.client)?,
            "Table could not reopen",
        )?;
        // Leave a ready mirrored stair recipe visible for the native image check.
        let s = app.client.scene.engine.as_mut().unwrap();
        for i in [2, 4, 5, 6, 7, 8] {
            s.player.table[i] = Some(Stack {
                data: None,
                id: oak,
                count: 1,
            });
        }
        std::fs::write(
            out.join("stations.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"complete":true,"station":name,"raycast_open":true,"three_by_three_slot_hits":true,"furnace_recipe_taken":true,"close_returns_inputs":true,"mirrored_stair_preview":true}),
            )?,
        )?;
    }
    app.palette
        .move_pointer(&app.client, [0., 0.], [480., 270.]);
    app.notice = None;
    app.frame_limit = 120;
    Ok(())
}
