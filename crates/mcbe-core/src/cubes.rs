//! Exact full-cube states. Native material comparisons are recorded separately.
use crate::{
    Result,
    blocks::{Palette, States},
    cells::scene_from,
    chunks::{BlockState, StateValue, World},
    leveldb::Snapshot,
    motion::{Actor, SolidScene},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cube {
    Stone,
    Dirt,
    Grass,
    Bedrock,
    Granite,
    Diorite,
    Andesite,
    Cobblestone,
    MossyCobblestone,
    Deepslate,
    CobbledDeepslate,
    Tuff,
    Calcite,
    CoalOre,
    IronOre,
    CopperOre,
    GoldOre,
    LapisOre,
    RedstoneOre,
    LitRedstoneOre,
    DiamondOre,
    EmeraldOre,
    DeepslateCoalOre,
    DeepslateIronOre,
    DeepslateCopperOre,
    DeepslateGoldOre,
    DeepslateLapisOre,
    DeepslateRedstoneOre,
    LitDeepslateRedstoneOre,
    DeepslateDiamondOre,
    DeepslateEmeraldOre,
    RedstoneBlock,
}
impl Cube {
    pub const ALL: [Self; 32] = [
        Self::Stone,
        Self::Dirt,
        Self::Grass,
        Self::Bedrock,
        Self::Granite,
        Self::Diorite,
        Self::Andesite,
        Self::Cobblestone,
        Self::MossyCobblestone,
        Self::Deepslate,
        Self::CobbledDeepslate,
        Self::Tuff,
        Self::Calcite,
        Self::CoalOre,
        Self::IronOre,
        Self::CopperOre,
        Self::GoldOre,
        Self::LapisOre,
        Self::RedstoneOre,
        Self::LitRedstoneOre,
        Self::DiamondOre,
        Self::EmeraldOre,
        Self::DeepslateCoalOre,
        Self::DeepslateIronOre,
        Self::DeepslateCopperOre,
        Self::DeepslateGoldOre,
        Self::DeepslateLapisOre,
        Self::DeepslateRedstoneOre,
        Self::LitDeepslateRedstoneOre,
        Self::DeepslateDiamondOre,
        Self::DeepslateEmeraldOre,
        Self::RedstoneBlock,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Stone => "minecraft:stone",
            Self::Dirt => "minecraft:dirt",
            Self::Grass => "minecraft:grass_block",
            Self::Bedrock => "minecraft:bedrock",
            Self::Granite => "minecraft:granite",
            Self::Diorite => "minecraft:diorite",
            Self::Andesite => "minecraft:andesite",
            Self::Cobblestone => "minecraft:cobblestone",
            Self::MossyCobblestone => "minecraft:mossy_cobblestone",
            Self::Deepslate => "minecraft:deepslate",
            Self::CobbledDeepslate => "minecraft:cobbled_deepslate",
            Self::Tuff => "minecraft:tuff",
            Self::Calcite => "minecraft:calcite",
            Self::CoalOre => "minecraft:coal_ore",
            Self::IronOre => "minecraft:iron_ore",
            Self::CopperOre => "minecraft:copper_ore",
            Self::GoldOre => "minecraft:gold_ore",
            Self::LapisOre => "minecraft:lapis_ore",
            Self::RedstoneOre => "minecraft:redstone_ore",
            Self::LitRedstoneOre => "minecraft:lit_redstone_ore",
            Self::DiamondOre => "minecraft:diamond_ore",
            Self::EmeraldOre => "minecraft:emerald_ore",
            Self::DeepslateCoalOre => "minecraft:deepslate_coal_ore",
            Self::DeepslateIronOre => "minecraft:deepslate_iron_ore",
            Self::DeepslateCopperOre => "minecraft:deepslate_copper_ore",
            Self::DeepslateGoldOre => "minecraft:deepslate_gold_ore",
            Self::DeepslateLapisOre => "minecraft:deepslate_lapis_ore",
            Self::DeepslateRedstoneOre => "minecraft:deepslate_redstone_ore",
            Self::LitDeepslateRedstoneOre => "minecraft:lit_deepslate_redstone_ore",
            Self::DeepslateDiamondOre => "minecraft:deepslate_diamond_ore",
            Self::DeepslateEmeraldOre => "minecraft:deepslate_emerald_ore",
            Self::RedstoneBlock => "minecraft:redstone_block",
        }
    }
    pub fn states(self) -> BTreeMap<String, StateValue> {
        match self {
            Self::Stone => {
                BTreeMap::from([("stone_type".into(), StateValue::String("stone".into()))])
            }
            Self::Dirt => {
                BTreeMap::from([("dirt_type".into(), StateValue::String("normal".into()))])
            }
            Self::Bedrock => BTreeMap::from([("infiniburn_bit".into(), StateValue::Bool(false))]),
            Self::Granite | Self::Diorite | Self::Andesite => BTreeMap::from([(
                "stone_type".into(),
                StateValue::String(self.resource_key().into()),
            )]),
            Self::Deepslate => {
                BTreeMap::from([("pillar_axis".into(), StateValue::String("y".into()))])
            }
            _ => BTreeMap::new(),
        }
    }
    pub fn resource_key(self) -> &'static str {
        if self == Self::Grass {
            "grass"
        } else {
            self.id()
                .strip_prefix("minecraft:")
                .expect("Static cube namespace")
        }
    }
    pub fn canonical(id: &str, states: &BTreeMap<String, StateValue>) -> Result<Option<Self>> {
        if id == "minecraft:air" && states.is_empty() {
            return Ok(None);
        }
        for kind in Self::ALL {
            if id == kind.id() && *states == kind.states() {
                return Ok(Some(kind));
            }
        }
        Err(format!("Unmeasured cube state: {id} {states:?}"))
    }
}
#[derive(Default)]
pub struct Classifier {
    cache: HashMap<(String, States), Option<Cube>>,
}
impl Classifier {
    pub fn saved(&mut self, block: &BlockState, palette: &Palette) -> Result<Option<Cube>> {
        let states = block
            .state_order
            .iter()
            .map(|key| {
                block
                    .states
                    .get(key)
                    .cloned()
                    .map(|value| (key.clone(), value))
                    .ok_or("Missing ordered state")
            })
            .collect::<std::result::Result<States, _>>()?;
        let key = (block.name.clone(), states);
        if let Some(kind) = self.cache.get(&key) {
            return Ok(*kind);
        }
        let canonical = palette.resolve_saved(block).map_err(|e| e.to_string())?;
        let kind = Cube::canonical(canonical.id(), canonical.states())?;
        self.cache.insert(key, kind);
        Ok(kind)
    }
}
pub struct WorldCubes<'a> {
    pub world: World,
    pub palette: &'a Palette,
    pub classifier: Classifier,
    pub cells: HashMap<[i32; 3], Option<Cube>>,
}
impl<'a> WorldCubes<'a> {
    pub fn new(snapshot: Snapshot, palette: &'a Palette) -> Self {
        Self {
            world: World::new(snapshot),
            palette,
            classifier: Classifier::default(),
            cells: HashMap::new(),
        }
    }
    pub fn block(&mut self, position: [i32; 3]) -> Result<Option<Cube>> {
        if let Some(kind) = self.cells.get(&position) {
            return Ok(*kind);
        }
        let kind = classify_cell(
            &mut self.world,
            position,
            &mut self.classifier,
            self.palette,
        )?;
        self.cells.insert(position, kind);
        Ok(kind)
    }
    pub fn scene_for(&mut self, actor: &Actor) -> Result<SolidScene> {
        scene_from(actor, |position| Ok(self.block(position)?.is_some()))
    }
}
pub fn classify_cell(
    world: &mut World,
    position: [i32; 3],
    classifier: &mut Classifier,
    palette: &Palette,
) -> Result<Option<Cube>> {
    if let Some(secondary) = world.block(position, 1)?
        && (secondary.name != "minecraft:air" || !secondary.states.is_empty())
    {
        return Err(format!("Unmeasured secondary cube layer at {position:?}"));
    }
    world
        .block(position, 0)?
        .map(|block| classifier.saved(&block, palette))
        .transpose()
        .map(Option::flatten)
}
pub fn region(
    world: &mut World,
    min: [i32; 3],
    max: [i32; 3],
    palette: &Palette,
) -> Result<HashMap<[i32; 3], Cube>> {
    let mut volume = 1u64;
    for axis in 0..3 {
        let size = i64::from(max[axis]) - i64::from(min[axis]);
        if size <= 0 {
            return Err("Region must have positive dimensions".into());
        }
        volume = volume
            .checked_mul(size as u64)
            .ok_or("Region volume overflow")?;
    }
    if min[1] < -64 || max[1] > 320 || volume > 1_000_000 {
        return Err("Cube region exceeds supported bounds".into());
    }
    let mut classifier = Classifier::default();
    let mut cells = HashMap::new();
    for x in min[0]..max[0] {
        for y in min[1]..max[1] {
            for z in min[2]..max[2] {
                if let Some(kind) = classify_cell(world, [x, y, z], &mut classifier, palette)? {
                    cells.insert([x, y, z], kind);
                }
            }
        }
    }
    Ok(cells)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_exact_cube_states_are_admitted() {
        assert_eq!(
            Cube::canonical("minecraft:air", &BTreeMap::new()).unwrap(),
            None
        );
        for kind in Cube::ALL {
            assert_eq!(
                Cube::canonical(kind.id(), &kind.states()).unwrap(),
                Some(kind)
            );
        }
        let mut unsupported = Cube::Bedrock.states();
        unsupported.insert("infiniburn_bit".into(), StateValue::Bool(true));
        assert!(Cube::canonical(Cube::Bedrock.id(), &unsupported).is_err());
        assert!(Cube::canonical("minecraft:glass", &BTreeMap::new()).is_err());
        assert!(Cube::canonical(Cube::Stone.id(), &BTreeMap::new()).is_err());
        assert!(Cube::canonical(Cube::Granite.id(), &BTreeMap::new()).is_err());
        assert!(
            Cube::canonical(
                Cube::Deepslate.id(),
                &BTreeMap::from([("pillar_axis".into(), StateValue::String("x".into()))])
            )
            .is_err()
        );
    }
}
