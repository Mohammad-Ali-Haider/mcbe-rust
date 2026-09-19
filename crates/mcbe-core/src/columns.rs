//! Sparse, exact-state assembly of recorded Overworld cube columns.
use crate::{
    Result, biomes,
    blocks::Palette,
    chunks::{self, BlockState},
    container_geometry::ContainerBlock,
    cubes::{Classifier, Cube},
    heightmaps,
    leveldb::Snapshot,
};
use std::collections::{BTreeMap, HashMap};

pub struct Column {
    pub position: [i32; 2],
    pub cells: BTreeMap<[i32; 3], Cube>,
    pub containers: BTreeMap<[i32; 3], ContainerBlock>,
    pub biomes: biomes::Column,
}
pub fn read(snapshot: &Snapshot, position: [i32; 2], palette: &Palette) -> Result<Option<Column>> {
    let mut classifier = Classifier::default();
    read_with(snapshot, position, |block| classifier.saved(block, palette))
}
fn read_with(
    snapshot: &Snapshot,
    position: [i32; 2],
    mut classify: impl FnMut(&BlockState) -> Result<Option<Cube>>,
) -> Result<Option<Column>> {
    read_parts(snapshot, position, |block| Ok((classify(block)?, None)))
}
/// Keep exact measured container states alongside cubes; never coerce them to cubes.
pub fn read_geometry(
    snapshot: &Snapshot,
    position: [i32; 2],
    palette: &Palette,
) -> Result<Option<Column>> {
    let mut classifier = Classifier::default();
    read_parts(snapshot, position, |block| {
        let canonical = palette.resolve_saved(block).map_err(|e| e.to_string())?;
        let states = canonical
            .states()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if let Some(container) = ContainerBlock::canonical(canonical.id(), &states) {
            Ok((None, Some(container)))
        } else {
            Ok((classifier.saved(block, palette)?, None))
        }
    })
}
fn read_parts(
    snapshot: &Snapshot,
    position: [i32; 2],
    mut classify: impl FnMut(&BlockState) -> Result<(Option<Cube>, Option<ContainerBlock>)>,
) -> Result<Option<Column>> {
    let [cx, cz] = position;
    let x = cx.checked_mul(16).ok_or("Column X overflow")?;
    let z = cz.checked_mul(16).ok_or("Column Z overflow")?;
    x.checked_add(15).ok_or("Column X extent overflow")?;
    z.checked_add(15).ok_or("Column Z extent overflow")?;
    let Some(data) = snapshot.get(&heightmaps::column_key(cx, cz)) else {
        return Ok(None);
    };
    let biomes = biomes::Column::decode(data.get(512..).ok_or("Truncated Data3D height prefix")?)?;
    let mut cells = BTreeMap::new();
    let mut containers = BTreeMap::new();
    for cy in -4..20 {
        let Some(data) = snapshot.get(&chunks::key(cx, cz, cy)) else {
            continue;
        };
        let chunk = chunks::decode(data, Some(cy))?;
        let primary = &chunk.storages[0];
        if let Some(secondary) = chunk.storages.get(1) {
            for &index in &secondary.indices {
                let block = &secondary.palette[usize::from(index)];
                if block.name != "minecraft:air" || !block.states.is_empty() {
                    return Err(format!(
                        "Unmeasured secondary layer in column {position:?}, section {cy}"
                    ));
                }
            }
        }
        let mut kinds = HashMap::new();
        for (i, &index) in primary.indices.iter().enumerate() {
            if let std::collections::hash_map::Entry::Vacant(entry) = kinds.entry(index) {
                entry.insert(classify(&primary.palette[usize::from(index)])?);
            }
            let position = [
                x + (i >> 8) as i32,
                i32::from(cy) * 16 + (i & 15) as i32,
                z + ((i >> 4) & 15) as i32,
            ];
            if let Some(container) = kinds[&index].1 {
                containers.insert(position, container);
            }
            if let Some(kind) = kinds[&index].0 {
                cells.insert(
                    [
                        x + (i >> 8) as i32,
                        i32::from(cy) * 16 + (i & 15) as i32,
                        z + ((i >> 4) & 15) as i32,
                    ],
                    kind,
                );
            }
        }
    }
    Ok(Some(Column {
        position,
        cells,
        containers,
        biomes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        chunks::{Storage, Subchunk},
        leveldb::Entry,
    };
    fn snapshot() -> Snapshot {
        let mut snapshot = Snapshot::default();
        let mut data = vec![0; 512];
        data.push(1);
        data.extend(1u32.to_le_bytes());
        data.extend([255; 23]);
        snapshot.entries.insert(
            heightmaps::column_key(-1, -2),
            Entry {
                sequence: 1,
                value: Some(data),
            },
        );
        snapshot
    }
    fn block(name: &str) -> BlockState {
        BlockState::new(name.into(), vec![], 1).unwrap()
    }
    #[test]
    fn sparse_air_and_negative_column_bounds() {
        let snapshot = snapshot();
        let column = read_with(&snapshot, [-1, -2], |_| Ok(None))
            .unwrap()
            .unwrap();
        assert!(column.cells.is_empty());
        assert_eq!(column.biomes.at(15, 319, 15).unwrap(), 1);
        assert!(
            read_with(&snapshot, [0, 0], |_| Ok(None))
                .unwrap()
                .is_none()
        );
        assert!(read_with(&snapshot, [i32::MAX, 0], |_| Ok(None)).is_err());
    }
    #[test]
    fn only_referenced_primary_entries_are_classified() {
        let mut snapshot = snapshot();
        let mut indices = vec![0; 4096];
        indices[(15 << 8) | (7 << 4) | 3] = 1;
        let chunk = Subchunk {
            y: -1,
            storages: vec![Storage {
                bits: 2,
                indices,
                palette: vec![
                    block("minecraft:air"),
                    block("minecraft:stone"),
                    block("unmeasured:unused"),
                ],
            }],
        };
        snapshot.entries.insert(
            chunks::key(-1, -2, -1),
            Entry {
                sequence: 1,
                value: Some(crate::chunk_write::encode(&chunk).unwrap()),
            },
        );
        let column = read_with(&snapshot, [-1, -2], |block| match block.name.as_str() {
            "minecraft:air" => Ok(None),
            "minecraft:stone" => Ok(Some(Cube::Stone)),
            _ => Err("Unexpected referenced palette entry".into()),
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            column.cells,
            BTreeMap::from([([-1, -13, -25], Cube::Stone)])
        );
    }
    #[test]
    fn secondary_fluids_and_malformed_columns_are_rejected() {
        let mut snapshot = snapshot();
        let chunk = Subchunk {
            y: -4,
            storages: vec![
                Storage {
                    bits: 1,
                    indices: vec![0; 4096],
                    palette: vec![block("minecraft:air")],
                },
                Storage {
                    bits: 1,
                    indices: vec![0; 4096],
                    palette: vec![block("minecraft:water")],
                },
            ],
        };
        snapshot.entries.insert(
            chunks::key(-1, -2, -4),
            Entry {
                sequence: 1,
                value: Some(crate::chunk_write::encode(&chunk).unwrap()),
            },
        );
        assert!(read_with(&snapshot, [-1, -2], |_| Ok(None)).is_err());
        snapshot
            .entries
            .get_mut(&heightmaps::column_key(-1, -2))
            .unwrap()
            .value = Some(vec![0; 512]);
        assert!(read_with(&snapshot, [-1, -2], |_| Ok(None)).is_err());
    }
}
