use crate::{REFERENCE, Result};
use mcbe_core::{
    inventory::{ContainerKind, Containers, InventoryError},
    items::{ItemStack, Registry},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;

#[derive(Deserialize)]
struct StackSpec {
    id: String,
    amount: f64,
}
#[derive(Deserialize)]
struct Operation {
    op: String,
    c: usize,
    #[serde(default)]
    slot: Option<f64>,
    #[serde(default)]
    to: Option<usize>,
    #[serde(default)]
    destination: Option<f64>,
    #[serde(default)]
    item: Option<StackSpec>,
    #[serde(default)]
    amount: Option<f64>,
}
#[derive(Deserialize)]
struct Scenario {
    index: usize,
    types: Vec<String>,
    ops: Vec<Operation>,
}
fn required<T: Copy>(value: Option<T>, name: &str) -> Result<T> {
    value.ok_or_else(|| format!("Operation requires {name}").into())
}
fn observation(stack: Option<&ItemStack>) -> Value {
    stack.map_or(Value::Null, ItemStack::observation)
}
fn predictions(registry: &Registry, raw: &Value) -> Result<Value> {
    let cases: Vec<Scenario> = serde_json::from_value(raw.clone())?;
    if cases.is_empty() || cases.len() > 1024 {
        return Err("Bounded nonempty inventory scenarios required".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut rows = vec![];
    for case in &cases {
        if !seen.insert(case.index) || case.types.len() != 2 || case.ops.len() > 4096 {
            return Err("Duplicate or unbounded inventory scenario".into());
        }
        let sizes = case
            .types
            .iter()
            .map(|kind| match kind.as_str() {
                "barrel" => Ok(ContainerKind::Barrel),
                "hopper" => Ok(ContainerKind::Hopper),
                "dispenser" => Ok(ContainerKind::Dispenser),
                _ => Err("Unmeasured container type"),
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut containers = Containers::with_kinds(registry, &sizes)?;
        rows.push(json!({"case":case.index,"step":-1,"success":true,"result":null,"inputAfter":null,"errorName":null,"state":containers.observation()}));
        for (step, op) in case.ops.iter().enumerate() {
            if ![
                "set", "set-copy", "get", "get-copy", "add", "clear", "move", "swap", "transfer",
            ]
            .contains(&op.op.as_str())
            {
                return Err("Unknown inventory scenario operation".into());
            }
            let slot = if matches!(op.op.as_str(), "add" | "clear") {
                0.
            } else {
                required(op.slot, "slot")?
            };
            let to = if matches!(op.op.as_str(), "move" | "swap" | "transfer") {
                required(op.to, "to")?
            } else {
                0
            };
            let destination = if matches!(op.op.as_str(), "move" | "swap") {
                required(op.destination, "destination")?
            } else {
                0.
            };
            let amount = if matches!(op.op.as_str(), "get-copy" | "set-copy") {
                required(op.amount, "amount")?
            } else {
                1.
            };
            let mut input = None;
            let result = (|| -> std::result::Result<Option<ItemStack>, InventoryError> {
                match op.op.as_str() {
                    "set" | "set-copy" => {
                        input = op
                            .item
                            .as_ref()
                            .map(|s| registry.create_stack(&s.id, s.amount))
                            .transpose()?;
                        containers.set(op.c, slot, input.as_ref())?;
                        if op.op == "set-copy" {
                            let stack = input.as_mut().ok_or(InventoryError::InvalidSlot)?;
                            stack.set_amount(amount)?;
                            Ok(Some(stack.clone()))
                        } else {
                            Ok(None)
                        }
                    }
                    "get" => containers.get(op.c, slot),
                    "get-copy" => {
                        input = containers.get(op.c, slot)?;
                        if let Some(stack) = &mut input {
                            stack.set_amount(amount)?;
                        }
                        Ok(input.clone())
                    }
                    "add" => {
                        let spec = op.item.as_ref().ok_or(InventoryError::InvalidSlot)?;
                        input = Some(registry.create_stack(&spec.id, spec.amount)?);
                        containers.add(op.c, input.as_ref().ok_or(InventoryError::InvalidSlot)?)
                    }
                    "clear" => {
                        containers.clear(op.c)?;
                        Ok(None)
                    }
                    "move" => {
                        containers.move_item(op.c, slot, to, destination)?;
                        Ok(None)
                    }
                    "swap" => {
                        containers.swap_items(op.c, slot, to, destination)?;
                        Ok(None)
                    }
                    "transfer" => containers.transfer_item(op.c, slot, to),
                    _ => Err(InventoryError::InvalidSlot),
                }
            })();
            let (success, result, error) = match result {
                Ok(value) => (true, observation(value.as_ref()), Value::Null),
                Err(error) => (false, Value::Null, json!(error.category())),
            };
            rows.push(json!({"case":case.index,"step":step,"success":success,"result":result,"inputAfter":observation(input.as_ref()),"errorName":error,"state":containers.observation()}));
        }
    }
    Ok(
        json!({"complete":true,"scenarios":cases.len(),"steps":rows.len(),"rows":rows,"registry_sha256":registry.source_id()}),
    )
}
fn normalized(row: &Value) -> Value {
    let mut value = json!({});
    for key in [
        "case",
        "step",
        "success",
        "result",
        "inputAfter",
        "errorName",
        "state",
    ] {
        value[key] = row[key].clone();
    }
    value
}
pub fn run(args: &[String], verify: bool) -> Result<bool> {
    if (!verify && args.len() != 3) || (verify && !(4..=5).contains(&args.len())) {
        return Err("predict-inventory REGISTRY CASES | verify-inventory REGISTRY CASES CAPTURE [FROZEN_PREDICTIONS]".into());
    }
    let registry = Registry::from_bytes(&fs::read(&args[1])?)?;
    if registry.metadata()["executable_sha256"] != REFERENCE {
        return Err("Unknown inventory reference registry".into());
    }
    let raw: Value = serde_json::from_slice(&fs::read(&args[2])?)?;
    let generated = predictions(&registry, &raw)?;
    if !verify {
        println!("{}", serde_json::to_string_pretty(&generated)?);
        return Ok(true);
    }
    if let Some(path) = args.get(4) {
        let frozen: Value = serde_json::from_slice(&fs::read(path)?)?;
        if generated != frozen {
            return Err("Frozen inventory predictions do not reproduce exactly".into());
        }
    }
    let bytes = fs::read(&args[3])?;
    let capture: Value = serde_json::from_slice(&bytes)?;
    let actual = capture["inventory_steps"]
        .as_array()
        .ok_or("Missing native inventory steps")?;
    if capture["complete"] != true
        || capture["metadata"]["executable_sha256"] != REFERENCE
        || capture["errors"].as_array().is_none_or(|r| !r.is_empty())
        || capture["completion_record"]["inventorySteps"].as_u64() != Some(actual.len() as u64)
    {
        return Err("Incomplete or unknown inventory capture".into());
    }
    let expected = generated["rows"]
        .as_array()
        .ok_or("Missing predicted rows")?;
    let mut unique = std::collections::BTreeMap::new();
    for row in actual {
        let key = (
            row["case"].as_u64().ok_or("Invalid native case")?,
            row["step"].as_i64().ok_or("Invalid native step")?,
        );
        if unique.insert(key, row).is_some() {
            return Err("Duplicate native inventory step".into());
        }
    }
    if unique.len() != expected.len() {
        return Err("Native inventory step count differs".into());
    }
    let mut errors = vec![];
    let mut failures = 0;
    let mut slot_comparisons = 0;
    let mut rejected = 0;
    let mut operations = std::collections::BTreeMap::<String, usize>::new();
    for row in expected {
        let case = row["case"].as_u64().ok_or("Invalid generated case")?;
        let step = row["step"].as_i64().ok_or("Invalid generated step")?;
        let native = unique
            .get(&(case, step))
            .ok_or("Missing native inventory step")?;
        slot_comparisons += row["state"]
            .as_array()
            .ok_or("Missing inventory state")?
            .iter()
            .map(|c| c["slots"].as_array().map_or(0, Vec::len))
            .sum::<usize>();
        if step >= 0 {
            let cases = raw.as_array().ok_or("Invalid cases")?;
            let scenario = cases
                .iter()
                .find(|c| c["index"].as_u64() == Some(case))
                .ok_or("Case absent")?;
            let name = scenario["ops"][step as usize]["op"]
                .as_str()
                .ok_or("Invalid operation")?;
            *operations.entry(name.to_owned()).or_default() += 1;
            if row["success"] == false {
                rejected += 1;
            }
        }
        if normalized(native) != *row {
            failures += 1;
            if errors.len() < 20 {
                errors.push(json!({"case":case,"step":step,"expected":row,"actual":native}));
            }
        }
    }
    let complete = failures == 0;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":complete,"scenarios":generated["scenarios"],"steps":expected.len(),"slot_comparisons":slot_comparisons,"operations":operations,"rejected_operations":rejected,"failures":failures,"errors":errors,"registry_sha256":registry.source_id(),"capture_sha256":format!("{:x}",Sha256::digest(bytes)),"scope":"Pristine container API state and result comparisons; no GUI gestures, item metadata, automation, player inventory or save-entity parity"})
        )?
    );
    Ok(complete)
}
