//! Stable string IDs for persistent machine inventories and processing timers.
use crate::{
    engine::{
        SavedStack, WorldState,
        machines::{
            MAX_MACHINES, Machines,
            furnace::Furnace,
            recipes::{Definitions, Recipes},
        },
    },
    engine_blocks::Registry,
    engine_inventory::Stack,
    scene::{Client, Result},
};
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedMachines {
    pub clock_ms: u64,
    pub furnaces: Vec<SavedFurnace>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedFurnace {
    position: [i32; 3],
    slots: [Option<SavedStack>; 3],
    burn_ms: u32,
    burn_total_ms: u32,
    cook_ms: u32,
    cooking: Option<String>,
    updated_ms: u64,
}
impl SavedFurnace {
    pub fn has_item_data(&self) -> bool {
        self.slots.iter().flatten().any(|s| s.data.is_some())
    }
}
impl SavedMachines {
    pub fn capture(m: &Machines, r: &Registry) -> Result<Self> {
        let mut furnaces = Vec::new();
        for (p, f) in &m.furnaces {
            f.validate(r.blocks.len(), m.clock_ms)?;
            for s in f.slots.iter().flatten() {
                s.validate_item(r)?;
            }
            furnaces.push(SavedFurnace {
                position: *p,
                slots: f.slots.map(|s| {
                    s.map(|s| SavedStack {
                        data: s.data,
                        block: r.get(s.id).id.clone(),
                        count: s.count,
                    })
                }),
                burn_ms: f.burn_ms,
                burn_total_ms: f.burn_total_ms,
                cook_ms: f.cook_ms,
                cooking: f.cooking.map(|id| r.get(id).id.clone()),
                updated_ms: f.updated_ms,
            });
        }
        Ok(Self {
            clock_ms: m.clock_ms,
            furnaces,
        })
    }
    /// Reset only changed recipes before strict timer validation under the new pack.
    pub fn reconfigured(
        m: &Machines,
        old: &Registry,
        new: &Registry,
        previous: &Recipes,
        next: &Recipes,
    ) -> Result<Self> {
        let previous = previous.definitions.clone().available(new).compile(new)?;
        let mut saved = Self::capture(m, old)?;
        for f in &mut saved.furnaces {
            if let Some(input) = &f.slots[0] {
                let id = new.find(&input.block)?;
                if previous.get(id) != next.get(id) {
                    f.cook_ms = 0;
                    f.cooking = None;
                }
            }
        }
        Ok(saved)
    }
    pub(crate) fn restore(self, r: &Registry, recipes: &Recipes) -> Result<Machines> {
        if self.furnaces.len() > MAX_MACHINES {
            return Err("Saved machine budget exceeded".into());
        }
        let mut m = Machines::at_clock(self.clock_ms);
        for saved in self.furnaces {
            let slots = saved
                .slots
                .into_iter()
                .map(|s| {
                    s.map(|s| {
                        Ok(Stack {
                            data: s.data,
                            id: r.find(&s.block)?,
                            count: s.count,
                        })
                    })
                    .transpose()
                })
                .collect::<Result<Vec<_>>>()?;
            let f = Furnace {
                slots: slots.try_into().unwrap(),
                burn_ms: saved.burn_ms,
                burn_total_ms: saved.burn_total_ms,
                cook_ms: saved.cook_ms,
                cooking: saved.cooking.map(|s| r.find(&s)).transpose()?,
                updated_ms: saved.updated_ms,
            };
            f.validate(r.blocks.len(), m.clock_ms)?;
            for s in f.slots.iter().flatten() {
                s.validate_item(r)?;
            }
            if f.cook_ms > 0
                && (f.slots[0].map(|s| s.id) != f.cooking
                    || f.slots[0]
                        .and_then(|s| recipes.get(s.id))
                        .is_none_or(|r| f.cook_ms >= r.cook_ms))
            {
                return Err("Saved furnace progress mismatches recipe".into());
            }
            if m.furnaces.insert(saved.position, f).is_some() {
                return Err("Duplicate furnace position".into());
            }
        }
        m.rehydrate(recipes);
        Ok(m)
    }
}
pub fn restore(
    client: &mut Client,
    saved: Option<SavedMachines>,
    definitions: Option<Definitions>,
    version: u32,
) -> Result<()> {
    let state = client.scene.engine.as_ref().unwrap();
    let recipes = match definitions {
        Some(d) if matches!(version, 6..=15) => d.compile(&state.registry)?,
        None if version < 6 => Definitions::built_in()
            .available(&state.registry)
            .compile(&state.registry)?,
        _ => return Err("Save smelting/version mismatch".into()),
    };
    let machines = match saved {
        Some(s) if matches!(version, 6..=15) => s.restore(&state.registry, &recipes)?,
        None if version < 6 => Machines::default(),
        _ => return Err("Save machines/version mismatch".into()),
    };
    validate_positions(state, &machines, &state.registry)?;
    let state = client.scene.engine.as_mut().unwrap();
    state.smelting = recipes;
    state.original_machines = machines.clone();
    state.machines = machines;
    Ok(())
}
pub fn validate_positions(state: &WorldState, machines: &Machines, r: &Registry) -> Result<()> {
    for p in machines.furnaces.keys() {
        let id = if let Some(s) = &state.stream {
            s.edits
                .get(p)
                .copied()
                .unwrap_or_else(|| s.generator.block(*p))
        } else {
            state.blocks.get(p).copied()
        };
        if id.is_none_or(|id| {
            r.get(id).interaction != Some(crate::engine::blocks::components::Interaction::Furnace)
        }) {
            return Err("Machine does not belong to a furnace block".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine;
    fn populate(c: &mut Client, p: [i32; 3]) {
        let r = &c.scene.engine.as_ref().unwrap().registry;
        let furnace = r.find("furnace").unwrap();
        let sand = r.find("sand").unwrap();
        let oak = r.find("oak_planks").unwrap();
        c.scene.change_engine(p, Some(furnace)).unwrap();
        let s = c.scene.engine.as_mut().unwrap();
        s.machines.ensure(p).unwrap();
        s.machines.furnaces.get_mut(&p).unwrap().slots = [
            Some(Stack {
                data: None,
                id: sand,
                count: 4,
            }),
            Some(Stack {
                data: None,
                id: oak,
                count: 3,
            }),
            None,
        ];
        s.machines.wake(p, &s.smelting);
        s.machines.advance(3500, &s.smelting);
        s.player.table[8] = Some(Stack {
            data: None,
            id: sand,
            count: 2,
        });
    }
    #[test]
    fn machine_save_snapshot_keeps_newer_progress_pending_and_remaps_stable_ids() {
        let mut c = engine::generate(360036).unwrap();
        let p = [4, 40 - 2, -2];
        populate(&mut c, p);
        let snapshot = engine::SaveSnapshot::capture(&c).unwrap();
        let directory = std::env::temp_dir().join(format!(
            "mc-stations-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let committed = snapshot.write(&directory).unwrap();
        let bytes = std::fs::read(&committed.path).unwrap();
        let s = c.scene.engine.as_mut().unwrap();
        s.machines.advance(7000, &s.smelting);
        assert_eq!(s.machines.furnaces[&p].slots[2].unwrap().count, 1);
        assert!(committed.adopt(&mut c).unwrap());
        assert!(c.pending_edits() > 0);
        let restored = engine::decode(&bytes, None).unwrap();
        let saved = restored.scene.engine.as_ref().unwrap();
        assert_eq!(saved.machines.furnaces[&p].cook_ms, 3500);
        assert_eq!(saved.player.table[8].unwrap().count, 2);
        assert_eq!(restored.pending_edits(), 0);
        let mut reverse = c.scene.engine.as_ref().unwrap().registry.clone();
        reverse.blocks.reverse();
        engine::apply_registry(&mut c, reverse).unwrap();
        let s = c.scene.engine.as_ref().unwrap();
        assert_eq!(
            s.registry
                .get(s.machines.furnaces[&p].slots[2].unwrap().id)
                .id,
            "glass"
        );
        assert_eq!(s.registry.get(s.player.table[8].unwrap().id).id, "sand");
        let edits = s.edit_revision;
        assert!(c.scene.change_engine(p, None).is_err());
        assert_eq!(c.scene.engine.as_ref().unwrap().edit_revision, edits);
        c.scene
            .engine
            .as_mut()
            .unwrap()
            .machines
            .furnaces
            .get_mut(&p)
            .unwrap()
            .slots = [None; 3];
        c.scene.change_engine(p, None).unwrap();
        assert!(
            !c.scene
                .engine
                .as_ref()
                .unwrap()
                .machines
                .furnaces
                .contains_key(&p)
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn evicted_furnaces_process_and_restore_without_resident_voxels() {
        let mut c = crate::engine_stream::generate(360036, Registry::built_in()).unwrap();
        let p = [4, 40, -2];
        populate(&mut c, p);
        crate::engine_stream::jump(&mut c, 1).unwrap();
        assert!(!c.scene.engine.as_ref().unwrap().blocks.contains_key(&p));
        let s = c.scene.engine.as_mut().unwrap();
        s.machines.advance(7000, &s.smelting);
        let value = serde_json::to_vec(&engine::document(&c).unwrap()).unwrap();
        let mut copy = engine::decode(&value, None).unwrap();
        let s = copy.scene.engine.as_ref().unwrap();
        assert_eq!(s.machines.furnaces[&p].slots[2].unwrap().count, 1);
        assert_eq!(s.machines.furnaces[&p].cook_ms, 500);
        assert_eq!(copy.pending_edits(), 0);
        copy.set_spawn(engine::SPAWN).unwrap();
        let s = copy.scene.engine.as_ref().unwrap();
        assert_eq!(s.registry.get(s.blocks[&p]).id, "furnace");
    }
    #[test]
    fn save_rejects_orphans_duplicates_and_invalid_timers_and_accepts_legacy_five() {
        let mut c = engine::generate(360036).unwrap();
        populate(&mut c, [4, 38, -2]);
        let value = serde_json::to_value(engine::document(&c).unwrap()).unwrap();
        for (field, v) in [
            ("burn_ms", serde_json::json!(15001)),
            ("cook_ms", serde_json::json!(10000)),
            ("updated_ms", serde_json::json!(3501)),
            ("position", serde_json::json!([0, 40, 0])),
        ] {
            let mut bad = value.clone();
            bad["machines"]["furnaces"][0][field] = v;
            assert!(
                engine::decode(&serde_json::to_vec(&bad).unwrap(), None).is_err(),
                "{field}"
            );
        }
        let mut bad = value.clone();
        let entry = bad["machines"]["furnaces"][0].clone();
        bad["machines"]["furnaces"]
            .as_array_mut()
            .unwrap()
            .push(entry);
        assert!(engine::decode(&serde_json::to_vec(&bad).unwrap(), None).is_err());
        let mut legacy =
            serde_json::to_value(engine::document(&engine::generate(37).unwrap()).unwrap())
                .unwrap();
        legacy["version"] = serde_json::json!(5);
        legacy.as_object_mut().unwrap().remove("entities");
        legacy.as_object_mut().unwrap().remove("machines");
        legacy.as_object_mut().unwrap().remove("fluids");
        legacy.as_object_mut().unwrap().remove("smelting");
        legacy["inventory"].as_object_mut().unwrap().remove("table");
        let old = engine::decode(&serde_json::to_vec(&legacy).unwrap(), None).unwrap();
        assert!(
            old.scene
                .engine
                .as_ref()
                .unwrap()
                .player
                .table
                .iter()
                .all(Option::is_none)
        );
    }
}
