use super::{REFERENCE, Result, blocks};
use mcbe_core::{
    blocks::Palette,
    heightmaps::{self, ColumnData, HeightRules},
    leveldb::{self, Snapshot},
    world_edit,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
};
type Maps = BTreeMap<[i32; 2], (ColumnData, [Option<i32>; 256])>;
fn load(args: &[String]) -> Result<(HeightRules, Palette, Value)> {
    let (measured, hash) = blocks::read(Path::new(&args[1]))?;
    let (palette, _, provenance) = blocks::load(Path::new(&args[2]), Path::new(&args[3]))?;
    if measured["complete"] != true
        || measured["palette_sha256"] != provenance["palette_sha256"]
        || measured["partial_palette_sha256"] != provenance["partial_sha256"]
    {
        return Err("Incomplete rules or palette mismatch".into());
    }
    Ok((
        HeightRules::read(&measured["rows"])?,
        palette,
        json!({"rules_sha256":hash,"palette":provenance}),
    ))
}
fn rebuild(snapshot: &Snapshot, rules: &mut HeightRules, palette: &Palette) -> Result<Maps> {
    let mut maps = BTreeMap::new();
    for (key, entry) in &snapshot.entries {
        if entry.value.is_some()
            && let Some([cx, cz]) = heightmaps::column_coordinates(key)
        {
            maps.insert(
                [cx, cz],
                heightmaps::rebuild_column(snapshot, cx, cz, rules, palette)?,
            );
        }
    }
    if maps.is_empty() {
        return Err("No Overworld columns".into());
    }
    Ok(maps)
}
pub fn query(args: &[String]) -> Result<bool> {
    let (mut rules, palette, provenance) = load(args)?;
    let snapshot = leveldb::read_snapshot(Path::new(&args[4]))?;
    let cx = args[5].parse()?;
    let cz = args[6].parse()?;
    let (column, tops) = heightmaps::rebuild_column(&snapshot, cx, cz, &mut rules, &palette)?;
    let saved = ColumnData::decode(
        snapshot
            .get(&heightmaps::column_key(cx, cz))
            .ok_or("Missing column")?,
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"chunk":[cx,cz],"index_order":"z * 16 + x","stored":saved.heights.to_vec(),"rebuilt":column.heights.to_vec(),"topmost_y":tops.to_vec(),"changed":saved.heights.iter().zip(column.heights).filter(|(a,b)|**a!=*b).count(),"biome_suffix_preserved":saved.biome_bytes==column.biome_bytes,"provenance":provenance})
        )?
    );
    Ok(true)
}
pub fn repair(args: &[String]) -> Result<bool> {
    let (mut rules, palette, provenance) = load(args)?;
    let source = Path::new(&args[4]);
    let snapshot = leveldb::read_snapshot(&source.join("db"))?;
    let maps = rebuild(&snapshot, &mut rules, &palette)?;
    let mut updates = vec![];
    let mut changed = 0;
    // Preserve database-key order for reproducible write batches.
    for (key, entry) in &snapshot.entries {
        if let Some([cx, cz]) = heightmaps::column_coordinates(key)
            && let Some(value) = &entry.value
        {
            let original = ColumnData::decode(value)?;
            let rebuilt = &maps[&[cx, cz]].0;
            if original.biome_bytes != rebuilt.biome_bytes {
                return Err("Biome suffix changed".into());
            }
            changed += original
                .heights
                .iter()
                .zip(rebuilt.heights)
                .filter(|(a, b)| **a != *b)
                .count();
            let encoded = rebuilt.encode();
            if encoded != *value {
                updates.push((key.clone(), Some(encoded)));
            }
        }
    }
    if updates.is_empty() {
        return Err("No heightmap corrections needed; no copy created".into());
    }
    let edit = world_edit::edit_copy(source, Path::new(&args[5]), &updates)?;
    let byte_identical = if let Some(path) = args.get(6) {
        Some(edit.destination_hashes == world_edit::tree_hashes(Path::new(path))?)
    } else {
        None
    };
    let complete = byte_identical != Some(false);
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":complete,"changed_columns":changed,"changed_chunks":updates.len(),"rebuilt_columns":maps.len()*256,"entire_world_byte_identical":byte_identical,"edit":edit,"provenance":provenance,"scope":"Measured heightmap repair only; lighting and other save invariants remain incomplete"})
        )?
    );
    Ok(complete)
}
fn xz(row: &Value) -> Result<[i32; 2]> {
    Ok([
        i32::try_from(row["x"].as_i64().ok_or("Missing x")?)?,
        i32::try_from(row["z"].as_i64().ok_or("Missing z")?)?,
    ])
}
pub fn verify(args: &[String]) -> Result<bool> {
    let (mut rules, palette, mut provenance) = load(args)?;
    let snapshot = leveldb::read_snapshot(Path::new(&args[4]))?;
    let (capture, capture_hash) = blocks::read(Path::new(&args[5]))?;
    let (predictions, predictions_hash) = blocks::read(Path::new(&args[6]))?;
    provenance["capture_sha256"] = json!(capture_hash);
    provenance["predictions_sha256"] = json!(predictions_hash);
    let rows = capture["height_columns"]
        .as_array()
        .ok_or("Missing native height columns")?;
    let predictions = predictions.as_array().ok_or("Missing predictions")?;
    if capture["complete"] != true
        || capture["errors"].as_array().is_none_or(|a| !a.is_empty())
        || capture["metadata"]["executable_sha256"] != REFERENCE
        || capture["completion_record"]["kind"] != "done"
        || capture["completion_record"]["heightColumns"].as_u64() != Some(rows.len() as u64)
        || rows.len() != predictions.len()
        || rows.is_empty()
    {
        return Err("Incomplete/mismatched height observations".into());
    }
    let mut observed = HashMap::new();
    for row in rows {
        if observed.insert(xz(row)?, row).is_some() {
            return Err("Duplicate native height position".into());
        }
    }
    let maps = rebuild(&snapshot, &mut rules, &palette)?;
    let mut map_failures = 0;
    let mut prediction_failures = 0;
    let mut errors = vec![];
    for ([cx, cz], (column, _)) in &maps {
        let original = ColumnData::decode(
            snapshot
                .get(&heightmaps::column_key(*cx, *cz))
                .ok_or("Missing saved column")?,
        )?;
        if original.biome_bytes != column.biome_bytes {
            return Err("Biome suffix changed".into());
        }
        for i in 0..256 {
            if original.heights[i] != column.heights[i] {
                map_failures += 1;
                if errors.len() < 10 {
                    errors.push(json!({"chunk":[cx,cz],"index":i,"stored":original.heights[i],"rebuilt":column.heights[i]}));
                }
            }
        }
    }
    let mut seen = HashSet::new();
    for prediction in predictions {
        let pos = xz(prediction)?;
        if !seen.insert(pos) {
            return Err("Duplicate prediction position".into());
        }
        let native = observed
            .get(&pos)
            .ok_or("Native prediction position absent")?;
        let chunk = [pos[0].div_euclid(16), pos[1].div_euclid(16)];
        let local = [pos[0].rem_euclid(16) as u8, pos[1].rem_euclid(16) as u8];
        let (column, tops) = maps
            .get(&chunk)
            .ok_or("Predicted column absent from snapshot")?;
        let top = tops[local[1] as usize * 16 + local[0] as usize];
        let native_top = if native["top"].is_null() {
            None
        } else {
            Some(i32::try_from(
                native["top"]["y"].as_i64().ok_or("Invalid native top")?,
            )?)
        };
        if json!(column.raw_height(local[0], local[1])?) != prediction["raw_height"]
            || json!(top) != prediction["top_y"]
            || native_top != top
        {
            prediction_failures += 1;
            if errors.len() < 10 {
                errors.push(
                    json!({"prediction":prediction,"native_top":native_top,"modeled_top":top}),
                );
            }
        }
    }
    let complete = map_failures == 0 && prediction_failures == 0;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":complete,"language":"Rust","height_comparisons":maps.len()*256,"predicted_columns":predictions.len(),"map_failures":map_failures,"prediction_failures":prediction_failures,"first_errors":errors,"database_files":snapshot.files,"provenance":provenance,"scope":"Rust replay of retained height/topmost observations, not a new native experiment"})
        )?
    );
    Ok(complete)
}
