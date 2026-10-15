//! One bounded loader/compiler for editable and embedded content packs.
use crate::engine::blocks::definition::{
    BlockDefinition as Definition, BlockStateDefinition as Variant, empty_object,
};
use crate::engine::blocks::templates::BlockTemplate as Template;
use crate::engine::blocks::{
    components::{BlockComponents, PlacementComponent},
    state::StateIdentity,
    templates,
};
use crate::{
    engine_blocks::{Block, Placement, Registry},
    engine_crafting::Recipes,
    scene::Result,
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Component, Path},
};
include!(concat!(env!("OUT_DIR"), "/builtin_content.rs"));

#[derive(Clone)]
pub struct ContentPack {
    pub asset_root: String,
    pub registry: Registry,
    pub recipes: Recipes,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    id: String,
    asset_root: String,
    blocks: Vec<String>,
    recipes: String,
    #[serde(default)]
    state_order: Vec<String>,
}
fn safe_path(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 240
        || name.contains(['\\', ':'])
        || Path::new(name)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Pack paths must be relative and contain no parent traversal".into());
    }
    Ok(())
}
struct Source<F> {
    read: F,
    files: BTreeMap<String, Vec<u8>>,
    bytes: usize,
}
impl<F: FnMut(&str) -> Result<Vec<u8>>> Source<F> {
    fn get(&mut self, path: &str) -> Result<Vec<u8>> {
        safe_path(path)?;
        if let Some(bytes) = self.files.get(path) {
            return Ok(bytes.clone());
        }
        if self.files.len() >= 1024 {
            return Err("Pack exceeds 1024 files".into());
        }
        let bytes = (self.read)(path)?;
        if bytes.len() > 256 * 1024 || self.bytes + bytes.len() > 4 * 1024 * 1024 {
            return Err("Pack exceeds file or aggregate byte budget".into());
        }
        self.bytes += bytes.len();
        self.files.insert(path.into(), bytes.clone());
        Ok(bytes)
    }
    fn inherited(&mut self, path: &str, chain: &mut Vec<String>) -> Result<Value> {
        if chain.len() >= 16 || chain.iter().any(|p| p == path) {
            return Err("Template inheritance cycle or depth exceeds 16".into());
        }
        chain.push(path.into());
        let t: Template = serde_json::from_slice(&self.get(path)?)?;
        let mut value = if let Some(parent) = t.extends {
            self.inherited(&parent, chain)?
        } else {
            empty_object()
        };
        templates::merge(&mut value, t.components);
        chain.pop();
        Ok(value)
    }
}
fn compile<F: FnMut(&str) -> Result<Vec<u8>>>(read: F) -> Result<ContentPack> {
    let mut source = Source {
        read,
        files: BTreeMap::new(),
        bytes: 0,
    };
    let manifest: Manifest = serde_json::from_slice(&source.get("pack.json")?)?;
    if manifest.version != 1
        || !crate::engine::blocks::state::identifier(&manifest.id)
        || manifest.blocks.is_empty()
        || manifest.blocks.len() > 256
    {
        return Err("Invalid pack version, ID or block count".into());
    }
    safe_path(&manifest.asset_root)?;
    let mut blocks = Vec::new();
    let mut names = HashSet::new();
    for path in &manifest.blocks {
        let d: Definition = serde_json::from_slice(&source.get(path)?)?;
        if !names.insert(d.id.clone()) {
            return Err("Duplicate block definition".into());
        }
        let mut base = if let Some(parent) = d.extends {
            source.inherited(&parent, &mut Vec::new())?
        } else {
            empty_object()
        };
        templates::merge(&mut base, d.components);
        let variants = if d.states.is_empty() {
            vec![Variant {
                id: d.id.clone(),
                label: None,
                hidden: d.hidden,
                properties: BTreeMap::new(),
                components: empty_object(),
            }]
        } else {
            d.states
        };
        if variants.len() > 256 || blocks.len() + variants.len() > 256 {
            return Err("Pack exceeds 256 compiled states".into());
        }
        let mut group = Vec::new();
        let mut policy = None;
        for v in variants {
            let mut value = base.clone();
            templates::merge(&mut value, v.components);
            let c: BlockComponents = serde_json::from_value(value)?;
            if policy.is_some_and(|p| p != c.placement) {
                return Err("States of one block must share a placement policy".into());
            }
            policy = Some(c.placement);
            let geometry: crate::engine_geometry::Geometry =
                serde_json::from_slice(&source.get(&c.visual.model)?)?;
            crate::engine_geometry::validate_boxes(&geometry.boxes, false)?;
            let state = StateIdentity {
                block: d.id.clone(),
                properties: v.properties,
            };
            state.validate()?;
            if group
                .iter()
                .any(|b: &Block| b.state.as_ref() == Some(&state))
            {
                return Err("Duplicate block state properties".into());
            }
            let textures = c
                .visual
                .textures
                .map(|files| files.map(|p| format!("{}/{p}", manifest.asset_root)));
            let collision = c.collision.compile()?;
            let selection = c.selection.compile()?;
            group.push(Block {
                behavior: c.behavior,
                interaction: c.interaction,
                id: v.id,
                label: v.label.unwrap_or_else(|| d.label.clone()),
                hidden: v.hidden,
                state: Some(state),
                signal: c.signal,
                selection,
                collision,
                render_layer: c.occlusion.layer,
                cull_group: c.occlusion.cull_group,
                color: c.visual.color,
                pattern: c.visual.pattern,
                side_color: c.visual.side_color,
                minecraft: c.visual.local,
                face_textures: c.visual.faces,
                texture_files: textures,
                geometry: if geometry.boxes == [crate::engine_geometry::UNIT] {
                    None
                } else {
                    Some(geometry)
                },
                placement: None,
            });
        }
        let lookup = |half: &str, facing: Option<&str>| -> Result<String> {
            group
                .iter()
                .find(|b| {
                    let p = &b.state.as_ref().unwrap().properties;
                    p.get("half").is_some_and(|v| v == half)
                        && facing.is_none_or(|f| p.get("facing").is_some_and(|v| v == f))
                })
                .map(|b| b.id.clone())
                .ok_or_else(|| "Placement policy is missing a required state".into())
        };
        let placement = match policy.unwrap() {
            PlacementComponent::Standard => None,
            PlacementComponent::Slab => Some(Placement::Slab {
                lower: lookup("lower", None)?,
                upper: lookup("upper", None)?,
                double: lookup("double", None)?,
            }),
            PlacementComponent::Stairs => {
                let directions = ["north", "east", "south", "west"];
                let lower = directions
                    .map(|f| lookup("lower", Some(f)))
                    .into_iter()
                    .collect::<Result<Vec<_>>>()?;
                let upper = directions
                    .map(|f| lookup("upper", Some(f)))
                    .into_iter()
                    .collect::<Result<Vec<_>>>()?;
                Some(Placement::Stairs {
                    lower: lower.try_into().unwrap(),
                    upper: upper.try_into().unwrap(),
                })
            }
        };
        for b in &mut group {
            b.placement = placement.clone();
        }
        blocks.extend(group);
    }
    let mut ordered = Vec::new();
    for id in &manifest.state_order {
        let index = blocks
            .iter()
            .position(|b| &b.id == id)
            .ok_or("Unknown or repeated state_order entry")?;
        ordered.push(blocks.remove(index));
    }
    ordered.extend(blocks);
    let registry = Registry {
        version: 1,
        blocks: ordered,
    };
    registry.validate()?;
    let recipes = Recipes::parse(&source.get(&manifest.recipes)?)?;
    recipes.validate_registry(&registry)?;
    // Reject files edited during this load rather than publish a mixed snapshot.
    for (path, bytes) in &source.files {
        if (source.read)(path)? != *bytes {
            return Err("Pack changed during loading; retry".into());
        }
    }
    Ok(ContentPack {
        asset_root: manifest.asset_root,
        registry,
        recipes,
    })
}
pub fn built_in() -> Result<ContentPack> {
    compile(|path| {
        EMBEDDED
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, b)| b.to_vec())
            .ok_or_else(|| format!("Missing embedded content: {path}").into())
    })
}
pub fn read(path: &Path) -> Result<ContentPack> {
    let root = if path.is_dir() {
        path
    } else {
        path.parent().ok_or("Pack directory missing")?
    }
    .canonicalize()?;
    compile(|name| {
        safe_path(name)?;
        let p = root.join(name).canonicalize()?;
        if !p.starts_with(&root) || !p.is_file() {
            return Err("Content file outside pack directory".into());
        }
        if fs::metadata(&p)?.len() > 256 * 1024 {
            return Err("Content file exceeds 256 KiB".into());
        }
        Ok(fs::read(p)?)
    })
}
/// Live pack first, legacy flat registries remain readable for old projects.
pub fn entry(home: &Path) -> std::path::PathBuf {
    let pack = home.join("content/base/pack.json");
    if pack.is_file() {
        pack
    } else {
        home.join("assets/blocks.json")
    }
}
#[cfg(test)]
pub(crate) fn signal_example() -> Result<ContentPack> {
    compile(|path| {
        let bytes = EMBEDDED
            .iter()
            .find(|(p, _)| *p == path)
            .ok_or("Missing example content")?
            .1;
        if path != "pack.json" {
            return Ok(bytes.to_vec());
        }
        let mut v: Value = serde_json::from_slice(bytes)?;
        let list = v["blocks"].as_array_mut().unwrap();
        list.push("examples/power_source.json".into());
        list.push("examples/wire.json".into());
        Ok(serde_json::to_vec(&v)?)
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn modified(path: &str, value: Value) -> Result<ContentPack> {
        compile(|name| {
            if name == path {
                Ok(serde_json::to_vec(&value)?)
            } else {
                EMBEDDED
                    .iter()
                    .find(|(p, _)| *p == name)
                    .map(|(_, b)| b.to_vec())
                    .ok_or_else(|| "Missing file".into())
            }
        })
    }
    fn value(path: &str) -> Value {
        serde_json::from_slice(EMBEDDED.iter().find(|(p, _)| *p == path).unwrap().1).unwrap()
    }
    #[test]
    fn migration_preserves_all_legacy_states_materials_and_shapes() {
        let old = Registry::parse(include_bytes!(
            "../../verification/fixtures/legacy-blocks-v1.json"
        ))
        .unwrap();
        let new = built_in().unwrap().registry;
        assert_eq!(old.blocks.len(), 36);
        assert_eq!(new.blocks.len(), 37);
        assert_eq!(new.find("furnace").unwrap(), 36);
        assert_eq!(
            new.get(16).interaction,
            Some(crate::engine::blocks::components::Interaction::CraftingTable)
        );
        for (a, b) in old.blocks.iter().zip(&new.blocks) {
            let mut a = serde_json::to_value(a).unwrap();
            let mut b = serde_json::to_value(b).unwrap();
            // Full cubes canonicalize to the implicit cube model.
            for v in [&mut a, &mut b] {
                v.as_object_mut().unwrap().remove("state");
                v.as_object_mut().unwrap().remove("interaction");
                if v.get("geometry") == Some(&serde_json::json!({"boxes":[[0.,0.,0.,1.,1.,1.]]})) {
                    v.as_object_mut().unwrap().remove("geometry");
                }
            }
            assert_eq!(a, b, "state {}", a["id"]);
        }
        let p = BTreeMap::from([("half".into(), "double".into())]);
        assert_eq!(
            new.resolve_state("oak_slab", &p).unwrap(),
            old.find("oak_slab_double").unwrap()
        );
        assert!(new.resolve_state("oak_slab", &BTreeMap::new()).is_err());
    }
    #[test]
    fn editable_pack_matches_embedded_pack_and_reload_failure_is_atomic() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pack = read(&root.join("content/base/pack.json")).unwrap();
        assert_eq!(
            serde_json::to_value(&pack.registry).unwrap(),
            serde_json::to_value(built_in().unwrap().registry).unwrap()
        );
        let mut c = crate::engine::generate(360036).unwrap();
        crate::engine::reload_definitions(&mut c, &root.join("content/base/pack.json")).unwrap();
        let before = serde_json::to_value(&c.scene.engine.as_ref().unwrap().registry).unwrap();
        assert!(
            crate::engine::reload_definitions(&mut c, &root.join("content/missing/pack.json"))
                .is_err()
        );
        assert_eq!(
            before,
            serde_json::to_value(&c.scene.engine.as_ref().unwrap().registry).unwrap()
        );
    }
    #[test]
    fn rejects_cycles_typos_missing_models_and_traversal() {
        let mut solid = value("templates/solid.json");
        solid["extends"] = "templates/solid.json".into();
        assert!(modified("templates/solid.json", solid).is_err());
        let mut block = value("blocks/meadow.json");
        block["components"]["colision"] = serde_json::json!({});
        assert!(modified("blocks/meadow.json", block).is_err());
        let mut block = value("blocks/meadow.json");
        block["components"]["visual"]["model"] = "models/missing.json".into();
        assert!(modified("blocks/meadow.json", block).is_err());
        assert!(safe_path("../outside.json").is_err());
        assert!(safe_path("C:/secret").is_err());
        assert!(safe_path("models/cube.json").is_ok());
    }
    #[test]
    fn collision_and_selection_are_independent_components() {
        let mut block = value("blocks/meadow.json");
        block["components"]["collision"] = serde_json::json!({"shape":"none"});
        let pack = modified("blocks/meadow.json", block.clone()).unwrap();
        let r = pack.registry;
        let id = r.find("meadow").unwrap();
        assert!(
            crate::engine_geometry::Library::compile(&r)
                .unwrap()
                .collision(id)
                .is_empty()
        );
        assert!(r.get(id).selection.is_none());
        block["components"]["selection"] = serde_json::json!({"shape":"none"});
        let r = modified("blocks/meadow.json", block).unwrap().registry;
        assert_eq!(r.get(id).selection, Some(vec![]));
        assert!(r.shapes().is_ok());
    }
}
