use crate::{REFERENCE, Result};
use mcbe_core::{biomes, leveldb};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::Path,
};
fn read(path: &str) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn integer(v: &Value) -> Result<i32> {
    Ok(i32::try_from(
        v.as_i64().ok_or("Integer biome coordinate required")?,
    )?)
}
fn identity(catalog: &Value, code: u32) -> Result<&str> {
    catalog["mapping"][code.to_string()]
        .as_str()
        .ok_or_else(|| format!("Unmeasured numeric biome identity {code}").into())
}
pub fn run(args: &[String], predict: bool) -> Result<bool> {
    if args.len() != 4 {
        return Err("predict-biomes|verify-biomes DB PLAN_OR_CAPTURE CATALOG".into());
    }
    let catalog = read(&args[3])?;
    if catalog["complete"] != true || catalog["executable_sha256"] != REFERENCE {
        return Err("Complete version-bound biome catalog required".into());
    }
    let input = read(&args[2])?;
    let snapshot = leveldb::read_snapshot(Path::new(&args[1]))?;
    let mut world = biomes::World::new(&snapshot);
    let mut positions = vec![];
    if predict {
        for column in input["columns"].as_array().ok_or("Missing columns")? {
            for offset in input["offsets"].as_array().ok_or("Missing offsets")? {
                for height in input["heights"].as_array().ok_or("Missing heights")? {
                    positions.push((
                        [
                            integer(&column["x"])?
                                .checked_add(integer(&offset[0])?)
                                .ok_or("Coordinate overflow")?,
                            integer(height)?,
                            integer(&column["z"])?
                                .checked_add(integer(&offset[1])?)
                                .ok_or("Coordinate overflow")?,
                        ],
                        None,
                    ));
                }
            }
        }
    } else {
        if input["complete"] != true || input["metadata"]["executable_sha256"] != REFERENCE {
            return Err("Complete known native biome capture required".into());
        }
        for row in input["biome_cells"]
            .as_array()
            .ok_or("Missing biome cells")?
        {
            positions.push((
                [
                    integer(&row["x"])?,
                    integer(&row["y"])?,
                    integer(&row["z"])?,
                ],
                Some(row["id"].as_str().ok_or("Missing native biome identity")?),
            ));
        }
    }
    if positions.is_empty() || positions.len() > 1_000_000 {
        return Err("Biome cell count outside bound".into());
    }
    let mut seen = HashSet::new();
    let mut rows = vec![];
    let mut failures = vec![];
    let mut counts = BTreeMap::new();
    for (position, expected) in positions {
        if !seen.insert(position) {
            return Err("Duplicate biome position".into());
        }
        let code = world.at(position)?;
        let id = identity(&catalog, code)?;
        *counts.entry(id).or_insert(0usize) += 1;
        rows.push(json!({"x":position[0],"y":position[1],"z":position[2],"code":code,"id":id}));
        if let Some(expected) = expected
            && id != expected
        {
            failures.push(json!({"position":position,"code":code,"actual":id,"expected":expected}));
        }
    }
    let complete = failures.is_empty();
    let mut report = json!({"complete":complete,"cells":rows.len(),"counts":counts,"failures":failures,"database_hashes":snapshot.files,"scope":"Saved Overworld numeric biome decoding and finite native identities; no generation or visual parity"});
    if predict {
        report["rows"] = json!(rows);
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(complete)
}
