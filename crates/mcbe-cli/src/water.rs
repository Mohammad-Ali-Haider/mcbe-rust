use super::{REFERENCE, Result, blocks};
use mcbe_core::{
    blocks::{self as block_model, Palette},
    chunks::{BlockState, Storage, Subchunk},
    heightmaps::{self, ColumnData, HeightRules},
    leveldb,
    waterlogging::{self, StoredCell, WaterError, WaterRules},
    world_edit,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
fn load(args: &[String]) -> Result<(WaterRules, HeightRules, Palette, Value)> {
    let (water, whash) = blocks::read(Path::new(&args[1]))?;
    let (height, hhash) = blocks::read(Path::new(&args[2]))?;
    let (palette, _, provenance) = blocks::load(Path::new(&args[3]), Path::new(&args[4]))?;
    for rules in [&water, &height] {
        if rules["complete"] != true
            || rules["palette_sha256"] != provenance["palette_sha256"]
            || rules["partial_palette_sha256"] != provenance["partial_sha256"]
        {
            return Err("Incomplete rules or mismatched palette".into());
        }
    }
    Ok((
        WaterRules::read(&water["rows"])?,
        HeightRules::read(&height["rows"])?,
        palette,
        json!({"water_rules_sha256":whash,"height_rules_sha256":hhash,"palette":provenance}),
    ))
}
fn position(row: &Value) -> Result<[i32; 3]> {
    Ok([
        i32::try_from(row["x"].as_i64().ok_or("Missing x")?)?,
        i32::try_from(row["y"].as_i64().ok_or("Missing y")?)?,
        i32::try_from(row["z"].as_i64().ok_or("Missing z")?)?,
    ])
}
fn observation(cell: &StoredCell, palette: &Palette) -> Result<Value> {
    let p = palette.resolve_saved(&cell.primary)?;
    Ok(json!({"id":p.id(),"states":p.states(),"waterlogged":cell.waterlogged()?}))
}
fn expected(row: &Value) -> Value {
    json!({"id":row["id"],"states":row["states"],"waterlogged":row["waterlogged"]})
}
fn layer_equal(a: Option<&BlockState>, b: Option<&BlockState>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.name == b.name && a.states == b.states && a.version == b.version,
        _ => false,
    }
}
pub fn inspect(args: &[String]) -> Result<bool> {
    let (palette, _, provenance) = blocks::load(Path::new(&args[1]), Path::new(&args[2]))?;
    let snapshot = leveldb::read_snapshot(Path::new(&args[3]))?;
    let position = [args[4].parse()?, args[5].parse()?, args[6].parse()?];
    let cell = waterlogging::read_cell(&snapshot, position)?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"position":position,"primary":cell.primary,"secondary":cell.secondary,"observation":observation(&cell,&palette)?,"provenance":provenance})
        )?
    );
    Ok(true)
}
pub fn edit(args: &[String]) -> Result<bool> {
    let (rules, mut heights, palette, provenance) = load(args)?;
    let source = Path::new(&args[5]);
    let destination = Path::new(&args[6]);
    let position: [i32; 3] = [args[7].parse()?, args[8].parse()?, args[9].parse()?];
    let value = match args[10].as_str() {
        "true" => true,
        "false" => false,
        _ => return Err("Waterlogged value must be true or false".into()),
    };
    let snapshot = leveldb::read_snapshot(&source.join("db"))?;
    let before = observation(&waterlogging::read_cell(&snapshot, position)?, &palette)?;
    let updates = waterlogging::prepare_edits(
        &snapshot,
        &[(position, value)],
        &[[position[0].div_euclid(16), position[2].div_euclid(16)]],
        &rules,
        &mut heights,
        &palette,
    )?;
    let report = world_edit::edit_copy(source, destination, &updates)?;
    let written = leveldb::read_snapshot(&destination.join("db"))?;
    let after = observation(&waterlogging::read_cell(&written, position)?, &palette)?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":true,"edit":report,"position":position,"before":before,"after":after,"provenance":provenance,"scope":"Measured saved waterlogging and height repair; no fluid ticks or lighting"})
        )?
    );
    Ok(true)
}
pub fn recreate(args: &[String]) -> Result<bool> {
    let (rules, mut heights, palette, provenance) = load(args)?;
    let source = Path::new(&args[5]);
    let destination = Path::new(&args[6]);
    let expected_world = Path::new(&args[7]);
    let (capture, capture_hash) = blocks::read(Path::new(&args[8]))?;
    let (plan, plan_hash) = blocks::read(Path::new(&args[9]))?;
    let cases = capture["water_cases"]
        .as_array()
        .ok_or("Missing initial water cases")?;
    if capture["complete"] != true
        || capture["metadata"]["executable_sha256"] != REFERENCE
        || cases.len() != 54
    {
        return Err("Invalid recreation capture".into());
    }
    let snapshot = leveldb::read_snapshot(&source.join("db"))?;
    let mut changes = vec![];
    let mut positions = HashSet::new();
    for case in cases {
        let position = position(case)?;
        if !positions.insert(position) {
            return Err("Duplicate water case position".into());
        }
        if matches!(
            case["id"].as_str(),
            Some(
                "minecraft:scaffolding" | "minecraft:stone" | "minecraft:glass" | "minecraft:water"
            )
        ) {
            continue;
        }
        changes.push((
            position,
            !waterlogging::read_cell(&snapshot, position)?.waterlogged()?,
        ));
    }
    let columns: Vec<[i32; 2]> = serde_json::from_value(plan["column_order"].clone())?;
    let actual: HashSet<_> = snapshot
        .entries
        .iter()
        .filter(|(_, e)| e.value.is_some())
        .filter_map(|(k, _)| heightmaps::column_coordinates(k))
        .collect();
    if actual != columns.iter().copied().collect() {
        return Err("Column plan is incomplete".into());
    }
    let updates = waterlogging::prepare_edits(
        &snapshot,
        &changes,
        &columns,
        &rules,
        &mut heights,
        &palette,
    )?;
    let edit = world_edit::edit_copy(source, destination, &updates)?;
    let identical = edit.destination_hashes == world_edit::tree_hashes(expected_world)?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":identical && changes.len()==42,"changed_cells":changes.len(),"controls":cases.len()-changes.len(),"entire_world_byte_identical":identical,"edit":edit,"provenance":provenance,"capture_sha256":capture_hash,"plan_sha256":plan_hash,"scope":"Rust recreation of retained native-accepted wet/dry experiment; no new native run"})
        )?
    );
    Ok(identical && changes.len() == 42)
}
pub fn verify(args: &[String]) -> Result<bool> {
    let (rules, mut heights, palette, mut provenance) = load(args)?;
    let snapshot = leveldb::read_snapshot(Path::new(&args[5]))?;
    let (capture, capture_hash) = blocks::read(Path::new(&args[6]))?;
    provenance["capture_sha256"] = json!(capture_hash);
    let cases = capture["water_cases"]
        .as_array()
        .ok_or("Missing water cases")?;
    if capture["complete"] != true
        || capture["metadata"]["executable_sha256"] != REFERENCE
        || capture["errors"].as_array().is_none_or(|a| !a.is_empty())
        || capture["completion_record"]["kind"] != "done"
        || capture["completion_record"]["waterCases"].as_u64() != Some(cases.len() as u64)
        || cases.is_empty()
    {
        return Err("Incomplete/mismatched water capture".into());
    }
    let predictions = if args.len() == 9 {
        let (p, h) = blocks::read(Path::new(&args[7]))?;
        provenance["predictions_sha256"] = json!(h);
        Some(p)
    } else {
        None
    };
    let previous = if args.len() == 9 {
        Some(leveldb::read_snapshot(Path::new(&args[8]))?)
    } else {
        None
    };
    let mut native = HashMap::new();
    for row in cases {
        if native.insert(position(row)?, row).is_some() {
            return Err("Duplicate native cell".into());
        }
    }
    let mut operations = 0;
    let mut failures = 0;
    let mut layer_failures = 0;
    let mut errors = vec![];
    let expected_cells = if let Some(p) = &predictions {
        p["cells"].as_array().ok_or("Missing predicted cells")?
    } else {
        cases
    };
    if expected_cells.len() != cases.len() {
        return Err("Cell count mismatch".into());
    }
    let mut seen = HashSet::new();
    for row in expected_cells {
        let pos = position(row)?;
        if !seen.insert(pos) {
            return Err("Duplicate expected cell".into());
        }
        let recorded = native.get(&pos).ok_or("Missing native position")?;
        let stored = waterlogging::read_cell(&snapshot, pos)?;
        if observation(&stored, &palette)? != expected(row) || expected(recorded) != expected(row) {
            failures += 1;
            if errors.len() < 10 {
                errors.push(json!({"cell":pos,"actual":observation(&stored,&palette)?,"expected":expected(row)}));
            }
        }
        if let Some(previous) = &previous {
            let old = waterlogging::read_cell(previous, pos)?;
            if !layer_equal(Some(&old.primary), Some(&stored.primary))
                || !layer_equal(old.secondary.as_ref(), stored.secondary.as_ref())
            {
                layer_failures += 1;
            }
        } else {
            let before = &row["before"];
            if before["waterlogged"] != false {
                return Err("Unexpected initially wet case".into());
            }
            let primary = BlockState::new(
                before["id"].as_str().ok_or("Missing before id")?.into(),
                block_model::states(&before["states"])?,
                stored.primary.version,
            )?;
            let mut chunk = Subchunk {
                y: 0,
                storages: vec![Storage {
                    bits: 1,
                    indices: vec![0; 4096],
                    palette: vec![primary],
                }],
            };
            for call in row["calls"].as_array().ok_or("Missing setter calls")? {
                operations += 1;
                let error = match rules.set(
                    &chunk,
                    [0, 0, 0],
                    call["value"]
                        .as_bool()
                        .ok_or("Unmeasured non-Boolean call")?,
                    &palette,
                ) {
                    Ok(next) => {
                        chunk = next;
                        None
                    }
                    Err(WaterError::Rejected) => Some(WaterError::Rejected.to_string()),
                    Err(e) => return Err(e.into()),
                };
                let actual =
                    observation(&waterlogging::cell_from_chunk(&chunk, [0, 0, 0])?, &palette)?;
                if json!(error.is_none()) != call["success"]
                    || (error.is_some() && json!(error) != call["error"])
                    || actual != expected(&call["result"])
                {
                    failures += 1;
                    if errors.len() < 10 {
                        errors.push(
                            json!({"case":row["index"],"call":call,"actual":actual,"error":error}),
                        );
                    }
                }
            }
        }
    }
    let mut maps = HashMap::new();
    let mut height_failures = 0;
    let mut height_comparisons = 0;
    for (key, entry) in &snapshot.entries {
        if let Some(pos) = heightmaps::column_coordinates(key)
            && let Some(bytes) = &entry.value
        {
            let result =
                heightmaps::rebuild_column(&snapshot, pos[0], pos[1], &mut heights, &palette)?;
            let saved = ColumnData::decode(bytes)?;
            if saved.biome_bytes != result.0.biome_bytes {
                return Err("Biome suffix changed".into());
            }
            height_comparisons += 256;
            height_failures += saved
                .heights
                .iter()
                .zip(result.0.heights)
                .filter(|(a, b)| **a != *b)
                .count();
            maps.insert(pos, result);
        }
    }
    let mut top_comparisons = 0;
    if let Some(predictions) = &predictions {
        let columns = capture["height_columns"]
            .as_array()
            .ok_or("Missing native heights")?;
        let predicted = predictions["columns"]
            .as_array()
            .ok_or("Missing predicted heights")?;
        if columns.len() != predicted.len()
            || capture["completion_record"]["heightColumns"].as_u64() != Some(columns.len() as u64)
        {
            return Err("Height count mismatch".into());
        }
        let mut native_columns = HashMap::new();
        for row in columns {
            let x = i32::try_from(row["x"].as_i64().ok_or("Missing x")?)?;
            let z = i32::try_from(row["z"].as_i64().ok_or("Missing z")?)?;
            if native_columns.insert([x, z], row).is_some() {
                return Err("Duplicate native height".into());
            }
        }
        let mut seen = HashSet::new();
        for row in predicted {
            let x = i32::try_from(row["x"].as_i64().ok_or("Missing x")?)?;
            let z = i32::try_from(row["z"].as_i64().ok_or("Missing z")?)?;
            if !seen.insert([x, z]) {
                return Err("Duplicate predicted height".into());
            }
            let native = native_columns.get(&[x, z]).ok_or("Missing native height")?;
            let (map, tops) = maps
                .get(&[x.div_euclid(16), z.div_euclid(16)])
                .ok_or("Missing rebuilt column")?;
            let i = z.rem_euclid(16) as usize * 16 + x.rem_euclid(16) as usize;
            if json!(map.heights[i]) != row["raw_height"]
                || json!(tops[i]) != row["top_y"]
                || native["top"]["y"] != row["top_y"]
            {
                height_failures += 1;
            }
            top_comparisons += 1;
        }
    } else {
        for row in cases {
            let [x, _, z] = position(row)?;
            let top = maps
                .get(&[x.div_euclid(16), z.div_euclid(16)])
                .ok_or("Missing column")?
                .1[z.rem_euclid(16) as usize * 16 + x.rem_euclid(16) as usize];
            if json!(top) != row["top"]["y"] {
                height_failures += 1;
            }
            top_comparisons += 1;
        }
    }
    let complete = failures == 0 && layer_failures == 0 && height_failures == 0;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":complete,"language":"Rust","operations":operations,"cells":cases.len(),"height_comparisons":height_comparisons,"top_comparisons":top_comparisons,"failures":failures,"layer_failures":layer_failures,"height_failures":height_failures,"first_errors":errors,"database_files":snapshot.files,"provenance":provenance,"scope":"Retained native waterlogging operations and saved layers; no fluid simulation or new native experiment"})
        )?
    );
    Ok(complete)
}
