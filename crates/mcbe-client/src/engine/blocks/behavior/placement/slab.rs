//! Half-height placement and merging, reused by every slab material.
use super::{PlacementBehavior, PlacementContext, PlacementResult, neighbor, target_position};
use crate::{
    engine_blocks::{BlockId, Placement, Registry},
    scene::Result,
};
use mcbe_core::raycast::Face;
pub struct Slab;
impl PlacementBehavior for Slab {
    fn validate(&self, registry: &Registry, id: BlockId) -> Result<()> {
        super::validate_references(registry, id)?;
        let Some(Placement::Slab {
            lower,
            upper,
            double,
        }) = &registry.get(id).placement
        else {
            return Err("Slab policy missing".into());
        };
        if lower == upper || lower == double || upper == double {
            return Err("Slab variants must be distinct registered blocks".into());
        }
        let boxes = |name: &str| {
            registry
                .get(registry.find(name).unwrap())
                .geometry
                .as_ref()
                .map_or_else(|| vec![crate::engine_geometry::UNIT], |g| g.boxes.clone())
        };
        if boxes(lower) != vec![[0., 0., 0., 1., 0.5, 1.]]
            || boxes(upper) != vec![[0., 0.5, 0., 1., 1., 1.]]
            || boxes(double) != vec![crate::engine_geometry::UNIT]
        {
            return Err(
                "Slab variants require lower half, upper half and full cube geometry".into(),
            );
        }
        Ok(())
    }

    fn place(&self, c: &PlacementContext<'_>) -> Result<PlacementResult> {
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
            p = target_position(c.hit);
            both
        } else {
            let id = if c.upper_half() { hi } else { lo };
            match c.adjacent {
                None => id,
                Some(old) if (old == lo && id == hi) || (old == hi && id == lo) => both,
                _ => return Ok(None),
            }
        };
        Ok(Some((p, id)))
    }
}
