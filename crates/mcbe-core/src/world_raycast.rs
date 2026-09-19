//! Lazy primary-layer selection through a stopped Overworld database.
use crate::{
    Result,
    blocks::Palette,
    chunks::World,
    leveldb::Snapshot,
    raycast::{self, Hit, Options, RayBlock, Shapes},
};
use std::collections::HashMap;

pub struct WorldRays {
    pub world: World,
    pub cells: HashMap<[i32; 3], Option<RayBlock>>,
}
impl WorldRays {
    pub fn new(snapshot: Snapshot) -> Self {
        Self {
            world: World::new(snapshot),
            cells: HashMap::new(),
        }
    }
    pub fn block_at(&mut self, position: [i32; 3], palette: &Palette) -> Result<Option<RayBlock>> {
        if let Some(cached) = self.cells.get(&position) {
            return Ok(cached.clone());
        }
        let block = match self.world.block(position, 0)? {
            None => None,
            Some(saved) => {
                let canonical = palette.resolve_saved(&saved).map_err(|e| e.to_string())?;
                Some(RayBlock {
                    position,
                    id: canonical.id().into(),
                    states: canonical
                        .states()
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                })
            }
        };
        self.cells.insert(position, block.clone());
        Ok(block)
    }
    pub fn cast(
        &mut self,
        origin: [f64; 3],
        direction: [f64; 3],
        options: Options,
        shapes: &Shapes,
        palette: &Palette,
    ) -> Result<Option<Hit>> {
        raycast::cast(origin, direction, options, shapes, |position| {
            self.block_at(position, palette)
        })
    }
}
