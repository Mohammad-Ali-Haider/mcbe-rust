//! Direct saved-world collision. Only measured persistent stone/air states.
use crate::{
    Result,
    cells::scene_from,
    chunks::World,
    leveldb::Snapshot,
    motion::{Actor, SolidScene},
};
use std::collections::HashMap;
pub struct WorldScene {
    pub world: World,
    pub cells: HashMap<[i32; 3], bool>,
}
impl WorldScene {
    pub fn new(snapshot: Snapshot) -> Self {
        Self {
            world: World::new(snapshot),
            cells: HashMap::new(),
        }
    }
    pub fn scene_for(&mut self, actor: &Actor) -> Result<SolidScene> {
        scene_from(actor, |position| {
            if let Some(solid) = self.cells.get(&position) {
                return Ok(*solid);
            }
            let solid = self.world.collision_solid(position)?;
            self.cells.insert(position, solid);
            Ok(solid)
        })
    }
}
