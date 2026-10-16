//! Controller-only search keyboard built from the ordinary screen/widget model.
use super::framework::{
    layout::{Align, Hit, Layout},
    screen::{Action, Document, Model, Screen},
    widgets::{Kind, Widget},
};
use crate::scene::{Client, Result};
const KEYS: &str = "abcdefghijklmnopqrstuvwxyz0123456789 _-";
pub struct KeyboardScreen;
impl Screen for KeyboardScreen {
    fn build(&self, _model: &Model) -> Document {
        let mut panel = Widget::new("keyboard", [0., 0., 252., 158.], Kind::Panel);
        panel.layout = Layout {
            anchor: [Align::Center; 2],
            ..panel.layout
        };
        panel = panel
            .child(Widget::new(
                "title",
                [12., 10., 228., 10.],
                Kind::Label {
                    text: "Search blocks".into(),
                },
            ))
            .child(Widget::new(
                "term",
                [12., 25., 228., 10.],
                Kind::Label {
                    text: "{search_term}".into(),
                },
            ));
        for (i, ch) in KEYS.chars().enumerate() {
            panel = panel.child(Widget::new(
                format!("key_{i}"),
                [
                    12. + (i % 10) as f32 * 23.,
                    43. + (i / 10) as f32 * 22.,
                    20.,
                    19.,
                ],
                Kind::Button {
                    text: if ch == ' ' {
                        "_".into()
                    } else {
                        ch.to_string()
                    },
                    action: Action::Custom {
                        id: format!("character_{i}"),
                    },
                },
            ));
        }
        panel = panel
            .child(Widget::new(
                "erase",
                [12., 134., 106., 18.],
                Kind::Button {
                    text: "Backspace".into(),
                    action: Action::Custom { id: "erase".into() },
                },
            ))
            .child(Widget::new(
                "done",
                [130., 134., 110., 18.],
                Kind::Button {
                    text: "Done".into(),
                    action: Action::Custom { id: "done".into() },
                },
            ));
        Document {
            version: 1,
            root: panel,
        }
    }
}
#[derive(Default)]
pub struct Keyboard {
    pub open: bool,
    pub pointer: [f32; 2],
    pub revision: u64,
}
pub enum Effect {
    Character(char),
    Erase,
    Done,
    None,
}
impl Keyboard {
    pub fn show(&mut self, client: &Client, view: [f32; 2]) -> Result<()> {
        self.open = true;
        let d = client.scene.engine.as_ref().unwrap().ui.resolve(
            "controller_keyboard",
            &Model::default(),
            view,
        )?;
        self.pointer = d.focus_targets().first().copied().unwrap_or([0.; 2]);
        self.revision += 1;
        Ok(())
    }
    pub fn resize(&mut self, client: &Client, old: [f32; 2], new: [f32; 2]) -> Result<()> {
        let ui = &client.scene.engine.as_ref().unwrap().ui;
        let old = ui.resolve("controller_keyboard", &Model::default(), old)?;
        let id = match old.hit(self.pointer) {
            Some(Hit::Button(i)) => Some(old.widgets[i].widget.id.clone()),
            _ => None,
        };
        let new = ui.resolve("controller_keyboard", &Model::default(), new)?;
        self.pointer = new.focus_targets().into_iter().find(|p| matches!(new.hit(*p), Some(Hit::Button(i)) if Some(&new.widgets[i].widget.id) == id.as_ref())).or_else(||new.focus_targets().first().copied()).unwrap_or([0.;2]);
        self.revision += 1;
        Ok(())
    }
    pub fn close(&mut self) {
        if self.open {
            self.open = false;
            self.revision += 1;
        }
    }
    pub fn navigate(&mut self, client: &Client, view: [f32; 2], direction: [f32; 2]) -> Result<()> {
        let d = client.scene.engine.as_ref().unwrap().ui.resolve(
            "controller_keyboard",
            &Model::default(),
            view,
        )?;
        if let Some(p) = d.neighbor(self.pointer, direction) {
            self.pointer = p;
            self.revision += 1;
        }
        Ok(())
    }
    pub fn activate(&self, client: &Client, view: [f32; 2]) -> Result<Effect> {
        let d = client.scene.engine.as_ref().unwrap().ui.resolve(
            "controller_keyboard",
            &Model::default(),
            view,
        )?;
        let Some(Hit::Button(index)) = d.hit(self.pointer) else {
            return Ok(Effect::None);
        };
        if let Kind::Button {
            action: Action::Custom { id },
            ..
        } = &d.widgets[index].widget.kind
        {
            if id == "erase" {
                return Ok(Effect::Erase);
            }
            if id == "done" {
                return Ok(Effect::Done);
            }
            if let Some(i) = id
                .strip_prefix("character_")
                .and_then(|s| s.parse::<usize>().ok())
                && let Some(ch) = KEYS.chars().nth(i)
            {
                return Ok(Effect::Character(ch));
            }
        }
        Ok(Effect::None)
    }
}
