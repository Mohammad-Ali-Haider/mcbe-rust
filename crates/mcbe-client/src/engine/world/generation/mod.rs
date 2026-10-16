//! Compiled generation interface. JSON selects a factory; voxel loops reuse its data.
pub mod cavern;
pub mod islands;
pub mod surface;
use crate::{engine, scene::Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WaterBiome {
    #[default]
    None,
    Ocean,
    River,
}
/// Solid floor ends at `top`; finite islands start at `bottom`. A cavern can
/// additionally contain a roof from `ceiling` up to `roof_top` (exclusive).
#[derive(Clone, Copy, Debug)]
pub struct Column {
    pub top: i32,
    pub bottom: Option<i32>,
    pub ceiling: Option<i32>,
    pub roof_top: Option<i32>,
    pub present: bool,
    pub coverage: f64,
    pub water: Option<i32>,
    pub wet: WaterBiome,
}
impl Column {
    pub fn surface(top: i32) -> Self {
        Self {
            top,
            bottom: None,
            ceiling: None,
            roof_top: None,
            present: true,
            coverage: 1.,
            water: None,
            wet: WaterBiome::None,
        }
    }
}
/// Implement and register another factory to add a compiled Rust generator.
/// Columns must be deterministic, within +/-1,000,000 Y, with ordered intervals.
pub trait Generator: Send + Sync {
    fn canonical_options(&self) -> Option<serde_json::Value> {
        None
    }
    fn column(&self, noise: Noise, sea: i32, x: i32, z: i32) -> Column;
    fn intersects(&self, _noise: Noise, _rect: [i32; 4]) -> bool {
        true
    }
    fn layered(&self) -> bool {
        true
    }
    fn ravines(&self) -> bool {
        false
    }
    /// Conservative occupied Y windows. None retains legacy adaptive residency.
    fn residency(&self) -> Option<[(i32, i32); 2]> {
        None
    }
}
pub type Factory = fn(&serde_json::Value) -> Result<Arc<dyn Generator>>;
pub struct Registry {
    factories: BTreeMap<String, Factory>,
}
impl Registry {
    pub fn built_in() -> Self {
        let mut r = Self {
            factories: BTreeMap::new(),
        };
        r.register("surface", surface::compile)
            .expect("Surface factory");
        r.register("cavern", cavern::compile)
            .expect("Cavern factory");
        r.register("islands", islands::compile)
            .expect("Island factory");
        r
    }
    pub fn register(&mut self, id: &str, factory: Factory) -> Result<()> {
        if !engine::blocks::state::identifier(id) || self.factories.contains_key(id) {
            return Err("Invalid or duplicate generator factory".into());
        }
        self.factories.insert(id.into(), factory);
        Ok(())
    }
    pub fn compile(&self, definition: &Definition) -> Result<Arc<dyn Generator>> {
        if !definition.options.is_object()
            || serde_json::to_vec(&definition.options)?.len() > 16 * 1024
        {
            return Err("Generator options must be an object under 16 KiB".into());
        }
        self.factories
            .get(&definition.kind)
            .ok_or("Unknown generator factory")?(&definition.options)
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub kind: String,
    pub options: serde_json::Value,
    #[serde(default)]
    pub climate: Climate,
    #[serde(default)]
    pub materials: Materials,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Climate {
    pub temperature_scale: i32,
    pub moisture_scale: i32,
    pub fallback: String,
    pub ocean: Option<String>,
    pub river: Option<String>,
}
impl Default for Climate {
    fn default() -> Self {
        Self {
            temperature_scale: 1024,
            moisture_scale: 640,
            fallback: "plains".into(),
            ocean: Some("ocean".into()),
            river: Some("river".into()),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Materials {
    pub stone: String,
    pub deep: String,
    pub water: String,
    pub leaves: String,
    pub log: String,
}
impl Default for Materials {
    fn default() -> Self {
        Self {
            stone: "slate".into(),
            deep: "basalt".into(),
            water: "water".into(),
            leaves: "oak_leaves".into(),
            log: "oak_log".into(),
        }
    }
}
#[derive(Clone, Copy)]
pub struct Noise(pub u64);
impl Noise {
    pub fn hash(self, x: i32, z: i32, salt: u64) -> u64 {
        engine::noise(x, z, self.0 ^ salt)
    }
    pub fn at2(self, x: i32, z: i32, scale: i32, salt: u64) -> f64 {
        let fade = |t: f64| t * t * t * (t * (t * 6. - 15.) + 10.);
        let gx = x.div_euclid(scale);
        let gz = z.div_euclid(scale);
        let u = fade(x.rem_euclid(scale) as f64 / scale as f64);
        let v = fade(z.rem_euclid(scale) as f64 / scale as f64);
        let h = |a, b| (self.hash(a, b, salt) >> 11) as f64 / (1u64 << 53) as f64 * 2. - 1.;
        let a = h(gx, gz) * (1. - u) + h(gx + 1, gz) * u;
        let b = h(gx, gz + 1) * (1. - u) + h(gx + 1, gz + 1) * u;
        a * (1. - v) + b * v
    }
}
pub fn scale(v: i32) -> bool {
    (8..=4096).contains(&v)
}
pub fn amplitude(v: f64) -> bool {
    v.is_finite() && (0. ..=512.).contains(&v)
}
#[cfg(test)]
mod tests;
