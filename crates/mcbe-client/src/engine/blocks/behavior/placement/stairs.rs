//! Facing and half selection, reused by every stair material.
use super::{PlacementBehavior, PlacementContext, PlacementResult};
use crate::{engine_blocks::Placement, scene::Result};
pub struct Stairs;
impl PlacementBehavior for Stairs {
    fn place(&self, c: &PlacementContext<'_>) -> Result<PlacementResult> {
        let Some(position) = c.empty_neighbor() else {
            return Ok(None);
        };
        let Some(Placement::Stairs { lower, upper: top }) = &c.registry.get(c.selected).placement
        else {
            return Err("Stair policy missing".into());
        };
        let states = if c.upper_half() { top } else { lower };
        let state = c.registry.find(&states[c.facing_index()])?;
        Ok(Some((position, state)))
    }
}
