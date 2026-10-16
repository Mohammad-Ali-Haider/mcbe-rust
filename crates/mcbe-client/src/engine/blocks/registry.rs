//! Engine-owned block identifiers, independent of the research cube enum.
use super::definition::*;
use crate::scene::Result;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, path::Path};
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub version: u32,
    pub blocks: Vec<Block>,
}
impl Registry {
    /// Add newly shipped built-ins to older engine saves. Existing custom models,
    /// textures and collisions remain authoritative. Only canonical models gain
    /// the new placement policy, so an edited model cannot acquire a wrong rule.
    pub fn with_new_builtins(&self) -> Result<Option<Self>> {
        let built = Self::built_in();
        let mut next = self.clone();
        let mut changed = false;
        for b in &built.blocks {
            if next.find(&b.id).is_err() {
                if next.blocks.len() == 256 {
                    return Ok(None);
                }
                let mut added = b.clone();
                added.placement = None;
                next.blocks.push(added);
                changed = true;
            }
        }
        for b in &built.blocks {
            let i = next.find(&b.id)? as usize;
            if next.blocks[i].interaction.is_none() && b.interaction.is_some() {
                next.blocks[i].interaction = b.interaction;
                changed = true;
            }
            if let Some(rule) = &b.placement {
                let refs = rule.state_names();
                let canonical = refs.iter().all(|name| {
                    let actual = next.get(next.find(name).unwrap());
                    let expected = built.get(built.find(name).unwrap());
                    actual.geometry.as_ref().map(|g| &g.boxes)
                        == expected.geometry.as_ref().map(|g| &g.boxes)
                });
                let index = next.find(&b.id)? as usize;
                if canonical
                    && next.blocks[index].behavior == super::components::BehaviorComponent::Passive
                    && b.behavior == super::components::BehaviorComponent::Stairs
                {
                    next.blocks[index].behavior = b.behavior;
                    if next.blocks[index]
                        .state
                        .as_ref()
                        .is_some_and(|s| s.properties.keys().all(|k| k == "half" || k == "facing"))
                    {
                        next.blocks[index].state = b.state.clone();
                    }
                    changed = true;
                }
                if canonical
                    && next.blocks[index].placement.is_none()
                    && matches!(rule, Placement::Facing { .. })
                {
                    let actual = &mut next.blocks[index];
                    if actual
                        .state
                        .as_ref()
                        .is_some_and(|s| s.properties.is_empty())
                    {
                        actual.state = b.state.clone();
                    }
                    let local_matches = matches!((&actual.minecraft,&b.minecraft),(Some(a),Some(b)) if a.pack==b.pack && a.block==b.block && a.face==b.face);
                    if local_matches
                        && actual.face_textures.is_none()
                        && actual.texture_files.is_none()
                    {
                        actual.face_textures = b.face_textures.clone();
                    }
                }
                if canonical && next.blocks[index].placement.is_none() {
                    next.blocks[index].placement = Some(rule.clone());
                    changed = true;
                }
            }
        }
        if changed {
            next.validate()?;
            Ok(Some(next))
        } else {
            Ok(None)
        }
    }
    pub fn built_in() -> Self {
        crate::engine::content::loader::built_in()
            .expect("Built-in unified content")
            .registry
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
        if path.is_dir() || path.file_name().is_some_and(|n| n == "pack.json") {
            return Ok(crate::engine::content::loader::read(path)?.registry);
        }
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
        for (index, b) in self.blocks.iter().enumerate() {
            b.material.validate()?;
            if let Some(connections) = &b.connections {
                connections.validate()?;
            }
            if let Some(c) = b.container
                && (!self.blocks.iter().any(|b| b.fluid == Some(c.kind))
                    || c.capacity == 0
                    || c.capacity > crate::engine::fluids::FULL
                    || b.fluid.is_some()
                    || b.interaction.is_some()
                    || b.placement.is_some()
                    || b.collision.as_ref().is_none_or(|v| !v.is_empty())
                    || b.selection.as_ref().is_none_or(|v| !v.is_empty()))
            {
                return Err(
                    "Fluid containers require capacity 1..1024 and non-world collision/selection"
                        .into(),
                );
            }
            if let Some(state) = &b.state
                && self.resolve_state(&state.block, &state.properties)? as usize != index
            {
                return Err("Duplicate compiled state identity".into());
            }
            if b.texture_files.as_ref().is_some_and(|files| {
                files
                    .iter()
                    .any(|n| !crate::engine_materials::valid_path(n))
            }) {
                return Err("Invalid relative PNG texture path".into());
            }
            for binding in b.minecraft.iter().chain(b.face_textures.iter().flatten()) {
                if !["vanilla", "vanilla_1.17.0", "vanilla_1.21.10"]
                    .contains(&binding.pack.as_str())
                    || binding.face.as_deref().is_some_and(|face| {
                        !["up", "down", "north", "south", "east", "west"].contains(&face)
                    })
                    || binding.block.is_empty()
                    || !binding
                        .block
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                {
                    return Err("Invalid Minecraft texture binding".into());
                }
            }
            if b.cull_group.as_ref().is_some_and(|g| {
                g.is_empty()
                    || g.len() > 48
                    || !g.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
            }) {
                return Err("Cull groups require 1..48 identifier characters".into());
            }
            if let Some(boxes) = &b.selection {
                crate::engine_geometry::validate_boxes(boxes, true)?;
            }
            if let Some(signal) = &b.signal {
                signal.validate()?;
            }
            if let Some(state) = &b.state {
                state.validate()?;
            }
            if let Some(boxes) = &b.collision {
                crate::engine_geometry::validate_boxes(boxes, true)?;
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
        for id in 0..self.blocks.len() {
            let id = id as BlockId;
            super::behavior::placement::get(self, id).validate(self, id)?;
            super::behavior::get(self, id).validate(self, id)?;
        }
        crate::engine_geometry::Library::compile(self)?;
        Ok(())
    }
    pub fn resolve_state(
        &self,
        block: &str,
        properties: &std::collections::BTreeMap<String, String>,
    ) -> Result<BlockId> {
        self.blocks
            .iter()
            .position(|b| {
                b.state
                    .as_ref()
                    .is_some_and(|s| s.block == block && &s.properties == properties)
            })
            .map(|i| i as BlockId)
            .ok_or_else(|| format!("Unknown state for {block}: {properties:?}").into())
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
        let rows:Vec<_>=self.blocks.iter().map(|b|serde_json::json!({"id":format!("blockwork:{}",b.id),"states":{},"boxes":b.selection.clone().unwrap_or_else(|| b.geometry.as_ref().map_or_else(||vec![crate::engine_geometry::UNIT],|g|g.boxes.clone())),"passable":false})).collect();
        Ok(mcbe_core::raycast::Shapes::read(&serde_json::json!(rows))?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn older_registries_gain_states_without_replacing_user_models() {
        let mut old = Registry::built_in();
        old.blocks.truncate(26);
        for b in &mut old.blocks {
            b.placement = None;
        }
        let mut upgraded = old.with_new_builtins().unwrap().unwrap();
        assert_eq!(upgraded.blocks.len(), Registry::built_in().blocks.len());
        assert!(upgraded.with_new_builtins().unwrap().is_none());
        assert!(
            upgraded
                .get(upgraded.find("oak_slab").unwrap())
                .placement
                .is_some()
        );
        let slab = old.find("oak_slab").unwrap() as usize;
        old.blocks[slab].geometry.as_mut().unwrap().boxes = vec![crate::engine_geometry::UNIT];
        upgraded = old.with_new_builtins().unwrap().unwrap();
        assert!(upgraded.blocks[slab].placement.is_none());
        assert_eq!(
            upgraded.blocks[slab].geometry.as_ref().unwrap().boxes,
            vec![crate::engine_geometry::UNIT]
        );
        let mut invalid = Registry::built_in();
        if let Some(Placement::Slab { upper, .. }) = &mut invalid.blocks[19].placement {
            *upper = "missing".into();
        }
        assert!(invalid.validate().is_err());
    }
    #[test]
    fn canonical_legacy_furnaces_gain_facing_without_overwriting_owned_textures() {
        for owned in [false, true] {
            let mut old = Registry::built_in();
            old.blocks.retain(|b| !b.id.starts_with("furnace_"));
            let id = old.find("furnace").unwrap() as usize;
            old.blocks[id].placement = None;
            old.blocks[id].state.as_mut().unwrap().properties.clear();
            old.blocks[id].face_textures = None;
            if owned {
                old.blocks[id].texture_files =
                    Some(std::array::from_fn(|_| "assets/own.png".into()));
            }
            let next = old.with_new_builtins().unwrap().unwrap();
            let b = next.get(next.find("furnace").unwrap());
            assert_eq!(b.state.as_ref().unwrap().properties["facing"], "north");
            assert!(matches!(b.placement, Some(Placement::Facing { .. })));
            if owned {
                assert!(b.face_textures.is_none());
                assert_eq!(b.texture_files.as_ref().unwrap()[0], "assets/own.png");
            } else {
                assert_eq!(
                    b.face_textures.as_ref().unwrap()[2].face.as_deref(),
                    Some("south")
                );
            }
        }
    }
    #[test]
    fn many_owned_blocks_and_strict_definition_validation() {
        let mut r = Registry::built_in();
        for i in 0..100 {
            r.blocks.push(Block {
                connections: None,
                material: Default::default(),
                container: None,
                texture_tint: None,
                behavior: Default::default(),
                interaction: None,
                fluid: None,
                state: None,
                selection: None,
                signal: None,
                texture_files: None,
                placement: None,
                collision: None,
                render_layer: RenderLayer::Opaque,
                cull_group: None,
                hidden: false,
                face_textures: None,
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
