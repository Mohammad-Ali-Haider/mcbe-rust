//! Deterministic raw XInput-compatible states through normal input routing and UI actions.
use crate::engine::{
    input::{
        gamepad::{Button as B, Device, Pad, Snapshot},
        hints,
    },
    ui::framework::widgets::Binding,
};
use crate::{
    App, engine,
    engine_inventory::{Inventory, Stack},
    engine_ui::Target,
    scene::Result,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
pub struct Live {
    case: String,
    out: PathBuf,
    done: bool,
}
fn require(ok: bool, message: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}
fn sample(app: &mut App, pad: Option<Pad>) -> Result<()> {
    let context = app.controller_context();
    let now = app.controls.now + Duration::from_millis(16);
    app.controls.injected = true;
    app.controls.injections += 1;
    let frame = app
        .controls
        .sample(pad.map(|pad| Snapshot { slot: 0, pad }), context, now);
    app.controller_apply(frame, context, 1. / 60.)
}
fn neutral(app: &mut App) -> Result<()> {
    sample(app, Some(Pad::default()))
}
fn tap(app: &mut App, b: B) -> Result<()> {
    sample(
        app,
        Some(Pad {
            buttons: b.bit(),
            ..Default::default()
        }),
    )?;
    neutral(app)
}
fn focus(app: &mut App, target: Target) -> Result<()> {
    let count = app.palette.control_points(&app.client)?.len();
    for _ in 0..=count {
        if app.palette.hover == Some(target) {
            return Ok(());
        }
        tap(app, B::RightTrigger)?;
    }
    Err(format!("Controller cannot reach {target:?}").into())
}
fn slot(app: &mut App, binding: Binding) -> Result<()> {
    let d = app.palette.document(&app.client, app.palette.viewport)?;
    let r = d
        .binding(binding)
        .ok_or("Controller test binding missing")?
        .0;
    let t = app
        .palette
        .hit(
            &app.client,
            [r[0] + r[2] / 2., r[1] + r[3] / 2.],
            app.palette.viewport,
        )
        .ok_or("Controller test slot hit missing")?;
    focus(app, t)
}
fn run(app: &mut App, case: &str) -> Result<()> {
    neutral(app)?;
    match case {
        "motion" => {
            let yaw = app.client.yaw;
            let pitch = app.client.pitch;
            sample(
                app,
                Some(Pad {
                    movement: [1., 1.],
                    look: [1., 1.],
                    buttons: B::LeftStick.bit(),
                    ..Default::default()
                }),
            )?;
            let input = app.movement_input();
            require(
                input.forward > 0. && input.side > 0. && input.sprint,
                "Movement/sprint mapping",
            )?;
            require(
                app.client.yaw < yaw && app.client.pitch > pitch,
                "Look mapping inverted",
            )?;
            app.client.tick_motion(input)?;
            neutral(app)?;
            tap(app, B::South)?;
            tap(app, B::South)?;
            require(
                app.client.movement.mode == crate::engine_player::Mode::Flying,
                "Double A did not toggle flight",
            )?;
            tap(app, B::Down)?;
            require(
                app.client.movement.mode == crate::engine_player::Mode::Noclip,
                "Noclip shortcut",
            )?;
            tap(app, B::Down)?;
            let selected = app.client.scene.engine.as_ref().unwrap().player.active;
            tap(app, B::RightShoulder)?;
            require(
                app.client.scene.engine.as_ref().unwrap().player.active != (selected),
                "Hotbar shoulder",
            )?;
            tap(app, B::Left)?;
            require(app.wireframe, "Wireframe shortcut")?;
            tap(app, B::Left)?;
            sample(
                app,
                Some(Pad {
                    buttons: B::East.bit(),
                    ..Default::default()
                }),
            )?;
            require(app.movement_input().sneak, "Descend/sneak mapping")?;
        }
        "inventory" => {
            app.client.scene.engine.as_mut().unwrap().player = Inventory::empty();
            tap(app, B::North)?;
            require(app.palette.open, "Inventory button")?;
            require(
                app.client
                    .scene
                    .engine
                    .as_ref()
                    .unwrap()
                    .player
                    .cursor
                    .is_none(),
                "Y leaked into quick move",
            )?;
            tap(app, B::South)?;
            require(
                app.client
                    .scene
                    .engine
                    .as_ref()
                    .unwrap()
                    .player
                    .cursor
                    .is_some(),
                "Catalog selection",
            )?;
            slot(app, Binding::Player(9))?;
            tap(app, B::South)?;
            require(
                app.client.scene.engine.as_ref().unwrap().player.slots[9].is_some(),
                "Slot deposit",
            )?;
            tap(app, B::West)?;
            require(
                app.client
                    .scene
                    .engine
                    .as_ref()
                    .unwrap()
                    .player
                    .cursor
                    .is_some(),
                "Split stack",
            )?;
            slot(app, Binding::Player(10))?;
            tap(app, B::South)?;
            tap(app, B::North)?;
            require(
                app.client.scene.engine.as_ref().unwrap().player.slots[10].is_none(),
                "Quick move",
            )?;
            slot(app, Binding::Craft(0))?;
            slot(app, Binding::Result)?;
            let old = app.palette.hover;
            let view = app.palette.viewport;
            app.palette
                .control_resize(&app.client, [view[0] + 80., view[1] + 40.])?;
            require(app.palette.hover == old, "Resize lost selected slot")?;
            app.palette.control_resize(&app.client, view)?;
        }
        "keyboard" => {
            tap(app, B::North)?;
            tap(app, B::View)?;
            require(app.controls.keyboard.open, "Search keyboard did not open")?;
            tap(app, B::South)?;
            tap(app, B::Right)?;
            tap(app, B::South)?;
            require(app.palette.filter == "ab", "Virtual keyboard characters")?;
            tap(app, B::West)?;
            require(app.palette.filter == "a", "Virtual keyboard erase")?;
            let view = app.palette.viewport;
            app.controls
                .keyboard
                .resize(&app.client, view, [view[0] + 60., view[1] + 40.])?;
            app.controls
                .keyboard
                .resize(&app.client, [view[0] + 60., view[1] + 40.], view)?;
            tap(app, B::South)?;
            require(app.palette.filter == "ab", "Keyboard resize lost key")?;
        }
        "menu" => {
            tap(app, B::Start)?;
            require(app.menu.open(), "Pause button")?;
            sample(
                app,
                Some(Pad {
                    buttons: B::South.bit(),
                    ..Default::default()
                }),
            )?;
            require(
                !app.menu.open() && !app.movement_input().jump,
                "Resume A leaked into jump",
            )?;
            neutral(app)?;
            tap(app, B::North)?;
            sample(
                app,
                Some(Pad {
                    buttons: B::East.bit(),
                    ..Default::default()
                }),
            )?;
            require(
                !app.palette.open && !app.movement_input().sneak,
                "Close B leaked into movement",
            )?;
            neutral(app)?;
            tap(app, B::Start)?;
            tap(app, B::Down)?;
            tap(app, B::South)?;
            require(
                app.menu.page == crate::engine_settings::Page::Settings,
                "Settings navigation",
            )?;
            let distance = app.settings.render_distance;
            tap(app, B::Right)?;
            require(
                app.settings.render_distance == distance + 1,
                "Controller setting adjustment",
            )?;
        }
        "lab" => {
            tap(app, B::View)?;
            let d = app.palette.document(&app.client, app.palette.viewport)?;
            let i = d
                .widgets
                .iter()
                .position(|w| w.widget.id == "increment")
                .ok_or("Increment missing")?;
            focus(app, Target::Button(i as u16))?;
            tap(app, B::South)?;
            require(
                app.palette.model.number("counter") == 1.,
                "Code-built button activation",
            )?;
        }
        "table" | "furnace" => {
            let pos = [0, 4, -5];
            let r = &app.client.scene.engine.as_ref().unwrap().registry;
            let station = r.find(if case == "table" {
                "crafting_table"
            } else {
                "furnace"
            })?;
            let sand = r.find("sand")?;
            let oak = r.find("oak_planks")?;
            let cobble = r.find("cobblestone")?;
            app.client.scene.change_engine(pos, Some(station))?;
            app.client.set_spawn(engine::SPAWN)?;
            app.client.yaw = 0.;
            app.client.pitch = -0.28;
            app.client.scene.engine.as_mut().unwrap().player = Inventory::empty();
            tap(app, B::LeftTrigger)?;
            require(app.palette.station == Some(pos), "LT raycast station open")?;
            if case == "table" {
                app.client.scene.engine.as_mut().unwrap().player.cursor = Some(Stack {
                    data: None,
                    id: cobble,
                    count: 8,
                });
                for i in [0, 1, 2, 3, 5, 6, 7, 8] {
                    slot(app, Binding::Table(i))?;
                    tap(app, B::West)?;
                }
                slot(app, Binding::TableResult)?;
                tap(app, B::North)?;
                require(
                    app.client
                        .scene
                        .engine
                        .as_ref()
                        .unwrap()
                        .player
                        .slots
                        .iter()
                        .flatten()
                        .any(|s| {
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
                    "Controller 3x3 crafting",
                )?;
            } else {
                {
                    let p = &mut app.client.scene.engine.as_mut().unwrap().player;
                    p.slots[0] = Some(Stack {
                        data: None,
                        id: sand,
                        count: 3,
                    });
                    p.slots[1] = Some(Stack {
                        data: None,
                        id: oak,
                        count: 2,
                    });
                }
                slot(app, Binding::Player(0))?;
                tap(app, B::North)?;
                slot(app, Binding::Player(1))?;
                tap(app, B::North)?;
                app.advance_machines(12.5);
                slot(app, Binding::Machine(2))?;
                tap(app, B::North)?;
                require(
                    app.client
                        .scene
                        .engine
                        .as_ref()
                        .unwrap()
                        .player
                        .slots
                        .iter()
                        .flatten()
                        .any(|s| {
                            s.id == app
                                .client
                                .scene
                                .engine
                                .as_ref()
                                .unwrap()
                                .registry
                                .find("glass")
                                .unwrap()
                        }),
                    "Controller smelting output",
                )?;
                app.advance_machines(10.);
            }
        }
        "disconnect" => {
            sample(
                app,
                Some(Pad {
                    movement: [0., 1.],
                    buttons: B::South.bit(),
                    ..Default::default()
                }),
            )?;
            sample(app, None)?;
            require(app.menu.open(), "Unplug did not pause")?;
            require(
                app.controls.router.device == Device::KeyboardMouse && !app.movement_input().jump,
                "Unplug left stuck input",
            )?;
        }
        "takeover" => {
            sample(
                app,
                Some(Pad {
                    movement: [0., 1.],
                    ..Default::default()
                }),
            )?;
            app.keyboard_activity();
            app.keys.insert(winit::keyboard::KeyCode::KeyA);
            sample(
                app,
                Some(Pad {
                    movement: [0., 1.],
                    ..Default::default()
                }),
            )?;
            require(
                app.movement_input().side < 0. && app.movement_input().forward == 0.,
                "Keyboard takeover",
            )?;
            app.controls.suspend();
            sample(
                app,
                Some(Pad {
                    buttons: B::South.bit(),
                    ..Default::default()
                }),
            )?;
            require(!app.movement_input().jump, "Focus restore accepted held A")?;
            neutral(app)?;
            tap(app, B::North)?;
            tap(app, B::View)?;
            app.keyboard_activity();
            require(
                !app.controls.keyboard.open,
                "Physical keyboard did not dismiss popup",
            )?;
            app.palette.text("oak");
            require(
                app.palette.filter == "oak",
                "Physical search after controller",
            )?;
        }
        _ => return Err("Unknown controller proof case".into()),
    }
    app.notice = None;
    Ok(())
}
pub fn prepare(app: &mut App, out: &Path, case: &str) -> Result<()> {
    require(!out.exists(), "Fresh controller proof directory required")?;
    std::fs::create_dir_all(out)?;
    app.controls.start()?;
    app.frame_limit = 1000;
    app.debug = false;
    app.controller_live = Some(Live {
        case: case.into(),
        out: out.into(),
        done: false,
    });
    Ok(())
}
pub fn advance(app: &mut App) -> Result<()> {
    let Some(mut live) = app.controller_live.take() else {
        return Ok(());
    };
    if !live.done && app.gpu.as_ref().is_some_and(|g| g.stats.meshes_ready) {
        run(app, &live.case)?;
        let (left, right) = hints::rows(
            &app.client,
            &app.palette,
            &app.menu,
            app.controls.keyboard.open,
            hints::Use::Place,
        );
        std::fs::write(
            live.out.join("controller.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"complete":true,"case":live.case,"input":app.controls.observation(),"keyboard_open":app.controls.keyboard.open,"keyboard_pointer":app.controls.keyboard.pointer,"filter":app.palette.filter,"left_hints":left,"right_hints":right,"scope":"Injected XInput-compatible raw states through ordinary router and application actions; real native rendering. Physical controller not connected during this proof."}),
            )?,
        )?;
        live.done = true;
        app.frame_limit = app.frames + 8;
    }
    app.controller_live = Some(live);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_states_drive_gameplay_shared_ui_stations_and_safe_transitions() {
        for case in [
            "motion",
            "inventory",
            "keyboard",
            "menu",
            "lab",
            "table",
            "furnace",
            "disconnect",
            "takeover",
        ] {
            let mut app = App::new(
                engine::generate(44).unwrap(),
                PathBuf::from("work"),
                false,
                true,
            )
            .unwrap();
            run(&mut app, case).unwrap_or_else(|e| panic!("{case}: {e}"));
        }
    }
}

#[cfg(test)]
mod input_boundary_tests {
    use super::*;
    #[test]
    fn flight_taps_do_not_cross_input_devices_and_grip_reserves_jump() {
        let mut app = App::new(
            engine::generate(44).unwrap(),
            PathBuf::from("work"),
            false,
            true,
        )
        .unwrap();
        neutral(&mut app).unwrap();
        let now = app.controls.event_time();
        app.jump_event(true, false, now).unwrap();
        app.jump_event(false, false, now + Duration::from_millis(1))
            .unwrap();
        tap(&mut app, B::South).unwrap();
        assert_eq!(
            app.client.movement.mode,
            crate::engine_player::Mode::Walking
        );
        app.keyboard_activity();
        let now = app.controls.event_time();
        app.jump_event(true, false, now).unwrap();
        app.jump_event(false, false, now + Duration::from_millis(1))
            .unwrap();
        assert_eq!(
            app.client.movement.mode,
            crate::engine_player::Mode::Walking
        );
        for _ in 0..2 {
            sample(
                &mut app,
                Some(Pad {
                    buttons: B::South.bit() | B::RightStick.bit(),
                    ..Default::default()
                }),
            )
            .unwrap();
            assert!(app.movement_input().grab && app.movement_input().jump);
            neutral(&mut app).unwrap();
        }
        assert_eq!(
            app.client.movement.mode,
            crate::engine_player::Mode::Walking
        );
    }
}
