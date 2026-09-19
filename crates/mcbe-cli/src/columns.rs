use crate::Result;
use mcbe_core::{blocks::Palette, columns, leveldb};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::Path,
};
fn integer(v: &Value) -> Result<i32> {
    Ok(i32::try_from(
        v.as_i64().ok_or("Integer column coordinate required")?,
    )?)
}
pub fn inspect(args: &[String]) -> Result<bool> {
    if args.len() != 6 {
        return Err("column PALETTE PARTIAL DB CX CZ".into());
    }
    let full: Value = serde_json::from_slice(&fs::read(&args[1])?)?;
    let partial: Value = serde_json::from_slice(&fs::read(&args[2])?)?;
    let palette = Palette::from_reports(&full, &partial)?;
    if palette.reference() != crate::REFERENCE {
        return Err("Unknown column palette reference".into());
    }
    let snapshot = leveldb::read_snapshot(Path::new(&args[3]))?;
    let p = [args[4].parse()?, args[5].parse()?];
    let Some(column) = columns::read(&snapshot, p, &palette)? else {
        println!("{}", json!({"complete":true,"recorded":false,"position":p}));
        return Ok(true);
    };
    let cells: Vec<_> = column
        .cells
        .iter()
        .map(|(p, kind)| json!([p[0], p[1], p[2], kind]))
        .collect();
    let mut grass = vec![];
    let mut biome_grid = vec![];
    for (p, kind) in &column.cells {
        if *kind == mcbe_core::cubes::Cube::Grass {
            grass.push(json!([
                p[0],
                p[1],
                p[2],
                column
                    .biomes
                    .at(p[0].rem_euclid(16) as u8, p[1], p[2].rem_euclid(16) as u8)?
            ]));
        }
    }
    for x in 0..16u8 {
        for z in 0..16u8 {
            biome_grid.push(json!([
                p[0] * 16 + i32::from(x),
                -61,
                p[1] * 16 + i32::from(z),
                column.biomes.at(x, -61, z)?
            ]));
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":true,"recorded":true,"position":p,"logical_cells":98304,"cells":cells,"grass":grass,"biome_grid_y":-61,"biome_grid":biome_grid,"database_hashes":snapshot.files,"scope":"Saved Overworld four-material column; finite biome grid sample at Y -61"})
        )?
    );
    Ok(true)
}
pub fn verify(args: &[String]) -> Result<bool> {
    if args.len() != 5 {
        return Err("verify-columns PALETTE PARTIAL DB FIXTURE".into());
    }
    let full: Value = serde_json::from_slice(&fs::read(&args[1])?)?;
    let partial: Value = serde_json::from_slice(&fs::read(&args[2])?)?;
    let palette = Palette::from_reports(&full, &partial)?;
    if palette.reference() != crate::REFERENCE {
        return Err("Unknown column palette reference".into());
    }
    let snapshot = leveldb::read_snapshot(Path::new(&args[3]))?;
    let fixture: Value = serde_json::from_slice(&fs::read(&args[4])?)?;
    if fixture["complete"] != true {
        return Err("Complete independent column fixture required".into());
    }
    let rows = fixture["columns"]
        .as_array()
        .ok_or("Missing column fixtures")?;
    if rows.is_empty() || rows.len() > 4096 {
        return Err("Column fixture count outside bound".into());
    }
    let mut seen = HashSet::new();
    let mut failures = vec![];
    let mut counts = BTreeMap::new();
    let mut grass_count = 0;
    for row in rows {
        let p = [integer(&row["position"][0])?, integer(&row["position"][1])?];
        if !seen.insert(p) {
            return Err("Duplicate column fixture".into());
        }
        let column =
            columns::read(&snapshot, p, &palette)?.ok_or("Recorded fixture column absent")?;
        let actual: Vec<_> = column
            .cells
            .iter()
            .map(|(p, kind)| json!([p[0], p[1], p[2], kind]))
            .collect();
        let mut grass = vec![];
        for (p, kind) in &column.cells {
            *counts.entry(*kind).or_insert(0usize) += 1;
            if *kind == mcbe_core::cubes::Cube::Grass {
                grass.push(json!([
                    p[0],
                    p[1],
                    p[2],
                    column
                        .biomes
                        .at(p[0].rem_euclid(16) as u8, p[1], p[2].rem_euclid(16) as u8)?
                ]));
            }
        }
        grass_count += grass.len();
        if json!(actual) != row["cells"] || json!(grass) != row["grass"] {
            failures.push(json!({"column":p,"actual_solid_cells":column.cells.len(),"expected_solid_cells":row["cells"].as_array().map(Vec::len)}));
        }
    }
    let missing = fixture["missing_columns"]
        .as_array()
        .ok_or("Missing absence fixtures")?;
    for p in missing {
        let position = [integer(&p[0])?, integer(&p[1])?];
        if columns::read(&snapshot, position, &palette)?.is_some() {
            return Err("Expected absent column is recorded".into());
        }
    }
    let complete = failures.is_empty();
    let solids: usize = counts.values().sum();
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":complete,"columns":rows.len(),"logical_cells":rows.len()*98304,"solid_cells":solids,"grass_cells":grass_count,"counts":counts,"missing_columns":missing.len(),"failures":failures,"database_hashes":snapshot.files,"scope":"Independent saved-column/state/biome comparison; native generation and streaming policy parity remain open"})
        )?
    );
    Ok(complete)
}
