use crate::engine_blocks::RenderLayer;
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OcclusionComponent {
    #[serde(default)]
    pub layer: RenderLayer,
    pub cull_group: Option<String>,
}
