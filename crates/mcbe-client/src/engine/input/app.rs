//! Connect semantic controller input to existing gameplay and UI transactions.
use super::gamepad::{Button as B, Context, Device, Direction, Frame};
use crate::{App, engine, engine_inventory, scene::Result};
use winit::keyboard::KeyCode as K;
impl App {
    pub(crate) fn keyboard_activity(&mut self) {
        if self.controls.router.device == Device::Gamepad {
            self.flight_tap.reset();
        }
        self.controls.keyboard_activity();
        let text = "Tab / arrows navigate, Enter selects";
        if self
            .palette
            .model
            .texts
            .get("ui_controls")
            .is_none_or(|s| s != text)
        {
            self.palette
                .model
                .texts
                .insert("ui_controls".into(), text.into());
            self.palette.revision += 1;
        }
        if !self.smoke
            && (!self.captured || self.menu.open() || self.palette.open)
            && let Some(w) = &self.window
        {
            w.set_cursor_visible(true);
        }
    }
    fn controller_hide_pointer(&self) {
        if !self.smoke
            && let Some(w) = &self.window
        {
            w.set_cursor_visible(false);
        }
    }

    pub(crate) fn controller_context(&self) -> Context {
        if self.client.scene.engine.is_none()
            || self.startup_blocked()
            || self.close_after_save
            || (!self.smoke && self.window.as_ref().is_none_or(|w| !w.has_focus()))
        {
            Context::Blocked
        } else if self.load_busy() || self.chat.open {
            Context::Busy
        } else if self.controls.keyboard.open {
            Context::Keyboard
        } else if self.menu.open() {
            Context::Menu
        } else if self.palette.open {
            Context::Panel
        } else {
            Context::Gameplay
        }
    }
    pub(crate) fn controller_poll(&mut self, dt: f32) -> Result<()> {
        if self.smoke {
            return Ok(());
        }
        let context = self.controller_context();
        let frame = self.controls.poll(context);
        self.controller_apply(frame, context, dt)
    }
    pub(crate) fn controller_focus_panel(&mut self) -> Result<()> {
        let (_, v) = self.inventory_pointer();
        self.palette.viewport = v;
        self.palette.control_focus(&self.client)?;
        self.capture(false);
        self.controller_hide_pointer();
        Ok(())
    }
    pub(crate) fn world_use(&mut self, shift: bool) -> Result<()> {
        self.world_use_hand(shift, false)
    }
    pub(crate) fn world_use_hand(&mut self, shift: bool, force_offhand: bool) -> Result<()> {
        if self.client.view.is_free() {
            return Ok(());
        }
        let hand = self
            .client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .player
            .use_hand(force_offhand);
        if !force_offhand && !shift && engine::entities::interaction::use_target(&mut self.client)?
        {
            return Ok(());
        }
        if engine::items::bucket::selected_hand(&self.client, hand) {
            engine::items::bucket::use_hand(&mut self.client, shift, hand)?;
        } else if !force_offhand && !shift && self.palette.open_target(&mut self.client)? {
            self.keys.clear();
            self.capture(false);
            self.accumulator = 0.;
            if self.controls.router.device == Device::Gamepad {
                self.controller_focus_panel()?;
            }
        } else {
            self.client.edit_hand(true, hand)?;
        }
        Ok(())
    }
    pub(crate) fn controller_apply(
        &mut self,
        frame: Frame,
        context: Context,
        dt: f32,
    ) -> Result<()> {
        if self.controls.switched {
            self.flight_tap.reset();
        }
        if frame.disconnected {
            self.flight_tap.reset();
            if !self.palette.open && !self.menu.open() && context == Context::Gameplay {
                self.engine_key(K::Escape)?;
            }
            return Ok(());
        }
        if self.controls.router.device != Device::Gamepad {
            return Ok(());
        }
        if context == Context::Blocked {
            return Ok(());
        }
        if frame.activity {
            self.keys.clear();
            let text = "D-pad / LS navigate, A selects, B closes";
            if self
                .palette
                .model
                .texts
                .get("ui_controls")
                .is_none_or(|s| s != text)
            {
                self.palette
                    .model
                    .texts
                    .insert("ui_controls".into(), text.into());
                self.palette.revision += 1;
            }
            if context == Context::Gameplay && !self.smoke && !self.captured {
                self.capture(true);
            }
            if let Some(w) = &self.window {
                w.set_cursor_visible(false);
            }
        }
        if context == Context::Busy {
            if frame.pressed(B::East) || frame.pressed(B::Start) {
                self.engine_key(K::Escape)?;
            }
            return Ok(());
        }
        if context == Context::Keyboard {
            let view = self.palette.viewport;
            if let Some(d) = frame.navigation {
                self.controls
                    .keyboard
                    .navigate(&self.client, view, d.vector())?;
            }
            let effect = if frame.pressed(B::East) || frame.pressed(B::Start) {
                crate::engine::ui::controller_keyboard::Effect::Done
            } else if frame.pressed(B::West) {
                crate::engine::ui::controller_keyboard::Effect::Erase
            } else if frame.pressed(B::South) {
                self.controls.keyboard.activate(&self.client, view)?
            } else {
                crate::engine::ui::controller_keyboard::Effect::None
            };
            match effect {
                crate::engine::ui::controller_keyboard::Effect::Character(ch) => {
                    self.palette.text(&ch.to_string())
                }
                crate::engine::ui::controller_keyboard::Effect::Erase => {
                    self.palette.key(&mut self.client, K::Backspace)?
                }
                crate::engine::ui::controller_keyboard::Effect::Done => {
                    self.controls.keyboard.close();
                    self.palette.search = false;
                    self.palette.revision += 1;
                }
                _ => {}
            }
            return Ok(());
        }
        if context == Context::Menu {
            if frame.pressed(B::Start) || frame.pressed(B::East) {
                self.engine_key(K::Escape)?;
                self.controller_hide_pointer();
                return Ok(());
            }
            if let Some(d) = frame.navigation {
                self.engine_key(match d {
                    Direction::Up => K::ArrowUp,
                    Direction::Down => K::ArrowDown,
                    Direction::Left => K::ArrowLeft,
                    Direction::Right => K::ArrowRight,
                })?;
            }
            if frame.pressed(B::South) {
                self.engine_key(K::Enter)?;
            }
            self.controller_hide_pointer();
            return Ok(());
        }
        if context == Context::Panel {
            if frame.pressed(B::Start) || frame.pressed(B::East) {
                self.palette.key(&mut self.client, K::Escape)?;
                self.controls.keyboard.close();
                if !self.palette.open && !self.load_busy() {
                    self.capture(true);
                }
                return Ok(());
            }
            if let Some(d) = frame.navigation {
                self.palette.control_navigate(&self.client, d.vector())?;
            }
            let focus = i32::from(frame.pressed(B::RightTrigger))
                - i32::from(frame.pressed(B::LeftTrigger));
            if focus != 0 {
                self.palette.control_cycle(&self.client, focus)?;
            }
            if frame.pressed(B::LeftShoulder) {
                self.palette.control_page(&self.client, -1)?;
            }
            if frame.pressed(B::RightShoulder) {
                self.palette.control_page(&self.client, 1)?;
            }
            if frame.pressed(B::View)
                && self
                    .palette
                    .document(&self.client, self.palette.viewport)?
                    .widgets
                    .iter()
                    .any(|w| {
                        matches!(
                            w.widget.kind,
                            crate::engine::ui::framework::widgets::Kind::Search
                        )
                    })
            {
                self.palette.search = true;
                self.palette.revision += 1;
            }
            if frame.pressed(B::South) {
                self.palette.control_activate(
                    &mut self.client,
                    engine_inventory::Button::Left,
                    false,
                )?;
            }
            if frame.pressed(B::West) {
                self.palette.control_activate(
                    &mut self.client,
                    engine_inventory::Button::Right,
                    false,
                )?;
            }
            if frame.pressed(B::North) {
                self.palette.control_activate(
                    &mut self.client,
                    engine_inventory::Button::Left,
                    true,
                )?;
            }
            self.ui_commands()?;
            if self.palette.search {
                self.controls
                    .keyboard
                    .show(&self.client, self.palette.viewport)?;
            }
            if !self.palette.open && !self.load_busy() {
                self.capture(true);
            }
            return Ok(());
        }
        if frame.pressed(B::Start) {
            self.engine_key(K::Escape)?;
            self.controller_hide_pointer();
            return Ok(());
        }
        if frame.pressed(B::North) {
            self.engine_key(K::KeyE)?;
            self.controller_focus_panel()?;
            return Ok(());
        }
        if frame.pressed(B::View) {
            self.engine_key(if frame.down(B::RightStick) {
                K::KeyU
            } else {
                K::F12
            })?;
            self.controller_focus_panel()?;
            return Ok(());
        }
        if frame.pressed(B::West) {
            self.engine_key(if frame.down(B::RightStick) {
                K::KeyV
            } else {
                K::F5
            })?;
            return Ok(());
        }
        if frame.pressed(B::Up) && !frame.down(B::Down) {
            self.engine_key(K::KeyF)?;
        }
        if frame.pressed(B::Down) && !frame.down(B::Up) {
            self.engine_key(K::KeyN)?;
        }
        if frame.pressed(B::Left) && !frame.down(B::Right) {
            self.engine_key(K::F4)?;
        }
        if frame.pressed(B::Right) && !frame.down(B::Left) {
            self.engine_key(K::F3)?;
        }
        let delta =
            i32::from(frame.pressed(B::RightShoulder)) - i32::from(frame.pressed(B::LeftShoulder));
        if delta != 0 {
            self.client
                .scene
                .engine
                .as_mut()
                .unwrap()
                .player
                .cycle(delta);
        }
        if frame.pressed(B::South) {
            let time = self.controls.event_time();
            self.jump_event(true, false, time)?;
        }
        if frame.released & B::South.bit() != 0 {
            let time = self.controls.event_time();
            self.jump_event(false, false, time)?;
        }
        if self.controls.router.device == Device::Gamepad {
            let speed = self.settings.sensitivity as f32 / 100.;
            self.client.look(
                frame.look[0] * dt.clamp(0., 0.1) * 2.6 * speed,
                -frame.look[1] * dt.clamp(0., 0.1) * 2.2 * speed,
            );
        }
        if frame.pressed(B::RightTrigger) {
            self.client.edit(false)?;
        }
        if frame.pressed(B::LeftTrigger) {
            self.world_use_hand(frame.down(B::East), frame.down(B::RightStick))?;
        }
        Ok(())
    }
    pub(crate) fn controller_motion(&self) -> crate::scene::Input {
        if self.controls.context != Context::Gameplay
            || self.controller_context() != Context::Gameplay
        {
            return Default::default();
        }
        let frame = self.controls.frame;
        crate::scene::Input {
            forward: frame.movement[1],
            side: frame.movement[0],
            jump: frame.down(B::South),
            sneak: frame.down(B::East),
            sprint: self.controls.sprint,
            grab: frame.down(B::RightStick),
        }
    }
}
