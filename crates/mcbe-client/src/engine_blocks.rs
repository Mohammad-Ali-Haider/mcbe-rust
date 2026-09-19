//! Engine-owned block identifiers, independent of the research cube enum.
use crate::scene::Result;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, path::Path};
pub type BlockId = u16;
#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    Grain,
    Bricks,
    Checker,
    Meadow,
    Solid,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Block {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<crate::engine_geometry::Geometry>,
    pub id: String,
    pub label: String,
    pub color: [u8; 3],
    pub pattern: Pattern,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_color: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft: Option<Binding>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub pack: String,
    pub block: String,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub version: u32,
    pub blocks: Vec<Block>,
}
impl Registry {
    pub fn built_in() -> Self {
        Self::parse(include_bytes!("../../../assets/blocks.json")).expect("Built-in engine blocks")
    }
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 256 * 1024 {
            return Err("Block definitions exceed 256 KiB".into());
        }
        let result: Self = serde_json::from_slice(bytes)?;
        result.validate()?;
        Ok(result)
    }
    pub fn read(path: &Path) -> Result<Self> {
        if fs::metadata(path)?.len() > 256 * 1024 {
            return Err("Block definitions exceed 256 KiB".into());
        }
        Self::parse(&fs::read(path)?)
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 || self.blocks.is_empty() || self.blocks.len() > 256 {
            return Err("Block registry requires version 1 and 1..256 blocks".into());
        }
        let mut seen = HashSet::new();
        for b in &self.blocks {
            if let Some(binding) = &b.minecraft
                && (!["vanilla", "vanilla_1.17.0", "vanilla_1.21.10"]
                    .contains(&binding.pack.as_str())
                    || binding.block.is_empty()
                    || !binding
                        .block
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'))
            {
                return Err("Invalid Minecraft texture binding".into());
            }
            if b.id.is_empty()
                || b.id.len() > 48
                || !b
                    .id
                    .bytes()
                    .all(|v| v.is_ascii_lowercase() || v.is_ascii_digit() || b"_-".contains(&v))
                || !seen.insert(&b.id)
                || b.label.is_empty()
                || b.label.chars().count() > 32
                || b.label.chars().any(char::is_control)
            {
                return Err("Block identifiers must be unique lowercase names; labels must contain 1..32 printable characters".into());
            }
        }
        crate::engine_geometry::Library::compile(self)?;
        Ok(())
    }
    pub fn find(&self, name: &str) -> Result<BlockId> {
        self.blocks
            .iter()
            .position(|b| b.id == name)
            .map(|i| i as BlockId)
            .ok_or_else(|| format!("Block definition missing: {name}").into())
    }
    pub fn get(&self, id: BlockId) -> &Block {
        &self.blocks[usize::from(id)]
    }
    pub fn shapes(&self) -> Result<mcbe_core::raycast::Shapes> {
        let rows:Vec<_>=self.blocks.iter().map(|b|serde_json::json!({"id":format!("blockwork:{}",b.id),"states":{},"boxes":b.geometry.as_ref().map_or_else(||vec![crate::engine_geometry::UNIT],|g|g.boxes.clone()),"passable":false})).collect();
        Ok(mcbe_core::raycast::Shapes::read(&serde_json::json!(rows))?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn many_owned_blocks_and_strict_definition_validation() {
        let mut r = Registry::built_in();
        for i in 0..100 {
            r.blocks.push(Block {
                geometry: None,
                id: format!("custom_{i}"),
                label: format!("Custom {i}"),
                color: [20, 60, 100],
                pattern: Pattern::Checker,
                side_color: None,
                minecraft: None,
            });
        }
        let bytes = serde_json::to_vec(&r).unwrap();
        let copy = Registry::parse(&bytes).unwrap();
        assert_eq!(
            usize::from(copy.find("custom_99").unwrap()),
            copy.blocks.len() - 1
        );
        assert!(copy.shapes().is_ok());
        r.blocks.push(r.blocks[0].clone());
        assert!(r.validate().is_err());
        assert!(Registry::parse(br#"{"version":1,"blocks":[{"id":"bad","label":"Bad","color":[1,2,3],"pattern":"unknown"}]}"#).is_err());
    }
}
