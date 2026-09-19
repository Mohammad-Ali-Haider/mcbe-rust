use super::{REFERENCE, Result};
use mcbe_core::{
    blocks::{self, BlockError, Palette},
    chunks::World,
    leveldb,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::Path,
};

pub(crate) fn read(path: &Path) -> Result<(Value, String)> {
    if fs::metadata(path)?.len() > 128 * 1024 * 1024 {
        return Err("Capture exceeds 128 MiB".into());
    }
    let raw = fs::read(path)?;
    Ok((
        serde_json::from_slice(&raw)?,
        format!("{:x}", Sha256::digest(&raw)),
    ))
}
pub(crate) fn load(palette: &Path, partial: &Path) -> Result<(Palette, Value, Value)> {
    let (full, full_hash) = read(palette)?;
    let (partial, partial_hash) = read(partial)?;
    let palette = Palette::from_reports(&full, &partial)?;
    if palette.reference() != REFERENCE {
        return Err("Unknown reference executable".into());
    }
    Ok((
        palette,
        full,
        json!({"palette_sha256":full_hash,"partial_sha256":partial_hash}),
    ))
}
fn operation(palette: &Palette, row: &Value) -> blocks::BlockResult<Value> {
    let id = row["id"]
        .as_str()
        .ok_or_else(|| BlockError::Unsupported("Identifier coercion".into()))?;
    let states = blocks::states(&row["states"])?;
    match row["operation"].as_str() {
        Some("resolve") => Ok(json!(palette.resolve(id, &states)?.as_ref())),
        Some("withState") => {
            if states.len() != 1 {
                return Err(BlockError::Unsupported("Expected one assignment".into()));
            }
            let initial = if row.get("baseStates").is_some() {
                blocks::states(&row["baseStates"])?
            } else {
                vec![]
            };
            let base = palette.resolve(id, &initial)?;
            let next = palette.with_state(&base, &states[0].0, states[0].1.clone())?;
            let mut value = json!(next.as_ref());
            value["original"] = json!(base.as_ref());
            Ok(value)
        }
        _ => Err(BlockError::Unsupported("Unmeasured operation".into())),
    }
}
pub fn verify(palette_path: &Path, partial_path: &Path, capture_path: &Path) -> Result<bool> {
    let (palette, _, mut provenance) = load(palette_path, partial_path)?;
    let (capture, hash) = read(capture_path)?;
    provenance["capture_sha256"] = json!(hash);
    if capture["complete"] != true
        || capture["errors"].as_array().is_none_or(|a| !a.is_empty())
        || capture["metadata"]["executable_sha256"] != palette.reference()
        || capture["completion_record"]["kind"] != "done"
    {
        return Err("Incomplete or mismatched capture".into());
    }
    let empty = vec![];
    let permutations = capture["permutations"].as_array().unwrap_or(&empty);
    let cases = capture["block_cases"].as_array().unwrap_or(&empty);
    if capture["completion_record"]["blockCases"].as_u64() != Some(cases.len() as u64)
        || (!permutations.is_empty()
            && capture["completion_record"]["permutations"].as_u64()
                != Some(permutations.len() as u64))
        || (permutations.is_empty() && cases.is_empty())
    {
        return Err("Capture counts differ".into());
    }
    let mut failures = 0;
    let mut first_errors = vec![];
    for (index, row) in permutations.iter().enumerate() {
        let actual = palette.resolve(
            row["inputId"].as_str().ok_or("Missing input id")?,
            &blocks::states(&row["inputStates"])?,
        )?;
        let expected = json!({"id":row["id"],"states":row["states"],"tags":row["tags"]});
        if json!(actual.as_ref()) != expected {
            failures += 1;
            if first_errors.len() < 10 {
                first_errors.push(
                    json!({"permutation":index,"actual":actual.as_ref(),"expected":expected}),
                );
            }
        }
    }
    let mut checked = 0;
    let mut exclusions: BTreeMap<String, usize> = BTreeMap::new();
    for (index, row) in cases.iter().enumerate() {
        let actual = match operation(&palette, row) {
            Ok(value) => json!({"success":true,"result":value}),
            Err(BlockError::Operation(_)) => json!({"success":false,"category":"Error"}),
            Err(BlockError::Unsupported(reason)) => {
                *exclusions.entry(reason).or_default() += 1;
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let expected = if row["success"] == true {
            json!({"success":true,"result":row["result"]})
        } else {
            json!({"success":false,"category":row["error"].as_str().ok_or("Missing error")?.split(':').next()})
        };
        checked += 1;
        if actual != expected {
            failures += 1;
            if first_errors.len() < 10 {
                first_errors
                    .push(json!({"case":index,"input":row,"actual":actual,"expected":expected}));
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":failures==0,"language":"Rust","permutations":permutations.len(),"operations":checked,"exclusions":exclusions,"failures":failures,"first_errors":first_errors,"canonical_permutations":palette.canonical_count(),"provenance":provenance,"scope":"Port regression of existing reference observations; unsupported inputs are explicitly excluded"})
        )?
    );
    Ok(failures == 0)
}
pub fn query(args: &[String]) -> Result<bool> {
    let (palette, _, _) = load(Path::new(&args[1]), Path::new(&args[2]))?;
    let input = serde_json::from_str(&args[4])?;
    let base = palette.resolve(&args[3], &blocks::states(&input)?)?;
    let output = if args.len() == 7 {
        palette.with_state(
            &base,
            &args[5],
            blocks::state_value(&serde_json::from_str(&args[6])?)?,
        )?
    } else {
        base
    };
    println!("{}", serde_json::to_string_pretty(output.as_ref())?);
    Ok(true)
}
pub fn verify_world(args: &[String]) -> Result<bool> {
    let (palette, _, mut provenance) = load(Path::new(&args[1]), Path::new(&args[2]))?;
    let (capture, hash) = read(Path::new(&args[4]))?;
    provenance["capture_sha256"] = json!(hash);
    let rows = capture["world_blocks"].as_array().ok_or("Missing blocks")?;
    if capture["complete"] != true
        || capture["errors"].as_array().is_none_or(|a| !a.is_empty())
        || capture["completion_record"]["kind"] != "done"
        || rows.is_empty()
        || capture["metadata"]["executable_sha256"] != palette.reference()
        || capture["completion_record"]["worldBlocks"].as_u64() != Some(rows.len() as u64)
    {
        return Err("Incomplete/mismatched world capture".into());
    }
    let mut world = World::new(leveldb::read_snapshot(Path::new(&args[3]))?);
    let mut seen = HashSet::new();
    let mut errors = vec![];
    let mut failures = 0;
    let mut normalized = 0;
    for row in rows {
        let mut pos = [0; 3];
        for (i, name) in ["x", "y", "z"].iter().enumerate() {
            pos[i] = i32::try_from(row[name].as_i64().ok_or("Invalid coordinate")?)?;
        }
        if !seen.insert(pos) {
            return Err("Duplicate captured cell".into());
        }
        let saved = world.block(pos, 0)?.ok_or("Captured subchunk absent")?;
        let result = palette.resolve_saved(&saved)?;
        let actual = json!({"id":result.id(),"states":result.states()});
        let expected = json!({"id":row["id"],"states":row["states"]});
        if json!(saved.states) != row["states"] {
            normalized += 1;
        }
        if actual != expected {
            failures += 1;
            if errors.len() < 10 {
                errors.push(json!({"position":pos,"actual":actual,"expected":expected}));
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":failures==0,"language":"Rust","comparisons":rows.len(),"failures":failures,"first_errors":errors,"raw_state_differences":normalized,"database_files":world.snapshot.files,"provenance":provenance,"scope":"Saved-state normalization against retained native cells; no new native experiment"})
        )?
    );
    Ok(failures == 0)
}

pub fn block_at(args: &[String]) -> Result<bool> {
    let (palette, _, provenance) = load(Path::new(&args[1]), Path::new(&args[2]))?;
    let mut world = World::new(leveldb::read_snapshot(Path::new(&args[3]))?);
    let position = [args[4].parse()?, args[5].parse()?, args[6].parse()?];
    let saved = world
        .block(position, 0)?
        .ok_or("No stored primary block at this position")?;
    let permutation = palette.resolve_saved(&saved)?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"position":position,"saved":saved,"canonical":permutation.as_ref(),"provenance":provenance})
        )?
    );
    Ok(true)
}
