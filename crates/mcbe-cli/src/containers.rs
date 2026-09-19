use crate::{REFERENCE, Result, blocks};
use mcbe_core::{chunks::World, container_storage::Store, items::Registry, leveldb, world_edit};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
fn registry(path: &str) -> Result<Registry> {
    let r = Registry::from_bytes(&fs::read(path)?)?;
    if r.metadata()["executable_sha256"] != REFERENCE {
        return Err("Unknown container registry reference".into());
    }
    Ok(r)
}
fn positions(rows: &[Value]) -> Result<Vec<[i32; 3]>> {
    rows.iter()
        .map(|r| serde_json::from_value(r["position"].clone()).map_err(Into::into))
        .collect()
}
fn inspect(
    registry: &Registry,
    palette: &mcbe_core::blocks::Palette,
    database: &Path,
) -> Result<Value> {
    let mut world = World::new(leveldb::read_snapshot(database)?);
    let store = Store::read(&world.snapshot, registry)?;
    let mut rows = vec![];
    for position in store.positions() {
        let mut row = store.observation(position, registry)?;
        let block = world
            .block(position, 0)?
            .ok_or("Stored container has no block")?;
        let canonical = palette.resolve_saved(&block)?;
        row["id"] = json!(canonical.id());
        row["states"] = json!(canonical.states());
        rows.push(row);
    }
    Ok(
        json!({"complete":true,"rows":rows,"keys":store.key_observations(),"template_hashes":store.template_hashes(),"database_files":world.snapshot.files,"registry_sha256":registry.source_id()}),
    )
}
pub fn run(args: &[String], verify: bool) -> Result<bool> {
    if args.len() != if verify { 6 } else { 5 } {
        return Err("containers REGISTRY PALETTE PARTIAL DB | verify-containers REGISTRY PALETTE PARTIAL DB EXPECTED".into());
    }
    let registry = registry(&args[1])?;
    let (palette, _, _) = blocks::load(Path::new(&args[2]), Path::new(&args[3]))?;
    let actual = inspect(&registry, &palette, Path::new(&args[4]))?;
    if !verify {
        println!("{}", serde_json::to_string_pretty(&actual)?);
        return Ok(true);
    }
    let (expected, expected_hash) = blocks::read(Path::new(&args[5]))?;
    if expected["complete"] != true {
        return Err("Complete container expectations required".into());
    }
    let fixture = expected.get("rows").is_some();
    if !fixture && expected["metadata"]["executable_sha256"] != REFERENCE {
        return Err("Unknown native container capture".into());
    }
    if fixture && expected["registry_sha256"] != registry.source_id() {
        return Err("Independent fixture registry differs".into());
    }
    let rows = expected[if fixture { "rows" } else { "container_states" }]
        .as_array()
        .ok_or("Container rows missing")?;
    let generated = actual["rows"].as_array().ok_or("Generated rows missing")?;
    let mut unique = BTreeMap::new();
    for row in rows {
        let position: [i32; 3] = serde_json::from_value(row["position"].clone())?;
        if unique.insert(position, row).is_some() {
            return Err("Duplicate expected container position".into());
        }
    }
    if unique.len() != generated.len() {
        return Err("Container expectation coverage differs".into());
    }
    let mut errors = vec![];
    let mut slots = 0;
    let mut items = 0;
    for row in generated {
        let p: [i32; 3] = serde_json::from_value(row["position"].clone())?;
        let expected = unique.get(&p).ok_or("Missing expected container")?;
        slots += row["size"].as_u64().ok_or("Invalid container capacity")?;
        items += row["slots"]
            .as_array()
            .ok_or("Missing slots")?
            .iter()
            .filter(|s| !s.is_null())
            .count();
        for key in ["id", "states", "size", "emptySlotsCount", "slots"] {
            if row[key] != expected[key] {
                errors.push(
                    json!({"position":p,"field":key,"expected":expected[key],"actual":row[key]}),
                );
            }
        }
        if fixture {
            for key in ["entity_id", "record_sha256"] {
                if row[key] != expected[key] {
                    errors.push(json!({"position":p,"field":key}));
                }
            }
        }
    }
    if fixture {
        if actual["template_hashes"] != expected["template_hashes"] {
            errors.push(json!({"field":"template_hashes"}));
        }
        let keyed = |value: &Value| -> Result<BTreeMap<String, Value>> {
            value
                .as_array()
                .ok_or("Missing container keys")?
                .iter()
                .map(|r| {
                    Ok((
                        r["key_hex"].as_str().ok_or("Key hex missing")?.to_owned(),
                        r.clone(),
                    ))
                })
                .collect::<Result<BTreeMap<_, _>>>()
        };
        if keyed(&actual["keys"])? != keyed(&expected["keys"])? {
            errors.push(json!({"field":"stream_keys"}));
        }
    }
    let complete = errors.is_empty();
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":complete,"containers":generated.len(),"slots":slots,"items":items,"stream_keys":actual["keys"].as_array().map_or(0,Vec::len),"pristine_templates":actual["template_hashes"].as_object().map_or(0,serde_json::Map::len),"failures":errors.len(),"errors":errors,"expected_sha256":expected_hash,"database_files":actual["database_files"],"scope":"Finite Overworld container records, metadata observations and byte-exact stream reconstruction. Mutations support pristine source-backed item prototypes only."})
        )?
    );
    Ok(complete)
}
pub fn edit(args: &[String]) -> Result<bool> {
    if args.len() != 7 {
        return Err(
            "container-edit REGISTRY PALETTE PARTIAL SOURCE_WORLD NEW_WORLD EDITS_JSON".into(),
        );
    }
    let registry = registry(&args[1])?;
    let (palette, _, _) = blocks::load(Path::new(&args[2]), Path::new(&args[3]))?;
    let source = Path::new(&args[4]);
    let destination = Path::new(&args[5]);
    if destination.exists() {
        return Err("Container edit destination already exists".into());
    }
    let before = world_edit::tree_hashes(source)?;
    let snapshot = leveldb::read_snapshot(&source.join("db"))?;
    let store = Store::read(&snapshot, &registry)?;
    let (edits, edit_hash) = blocks::read(Path::new(&args[6]))?;
    let rows = edits["containers"]
        .as_array()
        .filter(|r| !r.is_empty())
        .ok_or("Nonempty container edits required")?;
    let selected = positions(rows)?;
    store.check_blocks(&snapshot, &selected, &palette)?;
    let mut contents = store.load_group(&selected, &registry)?;
    for (index, row) in rows.iter().enumerate() {
        contents.clear(index)?;
        let mut seen = BTreeSet::new();
        for slot in row["slots"].as_array().ok_or("Edited slots missing")? {
            let position = slot["slot"].as_u64().ok_or("Invalid edited slot")?;
            if !seen.insert(position) {
                return Err("Duplicate edited slot".into());
            }
            let stack = registry.create_stack(
                slot["item"].as_str().ok_or("Item id missing")?,
                slot["amount"].as_f64().ok_or("Item amount missing")?,
            )?;
            contents.set(index, position as f64, Some(&stack))?;
        }
    }
    let prepared = store.prepare(&selected, &contents, &registry)?;
    if world_edit::tree_hashes(source)? != before {
        return Err("Source changed while preparing container edits".into());
    }
    let copied = world_edit::edit_copy(source, destination, &prepared.updates)?;
    let reloaded = leveldb::read_snapshot(&destination.join("db"))?;
    let after = Store::read(&reloaded, &registry)?;
    after.check_blocks(&reloaded, &selected, &palette)?;
    if after.load_group(&selected, &registry)?.observation() != contents.observation() {
        return Err("Copied-world inventory round trip differs".into());
    }
    let changed: BTreeSet<_> = prepared.updates.iter().map(|(key, _)| key).collect();
    let old_values: BTreeMap<_, _> = snapshot
        .entries
        .iter()
        .filter_map(|(k, e)| e.value.as_ref().map(|v| (k, v)))
        .collect();
    let new_values: BTreeMap<_, _> = reloaded
        .entries
        .iter()
        .filter_map(|(k, e)| e.value.as_ref().map(|v| (k, v)))
        .collect();
    if old_values.keys().ne(new_values.keys())
        || old_values
            .iter()
            .any(|(k, v)| !changed.contains(k) && new_values.get(k) != Some(v))
    {
        return Err("Container edit changed unrelated database values".into());
    }
    let mut preserved = 0;
    let before_records = store.record_hashes()?;
    let after_records = after.record_hashes()?;
    if before_records.keys().ne(after_records.keys()) {
        return Err("Block entity positions changed during container edit".into());
    }
    for (position, digest) in before_records {
        if !selected.contains(&position) {
            if after_records.get(&position) != Some(&digest) {
                return Err("Unselected container NBT changed".into());
            }
            preserved += 1;
        }
    }
    let complete = world_edit::tree_hashes(source)? == before;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":complete,"changed_containers":prepared.changed_containers,"changed_slots":prepared.changed_slots,"updated_keys":prepared.updates.len(),"unselected_container_records_preserved":preserved,"source_unchanged":complete,"edit":copied,"edits_sha256":edit_hash,"scope":"Prototype-backed pristine container contents in a new world copy; native acceptance requires separate reload."})
        )?
    );
    Ok(complete)
}
