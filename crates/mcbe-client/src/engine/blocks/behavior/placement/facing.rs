//! Cardinal placement shared by furnaces and any authored oriented family.
use super::{PlacementBehavior, PlacementContext, PlacementResult};
use crate::{engine_blocks::Placement, scene::Result};
pub struct Facing;
impl PlacementBehavior for Facing {
    fn place(&self, c: &PlacementContext<'_>) -> Result<PlacementResult> {
        let Some(position) = c.empty_neighbor() else {
            return Ok(None);
        };
        let Some(Placement::Facing { states }) = &c.registry.get(c.selected).placement else {
            return Err("Facing placement policy missing".into());
        };
        Ok(Some((
            position,
            c.registry.find(&states[c.facing_index()])?,
        )))
    }
}
