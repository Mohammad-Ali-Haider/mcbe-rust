//! Shared placement implementations; definitions select a policy, never an ID.
use super::{BlockBehavior, PlacementContext};
use crate::{engine_blocks::Placement, scene::Result};
use mcbe_core::raycast::{Face, Hit};
fn upper(hit: &Hit) -> bool {
    hit.face == Face::Down || (hit.face != Face::Up && hit.face_location.y > 0.5)
}
pub fn neighbor(hit: &Hit) -> [i32; 3] {
    let mut p = [hit.position.x, hit.position.y, hit.position.z];
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
pub struct Standard;
impl BlockBehavior for Standard {}
pub struct Slab;
impl BlockBehavior for Slab {
    fn place(&self, c: &PlacementContext<'_>) -> Result<super::PlacementResult> {
        let Some(Placement::Slab {
            lower,
            upper: top,
            double,
        }) = &c.registry.get(c.selected).placement
        else {
            return Err("Slab policy missing".into());
        };
        let lo = c.registry.find(lower)?;
        let hi = c.registry.find(top)?;
        let both = c.registry.find(double)?;
        let mut p = neighbor(c.hit);
        let id = if (c.target == Some(lo) && c.hit.face == Face::Up)
            || (c.target == Some(hi) && c.hit.face == Face::Down)
        {
            p = [c.hit.position.x, c.hit.position.y, c.hit.position.z];
            both
        } else {
            let id = if upper(c.hit) { hi } else { lo };
            match c.adjacent {
                None => id,
                Some(old) if (old == lo && id == hi) || (old == hi && id == lo) => both,
                _ => return Ok(None),
            }
        };
        Ok(Some((p, id)))
    }
}
pub struct Stairs;
impl BlockBehavior for Stairs {
    fn place(&self, c: &PlacementContext<'_>) -> Result<super::PlacementResult> {
        if c.adjacent.is_some() {
            return Ok(None);
        }
        let Some(Placement::Stairs { lower, upper: top }) = &c.registry.get(c.selected).placement
        else {
            return Err("Stair policy missing".into());
        };
        let heading = ((c.yaw / std::f32::consts::FRAC_PI_2).round() as i32).rem_euclid(4) as usize;
        let index = [0, 3, 2, 1][heading];
        Ok(Some((
            neighbor(c.hit),
            c.registry
                .find(&(if upper(c.hit) { top } else { lower })[index])?,
        )))
    }
}
