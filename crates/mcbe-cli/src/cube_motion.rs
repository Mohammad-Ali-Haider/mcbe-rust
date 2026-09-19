use crate::{Capture, Case, REFERENCE, Result, blocks, body};
use mcbe_core::{
    cubes::{Cube, WorldCubes},
    leveldb,
    motion::{Actor, Observation},
};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path};

fn predict(
    palette: &mcbe_core::blocks::Palette,
    database: &Path,
    cases: &[Value],
    steps: usize,
) -> Result<Value> {
    if cases.is_empty() || steps > 1000 || cases.len() > 256 {
        return Err("Bounded nonempty cube prediction cases required".into());
    }
    let mut world = WorldCubes::new(leveldb::read_snapshot(database)?, palette);
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in cases {
        let case: Case = serde_json::from_value(raw.clone())?;
        if !seen.insert(case.index) {
            return Err("Duplicate prediction case".into());
        }
        let mut actor = Actor::new(case.origin, body(&case)?)?.impulse(case.impulse)?;
        for step in 0..=steps {
            let mut row = serde_json::to_value(actor.observation())?;
            row["case"] = json!(case.index);
            row["step"] = json!(step);
            rows.push(row);
            if step < steps {
                actor = actor.tick_solid(&world.scene_for(&actor)?)?;
            }
        }
    }
    let mut positions: Vec<_> = world.cells.keys().copied().collect();
    positions.sort();
    let cells: Vec<_> = positions
        .iter()
        .map(|p| observation(*p, world.cells[p]))
        .collect();
    Ok(
        json!({"complete":true,"cases":cases,"steps":steps,"frames":rows,"cells":cells,"database_files":world.world.snapshot.files}),
    )
}
fn observation(p: [i32; 3], kind: Option<Cube>) -> Value {
    json!({"x":p[0],"y":p[1],"z":p[2],"id":kind.map_or("minecraft:air",Cube::id),"states":kind.map_or_else(std::collections::BTreeMap::new,Cube::states)})
}
fn same_frames(a: &Value, b: &Value) -> Result<bool> {
    let a = a.as_array().ok_or("Missing generated frame array")?;
    let b = b.as_array().ok_or("Missing frozen frame array")?;
    if a.len() != b.len() {
        return Ok(false);
    }
    for (a, b) in a.iter().zip(b) {
        let left: Observation = serde_json::from_value(a.clone())?;
        let right: Observation = serde_json::from_value(b.clone())?;
        if a["case"] != b["case"] || a["step"] != b["step"] || left != right {
            return Ok(false);
        }
    }
    Ok(true)
}
pub fn prepare(args: &[String]) -> Result<bool> {
    if args.len() != 6 {
        return Err("predict-cube-motion PALETTE PARTIAL DB CASES STEPS".into());
    }
    let (palette, _, _) = blocks::load(Path::new(&args[1]), Path::new(&args[2]))?;
    let (cases, _) = blocks::read(Path::new(&args[4]))?;
    let report = predict(
        &palette,
        Path::new(&args[3]),
        cases.as_array().ok_or("Cases must be array")?,
        args[5].parse()?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(true)
}
pub fn verify_region(args: &[String]) -> Result<bool> {
    if args.len() != 5 {
        return Err("verify-cube-region PALETTE PARTIAL DB FIXTURE".into());
    }
    let (palette, _, _) = blocks::load(Path::new(&args[1]), Path::new(&args[2]))?;
    let (fixture, hash) = blocks::read(Path::new(&args[4]))?;
    if fixture["complete"] != true {
        return Err("Complete region fixture required".into());
    }
    let min: [i32; 3] = serde_json::from_value(fixture["min"].clone())?;
    let max: [i32; 3] = serde_json::from_value(fixture["max"].clone())?;
    let mut world = mcbe_core::chunks::World::new(leveldb::read_snapshot(Path::new(&args[3]))?);
    let loaded = mcbe_core::cubes::region(&mut world, min, max, &palette)?;
    let rows = fixture["cells"].as_array().ok_or("Missing fixture cells")?;
    let volume = (0..3)
        .map(|a| (i64::from(max[a]) - i64::from(min[a])) as u64)
        .product::<u64>();
    if rows.len() as u64 != volume {
        return Err("Incomplete region coverage".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut failures = 0usize;
    let mut counts = std::collections::BTreeMap::new();
    for row in rows {
        let p = [
            i32::try_from(row[0].as_i64().ok_or("x")?)?,
            i32::try_from(row[1].as_i64().ok_or("y")?)?,
            i32::try_from(row[2].as_i64().ok_or("z")?)?,
        ];
        if !seen.insert(p) || (0..3).any(|a| p[a] < min[a] || p[a] >= max[a]) {
            return Err("Duplicate or out-of-region fixture cell".into());
        }
        let expected: Option<Cube> = serde_json::from_value(row[3].clone())?;
        let actual = loaded.get(&p).copied();
        if actual != expected {
            failures += 1;
        }
        let name = actual.map_or("air".to_owned(), |kind| {
            serde_json::to_value(kind)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        });
        *counts.entry(name).or_insert(0usize) += 1;
    }
    let complete = failures == 0;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":complete,"cells":rows.len(),"solid_cells":loaded.len(),"counts":counts,"failures":failures,"fixture_sha256":hash,"database_files":world.snapshot.files})
        )?
    );
    Ok(complete)
}
pub fn verify(args: &[String]) -> Result<bool> {
    if args.len() != 6 && !(args.len() == 7 && args[6] == "after") {
        return Err("verify-cube-motion PALETTE PARTIAL DB CAPTURE PREDICTIONS [after]".into());
    }
    let after = args.len() == 7;
    let (palette, _, provenance) = blocks::load(Path::new(&args[1]), Path::new(&args[2]))?;
    let (capture_raw, capture_hash) = blocks::read(Path::new(&args[4]))?;
    let capture: Capture = serde_json::from_value(capture_raw.clone())?;
    if !capture.complete
        || capture_raw["errors"]
            .as_array()
            .is_none_or(|v| !v.is_empty())
        || capture.metadata["executable_sha256"] != REFERENCE
    {
        return Err("Complete known-reference cube capture required".into());
    }
    let (frozen, frozen_hash) = blocks::read(Path::new(&args[5]))?;
    let cases = frozen["cases"].as_array().ok_or("Missing frozen cases")?;
    let steps = usize::try_from(frozen["steps"].as_u64().ok_or("Missing steps")?)?;
    let predicted = predict(&palette, Path::new(&args[3]), cases, steps)?;
    if frozen["complete"] != true
        || !same_frames(&predicted["frames"], &frozen["frames"])?
        || (!after && predicted["cells"] != frozen["cells"])
    {
        return Err("Frozen saved-world predictions do not reproduce".into());
    }
    if after {
        let positions = |value: &Value| -> Result<Vec<Value>> {
            Ok(value
                .as_array()
                .ok_or("Missing sampled cells")?
                .iter()
                .map(|row| json!([row["x"], row["y"], row["z"]]))
                .collect())
        };
        if positions(&predicted["cells"])? != positions(&frozen["cells"])? {
            return Err("Post-replay geometry accesses differ from frozen predictions".into());
        }
    }
    let native: HashMap<_, _> = capture
        .motion_frames
        .iter()
        .map(|r| ((r.case, r.step), r))
        .collect();
    let frames = predicted["frames"]
        .as_array()
        .ok_or("Missing generated frames")?;
    if native.len() != capture.motion_frames.len()
        || native.len() != frames.len()
        || capture.motion_cases.len() != cases.len()
    {
        return Err("Incomplete or duplicate native frames/cases".into());
    }
    let mut case_ids = std::collections::HashSet::new();
    for raw in cases {
        let expected: Case = serde_json::from_value(raw.clone())?;
        let actual = capture
            .motion_cases
            .iter()
            .find(|c| c.index == expected.index)
            .ok_or("Missing case")?;
        if !case_ids.insert(actual.index)
            || actual.origin != expected.origin
            || actual.impulse != expected.impulse
            || actual.entity_type != expected.entity_type
        {
            return Err("Native case input differs from frozen case".into());
        }
    }
    let mut errors = vec![];
    let mut failures = 0usize;
    let mut max_position = 0f32;
    let mut max_velocity = 0f32;
    for row in frames {
        let case = row["case"].as_u64().ok_or("Missing case index")? as usize;
        let step = row["step"].as_u64().ok_or("Missing step")? as usize;
        let expected: Observation = serde_json::from_value(row.clone())?;
        let actual = native.get(&(case, step)).ok_or("Missing native frame")?;
        let initial = native.get(&(case, 0)).ok_or("Missing initial frame")?;
        let pe = (0..3)
            .map(|a| (expected.location[a] - actual.observation.location[a]).abs())
            .fold(0f32, f32::max);
        let ve = (0..3)
            .map(|a| (expected.velocity[a] - actual.observation.velocity[a]).abs())
            .fold(0f32, f32::max);
        max_position = max_position.max(pe);
        max_velocity = max_velocity.max(ve);
        if pe > 2e-5
            || ve > 2e-5
            || expected.on_ground != actual.observation.on_ground
            || expected.falling != actual.observation.falling
            || actual.tick != initial.tick + step as u64
        {
            failures += 1;
            if errors.len() < 20 {
                errors.push(json!({"case":case,"step":step,"expected":expected,"actual":actual.observation,"position_error":pe,"velocity_error":ve}));
            }
        }
    }
    let expected_cells = predicted["cells"].as_array().ok_or("Missing cells")?;
    let mut cells = HashMap::new();
    let observed_cells = if after {
        &capture.world_blocks_after
    } else {
        &capture.world_blocks
    };
    for raw in observed_cells {
        let p = [
            raw["x"].as_i64().ok_or("Cell x")?,
            raw["y"].as_i64().ok_or("Cell y")?,
            raw["z"].as_i64().ok_or("Cell z")?,
        ];
        if cells.insert(p, raw).is_some() {
            return Err("Duplicate native world cell".into());
        }
    }
    if cells.len() != expected_cells.len() {
        return Err("Sampled cell coverage differs".into());
    }
    let mut cell_failures = 0;
    for expected in expected_cells {
        let p = [
            expected["x"].as_i64().ok_or("Cell x")?,
            expected["y"].as_i64().ok_or("Cell y")?,
            expected["z"].as_i64().ok_or("Cell z")?,
        ];
        let actual = cells.get(&p).ok_or("Missing sampled cell")?;
        if actual["id"] != expected["id"] || actual["states"] != expected["states"] {
            cell_failures += 1;
        }
    }
    let complete = failures == 0 && cell_failures == 0;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":complete,"cases":cases.len(),"frames":frames.len(),"failures":failures,"cell_comparisons":cells.len(),"cell_failures":cell_failures,"phase":if after{"after"}else{"before"},"max_position_error":max_position,"max_velocity_error":max_velocity,"first_errors":errors,"database_files":predicted["database_files"],"capture_sha256":capture_hash,"predictions_sha256":frozen_hash,"palette":provenance,"scope":"Saved-world covered cube collision with phase-specific observed states; no activation, player-control or arbitrary-shape parity"})
        )?
    );
    Ok(complete)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_reproduction_is_exact_in_float32_domain() {
        let x = 0.7f32 as f64;
        let row = |value| json!([{"case":0,"step":1,"location":{"x":value,"y":0,"z":0},"velocity":{"x":0,"y":0,"z":0},"onGround":true,"falling":false}]);
        assert!(same_frames(&row(x), &row(f64::from_bits(x.to_bits() + 1))).unwrap());
        assert!(!same_frames(&row(x), &row(f32::from_bits(0.7f32.to_bits() + 1) as f64)).unwrap());
        let mut wrong = row(x);
        wrong[0]["step"] = json!(2);
        assert!(!same_frames(&row(x), &wrong).unwrap());
    }
}
