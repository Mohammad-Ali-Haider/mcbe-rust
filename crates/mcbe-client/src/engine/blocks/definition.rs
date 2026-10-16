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
    Facing {
        states: [String; 4],
    },
}
impl Placement {
    /// Shared reference traversal for validation and legacy-save upgrades.
    pub fn state_names(&self) -> Vec<&str> {
        match self {
            Self::Slab {
                lower,
                upper,
                double,
            } => vec![lower.as_str(), upper.as_str(), double.as_str()],
            Self::Facing { states } => states.iter().map(String::as_str).collect(),
            Self::Stairs { lower, upper } => {
                lower.iter().chain(upper).map(String::as_str).collect()
            }
        }
    }
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
    pub connections: Option<super::components::connections::Connections>,
    #[serde(
        default,
        skip_serializing_if = "super::components::material::Material::is_default"
    )]
    pub material: super::components::material::Material,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<crate::engine::items::FluidContainer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture_tint: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fluid: Option<super::components::FluidKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction: Option<super::components::Interaction>,
    #[serde(
        default,
        skip_serializing_if = "super::components::BehaviorComponent::passive"
    )]
    pub behavior: super::components::BehaviorComponent,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
}

use serde_json::Value;
use std::collections::BTreeMap;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockDefinition {
    pub id: String,
    pub label: String,
    pub extends: Option<String>,
    pub components: Value,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub states: Vec<BlockStateDefinition>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockStateDefinition {
    pub id: String,
    pub label: Option<String>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
    #[serde(default = "empty_object")]
    pub components: Value,
}
pub(crate) fn empty_object() -> Value {
    serde_json::json!({})
}
