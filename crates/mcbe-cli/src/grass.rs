use crate::Result;
use mcbe_core::{grass, resources::Pack};
use serde_json::{Value, json};
use std::{collections::HashSet, fs, path::Path};
pub fn inspect(args: &[String]) -> Result<bool> {
    if args.len() != 4 {
        return Err("grass-tint RESOURCE_PACK BEHAVIOR_PACK MINECRAFT_BIOME".into());
    }
    let mut resource = Pack::open(Path::new(&args[1]))?;
    let mut behavior = Pack::open(Path::new(&args[2]))?;
    let tint = grass::load(&mut resource, &mut behavior, &args[3])?;
    println!("{}", serde_json::to_string_pretty(&tint.provenance)?);
    Ok(true)
}
pub fn verify(args: &[String]) -> Result<bool> {
    if args.len() != 2 {
        return Err("verify-grass FIXTURE".into());
    }
    let fixture: Value = serde_json::from_slice(&fs::read(&args[1])?)?;
    if fixture["complete"] != true {
        return Err("Complete independent grass fixture required".into());
    }
    let mut resource = Pack::open(Path::new(
        fixture["resource"]
            .as_str()
            .ok_or("Missing resource pack")?,
    ))?;
    let mut behavior = Pack::open(Path::new(
        fixture["behavior"]
            .as_str()
            .ok_or("Missing behavior pack")?,
    ))?;
    let rows = fixture["rows"].as_array().ok_or("Missing grass rows")?;
    if rows.is_empty() || rows.len() > 256 {
        return Err("Grass fixture count outside bounds".into());
    }
    let mut seen = HashSet::new();
    let mut failures = vec![];
    for expected in rows {
        let biome = expected["biome"].as_str().ok_or("Missing biome")?;
        if !seen.insert(biome) {
            return Err("Duplicate biome fixture".into());
        }
        let tint = grass::load(&mut resource, &mut behavior, biome)?;
        let p = &tint.provenance;
        let actual = json!({"biome":biome,"rgb":tint.rgb,"color_source":p["color_source"],"coordinates":p["coordinates"],"temperature_bits":p["temperature_bits"],"downfall_bits":p["downfall_bits"],"behavior_payload_sha256":p["behavior_origins"][0]["payload_sha256"],"client_payload_sha256":p["client_origins"][0]["payload_sha256"],"colormap_rgba_sha256":p["colormap_rgba_sha256"]});
        if actual != *expected {
            failures.push(json!({"biome":biome,"actual":actual,"expected":expected}));
        }
    }
    resource.verify_unchanged()?;
    behavior.verify_unchanged()?;
    let complete = failures.is_empty();
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":complete,"biomes":rows.len(),"failures":failures,"resource_hashes":resource.source_hashes(),"behavior_hashes":behavior.source_hashes(),"scope":"Independent resource/Pillow comparison of a provisional climate color model; no native visual parity"})
        )?
    );
    Ok(complete)
}
