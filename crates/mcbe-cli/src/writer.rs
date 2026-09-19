use super::Result;
use mcbe_core::{
    chunk_write,
    chunks::{self, BlockState, StateValue, Storage, Subchunk, World},
    leveldb,
    world_edit::{self, Updates},
};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path};
fn hex_key(value: &Value) -> Result<Vec<u8>> {
    let text = value.as_str().ok_or("Missing hex key")?;
    if !text.is_ascii() || !text.len().is_multiple_of(2) {
        return Err("Invalid hex key".into());
    }
    Ok((0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16))
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
pub fn verify_chunks(path: &Path) -> Result<bool> {
    let snapshot = leveldb::read_snapshot(path)?;
    let mut count = 0;
    let mut bytes = 0;
    let mut exact = 0;
    for (key, entry) in &snapshot.entries {
        if let Some(y) = chunks::key_y(key)
            && let Some(raw) = &entry.value
        {
            let decoded = chunks::decode(raw, Some(y))?;
            let encoded = chunk_write::encode(&decoded)?;
            if chunks::decode(&encoded, Some(y))? != decoded {
                return Err("Subchunk semantic round trip differs".into());
            }
            count += 1;
            bytes += raw.len();
            exact += usize::from(encoded == *raw);
        }
    }
    if count == 0 {
        return Err("No subchunks in snapshot".into());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":count==exact,"subchunks":count,"byte_exact":exact,"bytes":bytes,"database_files":snapshot.files})
        )?
    );
    Ok(count == exact)
}
pub fn copy_block(args: &[String]) -> Result<bool> {
    let source = Path::new(&args[1]);
    let destination = Path::new(&args[2]);
    let target: [i32; 3] = [args[3].parse()?, args[4].parse()?, args[5].parse()?];
    let donor = [args[6].parse()?, args[7].parse()?, args[8].parse()?];
    let mut world = World::new(leveldb::read_snapshot(&source.join("db"))?);
    let block = world
        .block(donor, 0)?
        .ok_or("Donor has no stored primary block")?;
    world
        .block(target, 0)?
        .ok_or("Target must have an existing subchunk")?;
    let key = chunks::key(
        target[0].div_euclid(16),
        target[2].div_euclid(16),
        target[1].div_euclid(16) as i8,
    );
    let chunk = chunks::decode(
        world.snapshot.get(&key).ok_or("Missing target subchunk")?,
        Some(target[1].div_euclid(16) as i8),
    )?;
    let local = target.map(|v| v.rem_euclid(16) as u8);
    let edited = chunk_write::replace_block(&chunk, local, block, 0)?;
    let report = world_edit::edit_copy(
        source,
        destination,
        &vec![(key, Some(chunk_write::encode(&edited)?))],
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"edit":report,"target":target,"donor":donor,"scope":"Primary block copy only; heightmap, lighting, neighbor and fluid updates are not maintained"})
        )?
    );
    Ok(true)
}
pub fn verify_experiment(args: &[String]) -> Result<bool> {
    let source = Path::new(&args[1]);
    let expected = Path::new(&args[2]);
    let destination = Path::new(&args[3]);
    let (plan, plan_hash) = super::blocks::read(Path::new(&args[4]))?;
    let snapshot = leveldb::read_snapshot(&source.join("db"))?;
    let mut world = World::new(leveldb::read_snapshot(&source.join("db"))?);
    let version = world
        .block([0, 80, 0], 0)?
        .ok_or("Missing version sample")?
        .version;
    let mut updates: Updates = vec![];
    let mut seen = HashSet::new();
    let mut roundtrips = 0;
    for raw_key in plan["ordered_keys"]
        .as_array()
        .ok_or("Missing ordered keys")?
    {
        let key = hex_key(raw_key)?;
        if !seen.insert(key.clone()) {
            return Err("Duplicate ordered key".into());
        }
        let y = chunks::key_y(&key).ok_or("Not a subchunk key")?;
        let original = snapshot.get(&key).ok_or("Missing source subchunk")?;
        let chunk = chunks::decode(original, Some(y))?;
        let encoded = chunk_write::encode(&chunk)?;
        if encoded != original {
            return Err("Original subchunk byte round trip differs".into());
        }
        updates.push((key, Some(encoded)));
        roundtrips += 1;
    }
    let actual_keys: HashSet<_> = snapshot
        .entries
        .iter()
        .filter(|(k, v)| chunks::key_y(k).is_some() && v.value.is_some())
        .map(|(k, _)| k.clone())
        .collect();
    if seen != actual_keys {
        return Err("Plan does not cover every source subchunk".into());
    }
    let colors = [
        "white",
        "orange",
        "magenta",
        "light_blue",
        "yellow",
        "lime",
        "pink",
        "gray",
        "light_gray",
        "cyan",
        "purple",
        "blue",
        "brown",
        "green",
        "red",
        "black",
    ];
    let mut blocks = vec![];
    for material in ["concrete", "wool"] {
        for color in colors {
            blocks.push(BlockState::new(
                format!("minecraft:{color}_{material}"),
                vec![],
                version,
            )?);
        }
    }
    for wood in ["oak", "spruce", "birch"] {
        for axis in ["x", "y", "z"] {
            blocks.push(BlockState::new(
                format!("minecraft:{wood}_log"),
                vec![("pillar_axis".into(), StateValue::String(axis.into()))],
                version,
            )?);
        }
    }
    for (cx, cz, cy, bits, count) in [(0, 0, 5, 3, 5), (1, 0, 4, 5, 20), (-1, -1, 6, 6, 40)] {
        let mut indices = vec![];
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..16 {
                    indices.push(((x * 17 + z * 7 + y * 3) % count) as u16);
                }
            }
        }
        let chunk = Subchunk {
            y: cy,
            storages: vec![Storage {
                bits,
                indices,
                palette: blocks[..count].to_vec(),
            }],
        };
        let key = chunks::key(cx, cz, cy);
        let target = updates
            .iter_mut()
            .find(|(k, _)| *k == key)
            .ok_or("Pattern subchunk absent from source")?;
        target.1 = Some(chunk_write::encode(&chunk)?);
    }
    let report = world_edit::edit_copy(source, destination, &updates)?;
    let expected_hashes = world_edit::tree_hashes(expected)?;
    let identical = report.destination_hashes == expected_hashes;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":identical,"language":"Rust","source_roundtrips":roundtrips,"entire_world_byte_identical":identical,"plan_sha256":plan_hash,"edit":report,"scope":"Independent Rust recreation of an existing native-accepted write experiment; no new native run"})
        )?
    );
    Ok(identical)
}
