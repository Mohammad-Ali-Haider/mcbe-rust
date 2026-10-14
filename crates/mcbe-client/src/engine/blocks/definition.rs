//! Compiled immutable state descriptors; cells store their small numeric ID.
use serde::{Deserialize, Serialize};
pub type BlockStateId = u16;
/// Compatibility name: world cells contain compiled state IDs.
pub type BlockId = BlockStateId;
#[derive(Default, Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderLayer {
    #[default]
    Opaque,
    Cutout,
    Translucent,
}
/// References ordinary registered blocks; no special numeric IDs or save encoding.
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Placement {
    Slab {
        lower: String,
        upper: String,
        double: String,
    },
    Stairs {
        lower: [String; 4],
        upper: [String; 4],
    },
}
#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    Grain,
    Bricks,
    Checker,
    Meadow,
    Solid,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Block {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<super::state::StateIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Vec<[f32; 6]>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<super::components::SignalComponent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<Placement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture_files: Option<[String; 6]>,
    /// None uses the visual model. Some([]) is non-solid but still selectable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collision: Option<Vec<[f32; 6]>>,
    #[serde(default)]
    pub render_layer: RenderLayer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cull_group: Option<String>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_textures: Option<[Binding; 6]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<crate::engine_geometry::Geometry>,
    pub id: String,
    pub label: String,
    pub color: [u8; 3],
    pub pattern: Pattern,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_color: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft: Option<Binding>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub pack: String,
    pub block: String,
}
