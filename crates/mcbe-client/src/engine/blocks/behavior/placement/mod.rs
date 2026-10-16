//! Reusable placement policies, independent of block event behaviors.
mod context;
mod facing;
mod slab;
mod stairs;
mod standard;

use super::Position;
use crate::{
    engine_blocks::{BlockId, Placement, Registry},
    scene::Result,
};
pub use context::{PlacementContext, neighbor, target_position};

pub type PlacementResult = Option<(Position, BlockId)>;

/// Policies override only their special rules; default placement is shared.
pub trait PlacementBehavior: Sync {
    fn validate(&self, registry: &Registry, id: BlockId) -> Result<()> {
        validate_references(registry, id)
    }

    fn place(&self, c: &PlacementContext<'_>) -> Result<PlacementResult> {
        Ok(c.empty_neighbor().map(|p| (p, c.selected)))
    }
}
pub fn get(registry: &Registry, id: BlockId) -> &'static dyn PlacementBehavior {
    static STANDARD: standard::Standard = standard::Standard;
    static SLAB: slab::Slab = slab::Slab;
    static STAIRS: stairs::Stairs = stairs::Stairs;
    static FACING: facing::Facing = facing::Facing;
    match registry.get(id).placement {
        Some(Placement::Slab { .. }) => &SLAB,
        Some(Placement::Stairs { .. }) => &STAIRS,
        Some(Placement::Facing { .. }) => &FACING,
        None => &STANDARD,
    }
}

fn validate_references(registry: &Registry, id: BlockId) -> Result<()> {
    if let Some(rule) = &registry.get(id).placement {
        for name in rule.state_names() {
            registry.find(name)?;
        }
    }
    Ok(())
}
