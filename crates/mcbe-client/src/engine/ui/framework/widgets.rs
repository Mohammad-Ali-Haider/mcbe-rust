//! Typed widgets shared by JSON authoring and Rust screen builders.
use super::{layout::Layout, screen::Action};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", content = "index", rename_all = "snake_case")]
pub enum Binding {
    Player(usize),
    Craft(usize),
    Result,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Kind {
    Panel,
    Label { text: String },
    Slot { binding: Binding },
    Image { sprite: String },
    Preview,
    Search,
    Catalog { columns: usize, rows: usize },
    Progress { value: String, maximum: f64 },
    Button { text: String, action: Action },
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Widget {
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
        if self.id.is_empty() || self.id.len() > 64 || self.style.len() > 64 {
            return Err("UI widget ID/style invalid".into());
        }
        match &self.kind {
            Kind::Slot {
                binding: Binding::Player(i),
            } if *i >= 36 => return Err("Player slot outside inventory".into()),
            Kind::Slot {
                binding: Binding::Craft(i),
            } if *i >= 4 => return Err("Craft slot outside grid".into()),
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
