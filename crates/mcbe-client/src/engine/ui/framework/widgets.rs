//! Typed widgets shared by JSON authoring and Rust screen builders.
use super::{layout::Layout, screen::Action};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", content = "index", rename_all = "snake_case")]
pub enum Binding {
    Offhand,
    Container { bank: usize, index: usize },
    Player(usize),
    Craft(usize),
    Table(usize),
    Machine(usize),
    Result,
    TableResult,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Kind {
    SlotGrid {
        binding: Binding,
        columns: Value,
        rows: Value,
        count: Value,
        #[serde(default)]
        offset: Value,
        #[serde(default)]
        page: Option<String>,
    },
    Panel,
    Label {
        text: String,
    },
    Slot {
        binding: Binding,
    },
    Image {
        sprite: String,
    },
    Preview,
    Search,
    Catalog {
        columns: usize,
        rows: usize,
    },
    Progress {
        value: String,
        maximum: f64,
    },
    Button {
        text: String,
        action: Action,
    },
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Widget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<Value>,
    pub id: String,
    pub layout: Layout,
    #[serde(default)]
    pub style: String,
    #[serde(flatten)]
    pub kind: Kind,
    #[serde(default)]
    pub children: Vec<Widget>,
}
impl Widget {
    pub fn new(id: impl Into<String>, rect: [f32; 4], kind: Kind) -> Self {
        Self {
            visible: None,
            id: id.into(),
            layout: Layout::at(rect),
            style: String::new(),
            kind,
            children: Vec::new(),
        }
    }
    pub fn styled(mut self, style: &str) -> Self {
        self.style = style.into();
        self
    }
    pub fn child(mut self, widget: Self) -> Self {
        self.children.push(widget);
        self
    }
    pub fn validate(&self) -> crate::scene::Result<()> {
        self.layout.validate()?;
        if let Some(v) = &self.visible {
            v.validate()?;
        }
        if self.id.is_empty() || self.id.len() > 64 || self.style.len() > 64 {
            return Err("UI widget ID/style invalid".into());
        }
        match &self.kind {
            Kind::Slot {
                binding: Binding::Container { bank, index },
            } if *bank >= 16 || *index >= 256 => {
                return Err("Container slot exceeds storage bounds".into());
            }
            Kind::SlotGrid {
                binding,
                columns,
                rows,
                count,
                offset,
                page,
            } => {
                binding.advanced(0)?;
                columns.validate()?;
                rows.validate()?;
                count.validate()?;
                offset.validate()?;
                if !matches!(
                    binding,
                    Binding::Player(_)
                        | Binding::Craft(_)
                        | Binding::Table(_)
                        | Binding::Container { .. }
                ) || page
                    .as_ref()
                    .is_some_and(|id| !crate::engine::blocks::state::identifier(id))
                {
                    return Err("Slot grid definition invalid".into());
                }
            }
            Kind::Slot {
                binding: Binding::Player(i),
            } if *i >= 256 => return Err("Player slot outside inventory".into()),
            Kind::Slot {
                binding: Binding::Craft(i),
            } if *i >= 64 => return Err("Craft slot outside grid".into()),
            Kind::Slot {
                binding: Binding::Machine(i),
            } if *i >= 3 => return Err("Machine slot outside furnace".into()),
            Kind::Slot {
                binding: Binding::Table(i),
            } if *i >= 64 => return Err("Table slot outside grid".into()),
            Kind::Label { text } | Kind::Button { text, .. } if text.len() > 512 => {
                return Err("UI label exceeds512 bytes".into());
            }
            Kind::Catalog { columns, rows }
                if *columns == 0 || *rows == 0 || *columns > 16 || *rows > 16 =>
            {
                return Err("UI catalog grid invalid".into());
            }
            Kind::Progress { maximum, value }
                if !maximum.is_finite() || *maximum <= 0. || value.len() > 64 =>
            {
                return Err("UI progress binding invalid".into());
            }
            Kind::Button { action, .. } => action.validate()?,
            _ => {}
        }
        Ok(())
    }
}

/// Small bound quantities let JSON layout follow a registered inventory profile.
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Constant(usize),
    Binding(String),
}
impl Default for Value {
    fn default() -> Self {
        Self::Constant(0)
    }
}
impl Value {
    pub fn validate(&self) -> crate::scene::Result<()> {
        match self {
            Self::Constant(v) if *v > 1024 => Err("UI quantity exceeds 1024".into()),
            Self::Binding(id) if !crate::engine::blocks::state::identifier(id) => {
                Err("Invalid UI quantity binding".into())
            }
            _ => Ok(()),
        }
    }
    pub fn read(&self, model: &super::screen::Model) -> crate::scene::Result<usize> {
        self.validate()?;
        let v = match self {
            Self::Constant(v) => *v as f64,
            Self::Binding(id) => model.number(id),
        };
        if !v.is_finite() || !(0. ..=1024.).contains(&v) || v.fract() != 0. {
            return Err("UI quantity binding must be a bounded integer".into());
        }
        Ok(v as usize)
    }
}
impl Binding {
    pub fn advanced(self, offset: usize) -> crate::scene::Result<Self> {
        let add = |i: usize| i.checked_add(offset).ok_or("UI slot index overflow");
        let binding = match self {
            Self::Player(i) => Self::Player(add(i)?),
            Self::Craft(i) => Self::Craft(add(i)?),
            Self::Table(i) => Self::Table(add(i)?),
            Self::Container { bank, index } => Self::Container {
                bank,
                index: add(index)?,
            },
            _ => return Err("Binding is not a general slot range".into()),
        };
        let valid = match binding {
            Self::Player(i) => i < 256,
            Self::Craft(i) | Self::Table(i) => i < 64,
            Self::Container { bank, index } => bank < 16 && index < 256,
            _ => false,
        };
        if !valid {
            return Err("UI slot range exceeds storage bounds".into());
        }
        Ok(binding)
    }
    pub fn valid_for(self, p: &crate::engine::player::inventory::profile::Profile) -> bool {
        match self {
            Self::Player(i) => i < p.player_slots,
            Self::Craft(i) => i < p.craft_side.pow(2),
            Self::Table(i) => i < p.table_side.pow(2),
            Self::Container { bank, index } => {
                p.containers.get(bank).is_some_and(|c| index < c.slots)
            }
            Self::Machine(i) => i < 3,
            _ => true,
        }
    }
}
