use crate::scene::Result;
use mcbe_core::{
    blocks::Palette,
    container_storage::Store,
    inventory::Containers,
    items::Registry,
    leveldb::{self, Snapshot},
    world_edit,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Storage {
    pub source: PathBuf,
    hashes: BTreeMap<String, String>,
    snapshot: Snapshot,
    store: Store,
    positions: Vec<[i32; 3]>,
    palette: Palette,
    pub last_save: Option<PathBuf>,
}
impl Storage {
    pub fn item_ids(&self) -> Vec<String> {
        self.store.template_hashes().keys().cloned().collect()
    }
    pub fn load_selected(
        root: &Path,
        registry: &Registry,
        database: &Path,
        selected: Option<&[[i32; 3]]>,
    ) -> Result<(Self, Containers)> {
        let database = database.canonicalize()?;
        if database.file_name().is_none_or(|name| name != "db") {
            return Err("Container study expects a stopped world/db folder".into());
        }
        let source = database
            .parent()
            .ok_or("Container database lacks world parent")?
            .to_path_buf();
        let hashes = world_edit::tree_hashes(&source)?;
        let snapshot = leveldb::read_snapshot(&database)?;
        let full: Value = serde_json::from_slice(&fs::read(
            root.join("evidence/bds-1.26.52.3/blocks-05/registry.json"),
        )?)?;
        let partial: Value = serde_json::from_slice(&fs::read(
            root.join("evidence/bds-1.26.52.3/block-partial-02/registry.json"),
        )?)?;
        let palette = Palette::from_reports(&full, &partial)?;
        if registry.metadata()["executable_sha256"] != palette.reference() {
            return Err("Container and block registry references differ".into());
        }
        let store = Store::read(&snapshot, registry)?;
        let positions = match selected {
            Some(positions) => positions.to_vec(),
            None => store.primary_positions(registry)?,
        };
        store.check_blocks(&snapshot, &positions, &palette)?;
        let contents = store.load_group(&positions, registry)?;
        if world_edit::tree_hashes(&source)? != hashes {
            return Err("Container source changed during loading".into());
        }
        Ok((
            Self {
                source,
                hashes,
                snapshot,
                store,
                positions,
                palette,
                last_save: None,
            },
            contents,
        ))
    }
    pub fn positions(&self) -> &[[i32; 3]] {
        &self.positions
    }
    pub fn names(&self, registry: &Registry) -> Result<Vec<String>> {
        self.positions
            .iter()
            .map(|p| {
                Ok(self.store.observation(*p, registry)?["entity_id"]
                    .as_str()
                    .ok_or("Container label missing")?
                    .to_owned())
            })
            .collect()
    }
    pub fn prepare_updates(
        &self,
        contents: &Containers,
        registry: &Registry,
    ) -> Result<world_edit::Updates> {
        if world_edit::tree_hashes(&self.source)? != self.hashes {
            return Err("Container source world changed".into());
        }
        self.store
            .check_blocks(&self.snapshot, &self.positions, &self.palette)?;
        Ok(self
            .store
            .prepare(&self.positions, contents, registry)?
            .updates)
    }
    pub fn adopt(
        &mut self,
        root: &Path,
        registry: &Registry,
        destination: &Path,
        contents: &Containers,
    ) -> Result<()> {
        let (mut storage, saved) = Self::load_selected(
            root,
            registry,
            &destination.join("db"),
            Some(&self.positions),
        )?;
        if saved.observation() != contents.observation() {
            return Err("Combined save changed selected container contents".into());
        }
        let old = self.store.record_hashes()?;
        let new = storage.store.record_hashes()?;
        if old.keys().ne(new.keys())
            || old
                .iter()
                .any(|(p, h)| !self.positions.contains(p) && new.get(p) != Some(h))
        {
            return Err("Combined save changed unrelated block entity records".into());
        }
        storage.last_save = Some(storage.source.clone());
        *self = storage;
        Ok(())
    }
    pub fn save_to(
        &mut self,
        contents: &Containers,
        registry: &Registry,
        destination: &Path,
    ) -> Result<Value> {
        if destination.exists() {
            return Err("Container save destination already exists".into());
        }
        if world_edit::tree_hashes(&self.source)? != self.hashes {
            return Err("Container source world changed; save cancelled".into());
        }
        self.store
            .check_blocks(&self.snapshot, &self.positions, &self.palette)?;
        let prepared = self.store.prepare(&self.positions, contents, registry)?;
        let copied = world_edit::edit_copy(&self.source, destination, &prepared.updates)?;
        let snapshot = leveldb::read_snapshot(&destination.join("db"))?;
        let store = Store::read(&snapshot, registry)?;
        store.check_blocks(&snapshot, &self.positions, &self.palette)?;
        if store.load_group(&self.positions, registry)?.observation() != contents.observation() {
            return Err("Saved container contents do not round trip".into());
        }
        let changed: BTreeSet<_> = prepared.updates.iter().map(|(key, _)| key).collect();
        let old_values: BTreeMap<_, _> = self
            .snapshot
            .entries
            .iter()
            .filter_map(|(k, e)| e.value.as_ref().map(|v| (k, v)))
            .collect();
        let new_values: BTreeMap<_, _> = snapshot
            .entries
            .iter()
            .filter_map(|(k, e)| e.value.as_ref().map(|v| (k, v)))
            .collect();
        if old_values.keys().ne(new_values.keys())
            || old_values
                .iter()
                .any(|(k, v)| !changed.contains(k) && new_values.get(k) != Some(v))
        {
            return Err("Inventory save changed unrelated database values".into());
        }
        let old_records = self.store.record_hashes()?;
        let new_records = store.record_hashes()?;
        let mut preserved = 0;
        if old_records.keys().ne(new_records.keys()) {
            return Err("Inventory save changed block entity positions".into());
        }
        for (position, digest) in &old_records {
            if !self.positions.contains(position) {
                if new_records.get(position) != Some(digest) {
                    return Err("Unselected block entity NBT changed".into());
                }
                preserved += 1;
            }
        }
        if world_edit::tree_hashes(&self.source)? != self.hashes {
            return Err("Container source changed during save".into());
        }
        let report = json!({"complete":true,"changed_containers":prepared.changed_containers,"changed_slots":prepared.changed_slots,"updated_keys":prepared.updates.len(),"unselected_records_preserved":preserved,"source_unchanged":true,"edit":copied});
        let destination = destination.canonicalize()?;
        self.hashes = world_edit::tree_hashes(&destination)?;
        self.source = destination.clone();
        self.snapshot = snapshot;
        self.store = store;
        self.last_save = Some(destination);
        Ok(report)
    }
    pub fn save_unique(
        &mut self,
        root: &Path,
        contents: &Containers,
        registry: &Registry,
    ) -> Result<PathBuf> {
        let base = root.join("evidence/inventory-client-saves");
        fs::create_dir_all(&base)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        for index in 0..100 {
            let destination = base.join(format!("{stamp}-{index}"));
            if !destination.exists() {
                self.save_to(contents, registry, &destination)?;
                return Ok(destination);
            }
        }
        Err("Could not allocate a fresh container save folder".into())
    }
    pub fn reload(&mut self, root: &Path, registry: &Registry) -> Result<Containers> {
        let latest = self
            .last_save
            .clone()
            .ok_or("No container copy saved in this session")?;
        let (mut storage, contents) =
            Self::load_selected(root, registry, &latest.join("db"), Some(&self.positions))?;
        storage.last_save = Some(latest);
        *self = storage;
        Ok(contents)
    }
}
