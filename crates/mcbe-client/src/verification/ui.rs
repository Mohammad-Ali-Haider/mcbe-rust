//! Native checks through real shared widgets and screen actions.
use crate::{
    App,
    engine_inventory::Button,
    engine_ui::{Pointer, Target},
    scene::Result,
};
use std::path::Path;
use winit::keyboard::KeyCode;
pub fn prepare(app: &mut App, out: &Path, lab: bool) -> Result<()> {
    if out.exists() {
        return Err("Fresh UI proof directory required".into());
    }
    std::fs::create_dir_all(out)?;
    if lab {
        app.engine_key(KeyCode::F12)?;
        let document = app.palette.document(&app.client, [480., 270.])?;
        let index = document
            .widgets
            .iter()
            .position(|w| w.widget.id == "increment")
            .ok_or("Code-built increment button missing")?;
        let r = document.widgets[index].rect.0;
        for _ in 0..3 {
            app.palette.click(
                &mut app.client,
                Pointer {
                    position: [r[0] + r[2] / 2., r[1] + r[3] / 2.],
                    viewport: [480., 270.],
                    button: Button::Left,
                    shift: false,
                },
            )?;
            if app.palette.hover != Some(Target::Button(index as u16)) {
                return Err("Shared button hit geometry mismatch".into());
            }
        }
        app.palette.key(&mut app.client, KeyCode::Enter)?;
        if app.palette.model.number("counter") != 4. {
            return Err("Mouse/keyboard code-screen actions failed".into());
        }
    } else {
        app.engine_key(KeyCode::KeyE)?;
        let document = app.palette.document(&app.client, [480., 270.])?;
        for i in 0..36 {
            let r = document
                .binding(crate::engine::ui::framework::widgets::Binding::Player(i))
                .ok_or("Player UI slot missing")?
                .0;
            if app.palette.hit(
                &app.client,
                [r[0] + r[2] / 2., r[1] + r[3] / 2.],
                [480., 270.],
            ) != Some(Target::Slot(crate::engine_inventory::Slot::Player(i)))
            {
                return Err("Shared inventory hit geometry mismatch".into());
            }
        }
        for i in 0..4 {
            let r = document
                .binding(crate::engine::ui::framework::widgets::Binding::Craft(i))
                .ok_or("Craft UI slot missing")?
                .0;
            if app.palette.hit(
                &app.client,
                [r[0] + r[2] / 2., r[1] + r[3] / 2.],
                [480., 270.],
            ) != Some(Target::Slot(crate::engine_inventory::Slot::Craft(i)))
            {
                return Err("Crafting slot hit geometry mismatch".into());
            }
        }
    }
    app.notice = None;
    app.frame_limit = 120;
    Ok(())
}
