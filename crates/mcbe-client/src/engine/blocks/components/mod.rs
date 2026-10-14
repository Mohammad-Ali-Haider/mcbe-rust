//! Typed capabilities resolved once when registering a block.
pub(crate) mod collision;
pub(crate) mod visual;
use crate::{engine_blocks::RenderLayer, scene::Result};
pub use collision::ShapeComponent;
use serde::{Deserialize, Serialize};
pub use visual::VisualComponent;
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OcclusionComponent {
    #[serde(default)]
    pub layer: RenderLayer,
    pub cull_group: Option<String>,
}
#[derive(Clone, Copy, Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlacementComponent {
    #[default]
    Standard,
    Slab,
    Stairs,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignalComponent {
    #[serde(default)]
    pub source: u8,
    #[serde(default = "one")]
    pub attenuation: u8,
}
fn one() -> u8 {
    1
}
impl SignalComponent {
    pub fn validate(&self) -> Result<()> {
        if self.source > 15 || !(1..=15).contains(&self.attenuation) {
            return Err("Signals require source 0..15 and attenuation 1..15".into());
        }
        Ok(())
    }
}
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockComponents {
    #[serde(default)]
    pub visual: VisualComponent,
    #[serde(default)]
    pub collision: ShapeComponent,
    #[serde(default)]
    pub selection: ShapeComponent,
    #[serde(default)]
    pub occlusion: OcclusionComponent,
    #[serde(default)]
    pub placement: PlacementComponent,
    pub signal: Option<SignalComponent>,
}
