use super::{REFERENCE, Result, blocks};
use mcbe_core::{
    blocks::states,
    chunks::StateValue,
    leveldb,
    raycast::{Hit, Options, RayBlock, Scene, Shapes},
    world_raycast::WorldRays,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

fn vector(row: &Value) -> Result<[f64; 3]> {
    Ok([
        row["x"].as_f64().ok_or("Invalid x")?,
        row["y"].as_f64().ok_or("Invalid y")?,
        row["z"].as_f64().ok_or("Invalid z")?,
    ])
}
fn position(row: &Value) -> Result<[i32; 3]> {
    Ok([
        i32::try_from(row["x"].as_i64().ok_or("Invalid x")?)?,
        i32::try_from(row["y"].as_i64().ok_or("Invalid y")?)?,
        i32::try_from(row["z"].as_i64().ok_or("Invalid z")?)?,
    ])
}
fn shape_table(path: &Path) -> Result<(Shapes, String)> {
    let (rules, hash) = blocks::read(path)?;
    if rules["reference_sha256"] != REFERENCE {
        return Err("Shape reference mismatch".into());
    }
    Ok((Shapes::read(&rules["rows"])?, hash))
}
pub fn verify(args: &[String]) -> Result<bool> {
    let (shapes, shape_hash) = shape_table(Path::new(&args[1]))?;
    let (capture, capture_hash) = blocks::read(Path::new(&args[2]))?;
    let rows = capture["ray_cases"].as_array().ok_or("Missing ray cases")?;
    if capture["complete"] != true
        || capture["errors"].as_array().is_none_or(|r| !r.is_empty())
        || capture["metadata"]["executable_sha256"] != REFERENCE
        || rows.is_empty()
        || capture["completion_record"]["kind"] != "done"
        || capture["completion_record"]["rayCases"].as_u64() != Some(rows.len() as u64)
    {
        return Err("Incomplete/mismatched ray capture".into());
    }
    let mut provenance = json!({"shapes_sha256":shape_hash,"capture_sha256":capture_hash});
    let palette = if args.len() == 6 {
        let (p, _, hashes) = blocks::load(Path::new(&args[3]), Path::new(&args[4]))?;
        provenance["palette"] = hashes;
        Some(p)
    } else {
        None
    };
    let mut world = if args.len() == 6 {
        Some(WorldRays::new(leveldb::read_snapshot(Path::new(&args[5]))?))
    } else {
        None
    };
    let scene = if world.is_none() {
        let layout = capture["ray_shapes"]
            .as_array()
            .ok_or("Missing scene layout")?;
        if layout.is_empty()
            || capture["completion_record"]["rayShapes"].as_u64() != Some(layout.len() as u64)
        {
            return Err("Scene shape count differs".into());
        }
        let mut objects = vec![];
        for row in layout {
            let pos = position(row)?;
            objects.push(RayBlock {
                position: pos,
                id: row["id"].as_str().ok_or("Missing block id")?.into(),
                states: states(&row["states"])?,
            });
            objects.push(RayBlock {
                position: [
                    pos[0],
                    pos[1].checked_sub(1).ok_or("Position overflow")?,
                    pos[2],
                ],
                id: "minecraft:stone".into(),
                states: vec![("stone_type".into(), StateValue::String("stone".into()))],
            });
        }
        Some(Scene::new(objects)?)
    } else {
        None
    };
    let mut indices = HashSet::new();
    let mut failures = 0;
    let mut errors = vec![];
    let mut maximum = 0.0_f64;
    let mut hits = 0;
    for row in rows {
        let index = row["index"].as_u64().ok_or("Missing case index")?;
        if index >= rows.len() as u64 || !indices.insert(index) {
            return Err("Duplicate/out-of-range ray index".into());
        }
        let options = Options {
            max_distance: row["options"]["maxDistance"]
                .as_f64()
                .ok_or("Missing maxDistance")?,
            include_passable: row["options"]["includePassableBlocks"]
                .as_bool()
                .ok_or("Missing passable option")?,
            include_liquids: row["options"]["includeLiquidBlocks"]
                .as_bool()
                .ok_or("Missing liquid option")?,
        };
        let origin = vector(&row["origin"])?;
        let direction = vector(&row["direction"])?;
        let actual = match &mut world {
            Some(world) => world.cast(
                origin,
                direction,
                options,
                &shapes,
                palette.as_ref().unwrap(),
            )?,
            None => scene
                .as_ref()
                .unwrap()
                .cast(origin, direction, options, &shapes)?,
        };
        if actual.is_some() {
            hits += 1;
        }
        let expected: Option<Hit> = serde_json::from_value(row["result"].clone())?;
        let same = match (&actual, &expected) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                let difference = [
                    f64::from(a.face_location.x) - f64::from(b.face_location.x),
                    f64::from(a.face_location.y) - f64::from(b.face_location.y),
                    f64::from(a.face_location.z) - f64::from(b.face_location.z),
                ]
                .into_iter()
                .map(f64::abs)
                .fold(0.0, f64::max);
                maximum = maximum.max(difference);
                a.id == b.id && a.position == b.position && a.face == b.face && difference <= 2e-5
            }
            _ => false,
        };
        if !same || !row["error"].is_null() {
            failures += 1;
            if errors.len() < 10 {
                errors.push(json!({"index":index,"actual":actual,"expected":expected,"native_error":row["error"]}));
            }
        }
    }
    let mut cell_comparisons = 0;
    let mut cell_failures = 0;
    let mut accessed_cells = 0;
    if let Some(world) = &mut world {
        let cells = capture["world_blocks"]
            .as_array()
            .ok_or("Missing observed world cells")?;
        if cells.is_empty()
            || capture["completion_record"]["worldBlocks"].as_u64() != Some(cells.len() as u64)
        {
            return Err("World cell count differs".into());
        }
        let traversed: HashSet<_> = world.cells.keys().copied().collect();
        accessed_cells = traversed.len();
        let mut seen = HashSet::new();
        for row in cells {
            let pos = position(row)?;
            if !seen.insert(pos) {
                return Err("Duplicate observed cell".into());
            }
            let block = world.block_at(pos, palette.as_ref().unwrap())?;
            let actual = match block {
                Some(block) => {
                    json!({"id":block.id,"states":block.states.into_iter().collect::<BTreeMap<_,_>>()})
                }
                None => json!({"id":"minecraft:air","states":{}}),
            };
            cell_comparisons += 1;
            if actual != json!({"id":row["id"],"states":row["states"]}) {
                cell_failures += 1;
                if errors.len() < 10 {
                    errors.push(json!({"cell":pos,"actual":actual,"expected":row}));
                }
            }
        }
        if traversed != seen {
            return Err("Traversed cell set differs from native sample set".into());
        }
    }
    let complete = failures == 0 && cell_failures == 0;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":complete,"language":"Rust","rays":rows.len(),"hits":hits,"misses":rows.len()-hits,"failures":failures,"cell_comparisons":cell_comparisons,"cell_failures":cell_failures,"accessed_cells":accessed_cells,"max_face_location_error":maximum,"tolerance":2e-5,"first_errors":errors,"provenance":provenance,"database_files":world.as_ref().map(|w|&w.world.snapshot.files),"scope":"Port regression against retained native observations; not a new native experiment"})
        )?
    );
    Ok(complete)
}
pub fn query(args: &[String]) -> Result<bool> {
    let (shapes, _) = shape_table(Path::new(&args[1]))?;
    let (palette, _, _) = blocks::load(Path::new(&args[2]), Path::new(&args[3]))?;
    let mut world = WorldRays::new(leveldb::read_snapshot(Path::new(&args[4]))?);
    let origin = [args[5].parse()?, args[6].parse()?, args[7].parse()?];
    let direction = [args[8].parse()?, args[9].parse()?, args[10].parse()?];
    let options = Options {
        max_distance: args[11].parse()?,
        include_passable: match args.get(12).map(String::as_str) {
            None => false,
            Some("include-passable") => true,
            _ => return Err("Unknown ray option".into()),
        },
        include_liquids: false,
    };
    let hit = world.cast(origin, direction, options, &shapes, &palette)?;
    println!("{}", serde_json::to_string_pretty(&hit)?);
    Ok(true)
}
