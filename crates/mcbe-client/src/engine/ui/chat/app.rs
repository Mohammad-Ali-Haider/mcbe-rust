//! Connect text focus and registered command effects to the application services.
use super::Kind;
use crate::{App, scene::Result};
use winit::keyboard::KeyCode as K;
impl App {
    pub(crate) fn open_chat(&mut self, slash: bool) {
        self.chat.begin(slash);
        self.keys.clear();
        self.flight_tap.reset();
        self.controls.suspend();
        self.accumulator = 0.;
        self.capture(false);
        if let Some(w) = &self.window {
            w.set_ime_allowed(true);
        }
    }
    pub(crate) fn close_chat(&mut self) {
        self.chat.close();
        self.keys.clear();
        self.flight_tap.reset();
        if let Some(w) = &self.window {
            w.set_ime_allowed(false);
        }
        if !self.load_busy() && !self.menu.open() && !self.palette.open && !self.startup_blocked() {
            self.capture(true);
        }
    }
    pub(crate) fn chat_event(
        &mut self,
        code: K,
        pressed: bool,
        repeat: bool,
        text: Option<&str>,
    ) -> Result<bool> {
        if !self.chat.open {
            return Ok(false);
        }
        if !pressed {
            return Ok(true);
        }
        match code {
            K::Escape => {
                if !repeat {
                    self.close_chat();
                }
            }
            K::F11 => {
                if !repeat {
                    self.toggle_fullscreen();
                }
            }
            K::Enter | K::NumpadEnter => {
                if !repeat && !self.chat.composing {
                    self.submit_chat();
                }
            }
            K::Tab => self.chat.complete(&self.commands, &self.client),
            K::PageUp => self.chat.scroll(6),
            K::PageDown => self.chat.scroll(-6),
            K::ArrowLeft
            | K::ArrowRight
            | K::ArrowUp
            | K::ArrowDown
            | K::Home
            | K::End
            | K::Backspace
            | K::Delete => {
                if !self.chat.composing {
                    self.chat.key(code);
                }
            }
            _ => {
                if !self.modifiers.control_key()
                    && !self.modifiers.alt_key()
                    && !self.chat.composing
                    && let Some(text) = text
                {
                    self.chat.text(text);
                }
            }
        }
        Ok(true)
    }
    fn submit_chat(&mut self) {
        let line = self.chat.submit();
        if let Some(w) = &self.window {
            w.set_ime_allowed(false);
        }
        if let Some(line) = line {
            if line.starts_with('/') {
                self.chat.push(Kind::Command, format!("> {line}"));
                let result = (|| -> Result<()> {
                    if self.load_busy() || self.startup_blocked() || self.close_after_save {
                        return Err("Wait for world preparation before running commands".into());
                    }
                    let outcome = self.commands.execute(&mut self.client, &line)?;
                    if let Some(id) = outcome.travel {
                        self.request_dimension(&id)?;
                    }
                    self.chat.push(Kind::System, outcome.message);
                    Ok(())
                })();
                if let Err(error) = result {
                    self.chat.push(Kind::Error, format!("Error: {error}"));
                }
            } else {
                self.chat.push(Kind::Player, format!("<Player> {line}"));
            }
        }
        self.keys.clear();
        self.flight_tap.reset();
        if !self.load_busy() && !self.startup_blocked() && !self.menu.open() && !self.palette.open {
            self.capture(true);
        }
    }
    pub(crate) fn request_dimension(&mut self, id: &str) -> Result<()> {
        if self.startup_blocked() || self.load_busy() || self.close_after_save {
            return Err("Wait for world preparation before dimension travel".into());
        }
        self.ensure_loader()?;
        self.loader.as_mut().unwrap().dimension(&self.client, id)?;
        self.flight_tap.reset();
        self.world_accumulator = 0.;
        self.begin_loading("Preparing dimension... | Escape cancels");
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine;
    #[test]
    fn typing_consumes_movement_save_and_use_keys_and_only_enter_executes() {
        let mut app = App::new(
            engine::generate(37).unwrap(),
            std::env::temp_dir(),
            false,
            true,
        )
        .unwrap();
        let pose = app.client.actor.position;
        let before = app.client.scene.engine.as_ref().unwrap().player.clone();
        app.engine_key(K::KeyT).unwrap();
        assert!(app.chat.open && app.keys.is_empty());
        for ch in "/give glass 70".chars() {
            app.chat_event(K::KeyW, true, false, Some(&ch.to_string()))
                .unwrap();
        }
        assert_eq!(
            app.client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .player
                .changed_slots(&before),
            0
        );
        app.chat_event(K::Enter, true, false, Some("\r")).unwrap();
        assert!(!app.chat.open);
        let s = app.client.scene.engine.as_ref().unwrap();
        let glass = s.registry.find("glass").unwrap();
        assert_eq!(
            s.player
                .slots
                .iter()
                .flatten()
                .filter(|s| s.id == glass)
                .map(|s| usize::from(s.count))
                .sum::<usize>(),
            70
        );
        assert_eq!(app.client.actor.position, pose);
        assert!(app.keys.is_empty());
        app.engine_key(K::KeyT).unwrap();
        app.chat_event(K::ArrowUp, true, false, None).unwrap();
        assert_eq!(app.chat.input, "/give glass 70");
        app.chat_event(K::Escape, true, false, None).unwrap();
        assert!(!app.chat.open);
        assert_eq!(
            app.client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .player
                .slots
                .iter()
                .flatten()
                .filter(|s| s.id == glass)
                .map(|s| usize::from(s.count))
                .sum::<usize>(),
            70
        );
    }
}

#[cfg(test)]
mod authored_command_tests {
    use super::*;
    use crate::engine::ui::{
        framework::{screen::Document, widgets::Kind},
        inventory::Pointer,
    };
    #[test]
    fn authored_button_runs_registered_extension_with_bound_arguments_without_a_window() {
        let mut app = App::new(
            crate::engine::generate(37).unwrap(),
            std::env::temp_dir(),
            false,
            true,
        )
        .unwrap();
        app.commands
            .register(
                "store",
                crate::engine::commands::Command {
                    usage: "/store <count>",
                    arguments: 1..=1,
                    complete: |_| vec![],
                    run: |client, args| {
                        let state = client.scene.engine.as_mut().unwrap();
                        let count = args[0].parse::<u8>()?;
                        let stack = crate::engine_inventory::Stack {
                            id: state.registry.find("glass")?,
                            count,
                            data: None,
                        };
                        if state.player.insert_container(1, stack)?.is_some() {
                            return Err("Container full".into());
                        }
                        Ok(crate::engine::commands::Outcome::message("Stored".into()))
                    },
                },
            )
            .unwrap();
        let doc:Document=serde_json::from_value(serde_json::json!({"version":1,"root":{"id":"test_root","layout":{"size":[100,40]},"kind":"panel","children":[{"id":"store","layout":{"offset":[4,4],"size":[90,30]},"kind":"button","text":"Store","action":{"type":"command","name":"store","args":["{amount}"]}}]}})).unwrap();
        {
            let state = app.client.scene.engine.as_mut().unwrap();
            let profile = serde_json::from_slice(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../content/base/inventory.json"
            )))
            .unwrap();
            state.player.configure(profile, &state.registry).unwrap();
            let mut ui = crate::engine::ui::framework::Library::built_in();
            ui.register("test_store", std::sync::Arc::new(doc)).unwrap();
            state.ui = std::sync::Arc::new(ui);
        }
        app.palette.model.numbers.insert("amount".into(), 13.);
        app.palette.open_screen(&app.client, "test_store").unwrap();
        let resolved = app.palette.document(&app.client, [480., 270.]).unwrap();
        let r = resolved
            .widgets
            .iter()
            .find(|w| matches!(w.widget.kind, Kind::Button { .. }))
            .unwrap()
            .rect
            .0;
        app.palette
            .click(
                &mut app.client,
                Pointer {
                    position: [r[0] + r[2] / 2., r[1] + r[3] / 2.],
                    viewport: [480., 270.],
                    button: crate::engine_inventory::Button::Left,
                    shift: false,
                },
            )
            .unwrap();
        assert_eq!(app.palette.pending_commands.len(), 1);
        app.ui_commands().unwrap();
        assert_eq!(
            app.client.scene.engine.as_ref().unwrap().player.containers[1][0]
                .unwrap()
                .count,
            13
        );
        assert!(app.palette.pending_commands.is_empty());
        assert_eq!(app.notice.as_deref(), Some("Stored"));
    }
}
