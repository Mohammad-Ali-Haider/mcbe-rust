//! Hit geometry shared by every placement policy and the player adapter.
use super::super::Position;
use crate::engine_blocks::{BlockId, Registry};
use mcbe_core::raycast::{Face, Hit};

pub struct PlacementContext<'a> {
    pub registry: &'a Registry,
    pub selected: BlockId,
    pub hit: &'a Hit,
    pub yaw: f32,
    pub target: Option<BlockId>,
    pub adjacent: Option<BlockId>,
}
impl PlacementContext<'_> {
    /// Standard and oriented blocks share the same occupancy rule.
    pub fn empty_neighbor(&self) -> Option<Position> {
        self.adjacent.is_none().then(|| neighbor(self.hit))
    }
    /// Undersides select the upper half; side hits use their local height.
    pub fn upper_half(&self) -> bool {
        self.hit.face == Face::Down || (self.hit.face != Face::Up && self.hit.face_location.y > 0.5)
    }
    /// Index in state arrays ordered north, east, south, west.
    pub fn facing_index(&self) -> usize {
        let quarter_turn =
            ((self.yaw / std::f32::consts::FRAC_PI_2).round() as i32).rem_euclid(4) as usize;
        [0, 3, 2, 1][quarter_turn]
    }
}
pub fn target_position(hit: &Hit) -> Position {
    [hit.position.x, hit.position.y, hit.position.z]
}
pub fn neighbor(hit: &Hit) -> Position {
    let mut p = target_position(hit);
    let (axis, step) = match hit.face {
        Face::Up => (1, 1),
        Face::Down => (1, -1),
        Face::East => (0, 1),
        Face::West => (0, -1),
        Face::North => (2, -1),
        Face::South => (2, 1),
    };
    p[axis] += step;
    p
}
