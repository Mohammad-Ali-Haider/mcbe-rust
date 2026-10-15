//! Interaction selects a shared station controller independently of block appearance.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interaction {
    CraftingTable,
    Furnace,
}
impl Interaction {
    pub fn screen(self) -> &'static str {
        match self {
            Self::CraftingTable => "crafting_table",
            Self::Furnace => "furnace",
        }
    }
}
