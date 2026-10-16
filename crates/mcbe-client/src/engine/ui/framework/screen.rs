//! Screen interface and model: JSON documents and Rust-built UIs use one path.
use super::{
    layout::{Align, Layout},
    widgets::{Kind, Widget},
};
use crate::scene::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
#[derive(Clone, Default)]
pub struct Model {
    pub numbers: BTreeMap<String, f64>,
    pub texts: BTreeMap<String, String>,
}
impl Model {
    pub fn number(&self, key: &str) -> f64 {
        self.numbers.get(key).copied().unwrap_or(0.)
    }
    pub fn text(&self, template: &str) -> String {
        let mut out = template.to_owned();
        for (key, value) in &self.numbers {
            out = out.replace(&format!("{{{key}}}"), &format!("{value:.0}"));
        }
        for (key, value) in &self.texts {
            out = out.replace(&format!("{{{key}}}"), value);
        }
        out.truncate(out.floor_char_boundary(512));
        out
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Command { name: String, args: Vec<String> },
    Close,
    Open { screen: String },
    Increment { key: String, amount: f64 },
    SetNumber { key: String, value: f64 },
    TravelDimension { dimension: String },
    SetText { key: String, value: String },
    Custom { id: String },
}
impl Action {
    pub fn validate(&self) -> Result<()> {
        if let Self::Command { name, args } = self {
            return crate::engine::commands::Invocation {
                name: name.clone(),
                args: args.clone(),
            }
            .validate();
        }
        let key = match self {
            Self::Command { .. } => unreachable!(),
            Self::Open { screen } => screen,
            Self::Increment { key, .. }
            | Self::SetNumber { key, .. }
            | Self::SetText { key, .. } => key,
            Self::TravelDimension { dimension } => dimension,
            Self::Custom { id } => id,
            Self::Close => return Ok(()),
        };
        if key.is_empty() || key.len() > 64 {
            return Err("UI action key invalid".into());
        }
        if matches!(self,Self::Increment{amount,..} if !amount.is_finite() || amount.abs()>1e6)
            || matches!(self,Self::SetNumber{value,..} if !value.is_finite() || value.abs()>1e6)
            || matches!(self,Self::TravelDimension{dimension} if !crate::engine::blocks::state::identifier(dimension))
            || matches!(self,Self::SetText{value,..} if value.len()>512)
        {
            return Err("UI action value invalid".into());
        }
        Ok(())
    }
}
pub enum Effect {
    Command(crate::engine::commands::Invocation),
    None,
    Close,
    Open(String),
    TravelDimension(String),
}
pub trait Screen: Send + Sync {
    fn build(&self, model: &Model) -> Document;
    fn action(&self, action: &Action, model: &mut Model) -> Result<Effect> {
        action.validate()?;
        match action {
            Action::Command { name, args } => {
                let invocation = crate::engine::commands::Invocation {
                    name: name.clone(),
                    args: args.iter().map(|s| model.text(s)).collect(),
                };
                invocation.validate()?;
                Ok(Effect::Command(invocation))
            }
            Action::TravelDimension { dimension } => Ok(Effect::TravelDimension(dimension.clone())),
            Action::SetNumber { key, value } => {
                if !model.numbers.contains_key(key)
                    && model.numbers.len() + model.texts.len() >= 128
                {
                    return Err("UI model exceeds128 values".into());
                }
                model.numbers.insert(key.clone(), *value);
                Ok(Effect::None)
            }
            Action::Close => Ok(Effect::Close),
            Action::Open { screen } => Ok(Effect::Open(screen.clone())),
            Action::Increment { key, amount } => {
                if !model.numbers.contains_key(key)
                    && model.numbers.len() + model.texts.len() >= 128
                {
                    return Err("UI model exceeds128 values".into());
                }
                let value = (model.number(key) + amount).clamp(-1e6, 1e6);
                model.numbers.insert(key.clone(), value);
                Ok(Effect::None)
            }
            Action::SetText { key, value } => {
                if !model.texts.contains_key(key) && model.numbers.len() + model.texts.len() >= 128
                {
                    return Err("UI model exceeds128 values".into());
                }
                model.texts.insert(key.clone(), value.clone());
                Ok(Effect::None)
            }
            Action::Custom { .. } => {
                Err("Custom UI actions require a Rust Screen::action implementation".into())
            }
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub version: u32,
    pub root: Widget,
}
impl Screen for Document {
    fn build(&self, _: &Model) -> Document {
        self.clone()
    }
}
impl Document {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err("Unsupported UI document version".into());
        }
        let mut names = HashSet::new();
        let mut pending = vec![(&self.root, 0)];
        while let Some((widget, depth)) = pending.pop() {
            if depth > 16 || names.len() >= 2048 || !names.insert(&widget.id) {
                return Err("UI widget depth/count/duplicate ID invalid".into());
            }
            widget.validate()?;
            pending.extend(widget.children.iter().map(|c| (c, depth + 1)));
        }
        Ok(())
    }
}
/// A concrete code-built screen: any subsystem can implement this same trait.
pub struct Laboratory;
impl Screen for Laboratory {
    fn build(&self, model: &Model) -> Document {
        let count = model.number("counter");
        let mut root = Widget::new("lab", [0., 0., 270., 176.], Kind::Panel).child(Widget::new(
            "footer",
            [14., 162., 242., 10.],
            Kind::Label {
                text: "{ui_controls}".into(),
            },
        ));
        root.layout = Layout {
            anchor: [Align::Center; 2],
            offset: [0.; 2],
            size: [270., 176.],
        };
        root.children.extend(vec![
            Widget::new(
                "heading",
                [14., 12., 242., 12.],
                Kind::Label {
                    text: "UI laboratory / Rust-built screen".into(),
                },
            )
            .styled("heading"),
            Widget::new(
                "description",
                [14., 32., 242., 24.],
                Kind::Label {
                    text: "Widgets, layout and input share one tree.".into(),
                },
            ),
            Widget::new(
                "counter",
                [14., 60., 242., 12.],
                Kind::Label {
                    text: format!("Button presses: {count:.0}"),
                },
            ),
            Widget::new(
                "progress",
                [14., 79., 242., 9.],
                Kind::Progress {
                    value: "counter".into(),
                    maximum: 10.,
                },
            )
            .styled("accent"),
            Widget::new(
                "increment",
                [14., 100., 116., 22.],
                Kind::Button {
                    text: "Increment".into(),
                    action: Action::Increment {
                        key: "counter".into(),
                        amount: 1.,
                    },
                },
            ),
            Widget::new(
                "reset",
                [140., 100., 116., 22.],
                Kind::Button {
                    text: "Reset".into(),
                    action: Action::Increment {
                        key: "counter".into(),
                        amount: -count,
                    },
                },
            ),
            Widget::new(
                "inventory",
                [14., 132., 116., 22.],
                Kind::Button {
                    text: "Inventory".into(),
                    action: Action::Open {
                        screen: "inventory".into(),
                    },
                },
            ),
            Widget::new(
                "close",
                [140., 132., 116., 22.],
                Kind::Button {
                    text: "Close".into(),
                    action: Action::Close,
                },
            ),
        ]);
        Document { version: 1, root }
    }
}
