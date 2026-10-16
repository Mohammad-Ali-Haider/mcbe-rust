use crate::{
    engine::{
        self,
        world::{
            stream,
            terrain::{Settings, Terrain},
        },
    },
    engine_blocks::Registry,
    scene::Result,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Component, Path},
};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    #[serde(default)]
    pub environment: Environment,
    pub version: u32,
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub seed_offset: u64,
    pub terrain: Settings,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Environment {
    pub fog_color: [f32; 3],
}
impl Default for Environment {
    fn default() -> Self {
        Self {
            fog_color: [0.62, 0.77, 0.89],
        }
    }
}
impl Definition {
    pub fn seed(&self, base: u64) -> u64 {
        // Stable string hash: neither registry order nor platform HashMap salts matter.
        let id = self.id.bytes().fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        });
        base ^ self.seed_offset ^ id
    }
    pub fn config(&self, radius: i32) -> stream::Config {
        stream::Config {
            version: 2,
            terrain: Some(self.terrain.clone()),
            radius,
            exhibits: Vec::new(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definitions {
    pub version: u32,
    pub start: String,
    pub dimensions: Vec<Definition>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    version: u32,
    start: String,
    dimensions: Vec<String>,
}
fn safe(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 160
        || name.contains(['\\', ':'])
        || !name.ends_with(".json")
        || Path::new(name)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Dimension files must be relative JSON paths without parent traversal".into());
    }
    Ok(())
}
impl Definitions {
    fn read(mut get: impl FnMut(&str) -> Result<Vec<u8>>) -> Result<Self> {
        let bytes = get("dimensions.json")?;
        if bytes.len() > 256 * 1024 {
            return Err("Dimension index exceeds 256 KiB".into());
        }
        let index: Index = serde_json::from_slice(&bytes)?;
        if index.version != 1 || index.dimensions.is_empty() || index.dimensions.len() > 256 {
            return Err("Invalid dimension index version/count".into());
        }
        let mut paths = BTreeSet::new();
        let mut dimensions = Vec::new();
        let mut total = bytes.len();
        for path in &index.dimensions {
            safe(path)?;
            if !paths.insert(path) {
                return Err("Duplicate dimension file".into());
            }
            let bytes = get(path)?;
            total += bytes.len();
            if bytes.len() > 256 * 1024 || total > 4 * 1024 * 1024 {
                return Err("Dimension definitions exceed file or aggregate byte budget".into());
            }
            dimensions.push(serde_json::from_slice(&bytes)?);
        }
        Ok(Self {
            version: 1,
            start: index.start,
            dimensions,
        })
    }
    pub fn built_in() -> Self {
        Self::read(|name| {
            crate::engine::content::loader::embedded_file(&format!("world/{name}"))
                .map(|v| v.to_vec())
                .ok_or_else(|| "Embedded dimension file missing".into())
        })
        .expect("Built-in dimensions")
    }
    pub fn read_home(home: &Path) -> Result<Self> {
        let root = home.join("content/base/world");
        if !root.join("dimensions.json").is_file() {
            return Ok(Self::built_in());
        }
        let root = root.canonicalize()?;
        Self::read(|name| {
            safe(name)?;
            let path = root.join(name).canonicalize()?;
            if !path.starts_with(&root)
                || !path.is_file()
                || std::fs::metadata(&path)?.len() > 256 * 1024
            {
                return Err("Dimension file escapes content root or exceeds budget".into());
            }
            Ok(std::fs::read(path)?)
        })
    }
    pub fn validate(&self, r: &Registry) -> Result<()> {
        if self.version != 1 || self.dimensions.is_empty() || self.dimensions.len() > 256 {
            return Err("Invalid dimension registry".into());
        }
        let mut names = BTreeSet::new();
        for d in &self.dimensions {
            if d.version != 1
                || !engine::blocks::state::identifier(&d.id)
                || !names.insert(&d.id)
                || d.label.is_empty()
                || d.label.len() > 64
            {
                return Err("Invalid or duplicate dimension definition".into());
            }
            if d.environment
                .fog_color
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            {
                return Err("Invalid dimension fog color".into());
            }
            Terrain::new(d.seed(0), r, d.terrain.clone())?;
        }
        self.get(&self.start)?;
        Ok(())
    }
    pub fn compiled(mut self, r: &Registry) -> Result<Self> {
        self.validate(r)?;
        for d in &mut self.dimensions {
            d.terrain = Terrain::new(d.seed(0), r, d.terrain.clone())?.settings;
        }
        Ok(self)
    }
    pub fn get(&self, id: &str) -> Result<&Definition> {
        self.dimensions
            .iter()
            .find(|d| d.id == id)
            .ok_or_else(|| format!("Dimension is not registered: {id}").into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editable_examples_register_and_another_dimension_needs_no_rust_case() {
        let r = Registry::built_in();
        let mut d = Definitions::built_in();
        d.validate(&r).unwrap();
        assert_eq!(d.dimensions.len(), 3);
        for id in ["surface", "cavern", "islands"] {
            let def = d.get(id).unwrap();
            stream::Generator::new(def.seed(9), &r, def.config(2)).unwrap();
        }
        let mut custom = d.get("islands").unwrap().clone();
        custom.id = "sky_gardens".into();
        custom.label = "Sky Gardens".into();
        custom.terrain.generator.as_mut().unwrap().options =
            serde_json::json!({"altitude":224,"spacing":160,"radius":56});
        d.dimensions.push(custom);
        d.validate(&r).unwrap();
        assert_ne!(
            d.get("islands").unwrap().seed(90),
            d.get("sky_gardens").unwrap().seed(90)
        );
        assert_eq!(
            d.get("sky_gardens").unwrap().seed(90),
            d.get("sky_gardens").unwrap().seed(90)
        );
        d.dimensions.push(d.dimensions[0].clone());
        assert!(d.validate(&r).is_err());
    }
    #[test]
    fn compiled_snapshots_expand_defaults_and_reject_invalid_environment() {
        let r = Registry::built_in();
        let mut d = Definitions::built_in();
        d.dimensions[2].terrain.generator.as_mut().unwrap().options =
            serde_json::json!({"altitude":224});
        let snapshot = d.clone().compiled(&r).unwrap();
        let options = &snapshot.dimensions[2]
            .terrain
            .generator
            .as_ref()
            .unwrap()
            .options;
        assert_eq!(options["version"], 1);
        assert_eq!(options["altitude"], 224);
        assert_eq!(options["spacing"], 192);
        for color in [[-0.1, 0., 0.], [1.1, 0., 0.], [f32::NAN, 0., 0.]] {
            d.dimensions[0].environment.fog_color = color;
            assert!(d.validate(&r).is_err());
        }
    }
    #[test]
    fn malformed_indices_paths_and_missing_start_are_rejected() {
        for path in [
            "../outside.json",
            "C:/outside.json",
            "/outside.json",
            "dimensions\\bad.json",
            "dimensions.txt",
        ] {
            assert!(safe(path).is_err());
        }
        let r = Registry::built_in();
        let mut d = Definitions::built_in();
        d.start = "missing".into();
        assert!(d.validate(&r).is_err());
        let index =
            serde_json::json!({"version":1,"start":"surface","dimensions":["../outside.json"]});
        assert!(Definitions::read(|_| Ok(serde_json::to_vec(&index).unwrap())).is_err());
    }
}
