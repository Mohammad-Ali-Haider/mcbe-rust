//! Finite, version-bound container shapes. Selection and movement are separate.
use crate::{
    Result,
    blocks::{self, Palette, States},
    cells::scene_from_shapes,
    chunks::World,
    cubes::Cube,
    leveldb::Snapshot,
    motion::{Actor, SolidScene},
    raycast::{RayBlock, Shapes},
    world_raycast::WorldRays,
};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ContainerKind {
    Barrel,
    Dispenser,
    Hopper,
    Chest,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContainerBlock(u16);
impl ContainerBlock {
    pub const BARREL: Self = Self(0);
    pub const DISPENSER: Self = Self(1);
    pub const HOPPER: Self = Self(2);
    pub const CHEST: Self = Self(3);
    pub const DEFAULTS: [Self; 4] = [Self::BARREL, Self::DISPENSER, Self::HOPPER, Self::CHEST];
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
    pub fn all() -> Vec<Self> {
        (0..Self::definitions().len())
            .map(|i| Self(i as u16))
            .collect()
    }
    pub fn kind(self) -> ContainerKind {
        match self.definition().id.as_str() {
            "minecraft:barrel" => ContainerKind::Barrel,
            "minecraft:dispenser" => ContainerKind::Dispenser,
            "minecraft:hopper" => ContainerKind::Hopper,
            "minecraft:chest" => ContainerKind::Chest,
            _ => unreachable!("Unknown built-in container kind"),
        }
    }
    fn definitions() -> &'static [RayBlock] {
        static DEFINITIONS: std::sync::OnceLock<Vec<RayBlock>> = std::sync::OnceLock::new();
        DEFINITIONS.get_or_init(|| {
            let table: Value =
                serde_json::from_str(include_str!("../../../docs/container-geometry.json"))
                    .expect("Container geometry JSON");
            let rows = table["rows"].as_array().expect("Container rows");
            assert!(rows.len() <= usize::from(u16::MAX) + 1);
            rows.iter()
                .map(|row| {
                    let mut states = blocks::states(&row["states"]).expect("Container states");
                    states.sort_by(|a, b| a.0.cmp(&b.0));
                    RayBlock {
                        position: [0, 0, 0],
                        id: row["id"].as_str().expect("Container id").into(),
                        states,
                    }
                })
                .collect()
        })
    }
    fn definition(self) -> &'static RayBlock {
        &Self::definitions()[self.index()]
    }
    pub fn block(self, position: [i32; 3]) -> RayBlock {
        RayBlock {
            position,
            ..self.definition().clone()
        }
    }
    pub fn canonical(id: &str, states: &States) -> Option<Self> {
        let mut states = states.clone();
        states.sort_by(|a, b| a.0.cmp(&b.0));
        Self::all().into_iter().find(|kind| {
            let b = kind.definition();
            b.id == id && b.states == states
        })
    }
}

pub fn catalog() -> Value {
    let mut catalog: Value =
        serde_json::from_str(include_str!("../../../docs/container-geometry.json"))
            .expect("Built-in container geometry JSON");
    let rows = catalog["rows"]
        .as_array_mut()
        .expect("Built-in geometry rows");
    rows.extend(Cube::ALL.map(|cube| {
        json!({"id":cube.id(),"states":cube.states(),
        "boxes":[[0,0,0,1,1,1]],"collision":[[0,0,0,1,1,1]],"passable":false})
    }));
    rows.push(json!({"id":"minecraft:air","states":{},"boxes":[],"collision":[],"passable":true}));
    catalog
}

pub struct Geometry {
    pub selection: Shapes,
    collision: HashMap<(String, States), Vec<[f32; 6]>>,
}
impl Geometry {
    pub fn measured() -> Result<Self> {
        Self::read(&catalog())
    }
    pub fn read(table: &Value) -> Result<Self> {
        if table["reference_sha256"]
            != "19c88569af2e4b7d984e999055a31cbcb0799dacf8bbbf7371eda42f5772a443"
        {
            return Err("Unknown container geometry reference".into());
        }
        let selection = Shapes::read(&table["rows"])?;
        let mut collision = HashMap::new();
        for row in table["rows"].as_array().ok_or("Missing geometry rows")? {
            let id = row["id"]
                .as_str()
                .ok_or("Missing geometry identifier")?
                .to_owned();
            let mut states = blocks::states(&row["states"]).map_err(|e| e.to_string())?;
            states.sort_by(|a, b| a.0.cmp(&b.0));
            let raw = row["collision"]
                .as_array()
                .ok_or("Missing collision boxes")?;
            if raw.len() > 64 {
                return Err("Too many collision boxes".into());
            }
            let mut boxes = Vec::new();
            for value in raw {
                let bounds: [f32; 6] =
                    serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
                if bounds.iter().any(|v| !v.is_finite())
                    || (0..3).any(|a| {
                        bounds[a] < 0.0 || bounds[a] >= bounds[a + 3] || bounds[a + 3] > 1.0
                    })
                {
                    return Err("Invalid container collision box".into());
                }
                boxes.push(bounds);
            }
            if collision.insert((id, states), boxes).is_some() {
                return Err("Duplicate collision state".into());
            }
        }
        Ok(Self {
            selection,
            collision,
        })
    }
    pub fn collision(&self, block: &RayBlock) -> Result<&[[f32; 6]]> {
        let mut states = block.states.clone();
        states.sort_by(|a, b| a.0.cmp(&b.0));
        self.collision
            .get(&(block.id.clone(), states))
            .map(Vec::as_slice)
            .ok_or_else(|| {
                format!(
                    "Unmeasured movement geometry: {} at {:?}",
                    block.id, block.position
                )
            })
    }
    /// Horizontal stepping for very small hopper actors remains unmeasured.
    pub fn collision_for(&self, block: &RayBlock, actor: &Actor) -> Result<Vec<[f32; 6]>> {
        let boxes = self.collision(block)?;
        if block.id == "minecraft:hopper"
            && (actor.body.width < 0.5 || actor.body.height < 0.5)
            && (actor.velocity.x != 0.0 || actor.velocity.z != 0.0)
        {
            return Err("Small-actor horizontal hopper stepping is unmeasured".into());
        }
        Ok(boxes.to_vec())
    }
}

/// Explicit saved Overworld columns, with unsupported states and layers rejected.
pub struct WorldGeometry {
    pub rays: WorldRays,
}
impl WorldGeometry {
    pub fn new(snapshot: Snapshot) -> Self {
        Self {
            rays: WorldRays::new(snapshot),
        }
    }
    pub fn scene_for(
        &mut self,
        actor: &Actor,
        palette: &Palette,
        geometry: &Geometry,
    ) -> Result<SolidScene> {
        scene_from_shapes(actor, |p| {
            if !(-64..320).contains(&p[1]) {
                return Err("Movement outside measured Overworld height".into());
            }
            let key = [
                p[0].div_euclid(16).to_le_bytes().as_slice(),
                p[2].div_euclid(16).to_le_bytes().as_slice(),
                &[43],
            ]
            .concat();
            if self.rays.world.snapshot.get(&key).is_none() {
                return Err(format!("Missing saved collision column at {p:?}"));
            }
            if let Some(secondary) = self.rays.world.block(p, 1)?
                && (secondary.name != "minecraft:air" || !secondary.states.is_empty())
            {
                return Err(format!("Unmeasured secondary collision layer at {p:?}"));
            }
            match self.rays.block_at(p, palette)? {
                Some(block) => geometry.collision_for(&block, actor),
                None => Ok(Vec::new()),
            }
        })
    }
    pub fn world(&self) -> &World {
        &self.rays.world
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_container_identity_rejects_unmeasured_state_changes() {
        for kind in ContainerBlock::all() {
            let mut block = kind.block([1, 64, -1]);
            block.states.reverse();
            assert_eq!(
                ContainerBlock::canonical(&block.id, &block.states),
                Some(kind)
            );
            block
                .states
                .push(("unknown_flag".into(), crate::chunks::StateValue::Bool(true)));
            assert!(ContainerBlock::canonical(&block.id, &block.states).is_none());
        }
    }
    #[test]
    fn hopper_cavity_does_not_become_a_full_cube() {
        let geometry = Geometry::measured().unwrap();
        let mut block = RayBlock {
            position: [0, 0, 0],
            id: "minecraft:hopper".into(),
            states: blocks::states(&json!({"toggle_bit":false,"facing_direction":0})).unwrap(),
        };
        let boxes = geometry.collision(&block).unwrap();
        assert!(!boxes.iter().any(|b| b[0] < 0.5
            && b[3] > 0.5
            && b[1] < 0.8
            && b[4] > 0.8
            && b[2] < 0.5
            && b[5] > 0.5));
        assert!(boxes.iter().any(|b| b[0] < 0.5
            && b[3] > 0.5
            && b[1] < 0.65
            && b[4] > 0.65
            && b[2] < 0.5
            && b[5] > 0.5));
        let tiny = Actor::new(
            crate::motion::Vec3::new(-1.2, 0.1, 0.5),
            crate::motion::Body {
                width: 0.0625,
                height: 0.0625,
            },
        )
        .unwrap()
        .impulse(crate::motion::Vec3::new(1.25, 0.0, 0.0))
        .unwrap();
        assert!(geometry.collision_for(&block, &tiny).is_err());
        let normal = Actor::new(tiny.position, crate::motion::Body::PROBE)
            .unwrap()
            .impulse(tiny.velocity)
            .unwrap();
        assert!(geometry.collision_for(&block, &normal).is_ok());
        block.states = blocks::states(&json!({"toggle_bit":false,"facing_direction":99})).unwrap();
        assert!(geometry.collision(&block).is_err());
    }
    #[test]
    fn invalid_and_unbound_collision_tables_fail() {
        let mut table = catalog();
        table["reference_sha256"] = json!("unknown");
        assert!(Geometry::read(&table).is_err());
        table = catalog();
        table["rows"][0]["collision"] = json!([[0, 0, 0, 2, 1, 1]]);
        assert!(Geometry::read(&table).is_err());
    }
}
