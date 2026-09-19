//! Measured Overworld height and topmost predicates; opaque biome bytes are preserved.
use crate::{
    Result,
    blocks::{self, Palette, States},
    chunks::{self, BlockState, StateValue},
    leveldb::Snapshot,
};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnData {
    pub heights: [u16; 256],
    pub biome_bytes: Vec<u8>,
}
impl ColumnData {
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < 512 {
            return Err("Truncated Data3D heightmap".into());
        }
        Ok(Self {
            heights: std::array::from_fn(|i| u16::from_le_bytes([data[2 * i], data[2 * i + 1]])),
            biome_bytes: data[512..].to_vec(),
        })
    }
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(512 + self.biome_bytes.len());
        for value in self.heights {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&self.biome_bytes);
        out
    }
    pub fn raw_height(&self, x: u8, z: u8) -> Result<u16> {
        if x >= 16 || z >= 16 {
            return Err("Local X/Z must be in 0..15".into());
        }
        Ok(self.heights[z as usize * 16 + x as usize])
    }
}
pub fn column_key(x: i32, z: i32) -> Vec<u8> {
    let mut result = x.to_le_bytes().to_vec();
    result.extend_from_slice(&z.to_le_bytes());
    result.push(43);
    result
}
pub fn column_coordinates(key: &[u8]) -> Option<[i32; 2]> {
    if key.len() != 9 || key[8] != 43 {
        return None;
    }
    Some([
        i32::from_le_bytes(key[..4].try_into().ok()?),
        i32::from_le_bytes(key[4..8].try_into().ok()?),
    ])
}
pub struct HeightRules {
    rules: HashMap<(String, States), (bool, bool)>,
    cache: HashMap<(String, States), (bool, bool)>,
}
impl HeightRules {
    pub fn read(rows: &Value) -> Result<Self> {
        let mut rules = HashMap::new();
        for row in rows.as_array().ok_or("Missing height rules")? {
            let id = row["id"]
                .as_str()
                .ok_or("Missing rule identifier")?
                .to_owned();
            let mut states = blocks::states(&row["states"]).map_err(|e| e.to_string())?;
            states.sort_by(|a, b| a.0.cmp(&b.0));
            let flags = (
                row["heightmap"]
                    .as_bool()
                    .ok_or("Heightmap flag must be boolean")?,
                row["topmost"]
                    .as_bool()
                    .ok_or("Topmost flag must be boolean")?,
            );
            if rules
                .insert((id, states), flags)
                .is_some_and(|old| old != flags)
            {
                return Err("Conflicting height rule".into());
            }
        }
        if rules.is_empty() {
            return Err("Empty height rules".into());
        }
        Ok(Self {
            rules,
            cache: HashMap::new(),
        })
    }
    pub fn flags(&mut self, block: &BlockState, palette: &Palette) -> Result<(bool, bool)> {
        let properties = block
            .state_order
            .iter()
            .map(|k| {
                block
                    .states
                    .get(k)
                    .map(|v| (k.clone(), v.clone()))
                    .ok_or("Missing ordered state")
            })
            .collect::<std::result::Result<States, _>>()?;
        let raw_key = (block.name.clone(), properties);
        if let Some(flags) = self.cache.get(&raw_key) {
            return Ok(*flags);
        }
        let resolved = palette.resolve_saved(block).map_err(|e| e.to_string())?;
        let key = (
            resolved.id().to_owned(),
            resolved
                .states()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        );
        let flags = *self.rules.get(&key).ok_or_else(|| {
            format!(
                "Unmeasured height state: {} {:?}",
                resolved.id(),
                resolved.states()
            )
        })?;
        self.cache.insert(raw_key, flags);
        Ok(flags)
    }
}
pub fn secondary_flags(block: &BlockState) -> Result<(bool, bool)> {
    if block.name == "minecraft:air" && block.states.is_empty() {
        return Ok((false, false));
    }
    if matches!(
        block.name.as_str(),
        "minecraft:water" | "minecraft:flowing_water"
    ) && block.states == BTreeMap::from([("liquid_depth".into(), StateValue::Int(0))])
    {
        return Ok((true, false));
    }
    Err(format!(
        "Unmeasured secondary height state: {} {:?}",
        block.name, block.states
    ))
}
pub fn rebuild_column(
    snapshot: &Snapshot,
    cx: i32,
    cz: i32,
    rules: &mut HeightRules,
    palette: &Palette,
) -> Result<(ColumnData, [Option<i32>; 256])> {
    rebuild_with(snapshot, cx, cz, |block| rules.flags(block, palette))
}
fn rebuild_with(
    snapshot: &Snapshot,
    cx: i32,
    cz: i32,
    mut flags: impl FnMut(&BlockState) -> Result<(bool, bool)>,
) -> Result<(ColumnData, [Option<i32>; 256])> {
    let original = ColumnData::decode(
        snapshot
            .get(&column_key(cx, cz))
            .ok_or("Column metadata is absent")?,
    )?;
    let mut chunks = vec![];
    for (key, entry) in &snapshot.entries {
        if key.len() == 10
            && key[8] == 47
            && key[..4] == cx.to_le_bytes()
            && key[4..8] == cz.to_le_bytes()
            && let Some(value) = &entry.value
        {
            let chunk = chunks::decode(value, Some(key[9] as i8))?;
            if !(-4..20).contains(&chunk.y) {
                return Err("Subchunk outside measured Overworld range".into());
            }
            chunks.push(chunk);
        }
    }
    chunks.sort_by_key(|chunk| std::cmp::Reverse(chunk.y));
    let mut heights = [0; 256];
    let mut tops = [None; 256];
    for z in 0..16u8 {
        for x in 0..16u8 {
            let mut light_y = None;
            let mut top_y = None;
            'column: for chunk in &chunks {
                for y in (0..16u8).rev() {
                    let (mut light, mut top) = flags(chunk.storages[0].block(x, y, z)?)?;
                    if chunk.storages.len() == 2 {
                        let (a, b) = secondary_flags(chunk.storages[1].block(x, y, z)?)?;
                        light |= a;
                        top |= b;
                    }
                    let absolute = i32::from(chunk.y) * 16 + i32::from(y);
                    if light_y.is_none() && light {
                        light_y = Some(absolute);
                    }
                    if top_y.is_none() && top {
                        top_y = Some(absolute);
                    }
                    if light_y.is_some() && top_y.is_some() {
                        break 'column;
                    }
                }
            }
            let index = z as usize * 16 + x as usize;
            heights[index] = light_y.map_or(0, |y| (y + 65) as u16);
            tops[index] = top_y;
        }
    }
    Ok((
        ColumnData {
            heights,
            biome_bytes: original.biome_bytes,
        },
        tops,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        chunk_write,
        chunks::{Storage, Subchunk},
        leveldb::Entry,
    };
    use serde_json::json;
    #[test]
    fn axis_order_and_opaque_suffix_roundtrip() {
        let column = ColumnData {
            heights: std::array::from_fn(|i| i as u16),
            biome_bytes: vec![0, 255, 16, 42],
        };
        let bytes = column.encode();
        let decoded = ColumnData::decode(&bytes).unwrap();
        assert_eq!(decoded.raw_height(3, 7).unwrap(), 115);
        assert_eq!(decoded, column);
        assert_eq!(decoded.encode(), bytes);
        assert!(decoded.raw_height(16, 0).is_err());
        assert!(ColumnData::decode(&bytes[..511]).is_err());
    }
    #[test]
    fn distinct_predicates_negative_heights_and_missing_subchunks() {
        let palette = ["air", "stone", "glass", "leaves"]
            .map(|name| BlockState::new(name.into(), vec![], 1).unwrap())
            .to_vec();
        let mut snapshot = Snapshot::default();
        snapshot.entries.insert(
            column_key(-1, 0),
            Entry {
                sequence: 1,
                value: Some(
                    ColumnData {
                        heights: [99; 256],
                        biome_bytes: b"biomes".to_vec(),
                    }
                    .encode(),
                ),
            },
        );
        for cy in [-4, 1] {
            let mut indices = vec![0; 4096];
            if cy == -4 {
                indices[(2 << 8) | (5 << 4) | 1] = 1;
            } else {
                indices[(2 << 8) | (5 << 4) | 3] = 2;
                indices[(2 << 8) | (5 << 4) | 8] = 3;
            }
            let chunk = Subchunk {
                y: cy,
                storages: vec![Storage {
                    bits: 2,
                    indices,
                    palette: palette.clone(),
                }],
            };
            snapshot.entries.insert(
                chunks::key(-1, 0, cy),
                Entry {
                    sequence: 2,
                    value: Some(chunk_write::encode(&chunk).unwrap()),
                },
            );
        }
        let flags = |block: &BlockState| {
            Ok(match block.name.as_str() {
                "air" => (false, false),
                "stone" => (true, true),
                "glass" => (false, true),
                "leaves" => (true, false),
                _ => panic!(),
            })
        };
        let (column, tops) = rebuild_with(&snapshot, -1, 0, flags).unwrap();
        assert_eq!(column.raw_height(2, 5).unwrap(), 89);
        assert_eq!(tops[5 * 16 + 2], Some(19));
        assert_eq!(column.heights[0], 0);
        assert_eq!(tops[0], None);
        assert_eq!(column.biome_bytes, b"biomes");
        assert!(rebuild_with(&snapshot, 0, 0, flags).is_err());
    }
    #[test]
    fn secondary_water_contributes_only_to_stored_height() {
        let air = BlockState::new("minecraft:air".into(), vec![], 1).unwrap();
        let water = BlockState::new(
            "minecraft:water".into(),
            vec![("liquid_depth".into(), StateValue::Int(0))],
            1,
        )
        .unwrap();
        assert_eq!(secondary_flags(&water).unwrap(), (true, false));
        let mut bad = water.clone();
        bad.states
            .insert("liquid_depth".into(), StateValue::Bool(false));
        assert!(secondary_flags(&bad).is_err());
        let chunk = Subchunk {
            y: 0,
            storages: vec![
                Storage {
                    bits: 1,
                    indices: vec![0; 4096],
                    palette: vec![air],
                },
                Storage {
                    bits: 1,
                    indices: vec![0; 4096],
                    palette: vec![water],
                },
            ],
        };
        let mut snapshot = Snapshot::default();
        snapshot.entries.insert(
            column_key(0, 0),
            Entry {
                sequence: 1,
                value: Some(vec![0; 512]),
            },
        );
        snapshot.entries.insert(
            chunks::key(0, 0, 0),
            Entry {
                sequence: 2,
                value: Some(chunk_write::encode(&chunk).unwrap()),
            },
        );
        let (column, tops) = rebuild_with(&snapshot, 0, 0, |_| Ok((false, false))).unwrap();
        assert_eq!(column.heights, [80; 256]);
        assert_eq!(tops, [None; 256]);
    }
    #[test]
    fn malformed_and_conflicting_rules_are_rejected() {
        let row = json!({"id":"stone","states":{},"heightmap":true,"topmost":true});
        let mut conflict = row.clone();
        conflict["topmost"] = json!(false);
        assert!(HeightRules::read(&json!([row.clone(), conflict])).is_err());
        let mut invalid = row.clone();
        invalid["heightmap"] = json!(1);
        assert!(HeightRules::read(&json!([invalid])).is_err());
        assert_eq!(
            HeightRules::read(&json!([row.clone(), row]))
                .unwrap()
                .rules
                .len(),
            1
        );
    }
}
