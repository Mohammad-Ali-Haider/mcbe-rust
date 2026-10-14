//! Shared Rust behavior traits: one implementation per policy, no objects per cell.
pub(crate) mod placement;
pub(crate) mod signal;
use super::{BlockId, Placement, Registry, components::BehaviorComponent};
use crate::scene::Result;
use mcbe_core::raycast::Hit;
use std::collections::HashMap;
pub type Position = [i32; 3];
pub type PlacementResult = Option<(Position, BlockId)>;
pub struct PlacementContext<'a> {
    pub registry: &'a Registry,
    pub selected: BlockId,
    pub hit: &'a Hit,
    pub yaw: f32,
    pub target: Option<BlockId>,
    pub adjacent: Option<BlockId>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockEvent {
    Placed,
    NeighborChanged,
    ScheduledTick,
}
pub struct EventContext<'a> {
    pub registry: &'a Registry,
    pub blocks: &'a HashMap<Position, BlockId>,
    pub position: Position,
    pub state: BlockId,
}
#[derive(Debug, Clone, Copy)]
pub enum Command {
    Set {
        position: Position,
        expected: BlockId,
        state: BlockId,
    },
    Schedule {
        position: Position,
        expected: BlockId,
        delay: u16,
    },
}
pub trait BlockBehavior: Sync {
    fn place(&self, c: &PlacementContext<'_>) -> Result<PlacementResult> {
        Ok(c.adjacent
            .is_none()
            .then(|| (placement::neighbor(c.hit), c.selected)))
    }
    fn on_event(
        &self,
        _c: &EventContext<'_>,
        _event: BlockEvent,
        _commands: &mut Vec<Command>,
    ) -> Result<()> {
        Ok(())
    }
}
pub fn get(registry: &Registry, id: BlockId) -> &'static dyn BlockBehavior {
    static STANDARD: placement::Standard = placement::Standard;
    static SLAB: placement::Slab = placement::Slab;
    static STAIRS: placement::Stairs = placement::Stairs;
    static WIRE: signal::Wire = signal::Wire;
    let b = registry.get(id);
    if b.behavior == BehaviorComponent::Wire {
        return &WIRE;
    }
    match b.placement {
        Some(Placement::Slab { .. }) => &SLAB,
        Some(Placement::Stairs { .. }) => &STAIRS,
        None => &STANDARD,
    }
}
pub fn neighbors(p: Position) -> impl Iterator<Item = Position> {
    [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
    ]
    .map(move |d| [p[0] + d[0], p[1] + d[1], p[2] + d[2]])
    .into_iter()
}
