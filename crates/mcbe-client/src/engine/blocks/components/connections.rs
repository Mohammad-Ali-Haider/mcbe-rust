//! Authored horizontal connection channels and precompiled mask models.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionComponent {
    pub channel: String,
    pub models: [String; 16],
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Connections {
    pub channel: String,
    pub models: Vec<crate::engine_geometry::Geometry>,
}
impl Connections {
    pub fn validate(&self) -> crate::scene::Result<()> {
        if !super::super::state::identifier(&self.channel) || self.models.len() != 16 {
            return Err("Connections require an identifier channel and 16 mask models".into());
        }
        for model in &self.models {
            crate::engine_geometry::validate_boxes(&model.boxes, false)?;
        }
        Ok(())
    }
}
