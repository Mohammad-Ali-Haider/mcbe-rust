use super::Result;
use mcbe_core::{
    chunks::{self, StateValue, Subchunk},
    leveldb,
    motion::{Actor, Body, Vec3},
    world_collision::WorldScene,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
fn fingerprint(chunk: &Subchunk) -> String {
    fn string(out: &mut Vec<u8>, s: &str) {
        out.extend_from_slice(&(s.len() as u32).to_le_bytes());
        out.extend_from_slice(s.as_bytes());
    }
    let mut out = vec![chunk.y as u8, chunk.storages.len() as u8];
    for storage in &chunk.storages {
        out.push(storage.bits);
        for index in &storage.indices {
            out.extend_from_slice(&index.to_le_bytes());
        }
        out.extend_from_slice(&(storage.palette.len() as u32).to_le_bytes());
        for block in &storage.palette {
            string(&mut out, &block.name);
            out.extend_from_slice(&block.version.to_le_bytes());
            out.extend_from_slice(&(block.states.len() as u32).to_le_bytes());
            for (name, value) in &block.states {
                string(&mut out, name);
                match value {
                    StateValue::Bool(v) => out.extend_from_slice(&[1, u8::from(*v)]),
                    StateValue::Int(v) => {
                        out.push(3);
                        out.extend_from_slice(&v.to_le_bytes());
                    }
                    StateValue::String(v) => {
                        out.push(8);
                        string(&mut out, v);
                    }
                }
            }
        }
    }
    format!("{:x}", Sha256::digest(&out))
}
pub fn verify(db: &Path, expected: &Path) -> Result<bool> {
    let snapshot = leveldb::read_snapshot(db)?;
    let expected: Value = serde_json::from_slice(&fs::read(expected)?)?;
    let entries = expected["entries"].as_array().ok_or("Missing entries")?;
    if entries.len() != snapshot.entries.len() {
        return Err("Snapshot entry count differs".into());
    }
    let expected_files = expected["files"].as_object().ok_or("Missing files")?;
    if expected_files.len() != snapshot.files.len()
        || snapshot.files.iter().any(|(name, digest)| {
            expected_files.get(name).map(|v| &v["sha256"]) != Some(&json!(digest))
        })
    {
        return Err("Database identities differ".into());
    }
    let mut count = 0;
    for row in entries {
        let hex = row["key"].as_str().ok_or("Missing key")?;
        if !hex.is_ascii() || hex.len() % 2 != 0 {
            return Err("Invalid hex key".into());
        }
        let key = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let actual = snapshot.entries.get(&key).ok_or("Missing logical key")?;
        let hash = actual
            .value
            .as_ref()
            .map(|v| format!("{:x}", Sha256::digest(v)));
        if row["sequence"].as_u64() != Some(actual.sequence) || row["value_sha256"] != json!(hash) {
            return Err(format!("Logical entry differs: {hex}").into());
        }
        if let Some(y) = chunks::key_y(&key)
            && let Some(raw) = &actual.value
        {
            let chunk = chunks::decode(raw, Some(y))?;
            if row["subchunk_sha256"] != fingerprint(&chunk) {
                return Err(format!("Decoded subchunk differs: {hex}").into());
            }
            count += 1;
        }
    }
    if expected["subchunks"].as_u64() != Some(count) {
        return Err("Subchunk count differs".into());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":true,"language":"Rust","entries":entries.len(),"subchunks":count,"files":snapshot.files.len(),"bits":expected["bits"],"layers":expected["layers"]})
        )?
    );
    Ok(true)
}
pub fn run_move(args: &[String]) -> Result<bool> {
    let p = Vec3::new(args[2].parse()?, args[3].parse()?, args[4].parse()?);
    let v = Vec3::new(args[5].parse()?, args[6].parse()?, args[7].parse()?);
    let steps = args[8].parse::<usize>()?;
    if steps > 100_000 {
        return Err("Trace exceeds 100000 ticks".into());
    }
    let body = match args.get(9).map(String::as_str) {
        None | Some("probe") => Body::PROBE,
        Some("armor-stand") => Body::ARMOR_STAND,
        _ => return Err("Unknown actor body".into()),
    };
    let mut world = WorldScene::new(leveldb::read_snapshot(Path::new(&args[1]))?);
    let mut actor = Actor::new(p, body)?.impulse(v)?;
    let mut frames = Vec::new();
    for step in 0..=steps {
        frames.push(json!({"step":step,"observation":actor.observation()}));
        if step < steps {
            actor = actor.tick_solid(&world.scene_for(&actor)?)?;
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"language":"Rust","frames":frames,"sampled_cells":world.cells.len()})
        )?
    );
    Ok(true)
}
