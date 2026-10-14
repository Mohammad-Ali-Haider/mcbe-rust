use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlacementComponent {
    #[default]
    Standard,
    Slab,
    Stairs,
}
