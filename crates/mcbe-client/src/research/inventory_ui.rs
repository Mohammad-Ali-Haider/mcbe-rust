//! Development controls around the native-measured container API model.
use crate::inventory_persistence::Storage;
use crate::scene::Result;
use mcbe_core::world_edit;
use mcbe_core::{inventory::Containers, items::Registry};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
use winit::keyboard::KeyCode;
const ITEMS: [&str; 4] = ["stone", "egg", "dirt", "diamond_sword"];
const AMOUNTS: [u8; 4] = [1, 16, 64, 255];
pub struct Panel {
    registry: Registry,
    pub containers: Containers,
    initial: Containers,
    storage: Storage,
    project_root: PathBuf,
    items: Vec<String>,
    focus: usize,
    destination: usize,
    cursors: Vec<usize>,
    names: Vec<String>,
    item: usize,
    amount: usize,
    notice: String,
}
impl Panel {
    pub fn load(root: &Path) -> Result<Self> {
        Self::load_from(root, None)
    }
    pub fn load_from(root: &Path, database: Option<&Path>) -> Result<Self> {
        Self::load_selected(root, database, None)
    }
    pub fn load_target(root: &Path, database: &Path, target: [i32; 3]) -> Result<Self> {
        Self::load_selected(root, Some(database), Some(&[target]))
    }
    fn load_selected(
        root: &Path,
        database: Option<&Path>,
        selected: Option<&[[i32; 3]]>,
    ) -> Result<Self> {
        let registry = Registry::from_bytes(&fs::read(
            root.join("evidence/bds-1.26.52.3/registry-probe-02/registry.json"),
        )?)?;
        if registry.metadata()["executable_sha256"]
            != "19c88569af2e4b7d984e999055a31cbcb0799dacf8bbbf7371eda42f5772a443"
        {
            return Err("Unknown inventory registry".into());
        }
        let default=root.join("tools/vendor/bds-1.26.52.3/runtime-container-storage-01/worlds/reconstruction-probe/db");
        let (storage, containers) =
            Storage::load_selected(root, &registry, database.unwrap_or(&default), selected)?;
        let names = storage.names(&registry)?;
        let available = storage.item_ids();
        let mut items = vec![];
        for favorite in ITEMS {
            let name = format!("minecraft:{favorite}");
            if available.contains(&name) {
                items.push(name);
            }
        }
        for name in available {
            if !items.contains(&name) {
                items.push(name);
            }
        }
        let count = names.len();
        let panel = Self {
            registry,
            initial: containers.clone(),
            containers,
            storage,
            project_root: root.to_path_buf(),
            items,
            focus: 0,
            destination: usize::from(count > 1),
            cursors: vec![0; count],
            names,
            item: 0,
            amount: 0,
            notice: "Select a slot; F transfers its stack into the destination container.".into(),
        };
        Ok(panel)
    }
    fn reset(&mut self) -> Result<()> {
        self.containers = self.initial.clone();
        self.focus = 0;
        self.destination = usize::from(self.names.len() > 1);
        self.cursors.fill(0);
        self.item = 0;
        self.amount = 0;
        Ok(())
    }
    pub fn save_to(&mut self, destination: &Path) -> Result<Value> {
        let report = self
            .storage
            .save_to(&self.containers, &self.registry, destination)?;
        self.initial = self.containers.clone();
        Ok(report)
    }
    pub fn reload_saved(&mut self) -> Result<()> {
        self.containers = self.storage.reload(&self.project_root, &self.registry)?;
        self.initial = self.containers.clone();
        Ok(())
    }
    pub fn source(&self) -> &Path {
        &self.storage.source
    }
    pub fn positions(&self) -> &[[i32; 3]] {
        self.storage.positions()
    }
    pub fn prepared_updates(&self) -> Result<world_edit::Updates> {
        if self.pending_slots()? == 0 {
            Ok(Vec::new())
        } else {
            self.storage
                .prepare_updates(&self.containers, &self.registry)
        }
    }
    pub fn adopt_saved(&mut self, destination: &Path) -> Result<()> {
        self.storage.adopt(
            &self.project_root,
            &self.registry,
            destination,
            &self.containers,
        )?;
        self.initial = self.containers.clone();
        Ok(())
    }
    pub fn pending_slots(&self) -> Result<usize> {
        let mut count = 0;
        for c in 0..self.names.len() {
            for slot in 0..self.containers.container(c)?.size() {
                if self
                    .containers
                    .get(c, slot as f64)?
                    .map(|s| s.observation())
                    != self.initial.get(c, slot as f64)?.map(|s| s.observation())
                {
                    count += 1;
                }
            }
        }
        Ok(count)
    }
    pub fn key(&mut self, key: KeyCode) -> Result<bool> {
        let size = self.containers.container(self.focus)?.size();
        match key {
            KeyCode::Tab => self.focus = (self.focus + 1) % self.names.len(),
            KeyCode::Digit1 => self.destination = 0,
            KeyCode::Digit2 if self.names.len() > 1 => self.destination = 1,
            KeyCode::Digit3 if self.names.len() > 2 => self.destination = 2,
            KeyCode::ArrowLeft => {
                self.cursors[self.focus] = (self.cursors[self.focus] + size - 1) % size
            }
            KeyCode::ArrowRight => self.cursors[self.focus] = (self.cursors[self.focus] + 1) % size,
            KeyCode::ArrowUp => {
                self.cursors[self.focus] = self.cursors[self.focus].saturating_sub(9)
            }
            KeyCode::ArrowDown => {
                self.cursors[self.focus] = (self.cursors[self.focus] + 9).min(size - 1)
            }
            KeyCode::KeyJ if !self.items.is_empty() => {
                self.item = (self.item + 1) % self.items.len()
            }
            KeyCode::KeyK => self.amount = (self.amount + 1) % AMOUNTS.len(),
            KeyCode::KeyA | KeyCode::KeyS => {
                if self.items.is_empty() {
                    return Err(
                        "No source-backed item templates are available in this world".into(),
                    );
                }
                let stack = self
                    .registry
                    .create_stack(&self.items[self.item], f64::from(AMOUNTS[self.amount]))?;
                if key == KeyCode::KeyS {
                    self.containers.set(
                        self.focus,
                        self.cursors[self.focus] as f64,
                        Some(&stack),
                    )?;
                    self.notice = "Assigned a copy of the chosen stack.".into();
                } else {
                    let remaining = self.containers.add(self.focus, &stack)?;
                    self.notice = format!(
                        "Insertion remainder: {}",
                        remaining.map_or(0, |s| s.amount())
                    );
                }
            }
            KeyCode::KeyF => {
                let remaining = self.containers.transfer_item(
                    self.focus,
                    self.cursors[self.focus] as f64,
                    self.destination,
                )?;
                self.notice = format!(
                    "Transfer remainder: {}",
                    remaining.map_or(0, |s| s.amount())
                );
            }
            KeyCode::KeyM => {
                self.containers.move_item(
                    self.focus,
                    self.cursors[self.focus] as f64,
                    self.destination,
                    self.cursors[self.destination] as f64,
                )?;
                self.notice = "Moved stack; destination slot was overwritten.".into();
            }
            KeyCode::KeyX => {
                self.containers.swap_items(
                    self.focus,
                    self.cursors[self.focus] as f64,
                    self.destination,
                    self.cursors[self.destination] as f64,
                )?;
                self.notice = "Swapped the two selected slots.".into();
            }
            KeyCode::Delete => {
                self.containers
                    .set(self.focus, self.cursors[self.focus] as f64, None)?
            }
            KeyCode::KeyC => self.containers.clear(self.focus)?,
            KeyCode::KeyR => {
                self.reset()?;
                self.notice = "Reset the study containers.".into();
            }
            KeyCode::F5 => {
                match self
                    .storage
                    .save_unique(&self.project_root, &self.containers, &self.registry)
                {
                    Ok(path) => {
                        self.initial = self.containers.clone();
                        self.notice = format!(
                            "Saved container world copy: {}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        );
                    }
                    Err(error) => {
                        self.notice = format!("Save: {error}");
                        return Err(error);
                    }
                }
            }
            KeyCode::F9 => {
                if let Err(error) = self.reload_saved() {
                    self.notice = format!("Reload: {error}");
                    return Err(error);
                }
                self.notice =
                    "Reloaded latest saved container copy; unsaved changes discarded.".into();
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    pub fn lines(&self) -> Result<Vec<String>> {
        let mut lines=vec!["INVENTORY STUDY / SAVED BEDROCK CONTAINERS".into(),
            "I or Esc: close / Tab: focus container / Arrows: select slot / 1, 2, 3: destination".into(),
            "F: transfer / M: move and overwrite destination / X: swap selected slots".into(),
            "J: choose item / K: choose amount / A: insert / S: set slot / Delete: empty slot / C: clear / R: reset".into(),
            format!("Chosen stack: {} x{} | destination: {} slot {}",self.items.get(self.item).map_or("none",String::as_str),AMOUNTS[self.amount],self.names[self.destination],self.cursors[self.destination])];
        lines.push(format!(
            "Source: {} | {} unsaved slots | F5: save world copy / F9: reload latest copy",
            self.storage
                .source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .chars()
                .take(40)
                .collect::<String>(),
            self.pending_slots()?
        ));
        for (c, name) in self.names.iter().enumerate() {
            let container = self.containers.container(c)?;
            lines.push(format!(
                "{} {} [{}] | {}/{} empty",
                if c == self.focus { ">" } else { " " },
                name,
                c + 1,
                container.empty_slots_count(),
                container.size()
            ));
            for start in (0..container.size()).step_by(9) {
                let mut line = String::new();
                for slot in start..(start + 9).min(container.size()) {
                    let stack = self.containers.get(c, slot as f64)?;
                    let label = stack.map_or_else(
                        || "--".to_owned(),
                        |s| {
                            format!(
                                "{}:{}",
                                s.definition()
                                    .type_id
                                    .strip_prefix("minecraft:")
                                    .unwrap_or(&s.definition().type_id)
                                    .chars()
                                    .take(5)
                                    .collect::<String>(),
                                s.amount()
                            )
                        },
                    );
                    line.push_str(&format!(
                        "{}{:02} {:<9}",
                        if self.cursors[c] == slot { '*' } else { ' ' },
                        slot,
                        label
                    ));
                }
                lines.push(line);
            }
        }
        let stack = self
            .containers
            .get(self.focus, self.cursors[self.focus] as f64)?;
        lines.push(format!(
            "Selected: {}",
            stack.map_or_else(
                || "empty".into(),
                |s| format!(
                    "{} x{} (item maximum {})",
                    s.definition().type_id,
                    s.amount(),
                    s.definition().max_amount
                )
            )
        ));
        lines.push(self.notice.clone());
        lines.push("Development keyboard controls. Player inventory, crafting and native gestures remain under study.".into());
        Ok(lines)
    }
    pub fn verify(root: &Path, out: &Path) -> Result<()> {
        if out.exists() {
            return Err("Fresh inventory client output required".into());
        }
        let mut panel = Self::load(root)?;
        fs::create_dir_all(out)?;
        let original_source = panel.storage.source.clone();
        let original_hashes = world_edit::tree_hashes(&original_source)?;
        let initial = panel.containers.observation();
        panel.key(KeyCode::KeyF)?;
        if panel
            .containers
            .get(0, 0.)?
            .ok_or("Missing transfer remainder")?
            .amount()
            != 15
            || panel
                .containers
                .get(1, 0.)?
                .ok_or("Missing target stack")?
                .amount()
                != 64
        {
            return Err("Inventory keyboard transfer mismatch".into());
        }
        let transferred = panel.containers.observation();
        panel.key(KeyCode::Tab)?;
        panel.key(KeyCode::Digit1)?;
        panel.key(KeyCode::KeyX)?;
        if panel
            .containers
            .get(0, 0.)?
            .ok_or("Missing swapped stack")?
            .amount()
            != 64
            || panel
                .containers
                .get(1, 0.)?
                .ok_or("Missing swapped remainder")?
                .amount()
                != 15
        {
            return Err("Inventory keyboard swap mismatch".into());
        }
        let swapped = panel.containers.observation();
        panel.key(KeyCode::KeyR)?;
        if panel.containers.observation() != initial {
            return Err("Inventory study reset mismatch".into());
        }
        panel.key(KeyCode::KeyJ)?;
        panel.key(KeyCode::KeyK)?;
        panel.key(KeyCode::KeyS)?;
        panel.key(KeyCode::KeyA)?;
        if panel
            .containers
            .get(0, 0.)?
            .ok_or("Missing assigned egg")?
            .definition()
            .type_id
            != "minecraft:egg"
            || panel
                .containers
                .get(0, 2.)?
                .ok_or("Missing inserted egg")?
                .amount()
                != 16
        {
            return Err("Inventory set/insert key mismatch".into());
        }
        panel.key(KeyCode::KeyM)?;
        if panel.containers.get(0, 0.)?.is_some()
            || panel
                .containers
                .get(1, 0.)?
                .ok_or("Missing moved egg")?
                .definition()
                .type_id
                != "minecraft:egg"
        {
            return Err("Inventory move key mismatch".into());
        }
        panel.key(KeyCode::KeyM)?;
        if panel.containers.get(1, 0.)?.is_some() {
            return Err("Empty source move must clear target".into());
        }
        panel.key(KeyCode::ArrowRight)?;
        panel.key(KeyCode::Delete)?;
        if panel.containers.get(0, 1.)?.is_some() {
            return Err("Inventory delete key mismatch".into());
        }
        panel.key(KeyCode::KeyC)?;
        if panel.containers.container(0)?.empty_slots_count() != 27 {
            return Err("Inventory clear key mismatch".into());
        }
        panel.key(KeyCode::KeyR)?;
        for _ in 0..3 {
            for _ in 0..32 {
                panel.key(KeyCode::ArrowRight)?;
            }
            panel.key(KeyCode::Tab)?;
        }
        panel.key(KeyCode::KeyR)?;
        panel.key(KeyCode::KeyF)?;
        panel.key(KeyCode::Tab)?;
        panel.key(KeyCode::Digit3)?;
        panel.key(KeyCode::KeyF)?;
        let expected_saved = panel.containers.observation();
        let first = panel.save_to(&out.join("saved-world"))?;
        if panel.pending_slots()? != 0 {
            return Err("Successful inventory save did not clear pending state".into());
        }
        let first_hashes = world_edit::tree_hashes(&out.join("saved-world"))?;
        if panel.save_to(&out.join("saved-world")).is_ok() {
            return Err("Existing inventory save must be rejected".into());
        }
        panel.key(KeyCode::Tab)?;
        panel.key(KeyCode::Tab)?;
        panel.key(KeyCode::KeyJ)?;
        panel.key(KeyCode::KeyK)?;
        panel.key(KeyCode::KeyS)?;
        if panel.containers.observation() == expected_saved {
            return Err("Inventory unsaved-change probe made no change".into());
        }
        panel.key(KeyCode::F9)?;
        if panel.containers.observation() != expected_saved || panel.pending_slots()? != 0 {
            return Err("Saved inventory reload failed to discard unsaved changes".into());
        }
        panel.key(KeyCode::ArrowRight)?;
        panel.key(KeyCode::Delete)?;
        let second = panel.save_to(&out.join("second-world"))?;
        if world_edit::tree_hashes(&out.join("saved-world"))? != first_hashes {
            return Err("Later inventory save changed earlier saved copy".into());
        }
        let changed_probe = panel.storage.source.join("source-change-probe.txt");
        fs::write(&changed_probe, b"changed snapshot probe")?;
        let rejected = panel.save_to(&out.join("must-not-write")).is_err();
        fs::remove_file(&changed_probe)?;
        if !rejected || out.join("must-not-write").exists() {
            return Err("Changed inventory source must be rejected before writing".into());
        }
        let second_hashes = world_edit::tree_hashes(&out.join("second-world"))?;
        panel.key(KeyCode::KeyS)?;
        let project_root = panel.project_root.clone();
        panel.project_root = out.join("key-save-root");
        let key_save_result = panel.key(KeyCode::F5);
        panel.project_root = project_root;
        key_save_result?;
        let key_save = panel
            .storage
            .last_save
            .clone()
            .ok_or("F5 did not create a container save")?;
        let key_saved_contents = panel.containers.observation();
        panel.key(KeyCode::Delete)?;
        panel.key(KeyCode::F9)?;
        if panel.containers.observation() != key_saved_contents || panel.pending_slots()? != 0 {
            return Err("F5/F9 keyboard container lifecycle mismatch".into());
        }
        if world_edit::tree_hashes(&out.join("second-world"))? != second_hashes {
            return Err("F5 changed the preceding saved copy".into());
        }
        if world_edit::tree_hashes(&original_source)? != original_hashes {
            return Err("Original inventory source changed".into());
        }
        panel.key(KeyCode::KeyR)?;
        let lines = Self::load(root)?.lines()?;
        if lines.len() > 25 || lines.iter().any(|line| line.len() > 132) {
            return Err("Inventory panel exceeds default window text layout".into());
        }
        fs::create_dir_all(out)?;
        let key_save_hashes = world_edit::tree_hashes(&key_save)?;
        let report: Value = json!({"complete":true,"registry_sha256":panel.registry.source_id(),"capacities":[27,5,9],"initial":initial,"transferred":transferred,"swapped":swapped,"reset_matches":true,"cursor_bounds":true,"lines":lines,"available_items":panel.items,"first_save":first,"second_save":second,"key_save":key_save,"key_save_hashes":key_save_hashes,"keyboard_save_reload":true,"reload_discards_unsaved":true,"existing_save_rejected":true,"changed_source_rejected":true,"first_save_unchanged":true,"source_unchanged":true,"scope":"Saved-world container editing through development keyboard controls and copied-world persistence. Native GUI gestures, player inventories, drops and crafting remain open."});
        fs::write(
            out.join("inventory.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        Ok(())
    }
}
