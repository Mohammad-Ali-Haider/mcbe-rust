//! Collision queries shared by placement and atomic definition reloads.
use crate::engine_blocks::BlockId;
use mcbe_core::motion::Actor;
pub fn intersects_actor(
    library: &super::super::shapes::Library,
    id: BlockId,
    p: [i32; 3],
    actor: &Actor,
) -> bool {
    let a = actor.position;
    let half = actor.body.width * 0.5;
    let low = [a.x - half, a.y, a.z - half];
    let high = [a.x + half, a.y + actor.body.height, a.z + half];
    library
        .collision(id)
        .iter()
        .any(|b| (0..3).all(|n| low[n] < p[n] as f32 + b[n + 3] && high[n] > p[n] as f32 + b[n]))
}
