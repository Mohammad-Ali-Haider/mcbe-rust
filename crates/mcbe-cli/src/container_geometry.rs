use crate::{REFERENCE, Result, blocks};
use mcbe_core::{
    blocks::states,
    cells::scene_from_shapes,
    container_geometry::{Geometry, WorldGeometry},
    leveldb,
    motion::{Actor, Body, Observation, Vec3},
    raycast::{self, Options, RayBlock},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
};

#[derive(Deserialize)]
struct Case {
    index: usize,
    #[serde(rename = "entityType")]
    entity_type: String,
    origin: Vec3,
    impulse: Vec3,
    block: [i32; 3],
    material: Value,
    #[serde(rename = "groundY", default = "default_ground")]
    ground_y: i32,
    #[serde(default)]
    support_blocks: Vec<Value>,
}
fn default_ground() -> i32 {
    60
}
fn body(kind: &str) -> Result<Body> {
    match kind {
        "reconstruction:shape_probe" => Ok(Body {
            width: 0.0625,
            height: 0.0625,
        }),
        "reconstruction:motion_probe" => Ok(Body::PROBE),
        "minecraft:armor_stand" => Ok(Body::ARMOR_STAND),
        _ => Err("Unknown geometry actor".into()),
    }
}
struct Fixture {
    cells: HashMap<[i32; 3], RayBlock>,
}
impl Fixture {
    fn read(cases: &[Case]) -> Result<Self> {
        let mut cells = HashMap::new();
        for c in cases {
            if !(64..=80).contains(&c.block[1])
                || !(60..=75).contains(&c.ground_y)
                || c.ground_y >= c.block[1]
                || c.support_blocks.len() > 8
                || c.block[0].unsigned_abs() > 256
                || c.block[2].unsigned_abs() > 256
            {
                return Err("Bounded geometry fixture required".into());
            }
            for x in c.block[0] - 6..=c.block[0] + 6 {
                for z in c.block[2] - 6..=c.block[2] + 6 {
                    let p = [x, c.ground_y, z];
                    if cells
                        .insert(
                            p,
                            RayBlock {
                                position: p,
                                id: "minecraft:stone".into(),
                                states: states(&json!({"stone_type":"stone"}))?,
                            },
                        )
                        .is_some()
                    {
                        return Err("Overlapping fixture floors".into());
                    }
                }
            }
            let block = RayBlock {
                position: c.block,
                id: c.material["id"]
                    .as_str()
                    .ok_or("Missing fixture id")?
                    .into(),
                states: states(&c.material["states"])?,
            };
            if cells.insert(c.block, block).is_some() {
                return Err("Duplicate fixture container".into());
            }
            for raw in &c.support_blocks {
                let position: [i32; 3] = serde_json::from_value(raw["position"].clone())?;
                let block = RayBlock {
                    position,
                    id: raw["id"].as_str().ok_or("Missing support-block id")?.into(),
                    states: states(&raw["states"])?,
                };
                if position[0].unsigned_abs() > 256
                    || position[2].unsigned_abs() > 256
                    || !(60..=80).contains(&position[1])
                    || cells.insert(position, block).is_some()
                {
                    return Err("Conflicting or unbounded support block".into());
                }
            }
        }
        Ok(Self { cells })
    }
    fn block(&self, p: [i32; 3]) -> mcbe_core::Result<Option<RayBlock>> {
        if p[0].unsigned_abs() > 280 || p[2].unsigned_abs() > 280 || !(54..90).contains(&p[1]) {
            return Err("Outside explicit geometry fixture".into());
        }
        Ok(self.cells.get(&p).cloned())
    }
}
fn cases(value: &Value) -> Result<Vec<Case>> {
    let cases: Vec<Case> = serde_json::from_value(value.clone())?;
    if cases.is_empty()
        || cases.len() > 512
        || cases.iter().map(|c| c.index).collect::<BTreeSet<_>>().len() != cases.len()
    {
        return Err("Bounded unique geometry cases required".into());
    }
    Ok(cases)
}
fn predict(raw: &Value, steps: usize) -> Result<Value> {
    if steps > 200 {
        return Err("Geometry prediction exceeds 200 steps".into());
    }
    let cases = cases(raw)?;
    let fixture = Fixture::read(&cases)?;
    let geometry = Geometry::measured()?;
    let mut visited = BTreeSet::new();
    let mut frames = Vec::new();
    for c in &cases {
        let mut actor = Actor::new(c.origin, body(&c.entity_type)?)?.impulse(c.impulse)?;
        for step in 0..=steps {
            frames.push(json!({"case":c.index,"step":step,"observation":actor.observation()}));
            if step < steps {
                let scene = scene_from_shapes(&actor, |p| {
                    visited.insert(p);
                    match fixture.block(p)? {
                        Some(block) => geometry.collision_for(&block, &actor),
                        None => Ok(Vec::new()),
                    }
                })?;
                actor = actor.tick_solid(&scene)?;
            }
        }
    }
    let cells:Vec<_>=visited.into_iter().map(|p|match fixture.cells.get(&p) {
        Some(b)=>json!({"position":p,"id":b.id,"states":b.states.iter().cloned().collect::<std::collections::BTreeMap<_,_>>()}),
        None=>json!({"position":p,"id":"minecraft:air","states":{}}),
    }).collect();
    Ok(json!({"complete":true,"cases":raw,"steps":steps,"frames":frames,"cells":cells}))
}
pub fn prepare(args: &[String]) -> Result<bool> {
    if args.len() != 3 {
        return Err("predict-container-motion CASES STEPS".into());
    }
    let (raw, _) = blocks::read(Path::new(&args[1]))?;
    println!(
        "{}",
        serde_json::to_string_pretty(&predict(&raw, args[2].parse()?)?)?
    );
    Ok(true)
}
pub fn prepare_rays(args: &[String]) -> Result<bool> {
    if args.len() != 3 {
        return Err("predict-container-rays CASES QUERIES".into());
    }
    let (raw, _) = blocks::read(Path::new(&args[1]))?;
    let fixture = Fixture::read(&cases(&raw)?)?;
    let (raw_queries, _) = blocks::read(Path::new(&args[2]))?;
    let queries = raw_queries.as_array().ok_or("Ray query array required")?;
    if queries.is_empty() || queries.len() > 4096 {
        return Err("Bounded ray queries required".into());
    }
    let geometry = Geometry::measured()?;
    let mut predictions = Vec::new();
    let mut visited = BTreeSet::new();
    for q in queries {
        let vector =
            |name: &str| -> Result<[f64; 3]> { Ok(serde_json::from_value(q[name].clone())?) };
        predictions.push(raycast::cast(
            vector("origin")?,
            vector("direction")?,
            Options {
                max_distance: q["distance"].as_f64().ok_or("Missing ray distance")?,
                include_passable: false,
                include_liquids: false,
            },
            &geometry.selection,
            |p| {
                visited.insert(p);
                fixture.block(p)
            },
        )?);
    }
    let cells:Vec<_>=visited.into_iter().map(|p|match fixture.cells.get(&p) {
        Some(b)=>json!({"x":p[0],"y":p[1],"z":p[2],"id":b.id,"states":b.states.iter().cloned().collect::<std::collections::BTreeMap<_,_>>()}),
        None=>json!({"x":p[0],"y":p[1],"z":p[2],"id":"minecraft:air","states":{}}),
    }).collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"predictions":predictions,"cells":cells}))?
    );
    Ok(true)
}
pub fn verify(args: &[String]) -> Result<bool> {
    if args.len() != 6 {
        return Err("verify-container-motion PALETTE PARTIAL DB CAPTURE PREDICTIONS".into());
    }
    let (palette, _, _) = blocks::load(Path::new(&args[1]), Path::new(&args[2]))?;
    let (capture, capture_hash) = blocks::read(Path::new(&args[4]))?;
    let (frozen, frozen_hash) = blocks::read(Path::new(&args[5]))?;
    let native_cases = capture["motion_cases"]
        .as_array()
        .ok_or("Missing native cases")?;
    let expected_cases = frozen["cases"].as_array().ok_or("Missing frozen cases")?;
    if capture["complete"] != true
        || capture["metadata"]["executable_sha256"] != REFERENCE
        || native_cases.len() != expected_cases.len()
    {
        return Err("Complete matching reference required".into());
    }
    for (native, expected) in native_cases.iter().zip(expected_cases) {
        for (key, value) in expected.as_object().ok_or("Expected case object")? {
            let same = if key == "origin" || key == "impulse" {
                let a: Vec3 = serde_json::from_value(native[key].clone())?;
                let b: Vec3 = serde_json::from_value(value.clone())?;
                a == b
            } else {
                &native[key] == value
            };
            if !same {
                return Err(format!("Native case differs at {key}").into());
            }
        }
        if native["actualSurface"] != expected["material"] {
            return Err("Native container state differs".into());
        }
    }
    let steps = frozen["steps"].as_u64().ok_or("Missing frozen steps")? as usize;
    let reproduced = predict(&frozen["cases"], steps)?;
    if reproduced["cases"] != frozen["cases"]
        || reproduced["cells"] != frozen["cells"]
        || reproduced["steps"] != frozen["steps"]
    {
        return Err("Frozen motion prediction changed".into());
    }
    let repeated = reproduced["frames"]
        .as_array()
        .ok_or("Missing reproduced frames")?;
    let prior = frozen["frames"].as_array().ok_or("Missing frozen frames")?;
    if repeated.len() != prior.len() {
        return Err("Frozen frame count changed".into());
    }
    for (a, b) in repeated.iter().zip(prior) {
        let a_obs: Observation = serde_json::from_value(a["observation"].clone())?;
        let b_obs: Observation = serde_json::from_value(b["observation"].clone())?;
        if a["case"] != b["case"] || a["step"] != b["step"] || a_obs != b_obs {
            return Err("Frozen float32 frame changed".into());
        }
    }
    let native: HashMap<_, _> = capture["motion_frames"]
        .as_array()
        .ok_or("Missing native frames")?
        .iter()
        .map(|r| ((r["case"].as_u64(), r["step"].as_u64()), r))
        .collect();
    let expected = frozen["frames"].as_array().ok_or("Missing frozen frames")?;
    let expected_map: HashMap<_, _> = expected
        .iter()
        .map(|r| ((r["case"].as_u64(), r["step"].as_u64()), r))
        .collect();
    if expected_map.len() != expected.len() {
        return Err("Duplicate frozen frames".into());
    }
    if native.len() != expected.len()
        || capture["motion_frames"].as_array().unwrap().len() != expected.len()
    {
        return Err("Duplicate/missing frames".into());
    }
    let geometry = Geometry::measured()?;
    let mut world = WorldGeometry::new(leveldb::read_snapshot(Path::new(&args[3]))?);
    let cases = cases(&frozen["cases"])?;
    let mut errors = Vec::new();
    let mut maximum = 0_f32;
    for c in &cases {
        let mut actor = Actor::new(c.origin, body(&c.entity_type)?)?.impulse(c.impulse)?;
        let first = native
            .get(&(Some(c.index as u64), Some(0)))
            .ok_or("Missing first frame")?;
        for step in 0..=steps {
            let observed = native
                .get(&(Some(c.index as u64), Some(step as u64)))
                .ok_or("Missing native frame")?;
            let frame = expected_map
                .get(&(Some(c.index as u64), Some(step as u64)))
                .ok_or("Missing expected frame")?;
            let predicted: Observation = serde_json::from_value(frame["observation"].clone())?;
            if actor.observation() != predicted {
                return Err(format!("Saved world differs in case {} step {step}", c.index).into());
            }
            let actual: Observation = serde_json::from_value((*observed).clone())?;
            let pe = (0..3)
                .map(|a| (predicted.location[a] - actual.location[a]).abs())
                .fold(0_f32, f32::max);
            let ve = (0..3)
                .map(|a| (predicted.velocity[a] - actual.velocity[a]).abs())
                .fold(0_f32, f32::max);
            maximum = maximum.max(pe).max(ve);
            if pe > 2e-5
                || ve > 2e-5
                || predicted.on_ground != actual.on_ground
                || predicted.falling != actual.falling
                || observed["tick"].as_u64() != first["tick"].as_u64().map(|v| v + step as u64)
            {
                errors.push(json!({"case":c.index,"step":step,"expected":predicted,"observed":actual,"position_error":pe,"velocity_error":ve}));
            }
            let bounds = &observed["bounds"];
            let b = body(&c.entity_type)?;
            for (a, extent) in [(0, b.width * 0.5), (1, b.height * 0.5), (2, b.width * 0.5)] {
                let axis = ["x", "y", "z"][a];
                let center = actual.location[a] + if a == 1 { b.height * 0.5 } else { 0.0 };
                if (bounds["extent"][axis]
                    .as_f64()
                    .ok_or("Missing native extent")? as f32
                    - extent)
                    .abs()
                    > 2e-5
                    || (bounds["center"][axis]
                        .as_f64()
                        .ok_or("Missing native center")? as f32
                        - center)
                        .abs()
                        > 2e-5
                {
                    return Err("Native body dimensions differ".into());
                }
            }
            if step < steps {
                actor = actor.tick_solid(&world.scene_for(&actor, &palette, &geometry)?)?;
            }
        }
    }
    let mut cell_errors = Vec::new();
    let native_cells = capture["world_blocks_after"]
        .as_array()
        .ok_or("Missing native motion cells")?;
    let observed_cells: Vec<_> = native_cells
        .iter()
        .map(|r| json!({"position":r["position"],"id":r["id"],"states":r["states"]}))
        .collect();
    if observed_cells != *frozen["cells"].as_array().ok_or("Missing sampled cells")?
        || capture["completion_record"]["worldBlocksAfter"].as_u64()
            != Some(observed_cells.len() as u64)
    {
        return Err("Native collision cells differ from frozen fixture".into());
    }
    for cell in frozen["cells"].as_array().ok_or("Missing sampled cells")? {
        let p: [i32; 3] = serde_json::from_value(cell["position"].clone())?;
        let block = world.rays.block_at(p, &palette)?;
        let actual = match block {
            Some(b) => {
                json!({"position":p,"id":b.id,"states":b.states.iter().cloned().collect::<std::collections::BTreeMap<_,_>>()})
            }
            None => json!({"position":p,"id":"minecraft:air","states":{}}),
        };
        if actual != *cell {
            cell_errors.push(json!({"expected":cell,"actual":actual}));
        }
    }
    let complete = errors.is_empty() && cell_errors.is_empty();
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":complete,"cases":cases.len(),"frames":expected.len(),"cells":frozen["cells"].as_array().unwrap().len(),"failures":errors.len(),"errors":errors,"cell_errors":cell_errors,"max_error":maximum,"capture_sha256":capture_hash,"predictions_sha256":frozen_hash,"saved_world_reproduces":true})
        )?
    );
    Ok(complete)
}
