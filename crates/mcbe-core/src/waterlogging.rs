//! Exact measured waterlogging operations, separate from fluid simulation.
use crate::{
    Result,
    blocks::{self, Palette, States},
    chunk_write,
    chunks::{self, BlockState, StateValue, Storage, Subchunk},
    heightmaps,
    leveldb::Snapshot,
};
use serde_json::Value;
use std::{collections::HashMap, fmt};
#[derive(Debug, PartialEq, Eq)]
pub enum WaterError {
    Rejected,
    Unsupported(String),
    Invalid(String),
}
impl fmt::Display for WaterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected => write!(f, "Error: Block type cannot be waterlogged."),
            Self::Unsupported(s) => write!(f, "Unmeasured waterlogging: {s}"),
            Self::Invalid(s) => write!(f, "Invalid waterlogging input: {s}"),
        }
    }
}
impl std::error::Error for WaterError {}
#[derive(Clone, Debug, PartialEq)]
pub struct StoredCell {
    pub primary: BlockState,
    pub secondary: Option<BlockState>,
}
impl StoredCell {
    pub fn waterlogged(&self) -> Result<bool> {
        self.secondary.as_ref().map_or(Ok(false), |block| {
            heightmaps::secondary_flags(block).map(|flags| flags.0)
        })
    }
}
pub fn cell_from_chunk(chunk: &Subchunk, pos: [u8; 3]) -> Result<StoredCell> {
    if !(1..=2).contains(&chunk.storages.len()) {
        return Err("Invalid cell layer count".into());
    }
    let [x, y, z] = pos;
    Ok(StoredCell {
        primary: chunk.storages[0].block(x, y, z)?.clone(),
        secondary: chunk
            .storages
            .get(1)
            .map(|s| s.block(x, y, z).cloned())
            .transpose()?,
    })
}
pub fn read_cell(snapshot: &Snapshot, pos: [i32; 3]) -> Result<StoredCell> {
    if !(-64..320).contains(&pos[1]) {
        return Err("Outside measured Overworld height".into());
    }
    let key = chunks::key(
        pos[0].div_euclid(16),
        pos[2].div_euclid(16),
        pos[1].div_euclid(16) as i8,
    );
    let data = snapshot.get(&key).ok_or("Subchunk absent")?;
    cell_from_chunk(
        &chunks::decode(data, Some(pos[1].div_euclid(16) as i8))?,
        pos.map(|v| v.rem_euclid(16) as u8),
    )
}
pub struct WaterRules {
    rules: HashMap<(String, States), bool>,
}
pub fn prepare_edits(
    snapshot: &Snapshot,
    changes: &[([i32; 3], bool)],
    columns: &[[i32; 2]],
    rules: &WaterRules,
    heights: &mut heightmaps::HeightRules,
    palette: &Palette,
) -> Result<crate::world_edit::Updates> {
    if changes.is_empty() {
        return Err("No waterlogging changes requested".into());
    }
    let mut updates: crate::world_edit::Updates = vec![];
    for (position, value) in changes {
        read_cell(snapshot, *position)?;
        let [x, y, z] = *position;
        let key = chunks::key(x.div_euclid(16), z.div_euclid(16), y.div_euclid(16) as i8);
        let slot = updates.iter().position(|(k, _)| *k == key);
        let raw = match slot {
            Some(i) => updates[i].1.as_deref(),
            None => snapshot.get(&key),
        }
        .ok_or("Missing edited subchunk")?;
        let chunk = chunks::decode(raw, Some(y.div_euclid(16) as i8))?;
        let edited = rules
            .set(
                &chunk,
                position.map(|v| v.rem_euclid(16) as u8),
                *value,
                palette,
            )
            .map_err(|e| e.to_string())?;
        let encoded = Some(chunk_write::encode(&edited)?);
        if let Some(i) = slot {
            updates[i].1 = encoded;
        } else {
            updates.push((key, encoded));
        }
    }
    let mut prospective = Snapshot {
        entries: snapshot.entries.clone(),
        ..Snapshot::default()
    };
    for (key, value) in &updates {
        prospective.entries.insert(
            key.clone(),
            crate::leveldb::Entry {
                sequence: 0,
                value: value.clone(),
            },
        );
    }
    let mut seen = std::collections::HashSet::new();
    for [cx, cz] in columns {
        if !seen.insert([*cx, *cz]) {
            return Err("Duplicate height repair column".into());
        }
        let (rebuilt, _) = heightmaps::rebuild_column(&prospective, *cx, *cz, heights, palette)?;
        updates.push((heightmaps::column_key(*cx, *cz), Some(rebuilt.encode())));
    }
    if changes
        .iter()
        .any(|(p, _)| !seen.contains(&[p[0].div_euclid(16), p[2].div_euclid(16)]))
    {
        return Err("Edited column omitted from height repair".into());
    }
    Ok(updates)
}
impl WaterRules {
    pub fn read(rows: &Value) -> Result<Self> {
        let mut rules = HashMap::new();
        for row in rows.as_array().ok_or("Missing water rules")? {
            let id = row["id"].as_str().ok_or("Missing rule id")?.to_owned();
            let mut states = blocks::states(&row["states"]).map_err(|e| e.to_string())?;
            states.sort_by(|a, b| a.0.cmp(&b.0));
            let flag = row["can_waterlog"]
                .as_bool()
                .ok_or("Waterlogging predicate must be Boolean")?;
            if rules
                .insert((id, states), flag)
                .is_some_and(|old| old != flag)
            {
                return Err("Conflicting waterlogging rule".into());
            }
        }
        if rules.is_empty() {
            return Err("Empty waterlogging rules".into());
        }
        Ok(Self { rules })
    }
    pub fn set(
        &self,
        chunk: &Subchunk,
        pos: [u8; 3],
        value: bool,
        palette: &Palette,
    ) -> std::result::Result<Subchunk, WaterError> {
        self.set_with(chunk, pos, value, |block| {
            let p = palette
                .resolve_saved(block)
                .map_err(|e| WaterError::Unsupported(e.to_string()))?;
            Ok((
                p.id().to_owned(),
                p.states()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            ))
        })
    }
    fn set_with(
        &self,
        chunk: &Subchunk,
        pos: [u8; 3],
        value: bool,
        resolve: impl FnOnce(&BlockState) -> std::result::Result<(String, States), WaterError>,
    ) -> std::result::Result<Subchunk, WaterError> {
        let cell = cell_from_chunk(chunk, pos).map_err(WaterError::Invalid)?;
        let (id, mut states) = resolve(&cell.primary)?;
        states.sort_by(|a, b| a.0.cmp(&b.0));
        let allowed = *self
            .rules
            .get(&(id.clone(), states))
            .ok_or(WaterError::Unsupported(id))?;
        if value && !allowed {
            return Err(WaterError::Rejected);
        }
        if !value && chunk.storages.len() == 1 {
            return Ok(chunk.clone());
        }
        let air = BlockState::new("minecraft:air".into(), vec![], cell.primary.version)
            .map_err(WaterError::Invalid)?;
        let mut changed = chunk.clone();
        if changed.storages.len() == 1 {
            changed.storages.push(Storage {
                bits: 1,
                indices: vec![0; 4096],
                palette: vec![air.clone()],
            });
        }
        let fluid = if value {
            BlockState::new(
                if cell.primary.name == "minecraft:air" {
                    "minecraft:flowing_water"
                } else {
                    "minecraft:water"
                }
                .into(),
                vec![("liquid_depth".into(), StateValue::Int(0))],
                cell.primary.version,
            )
            .map_err(WaterError::Invalid)?
        } else {
            air
        };
        chunk_write::replace_block(&changed, pos, fluid, 1).map_err(WaterError::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn block(name: &str) -> BlockState {
        BlockState::new(format!("minecraft:{name}"), vec![], 1).unwrap()
    }
    fn rules() -> WaterRules {
        WaterRules::read(&json!([
            {"id":"minecraft:air","states":{},"can_waterlog":true},
            {"id":"minecraft:oak_slab","states":{},"can_waterlog":true},
            {"id":"minecraft:stone","states":{},"can_waterlog":false}
        ]))
        .unwrap()
    }
    fn resolve(block: &BlockState) -> std::result::Result<(String, States), WaterError> {
        Ok((
            block.name.clone(),
            block
                .states
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        ))
    }
    #[test]
    fn primary_and_secondary_water_are_distinct() {
        let water = BlockState::new(
            "minecraft:water".into(),
            vec![("liquid_depth".into(), StateValue::Int(0))],
            1,
        )
        .unwrap();
        assert!(
            !StoredCell {
                primary: water.clone(),
                secondary: None
            }
            .waterlogged()
            .unwrap()
        );
        assert!(
            StoredCell {
                primary: block("air"),
                secondary: Some(water.clone())
            }
            .waterlogged()
            .unwrap()
        );
        let mut bad = water;
        bad.states.insert("liquid_depth".into(), StateValue::Int(3));
        assert!(
            StoredCell {
                primary: block("oak_slab"),
                secondary: Some(bad)
            }
            .waterlogged()
            .is_err()
        );
    }
    #[test]
    fn enable_clear_preserves_primary_neighbors_and_input() {
        for name in ["air", "oak_slab"] {
            let primary = block(name);
            let original = Subchunk {
                y: -1,
                storages: vec![Storage {
                    bits: 1,
                    indices: vec![0; 4096],
                    palette: vec![primary.clone()],
                }],
            };
            let wet = rules()
                .set_with(&original, [3, 4, 5], true, resolve)
                .unwrap();
            assert_eq!(original.storages.len(), 1);
            let decoded = chunks::decode(&chunk_write::encode(&wet).unwrap(), Some(-1)).unwrap();
            let cell = cell_from_chunk(&decoded, [3, 4, 5]).unwrap();
            assert_eq!(cell.primary, primary);
            assert!(cell.waterlogged().unwrap());
            assert_eq!(
                cell.secondary.unwrap().name,
                if name == "air" {
                    "minecraft:flowing_water"
                } else {
                    "minecraft:water"
                }
            );
            assert!(
                !cell_from_chunk(&wet, [3, 4, 6])
                    .unwrap()
                    .waterlogged()
                    .unwrap()
            );
            let dry = rules().set_with(&wet, [3, 4, 5], false, resolve).unwrap();
            assert!(
                !cell_from_chunk(&dry, [3, 4, 5])
                    .unwrap()
                    .waterlogged()
                    .unwrap()
            );
            assert_eq!(dry.storages[0], original.storages[0]);
        }
    }
    #[test]
    fn rejection_unknown_states_and_rule_conflicts() {
        let chunk = Subchunk {
            y: 0,
            storages: vec![Storage {
                bits: 1,
                indices: vec![0; 4096],
                palette: vec![block("stone")],
            }],
        };
        assert_eq!(
            rules()
                .set_with(&chunk, [0, 0, 0], true, resolve)
                .unwrap_err(),
            WaterError::Rejected
        );
        assert_eq!(
            rules().set_with(&chunk, [0, 0, 0], false, resolve).unwrap(),
            chunk
        );
        let mut unknown = chunk;
        unknown.storages[0].palette[0] = block("unknown");
        assert!(matches!(
            rules().set_with(&unknown, [0, 0, 0], false, resolve),
            Err(WaterError::Unsupported(_))
        ));
        assert!(WaterRules::read(&json!([{"id":"x","states":{},"can_waterlog":true},{"id":"x","states":{},"can_waterlog":false}])).is_err());
    }
}
