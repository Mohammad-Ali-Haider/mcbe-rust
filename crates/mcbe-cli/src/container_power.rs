use crate::{Result, blocks};
use mcbe_core::{
    container_geometry::ContainerKind,
    container_power::{Controller, Event},
};
use serde_json::json;
use std::path::Path;
pub fn predict(args: &[String]) -> Result<bool> {
    if args.len() != 3 {
        return Err("predict-container-power CASES STEPS".into());
    }
    let (raw, _) = blocks::read(Path::new(&args[1]))?;
    let cases = raw
        .as_array()
        .ok_or("Container power case array required")?;
    let steps: usize = args[2].parse()?;
    if cases.is_empty() || cases.len() > 1024 || steps > 1000 {
        return Err("Bounded container power cases required".into());
    }
    let mut rows = Vec::new();
    for c in cases {
        let kind = match c["family"].as_str() {
            Some("barrel") => ContainerKind::Barrel,
            Some("dispenser") => ContainerKind::Dispenser,
            Some("hopper") => ContainerKind::Hopper,
            _ => return Err("Unsupported powered container".into()),
        };
        let gap = c["gap"].as_u64().ok_or("Missing power topology")?;
        if gap != 1 && gap != 2 {
            return Err("Only direct and single-stone-spacer controls are measured".into());
        }
        let initial_flag = c["initial"].as_bool().ok_or("Missing initial flag")?;
        let phase = u8::try_from(c["phase"].as_u64().unwrap_or(0))?;
        let mut model = Controller::with_phase(kind, initial_flag, phase)?;
        let mut drops = 0;
        let initial = c["initialPower"].as_bool().ok_or("Missing initial power")?;
        for tick in 0..=steps {
            drops += model
                .tick(tick as u64)?
                .iter()
                .filter(|e| **e == Event::Dispense)
                .count();
            if tick == 0 {
                model.permutation_changed(0, initial_flag, false)?;
                if gap == 1 {
                    model.neighbor_changed(0, initial)?;
                }
            }
            let action = c["actions"]
                .as_array()
                .and_then(|v| v.iter().find(|a| a["step"].as_u64() == Some(tick as u64)))
                .map(|a| a["power"].as_bool().ok_or("Invalid power action"))
                .transpose()?;
            let power = if c["actions"].is_array() {
                action
            } else {
                action.or(match tick {
                    20 | 60 => Some(false),
                    40 | 80 => Some(true),
                    _ => None,
                })
            };
            if let Some(power) = power
                && gap == 1
            {
                model.neighbor_changed(tick as u64, power)?;
            }
            rows.push(json!({"case":c["index"],"step":tick,"flag":model.flag(),"dispense_requests":drops}));
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":true,"rows":rows,"scope":"Measured neighbor notification model; no item/entity executor or full redstone graph"})
        )?
    );
    Ok(true)
}
