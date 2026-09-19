//! Version-9 persistent subchunk serialization and immutable palette edits.
use crate::{
    Result,
    chunks::{BlockState, Storage, Subchunk},
    leveldb::MAX_BYTES,
    nbt::{self, Limits, Tag, Value},
};
use std::collections::HashSet;
fn block_tag(block: &BlockState) -> Result<Tag> {
    if block.state_order.len() != block.states.len() {
        return Err("Incomplete block state order".into());
    }
    let mut seen = HashSet::new();
    let mut properties = vec![];
    for name in &block.state_order {
        if !seen.insert(name) {
            return Err("Duplicate block state order".into());
        }
        let value = match block.states.get(name).ok_or("Missing block property")? {
            crate::chunks::StateValue::Bool(v) => Value::Byte(i8::from(*v)),
            crate::chunks::StateValue::Int(v) => Value::Int(*v),
            crate::chunks::StateValue::String(v) => Value::String(v.as_bytes().to_vec()),
        };
        properties.push(Tag {
            name: name.as_bytes().to_vec(),
            value,
        });
    }
    let fields = [
        (
            b"name".to_vec(),
            Value::String(block.name.as_bytes().to_vec()),
        ),
        (b"states".to_vec(), Value::Compound(properties)),
        (b"version".to_vec(), Value::Int(block.version)),
    ];
    let mut root = block.raw_tag.clone().unwrap_or(Tag {
        name: vec![],
        value: Value::Compound(
            fields
                .iter()
                .map(|(name, value)| Tag {
                    name: name.clone(),
                    value: value.clone(),
                })
                .collect(),
        ),
    });
    let Value::Compound(tags) = &mut root.value else {
        return Err("Block palette root must be compound".into());
    };
    for (name, value) in fields {
        let mut matching = tags.iter_mut().filter(|t| t.name == name);
        matching.next().ok_or("Missing retained block field")?.value = value;
        if matching.next().is_some() {
            return Err("Duplicate retained block field".into());
        }
    }
    Ok(root)
}
pub fn encode(chunk: &Subchunk) -> Result<Vec<u8>> {
    if !(1..=2).contains(&chunk.storages.len()) {
        return Err("Unsupported storage count".into());
    }
    let mut out = vec![9, chunk.storages.len() as u8, chunk.y as u8];
    for storage in &chunk.storages {
        let bits = storage.bits;
        if ![1, 2, 3, 4, 5, 6, 8, 16].contains(&bits) {
            return Err("Unsupported storage width".into());
        }
        if storage.indices.len() != 4096
            || storage.palette.is_empty()
            || storage.palette.len() > 4096.min(1 << bits)
        {
            return Err("Invalid storage dimensions".into());
        }
        let per_word = 32usize / bits as usize;
        let mut words = vec![0u32; 4096usize.div_ceil(per_word)];
        for (i, index) in storage.indices.iter().enumerate() {
            if *index as usize >= storage.palette.len() {
                return Err("Palette index out of range".into());
            }
            words[i / per_word] |= u32::from(*index) << ((i % per_word) * bits as usize);
        }
        out.push(bits << 1);
        for word in words {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(&(storage.palette.len() as i32).to_le_bytes());
        for block in &storage.palette {
            out.extend_from_slice(&nbt::write(&block_tag(block)?, Limits::default())?);
            if out.len() > MAX_BYTES {
                return Err("Encoded subchunk exceeds byte limit".into());
            }
        }
    }
    Ok(out)
}
pub fn replace_block(
    chunk: &Subchunk,
    position: [u8; 3],
    block: BlockState,
    layer: usize,
) -> Result<Subchunk> {
    let storage = chunk.storages.get(layer).ok_or("Storage layer is absent")?;
    let [x, y, z] = position;
    storage.block(x, y, z)?;
    if storage.indices.len() != 4096 {
        return Err("Invalid storage length".into());
    }
    let mut palette = storage.palette.clone();
    let index = match palette.iter().position(|v| v == &block) {
        Some(i) => i,
        None => {
            palette.push(block);
            palette.len() - 1
        }
    };
    if palette.len() > 4096 {
        return Err("Palette compaction required".into());
    }
    let bits = [1, 2, 3, 4, 5, 6, 8, 16]
        .into_iter()
        .find(|width| *width >= storage.bits && palette.len() <= 1 << width)
        .ok_or("Unsupported palette width")?;
    let mut indices = storage.indices.clone();
    indices[((x as usize) << 8) | ((z as usize) << 4) | y as usize] = index as u16;
    let mut result = chunk.clone();
    result.storages[layer] = Storage {
        bits,
        indices,
        palette,
    };
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::{StateValue, decode};
    fn block(name: &str) -> BlockState {
        BlockState::new(name.into(), vec![], 42).unwrap()
    }
    #[test]
    fn packing_growth_and_immutable_replacement() {
        for bits in [1, 2, 3, 4, 5, 6, 8, 16] {
            let chunk = Subchunk {
                y: -1,
                storages: vec![Storage {
                    bits,
                    indices: (0..4096).map(|i| (i % 2) as u16).collect(),
                    palette: vec![block("a"), block("b")],
                }],
            };
            let bytes = encode(&chunk).unwrap();
            assert_eq!(decode(&bytes, Some(-1)).unwrap(), chunk);
            assert_eq!(encode(&decode(&bytes, None).unwrap()).unwrap(), bytes);
        }
        let chunk = Subchunk {
            y: 2,
            storages: vec![Storage {
                bits: 1,
                indices: vec![0; 4096],
                palette: vec![block("a"), block("b")],
            }],
        };
        let added = BlockState::new(
            "c".into(),
            vec![
                ("flag".into(), StateValue::Bool(true)),
                ("axis".into(), StateValue::String("x".into())),
            ],
            42,
        )
        .unwrap();
        let edited = replace_block(&chunk, [15, 2, 7], added.clone(), 0).unwrap();
        assert_eq!(edited.storages[0].bits, 2);
        assert_eq!(chunk.storages[0].block(15, 2, 7).unwrap().name, "a");
        assert_eq!(
            decode(&encode(&edited).unwrap(), None).unwrap().storages[0]
                .block(15, 2, 7)
                .unwrap(),
            &added
        );
    }
    #[test]
    fn unknown_nbt_fields_and_root_name_survive() {
        let mut b = block("a");
        let mut tag = block_tag(&b).unwrap();
        tag.name = b"retained-root".to_vec();
        if let Value::Compound(tags) = &mut tag.value {
            tags.insert(
                1,
                Tag {
                    name: b"future".to_vec(),
                    value: Value::Float(0x7fc01234),
                },
            );
        }
        b.raw_tag = Some(tag.clone());
        let chunk = Subchunk {
            y: 0,
            storages: vec![Storage {
                bits: 1,
                indices: vec![0; 4096],
                palette: vec![b],
            }],
        };
        let decoded = decode(&encode(&chunk).unwrap(), None).unwrap();
        assert_eq!(decoded.storages[0].palette[0].raw_tag, Some(tag));
        assert_eq!(encode(&decoded).unwrap(), encode(&chunk).unwrap());
    }
    #[test]
    fn invalid_storage_and_absent_layer_rejected() {
        let mut chunk = Subchunk {
            y: 0,
            storages: vec![Storage {
                bits: 1,
                indices: vec![0; 4096],
                palette: vec![block("a")],
            }],
        };
        assert!(replace_block(&chunk, [0, 0, 0], block("b"), 1).is_err());
        chunk.storages[0].indices[0] = 2;
        assert!(encode(&chunk).is_err());
        assert!(chunk.storages[0].block(0, 0, 0).is_err());
        chunk.storages[0].indices.pop();
        assert!(encode(&chunk).is_err());
    }
}
