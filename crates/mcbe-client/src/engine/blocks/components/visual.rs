//! Appearance and model references; independent of physical collision.
use crate::engine_blocks::{Binding, Pattern};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualComponent {
    #[serde(default = "cube")]
    pub model: String,
    pub color: [u8; 3],
    pub pattern: Pattern,
    pub side_color: Option<[u8; 3]>,
    pub textures: Option<[String; 6]>,
    pub local: Option<Binding>,
    pub faces: Option<[Binding; 6]>,
}
fn cube() -> String {
    "models/cube.json".into()
}
impl Default for VisualComponent {
    fn default() -> Self {
        Self {
            model: cube(),
            color: [255; 3],
            pattern: Pattern::Solid,
            side_color: None,
            textures: None,
            local: None,
            faces: None,
        }
    }
}
