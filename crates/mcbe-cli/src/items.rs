use crate::{REFERENCE, Result};
use mcbe_core::items::{ItemDefinition, Registry};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;

fn cases<'a>(raw: &'a Value, key: &str) -> Result<&'a [Value]> {
    match raw.get(key) {
        None => Ok(&[]),
        Some(Value::Array(rows)) => Ok(rows),
        _ => Err(format!("Invalid behavior array: {key}").into()),
    }
}

fn load(path: &str) -> Result<Registry> {
    let registry = Registry::from_bytes(&fs::read(path)?)?;
    if registry.metadata()["executable_sha256"] != REFERENCE {
        return Err("Registry does not identify the measured reference executable".into());
    }
    Ok(registry)
}
pub fn inspect(args: &[String]) -> Result<bool> {
    if !(4..=5).contains(&args.len()) {
        return Err("item REGISTRY ID AMOUNT [SET_AMOUNT]".into());
    }
    let registry = load(&args[1])?;
    let mut stack = registry.create_stack(&args[2], args[3].parse()?)?;
    let constructed = stack.observation();
    let assignment = if let Some(value) = args.get(4) {
        match stack.set_amount(value.parse()?) {
            Ok(()) => json!({"success":true,"observation":stack.observation()}),
            Err(error) => {
                json!({"success":false,"category":error.category(),"observation":stack.observation()})
            }
        }
    } else {
        Value::Null
    };
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"registry_sha256":registry.source_id(),"definition":stack.definition(),"constructed":constructed,"assignment":assignment})
        )?
    );
    Ok(true)
}

pub fn verify(args: &[String]) -> Result<bool> {
    if args.len() != 3 {
        return Err("verify-items MODEL_REGISTRY CAPTURE".into());
    }
    let registry = load(&args[1])?;
    let bytes = fs::read(&args[2])?;
    let raw: Value = serde_json::from_slice(&bytes)?;
    // The holdout contains behavior only, with zero registry rows.
    let observed_items = raw["items"].as_array().ok_or("Missing item rows")?;
    let observed_blocks = raw["blocks"].as_array().ok_or("Missing block rows")?;
    let done = &raw["completion_record"];
    if raw["complete"] != true
        || raw["errors"].as_array().is_none_or(|v| !v.is_empty())
        || raw["metadata"]["executable_sha256"] != REFERENCE
        || done["kind"] != "done"
        || done["errors"] != 0
        || done["items"].as_u64() != Some(observed_items.len() as u64)
        || done["blocks"].as_u64() != Some(observed_blocks.len() as u64)
    {
        return Err("Incomplete or inconsistent behavior capture".into());
    }
    if !observed_items.is_empty() {
        load(&args[2])?;
    }
    let mut failures = Vec::new();
    for row in observed_items {
        let definition: ItemDefinition = serde_json::from_value(row.clone())?;
        if registry.definition(&definition.id) != Some(&definition) {
            failures.push(json!({"operation":"registry-item","id":definition.id}));
        }
    }
    for row in observed_blocks {
        if registry.default_block(row["id"].as_str().ok_or("Missing block ID")?)? != Some(row) {
            failures.push(json!({"operation":"registry-block","id":row["id"]}));
        }
    }
    let mut counts = [0usize; 3];
    for (index, key) in ["stack_cases", "stack_set_cases"].iter().enumerate() {
        let rows = cases(&raw, key)?;
        for case in rows {
            let id = case["item"].as_str().ok_or("Missing item identifier")?;
            let value = case["inputAmount"]
                .as_f64()
                .ok_or("Only numeric inputs are measured")?;
            let mut stack = if index == 1 {
                Some(registry.create_stack(id, 1.0)?)
            } else {
                None
            };
            let outcome = if let Some(stack) = stack.as_mut() {
                stack.set_amount(value).map(|()| stack.observation())
            } else {
                registry.create_stack(id, value).map(|s| s.observation())
            };
            let (actual, expected) = match outcome {
                Ok(mut observation) => {
                    observation["success"] = json!(true);
                    let mut expected = json!({});
                    for key in ["success", "typeId", "amount", "maxAmount", "isStackable"] {
                        expected[key] = case[key].clone();
                    }
                    (observation, expected)
                }
                Err(error) => {
                    let category = case["error"]
                        .as_str()
                        .unwrap_or("")
                        .split(':')
                        .next()
                        .unwrap_or("");
                    let mut actual = json!({"success":false,"category":error.category()});
                    let mut expected = json!({"success":case["success"],"category":category});
                    if let Some(stack) = stack.as_ref() {
                        actual["amountAfterError"] = json!(stack.amount());
                        expected["amountAfterError"] = case["amountAfterError"].clone();
                    }
                    (actual, expected)
                }
            };
            counts[index] += 1;
            if actual != expected {
                failures
                    .push(json!({"operation":key,"case":case,"actual":actual,"expected":expected}));
            }
        }
    }
    for case in cases(&raw, "stack_pairs")? {
        let a = registry.create_stack(
            case["a"].as_str().ok_or("Missing a")?,
            case["aAmount"].as_f64().ok_or("Missing a amount")?,
        )?;
        let b = registry.create_stack(
            case["b"].as_str().ok_or("Missing b")?,
            case["bAmount"].as_f64().ok_or("Missing b amount")?,
        )?;
        let actual = a.is_stackable_with(&b)?;
        if case["stackable"].as_bool() != Some(actual) {
            failures.push(json!({"operation":"stackability","case":case,"actual":actual}));
        }
        counts[2] += 1;
    }
    if counts.iter().sum::<usize>() == 0 {
        return Err("Empty behavior capture".into());
    }
    for (key, count) in ["stackCases", "stackSetCases", "stackPairs"]
        .iter()
        .zip(counts)
    {
        if let Some(recorded) = done.get(key)
            && recorded.as_u64() != Some(count as u64)
        {
            return Err("Behavior completion counts disagree".into());
        }
    }
    let complete = failures.is_empty();
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":complete,"constructors":counts[0],"setters":counts[1],"pairs":counts[2],"comparisons":counts.iter().sum::<usize>(),"failures":failures,"observed_items":observed_items.len(),"observed_blocks":observed_blocks.len(),"model_definitions":registry.item_count(),"tags":registry.tag_items().len(),"model_sha256":registry.source_id(),"capture_sha256":format!("{:x}",Sha256::digest(&bytes)),"scope":"Pristine numeric stack operations; retained native evidence replay, not a new native experiment"})
        )?
    );
    Ok(complete)
}
