//! Validated data-authored smelting and fuels, compiled to numeric IDs once.
use crate::{
    engine_blocks::{BlockId, Registry},
    scene::Result,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Smelt {
    pub input: String,
    pub result: String,
    pub count: u8,
    pub cook_ms: u32,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fuel {
    pub item: String,
    pub burn_ms: u32,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definitions {
    pub version: u32,
    pub smelting: Vec<Smelt>,
    pub fuels: Vec<Fuel>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Recipe {
    pub result: BlockId,
    pub count: u8,
    pub cook_ms: u32,
}
#[derive(Clone)]
pub struct Recipes {
    pub definitions: Definitions,
    smelting: Vec<Option<Recipe>>,
    fuels: Vec<u32>,
}
impl Definitions {
    pub fn built_in() -> Self {
        serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../content/base/recipes/smelting.json"
        )))
        .expect("Built-in smelting")
    }
    pub fn read_home(home: &std::path::Path) -> Result<Self> {
        let p = home.join("content/base/recipes/smelting.json");
        if !p.is_file() {
            return Ok(Self::built_in());
        }
        if std::fs::metadata(&p)?.len() > 256 * 1024 {
            return Err("Smelting JSON exceeds 256 KiB".into());
        }
        Ok(serde_json::from_slice(&std::fs::read(p)?)?)
    }
    pub fn available(mut self, r: &Registry) -> Self {
        self.smelting
            .retain(|v| r.find(&v.input).is_ok() && r.find(&v.result).is_ok());
        self.fuels.retain(|v| r.find(&v.item).is_ok());
        self
    }
    pub fn compile(self, r: &Registry) -> Result<Recipes> {
        if self.version != 1 || self.smelting.len() > 256 || self.fuels.len() > 256 {
            return Err("Smelting version/count invalid".into());
        }
        let mut smelting = vec![None; r.blocks.len()];
        let mut fuels = vec![0; r.blocks.len()];
        for v in &self.smelting {
            let i = r.find(&v.input)? as usize;
            let result = r.find(&v.result)?;
            if smelting[i].is_some()
                || v.count == 0
                || v.count > 64
                || !(100..=3_600_000).contains(&v.cook_ms)
            {
                return Err("Duplicate or invalid smelting recipe".into());
            }
            smelting[i] = Some(Recipe {
                result,
                count: v.count,
                cook_ms: v.cook_ms,
            });
        }
        for v in &self.fuels {
            let i = r.find(&v.item)? as usize;
            if fuels[i] != 0 || !(100..=3_600_000).contains(&v.burn_ms) {
                return Err("Duplicate or invalid furnace fuel".into());
            }
            fuels[i] = v.burn_ms;
        }
        Ok(Recipes {
            definitions: self,
            smelting,
            fuels,
        })
    }
}
impl Recipes {
    pub fn get(&self, id: BlockId) -> Option<Recipe> {
        self.smelting.get(id as usize).copied().flatten()
    }
    pub fn fuel(&self, id: BlockId) -> u32 {
        self.fuels.get(id as usize).copied().unwrap_or(0)
    }
}
