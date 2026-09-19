//! Persistent version-9 subchunks and finite stopped-world block access.
use crate::{
    Result,
    leveldb::{Cursor, Snapshot},
    nbt::{self, Limits, Tag, Value},
};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(untagged)]
pub enum StateValue {
    Bool(bool),
    Int(i32),
    String(String),
}
#[derive(Clone, Debug, Serialize)]
pub struct BlockState {
    pub name: String,
    pub states: BTreeMap<String, StateValue>,
    pub state_order: Vec<String>,
    pub version: i32,
    #[serde(skip)]
    pub(crate) raw_tag: Option<Tag>,
}
impl PartialEq for BlockState {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.states == other.states
            && self.state_order == other.state_order
            && self.version == other.version
    }
}
impl BlockState {
    pub fn new(name: String, properties: Vec<(String, StateValue)>, version: i32) -> Result<Self> {
        let state_order = properties
            .iter()
            .map(|(k, _)| k.clone())
            .collect::<Vec<_>>();
        let states = properties.into_iter().collect::<BTreeMap<_, _>>();
        if states.len() != state_order.len() {
            return Err("Duplicate block property".into());
        }
        Ok(Self {
            name,
            states,
            state_order,
            version,
            raw_tag: None,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Storage {
    pub bits: u8,
    pub indices: Vec<u16>,
    pub palette: Vec<BlockState>,
}
impl Storage {
    pub fn block(&self, x: u8, y: u8, z: u8) -> Result<&BlockState> {
        if x >= 16 || y >= 16 || z >= 16 {
            return Err("Local coordinate outside subchunk".into());
        }
        let index = *self
            .indices
            .get(((x as usize) << 8) | ((z as usize) << 4) | y as usize)
            .ok_or("Missing storage index")?;
        self.palette
            .get(index as usize)
            .ok_or_else(|| "Palette index out of range".into())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Subchunk {
    pub y: i8,
    pub storages: Vec<Storage>,
}
fn string(raw: &[u8]) -> Result<String> {
    String::from_utf8(raw.to_vec()).map_err(|_| "Invalid UTF-8 block name/state".into())
}
fn child<'a>(root: &'a Tag, name: &[u8]) -> Result<&'a Value> {
    let tags = match &root.value {
        Value::Compound(tags) => tags,
        _ => return Err("Block palette root must be compound".into()),
    };
    let mut matches = tags.iter().filter(|t| t.name == name);
    let value = &matches.next().ok_or("Missing block field")?.value;
    if matches.next().is_some() {
        return Err("Duplicate block field".into());
    }
    Ok(value)
}
pub fn decode(data: &[u8], expected_y: Option<i8>) -> Result<Subchunk> {
    let mut c = Cursor::new(data)?;
    let header = c.take(3)?;
    let (version, count, y) = (header[0], header[1], header[2] as i8);
    if version != 9 || !(1..=2).contains(&count) {
        return Err("Unsupported subchunk version/storage count".into());
    }
    if expected_y.is_some_and(|wanted| wanted != y) {
        return Err("Subchunk Y disagrees with key".into());
    }
    let mut storages = Vec::new();
    for _ in 0..count {
        let header = c.take(1)?[0];
        let bits = header >> 1;
        if header & 1 != 0 || ![1, 2, 3, 4, 5, 6, 8, 16].contains(&bits) {
            return Err("Unsupported persistent storage header".into());
        }
        let per_word = 32usize / bits as usize;
        let count = 4096usize.div_ceil(per_word);
        let words = c
            .take(count * 4)?
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
            .collect::<Vec<_>>();
        let indices = (0..4096)
            .map(|i| {
                ((words[i / per_word] >> ((i % per_word) * bits as usize)) & ((1u32 << bits) - 1))
                    as u16
            })
            .collect::<Vec<_>>();
        let size = i32::from_le_bytes(c.fixed()?);
        if size < 1 || size > 4096.min(1i32 << bits) {
            return Err("Invalid palette size".into());
        }
        let mut palette = Vec::new();
        for _ in 0..size {
            let (tag, used) = nbt::read_prefix(&c.data[c.pos..], Limits::default())?;
            c.take(used)?;
            let name = match child(&tag, b"name")? {
                Value::String(raw) => string(raw)?,
                _ => return Err("Block name must be string".into()),
            };
            let version = match child(&tag, b"version")? {
                Value::Int(v) => *v,
                _ => return Err("Block version must be int".into()),
            };
            let tags = match child(&tag, b"states")? {
                Value::Compound(tags) => tags,
                _ => return Err("Block states must be compound".into()),
            };
            let mut states = BTreeMap::new();
            let mut state_order = Vec::new();
            for t in tags {
                let value = match &t.value {
                    Value::Byte(0) => StateValue::Bool(false),
                    Value::Byte(1) => StateValue::Bool(true),
                    Value::Int(v) => StateValue::Int(*v),
                    Value::String(v) => StateValue::String(string(v)?),
                    _ => return Err("Unsupported block-state property".into()),
                };
                let name = string(&t.name)?;
                state_order.push(name.clone());
                if states.insert(name, value).is_some() {
                    return Err("Duplicate block-state property".into());
                }
            }
            palette.push(BlockState {
                name,
                states,
                state_order,
                version,
                raw_tag: Some(tag),
            });
        }
        if indices.iter().any(|i| *i as usize >= palette.len()) {
            return Err("Block index exceeds palette".into());
        }
        storages.push(Storage {
            bits,
            indices,
            palette,
        });
    }
    if c.pos != data.len() {
        return Err("Trailing subchunk bytes".into());
    }
    Ok(Subchunk { y, storages })
}
pub fn key(x: i32, z: i32, y: i8) -> Vec<u8> {
    let mut key = x.to_le_bytes().to_vec();
    key.extend_from_slice(&z.to_le_bytes());
    key.extend_from_slice(&[47, y as u8]);
    key
}
pub fn key_y(key: &[u8]) -> Option<i8> {
    match key.len() {
        10 if key[8] == 47 => Some(key[9] as i8),
        14 if key[12] == 47 => Some(key[13] as i8),
        _ => None,
    }
}
pub struct World {
    pub snapshot: Snapshot,
    cache: HashMap<[i32; 3], Subchunk>,
    columns: HashSet<[i32; 2]>,
}
impl World {
    pub fn new(snapshot: Snapshot) -> Self {
        Self {
            snapshot,
            cache: HashMap::new(),
            columns: HashSet::new(),
        }
    }
    pub fn block(&mut self, position: [i32; 3], layer: usize) -> Result<Option<BlockState>> {
        let [x, y, z] = position;
        if !(-64..320).contains(&y) {
            return Err("Outside measured Overworld height".into());
        }
        let column = [x.div_euclid(16), z.div_euclid(16)];
        if !self.columns.contains(&column) {
            let mut key = column[0].to_le_bytes().to_vec();
            key.extend_from_slice(&column[1].to_le_bytes());
            key.push(43);
            let data = self
                .snapshot
                .get(&key)
                .ok_or_else(|| format!("Unrecorded world column: {column:?}"))?;
            if data.len() < 512 {
                return Err("Truncated Data3D heightmap".into());
            }
            self.columns.insert(column);
        }
        let chunk = [column[0], column[1], y.div_euclid(16)];
        let encoded = key(chunk[0], chunk[1], chunk[2] as i8);
        if !self.cache.contains_key(&chunk) {
            let Some(data) = self.snapshot.get(&encoded) else {
                return Ok(None);
            };
            self.cache
                .insert(chunk, decode(data, Some(chunk[2] as i8))?);
        }
        let storages = &self.cache[&chunk].storages;
        if layer >= storages.len() {
            return Ok(None);
        }
        Ok(Some(
            storages[layer]
                .block(
                    x.rem_euclid(16) as u8,
                    y.rem_euclid(16) as u8,
                    z.rem_euclid(16) as u8,
                )?
                .clone(),
        ))
    }
    pub fn collision_solid(&mut self, position: [i32; 3]) -> Result<bool> {
        if let Some(secondary) = self.block(position, 1)?
            && (secondary.name != "minecraft:air" || !secondary.states.is_empty())
        {
            return Err(format!("Unmeasured secondary layer at {position:?}"));
        }
        let Some(block) = self.block(position, 0)? else {
            return Ok(false);
        };
        if !block.states.is_empty() {
            return Err(format!("Unmeasured collision properties at {position:?}"));
        }
        match block.name.as_str() {
            "minecraft:air" => Ok(false),
            "minecraft:stone" => Ok(true),
            _ => Err(format!(
                "Unmeasured collision block at {position:?}: {}",
                block.name
            )),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn entry(name: &str) -> Vec<u8> {
        nbt::write(
            &Tag {
                name: vec![],
                value: Value::Compound(vec![
                    Tag {
                        name: b"name".to_vec(),
                        value: Value::String(name.as_bytes().to_vec()),
                    },
                    Tag {
                        name: b"states".to_vec(),
                        value: Value::Compound(vec![]),
                    },
                    Tag {
                        name: b"version".to_vec(),
                        value: Value::Int(18168865),
                    },
                ]),
            },
            Limits::default(),
        )
        .unwrap()
    }
    #[test]
    fn absent_subchunk_requires_recorded_column() {
        let mut snapshot = Snapshot::default();
        let mut column = (-1_i32).to_le_bytes().to_vec();
        column.extend_from_slice(&(-1_i32).to_le_bytes());
        column.push(43);
        snapshot.entries.insert(
            column,
            crate::leveldb::Entry {
                sequence: 1,
                value: Some(vec![0; 512]),
            },
        );
        let mut world = World::new(snapshot);
        assert!(!world.collision_solid([-1, -1, -1]).unwrap());
        assert!(world.collision_solid([0, -1, -1]).is_err());
        assert!(world.block([-1, -65, -1], 0).is_err());
    }

    #[test]
    fn all_widths_and_xzy_coordinates() {
        for bits in [1, 2, 3, 4, 5, 6, 8, 16] {
            let mut data = vec![9, 1, 255, bits << 1];
            let per_word = 32usize / bits as usize;
            let mut words = vec![0u32; 4096usize.div_ceil(per_word)];
            let index = (15 << 8) | (7 << 4) | 3;
            words[index / per_word] |= 1 << ((index % per_word) * bits as usize);
            for word in words {
                data.extend_from_slice(&word.to_le_bytes());
            }
            data.extend_from_slice(&2i32.to_le_bytes());
            data.extend(entry("minecraft:air"));
            data.extend(entry("minecraft:stone"));
            let chunk = decode(&data, Some(-1)).unwrap();
            assert_eq!(
                chunk.storages[0].block(15, 3, 7).unwrap().name,
                "minecraft:stone"
            );
            assert_eq!(
                chunk.storages[0].block(15, 7, 3).unwrap().name,
                "minecraft:air"
            );
            assert!(decode(&data, Some(1)).is_err());
            data.push(0);
            assert!(decode(&data, None).is_err());
        }
    }
}
