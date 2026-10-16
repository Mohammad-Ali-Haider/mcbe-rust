//! Typed capabilities resolved once when registering a block.
pub(crate) mod collision;
pub(crate) mod interaction;
pub use interaction::Interaction;
pub(crate) mod visual;
pub use collision::ShapeComponent;
use serde::{Deserialize, Serialize};
pub use visual::VisualComponent;
pub(crate) mod occlusion;
pub use occlusion::OcclusionComponent;
pub(crate) mod placement;
pub use placement::PlacementComponent;
pub(crate) mod signal;
pub use signal::SignalComponent;
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockComponents {
    #[serde(default)]
    pub material: material::Material,
    pub container: Option<crate::engine::items::FluidContainer>,
    pub fluid: Option<FluidKind>,
    pub interaction: Option<Interaction>,
    #[serde(default)]
    pub behavior: BehaviorComponent,
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

#[derive(Clone, Copy, Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorComponent {
    #[default]
    Passive,
    Wire,
}
impl BehaviorComponent {
    pub fn receives_events(self) -> bool {
        self == Self::Wire
    }
    pub fn passive(&self) -> bool {
        *self == Self::Passive
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FluidKind {
    Water,
}

pub(crate) mod material;
