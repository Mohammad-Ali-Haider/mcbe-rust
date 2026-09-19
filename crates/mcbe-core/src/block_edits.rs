//! Measured stone/air edits to existing Overworld subchunks, with height repair.
use crate::{
    Result,
    blocks::Palette,
    chunk_write,
    chunks::{self, BlockState, Storage, Subchunk},
    cubes::{Classifier, Cube},
    heightmaps::{self, ColumnData, HeightRules},
    leveldb::{Entry, Snapshot},
    world_edit::Updates,
};
use std::collections::{BTreeMap, BTreeSet};

pub struct Prepared {
    pub updates: Updates,
    pub changed_cells: usize,
    pub repaired_height_entries: usize,
    pub columns: Vec<[i32; 2]>,
}

pub fn prepare_stone_edits(
    snapshot: &Snapshot,
    changes: &[([i32; 3], bool)],
    rules: &mut HeightRules,
    palette: &Palette,
) -> Result<Prepared> {
    if changes.is_empty() {
        return Err("No stone/air edits requested".into());
    }
    let mut seen = BTreeSet::new();
    let mut encoded = BTreeMap::new();
    let mut columns = BTreeSet::new();
    let mut changed_cells = 0;
    for (position, solid) in changes {
        let [x, y, z] = *position;
        if !(-64..320).contains(&y) || !seen.insert(*position) {
            return Err("Duplicate edit or position outside measured Overworld height".into());
        }
        let key = chunks::key(x.div_euclid(16), z.div_euclid(16), y.div_euclid(16) as i8);
        let raw = encoded
            .get(&key)
            .map(Vec::as_slice)
            .or_else(|| snapshot.get(&key))
            .ok_or("Edited subchunk must already exist")?;
        let chunk = chunks::decode(raw, Some(y.div_euclid(16) as i8))?;
        let local = position.map(|v| v.rem_euclid(16) as u8);
        let old = chunk.storages[0].block(local[0], local[1], local[2])?;
        if !old.states.is_empty()
            || !["minecraft:stone", "minecraft:air"].contains(&old.name.as_str())
        {
            return Err("Stone edits require measured persistent stone/air states".into());
        }
        if let Some(layer) = chunk.storages.get(1) {
            let secondary = layer.block(local[0], local[1], local[2])?;
            if secondary.name != "minecraft:air" || !secondary.states.is_empty() {
                return Err("Stone edits do not implement secondary-fluid transitions".into());
            }
        }
        let name = if *solid {
            "minecraft:stone"
        } else {
            "minecraft:air"
        };
        if old.name == name {
            continue;
        }
        let block = BlockState::new(name.into(), vec![], old.version)?;
        let edited = chunk_write::replace_block(&chunk, local, block, 0)?;
        encoded.insert(key, chunk_write::encode(&edited)?);
        columns.insert([x.div_euclid(16), z.div_euclid(16)]);
        changed_cells += 1;
    }
    finish(snapshot, encoded, columns, changed_cells, rules, palette)
}
fn finish(
    snapshot: &Snapshot,
    encoded: BTreeMap<Vec<u8>, Vec<u8>>,
    columns: BTreeSet<[i32; 2]>,
    changed_cells: usize,
    rules: &mut HeightRules,
    palette: &Palette,
) -> Result<Prepared> {
    if changed_cells == 0 {
        return Err("Requested blocks already match source".into());
    }
    let mut prospective = Snapshot {
        entries: snapshot.entries.clone(),
        ..Snapshot::default()
    };
    let mut updates: Updates = encoded
        .into_iter()
        .map(|(key, data)| (key, Some(data)))
        .collect();
    for (key, data) in &updates {
        prospective.entries.insert(
            key.clone(),
            Entry {
                sequence: 0,
                value: data.clone(),
            },
        );
    }
    let mut repaired_height_entries = 0;
    for [cx, cz] in &columns {
        let key = heightmaps::column_key(*cx, *cz);
        let old = ColumnData::decode(
            snapshot
                .get(&key)
                .ok_or("Missing changed-column metadata")?,
        )?;
        let (rebuilt, _) = heightmaps::rebuild_column(&prospective, *cx, *cz, rules, palette)?;
        if old.biome_bytes != rebuilt.biome_bytes {
            return Err("Height rebuild altered opaque biome bytes".into());
        }
        repaired_height_entries += old
            .heights
            .iter()
            .zip(rebuilt.heights)
            .filter(|(a, b)| **a != *b)
            .count();
        if old != rebuilt {
            updates.push((key, Some(rebuilt.encode())));
        }
    }
    Ok(Prepared {
        updates,
        changed_cells,
        repaired_height_entries,
        columns: columns.into_iter().collect(),
    })
}

pub fn prepare_cube_edits(
    snapshot: &Snapshot,
    changes: &[([i32; 3], Option<Cube>)],
    rules: &mut HeightRules,
    palette: &Palette,
) -> Result<Prepared> {
    if changes.is_empty() {
        return Err("No cube changes requested".into());
    }
    let mut prototypes = BTreeMap::new();
    let mut classifier = Classifier::default();
    let requested: BTreeSet<_> = changes.iter().filter_map(|(_, kind)| *kind).collect();
    for (key, entry) in &snapshot.entries {
        if let Some(y) = chunks::key_y(key)
            && let Some(raw) = &entry.value
        {
            let chunk = chunks::decode(raw, Some(y))?;
            for block in &chunk.storages[0].palette {
                if let Ok(Some(kind)) = classifier.saved(block, palette)
                    && requested.contains(&kind)
                {
                    prototypes.entry(kind).or_insert_with(|| block.clone());
                }
            }
            if prototypes.len() == requested.len() {
                break;
            }
        }
    }
    if prototypes.len() != requested.len() {
        return Err("A requested cube has no persistent prototype in the source world".into());
    }
    let mut encoded = BTreeMap::<Vec<u8>, Vec<u8>>::new();
    let mut columns = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut changed = 0;
    for (position, target) in changes {
        let [x, y, z] = *position;
        if !(-64..320).contains(&y) || !seen.insert(*position) {
            return Err("Duplicate cube edit or invalid height".into());
        }
        let column = heightmaps::column_key(x.div_euclid(16), z.div_euclid(16));
        if snapshot.get(&column).is_none_or(|bytes| bytes.len() < 512) {
            return Err("Cube edits require an existing recorded column".into());
        }
        let cy = y.div_euclid(16) as i8;
        let key = chunks::key(x.div_euclid(16), z.div_euclid(16), cy);
        let raw = encoded
            .get(&key)
            .map(Vec::as_slice)
            .or_else(|| snapshot.get(&key));
        let chunk = if let Some(raw) = raw {
            chunks::decode(raw, Some(cy))?
        } else {
            let Some(target) = target else {
                continue;
            };
            let air = BlockState::new("minecraft:air".into(), vec![], prototypes[target].version)?;
            Subchunk {
                y: cy,
                storages: vec![Storage {
                    bits: 1,
                    indices: vec![0; 4096],
                    palette: vec![air],
                }],
            }
        };
        let local = position.map(|v| v.rem_euclid(16) as u8);
        let old = chunk.storages[0].block(local[0], local[1], local[2])?;
        if classifier.saved(old, palette)? == *target {
            continue;
        }
        if let Some(secondary) = chunk.storages.get(1) {
            let cell = secondary.block(local[0], local[1], local[2])?;
            if cell.name != "minecraft:air" || !cell.states.is_empty() {
                return Err("Cube edits do not implement secondary-fluid transitions".into());
            }
        }
        let block = match target {
            Some(kind) => prototypes[kind].clone(),
            None => BlockState::new("minecraft:air".into(), vec![], old.version)?,
        };
        let edited = chunk_write::replace_block(&chunk, local, block, 0)?;
        encoded.insert(key, chunk_write::encode(&edited)?);
        columns.insert([x.div_euclid(16), z.div_euclid(16)]);
        changed += 1;
    }
    finish(snapshot, encoded, columns, changed, rules, palette)
}
