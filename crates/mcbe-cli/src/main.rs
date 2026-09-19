mod biomes;
mod blocks;
mod columns;
mod container_geometry;
mod container_power;
mod containers;
mod cube_motion;
mod database;
mod grass;
mod heights;
mod inventory;
mod items;
mod rays;
mod recipes;
mod resources;
mod water;
mod writer;
use mcbe_core::{
    archive,
    cells::CellScene,
    leveldb,
    motion::{Actor, Body, Observation, Plane, SolidBox, SolidScene, Vec3},
    world_collision::WorldScene,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, env, fs, path::Path, process::ExitCode};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const REFERENCE: &str = "19c88569af2e4b7d984e999055a31cbcb0799dacf8bbbf7371eda42f5772a443";
#[derive(Deserialize)]
struct BoxInput {
    min: [f32; 3],
    max: [f32; 3],
}
#[derive(Deserialize)]
struct Case {
    index: usize,
    #[serde(rename = "entityType")]
    entity_type: String,
    origin: Vec3,
    impulse: Vec3,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    z: f32,
    #[serde(rename = "groundY", default)]
    ground_y: f32,
    #[serde(default)]
    boxes: Vec<BoxInput>,
    #[serde(default)]
    surface: Value,
}
#[derive(Deserialize)]
struct Frame {
    case: usize,
    step: usize,
    tick: u64,
    #[serde(flatten)]
    observation: Observation,
}
#[derive(Deserialize)]
struct Capture {
    complete: bool,
    metadata: Value,
    motion_cases: Vec<Case>,
    motion_frames: Vec<Frame>,
    #[serde(default)]
    world_blocks: Vec<Value>,
    #[serde(default)]
    world_blocks_after: Vec<Value>,
}
fn vec(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0], v[1], v[2])
}
fn body(case: &Case) -> Result<Body> {
    match case.entity_type.as_str() {
        "reconstruction:motion_probe" => Ok(Body::PROBE),
        "minecraft:armor_stand" => Ok(Body::ARMOR_STAND),
        _ => Err("Unmeasured actor type".into()),
    }
}
fn load_cells(path: &Path) -> Result<CellScene> {
    let values: Vec<Value> = serde_json::from_slice(&fs::read(path)?)?;
    let mut cells = HashMap::new();
    for value in values {
        let coord = |a: &str| -> Result<i32> {
            Ok(i32::try_from(
                value[a]
                    .as_i64()
                    .ok_or("Integer cell coordinate required")?,
            )?)
        };
        let key = [coord("x")?, coord("y")?, coord("z")?];
        let solid = match value["id"].as_str() {
            Some("minecraft:air") if value["states"] == json!({}) => false,
            Some("minecraft:stone") if value["states"] == json!({"stone_type":"stone"}) => true,
            _ => return Err("Unmeasured cell collision state".into()),
        };
        if cells.insert(key, solid).is_some() {
            return Err("Duplicate cell".into());
        }
    }
    Ok(CellScene { cells })
}
fn verify(path: &Path, mode: &str, cells: Option<&Path>) -> Result<bool> {
    let capture: Capture = serde_json::from_slice(&fs::read(path)?)?;
    if !capture.complete
        || capture.metadata["executable_sha256"] != REFERENCE
        || capture.motion_cases.is_empty()
    {
        return Err("Complete known-reference capture required".into());
    }
    let native: HashMap<_, _> = capture
        .motion_frames
        .iter()
        .map(|r| ((r.case, r.step), r))
        .collect();
    let steps = capture
        .motion_frames
        .iter()
        .map(|r| r.step)
        .max()
        .ok_or("No frames")?;
    if native.len() != capture.motion_frames.len()
        || native.len() != capture.motion_cases.len() * (steps + 1)
    {
        return Err("Duplicate or incomplete frames".into());
    }
    let world = if mode == "cells" {
        Some(load_cells(cells.ok_or("Cell file required")?)?)
    } else {
        None
    };
    if !["plane", "solid", "cells", "world"].contains(&mode) {
        return Err("Unknown motion mode".into());
    }
    let mut saved_world = if mode == "world" {
        Some(WorldScene::new(leveldb::read_snapshot(
            cells.ok_or("Database path required")?,
        )?))
    } else {
        None
    };
    let mut maximum_position = 0f32;
    let mut maximum_velocity = 0f32;
    let mut failures = 0usize;
    let mut errors = Vec::new();
    let mut case_ids = std::collections::HashSet::new();
    for case in &capture.motion_cases {
        if !case_ids.insert(case.index) {
            return Err("Duplicate case index".into());
        }
        let mut actor = Actor::new(case.origin, body(case)?)?.impulse(case.impulse)?;
        let mut boxes = vec![SolidBox::new(
            Vec3::new(case.x - 6.0, case.ground_y, case.z - 6.0),
            Vec3::new(case.x + 7.0, case.ground_y + 1.0, case.z + 7.0),
        )?];
        for b in &case.boxes {
            boxes.push(SolidBox::new(vec(b.min), vec(b.max))?);
        }
        let scene = SolidScene { boxes };
        let (slipperiness, slow, bounce) = match case.surface["id"].as_str() {
            Some("minecraft:ice") => (0.98, false, false),
            Some("minecraft:blue_ice") => (0.989, false, false),
            Some("minecraft:honey_block") => (0.8, true, false),
            Some("minecraft:slime") => (0.8, true, true),
            Some("minecraft:stone") => (0.6, false, false),
            _ if mode != "plane" => (0.6, false, false),
            _ => return Err("Unmeasured planar surface".into()),
        };
        let plane = Plane {
            y: case.ground_y + 1.0,
            slipperiness,
            slow,
            bounce,
        };
        let first = native
            .get(&(case.index, 0))
            .ok_or("Missing initial frame")?;
        for step in 0..=steps {
            let expected = actor.observation();
            let observed = *native.get(&(case.index, step)).ok_or("Missing frame")?;
            let pe = (0..3)
                .map(|a| (expected.location[a] - observed.observation.location[a]).abs())
                .fold(0f32, f32::max);
            let ve = (0..3)
                .map(|a| (expected.velocity[a] - observed.observation.velocity[a]).abs())
                .fold(0f32, f32::max);
            maximum_position = maximum_position.max(pe);
            maximum_velocity = maximum_velocity.max(ve);
            if pe > 2e-5
                || ve > 2e-5
                || expected.on_ground != observed.observation.on_ground
                || expected.falling != observed.observation.falling
                || observed.tick != first.tick + step as u64
            {
                failures += 1;
                if errors.len() < 20 {
                    errors.push(json!({"case":case.index,"step":step,"expected":expected,"observed":observed.observation,"position_error":pe,"velocity_error":ve}));
                }
            }
            if step < steps {
                actor = match mode {
                    "plane" => actor.tick_plane(plane)?,
                    "solid" => actor.tick_solid(&scene)?,
                    "cells" => actor.tick_solid(&world.as_ref().unwrap().scene_for(&actor)?)?,
                    "world" => {
                        actor.tick_solid(&saved_world.as_mut().unwrap().scene_for(&actor)?)?
                    }
                    _ => unreachable!(),
                };
            }
        }
    }
    let mut cell_count = 0;
    let mut cell_failures = 0;
    if let Some(scene) = &mut saved_world {
        let mut seen = std::collections::HashSet::new();
        for cell in &capture.world_blocks {
            let coord = |a: &str| -> Result<i32> {
                Ok(i32::try_from(
                    cell[a].as_i64().ok_or("Invalid cell coordinate")?,
                )?)
            };
            let position = [coord("x")?, coord("y")?, coord("z")?];
            if !seen.insert(position) {
                return Err("Duplicate native cell".into());
            }
            let solid = scene.world.collision_solid(position)?;
            let (id, states) = if solid {
                ("minecraft:stone", json!({"stone_type":"stone"}))
            } else {
                ("minecraft:air", json!({}))
            };
            if cell["id"] != id || cell["states"] != states {
                cell_failures += 1;
            }
            cell_count += 1;
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":failures==0 && cell_failures==0,"language":"Rust","mode":mode,"cases":capture.motion_cases.len(),"frames":native.len(),"failures":failures,"cell_comparisons":cell_count,"cell_failures":cell_failures,"max_position_error":maximum_position,"max_velocity_error":maximum_velocity,"tolerance":2e-5,"first_errors":errors,"database_files":saved_world.as_ref().map(|s| &s.world.snapshot.files)})
        )?
    );
    Ok(failures == 0 && cell_failures == 0)
}
fn run() -> Result<bool> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("container-geometry-catalog") if args.len() == 1 => {
            println!(
                "{}",
                serde_json::to_string_pretty(&mcbe_core::container_geometry::catalog())?
            );
            Ok(true)
        }
        Some("predict-container-motion") => container_geometry::prepare(&args),
        Some("predict-container-power") => container_power::predict(&args),
        Some("predict-container-rays") => container_geometry::prepare_rays(&args),
        Some("verify-container-motion") => container_geometry::verify(&args),
        Some("predict-cube-motion") => cube_motion::prepare(&args),
        Some("verify-cube-motion") => cube_motion::verify(&args),
        Some("verify-cube-region") => cube_motion::verify_region(&args),
        Some("verify-columns") => columns::verify(&args),
        Some("column") => columns::inspect(&args),
        Some("cube-catalog") if args.len() == 1 => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"reference_sha256":REFERENCE,"rows":mcbe_core::cubes::Cube::ALL.iter().map(|kind|json!({"kind":kind,"id":kind.id(),"states":kind.states(),"resource_key":kind.resource_key(),"boxes":[[0,0,0,1,1,1]],"passable":false})).collect::<Vec<_>>()})
                )?
            );
            Ok(true)
        }
        Some("predict-inventory") => inventory::run(&args, false),
        Some("containers") => containers::run(&args, false),
        Some("verify-containers") => containers::run(&args, true),
        Some("container-edit") => containers::edit(&args),
        Some("verify-inventory") => inventory::run(&args, true),
        Some("predict-biomes") => biomes::run(&args, true),
        Some("verify-biomes") => biomes::run(&args, false),
        Some("resource") => resources::inspect(&args, false),
        Some("texture") => resources::inspect(&args, true),
        Some("material") => resources::material(&args),
        Some("block-material") => resources::block_material(&args),
        Some("block-material-split") => resources::block_material_split(&args),
        Some("grass-tint") => grass::inspect(&args),
        Some("verify-grass") => grass::verify(&args),
        Some("verify-textures") => resources::verify(&args),
        Some("recipe") => recipes::inspect(&args),
        Some("verify-recipes") => recipes::verify(&args),
        Some("verify-recipe-symmetry") => recipes::symmetry(&args),
        Some("item") => items::inspect(&args),
        Some("verify-items") => items::verify(&args),
        Some("verify-database") if args.len() == 3 => {
            database::verify(Path::new(&args[1]), Path::new(&args[2]))
        }
        Some("move") if args.len() == 9 || args.len() == 10 => database::run_move(&args),
        Some("block") if args.len() == 5 || args.len() == 7 => blocks::query(&args),
        Some("block-at") if args.len() == 7 => blocks::block_at(&args),
        Some("water-cell") if args.len() == 7 => water::inspect(&args),
        Some("waterlog") if args.len() == 11 => water::edit(&args),
        Some("verify-water") if args.len() == 7 || args.len() == 9 => water::verify(&args),
        Some("recreate-water") if args.len() == 10 => water::recreate(&args),
        Some("heights") if args.len() == 7 => heights::query(&args),
        Some("repair-heights") if args.len() == 6 || args.len() == 7 => heights::repair(&args),
        Some("verify-heights") if args.len() == 7 => heights::verify(&args),
        Some("verify-chunks") if args.len() == 2 => writer::verify_chunks(Path::new(&args[1])),
        Some("copy-block") if args.len() == 9 => writer::copy_block(&args),
        Some("verify-writer") if args.len() == 5 => writer::verify_experiment(&args),
        Some("verify-rays") if args.len() == 3 || args.len() == 6 => rays::verify(&args),
        Some("ray") if args.len() == 12 || args.len() == 13 => rays::query(&args),
        Some("verify-blocks") if args.len() == 4 => blocks::verify(
            Path::new(&args[1]),
            Path::new(&args[2]),
            Path::new(&args[3]),
        ),
        Some("verify-world-blocks") if args.len() == 5 => blocks::verify_world(&args),
        Some("verify-archives") if args.len() == 3 => {
            let index = fs::read_to_string(&args[1])?;
            let root = Path::new(&args[2]);
            let mut archives = 0;
            let mut entries = 0;
            for line in index.lines().filter(|s| !s.is_empty()) {
                let row: Value = serde_json::from_str(line)?;
                let path = row["path"].as_str().ok_or("Missing archive path")?;
                let relative = Path::new(path);
                if relative
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
                {
                    return Err("Archive path must stay relative".into());
                }
                let bytes = fs::read(root.join(relative))?;
                if row["sha256"] != format!("{:x}", Sha256::digest(&bytes))
                    || row["size"].as_u64() != Some(bytes.len() as u64)
                {
                    return Err(format!("Archive identity differs: {path}").into());
                }
                let parsed = archive::read(&bytes)?;
                let expected = row["entries"].as_array().ok_or("Missing entry array")?;
                if parsed.len() != expected.len() {
                    return Err("Entry count differs".into());
                }
                for (i, (entry, known)) in parsed.iter().zip(expected).enumerate() {
                    if known["index"].as_u64() != Some(i as u64)
                        || known["name"] != entry.name
                        || known["descriptor_offset"].as_u64()
                            != Some(entry.descriptor_offset as u64)
                        || known["file_offset"].as_u64() != Some(entry.offset as u64)
                        || known["length"].as_u64() != Some(entry.payload.len() as u64)
                        || known["sha256"] != format!("{:x}", Sha256::digest(entry.payload))
                    {
                        return Err(format!("Entry differs: {path} / {}", entry.name).into());
                    }
                }
                archives += 1;
                entries += parsed.len();
            }
            if archives == 0 {
                return Err("Empty archive index".into());
            }
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"complete":true,"language":"Rust","archives":archives,"entries":entries,"payload_hashes_match":true})
                )?
            );
            Ok(true)
        }
        Some("verify-leveldat") if args.len() > 1 => {
            let mut files = Vec::new();
            for path in &args[1..] {
                let bytes = fs::read(path)?;
                let decoded = mcbe_core::nbt::LevelDat::read(&bytes)?;
                if decoded.write()? != bytes {
                    return Err(format!("Round trip differs: {path}").into());
                }
                files.push(json!({"path":path,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}));
            }
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"complete":true,"language":"Rust","byte_exact":true,"files":files})
                )?
            );
            Ok(true)
        }
        Some("verify-motion") if args.len() >= 3 => {
            verify(Path::new(&args[2]), &args[1], args.get(3).map(Path::new))
        }
        Some("archive") if args.len() == 2 => {
            let data = fs::read(&args[1])?;
            let entries = archive::read(&data)?;
            let rows:Vec<_>=entries.iter().map(|e|json!({"name":e.name,"descriptor_offset":e.descriptor_offset,"offset":e.offset,"length":e.payload.len()})).collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({"entries":rows}))?
            );
            Ok(true)
        }
        Some("demo") => {
            let scene = SolidScene {
                boxes: vec![
                    SolidBox::new(Vec3::new(-6.0, 64.0, -6.0), Vec3::new(7.0, 65.0, 7.0))?,
                    SolidBox::new(Vec3::new(2.0, 65.0, -5.0), Vec3::new(3.0, 74.0, 6.0))?,
                ],
            };
            let mut actor = Actor::new(Vec3::new(0.5, 65.0, 0.5), Body::PROBE)?
                .impulse(Vec3::new(5.0, 0.0, 0.4))?;
            let mut frames = Vec::new();
            for step in 0..=20 {
                frames.push(json!({"step":step,"observation":actor.observation()}));
                actor = actor.tick_solid(&scene)?;
            }
            println!("{}", serde_json::to_string_pretty(&frames)?);
            Ok(true)
        }
        _ => {
            eprintln!(
                "Resource commands: resource PACK LOGICAL_PATH | texture PACK LOGICAL_PATH | material PACK ALIAS VARIANT | block-material PACK BLOCK FACE VARIANT | verify-textures FIXTURE"
            );
            eprintln!(
                "Recipe commands: recipe REGISTRY DEFINITION GRID_JSON [STATION] | verify-recipes REGISTRY DEFINITION_ROOT EXPECTED_REPORT | verify-recipe-symmetry NATIVE_EVIDENCE"
            );
            eprintln!(
                "Item commands: item REGISTRY ID AMOUNT [SET_AMOUNT] | verify-items MODEL_REGISTRY CAPTURE"
            );
            eprintln!(
                "Usage: mcbe demo | archive FILE | verify-archives INDEX ROOT | verify-leveldat FILE... | verify-motion plane|solid CAPTURE | verify-motion cells CAPTURE CELLS | verify-motion world CAPTURE DB | verify-database DB EXPECTED | move DB X Y Z VX VY VZ STEPS [probe|armor-stand]"
            );
            eprintln!(
                "Block commands: block PALETTE PARTIAL ID STATES_JSON [PROPERTY VALUE_JSON] | block-at PALETTE PARTIAL DB X Y Z | verify-blocks PALETTE PARTIAL CAPTURE | verify-world-blocks PALETTE PARTIAL DB CAPTURE"
            );
            eprintln!(
                "Ray commands: ray SHAPES PALETTE PARTIAL DB OX OY OZ DX DY DZ DISTANCE [include-passable] | verify-rays SHAPES CAPTURE [PALETTE PARTIAL DB]"
            );
            eprintln!(
                "Write commands: copy-block SOURCE NEW_DEST X Y Z FROM_X FROM_Y FROM_Z | verify-chunks DB | verify-writer SOURCE EXPECTED NEW_DEST PLAN"
            );
            eprintln!(
                "Height commands: heights RULES PALETTE PARTIAL DB CHUNK_X CHUNK_Z | repair-heights RULES PALETTE PARTIAL SOURCE NEW_DEST [EXPECTED_WORLD] | verify-heights RULES PALETTE PARTIAL DB CAPTURE PREDICTIONS"
            );
            eprintln!(
                "Water commands: water-cell PALETTE PARTIAL DB X Y Z | waterlog WATER_RULES HEIGHT_RULES PALETTE PARTIAL SOURCE NEW_DEST X Y Z true|false | verify-water WATER_RULES HEIGHT_RULES PALETTE PARTIAL DB CAPTURE [PREDICTIONS EXPECTED_DB] | recreate-water WATER_RULES HEIGHT_RULES PALETTE PARTIAL SOURCE NEW_DEST EXPECTED_WORLD CAPTURE PLAN"
            );
            Ok(false)
        }
    }
}
fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}
