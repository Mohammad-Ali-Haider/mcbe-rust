//! Shared Rust behavior traits: one implementation per policy, no objects per cell.
mod passive;
pub(crate) mod placement;
pub(crate) mod wire;
use super::{BlockId, Registry, components::BehaviorComponent};
use crate::scene::Result;
use std::collections::HashMap;
pub type Position = [i32; 3];
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
    fn validate(&self, _registry: &Registry, _id: BlockId) -> Result<()> {
        Ok(())
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
/// Event dispatch does not select or override a block's placement policy.
pub fn get(registry: &Registry, id: BlockId) -> &'static dyn BlockBehavior {
    static PASSIVE: passive::Passive = passive::Passive;
    static WIRE: wire::Wire = wire::Wire;
    match registry.get(id).behavior {
        BehaviorComponent::Passive => &PASSIVE,
        BehaviorComponent::Wire => &WIRE,
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
