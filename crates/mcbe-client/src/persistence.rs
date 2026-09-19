use crate::scene::{Client, Result, Scene};
use mcbe_core::{
    block_edits,
    blocks::Palette,
    cubes::Cube,
    heightmaps::{self, ColumnData, HeightRules},
    world_edit,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub struct SaveRules {
    palette: Palette,
    heights: HeightRules,
    provenance: Value,
}
fn read(path: &Path) -> Result<(Value, String)> {
    let bytes = fs::read(path)?;
    Ok((
        serde_json::from_slice(&bytes)?,
        format!("{:x}", Sha256::digest(&bytes)),
    ))
}
impl SaveRules {
    fn load(root: &Path) -> Result<Self> {
        let (palette, full_hash) =
            read(&root.join("evidence/bds-1.26.52.3/blocks-05/registry.json"))?;
        let (partial, partial_hash) =
            read(&root.join("evidence/bds-1.26.52.3/block-partial-02/registry.json"))?;
        let (heights, height_hash) =
            read(&root.join("evidence/container-state-heights-01/rules.json"))?;
        if heights["complete"] != true
            || heights["palette_sha256"] != full_hash
            || heights["partial_palette_sha256"] != partial_hash
        {
            return Err("Height rules do not match the complete measured palettes".into());
        }
        Ok(Self {
            palette: Palette::from_reports(&palette, &partial)?,
            heights: HeightRules::read(&heights["rows"])?,
            provenance: json!({"palette_sha256":full_hash,"partial_sha256":partial_hash,"height_rules_sha256":height_hash}),
        })
    }
}
impl Client {
    pub fn pending_edits(&self) -> usize {
        if let Some(state) = &self.scene.engine {
            return state.pending();
        }
        self.scene.pending_changes().len()
    }
    pub fn save_to(&mut self, root: &Path, destination: &Path) -> Result<Value> {
        self.save_with_updates(root, destination, &[])
    }
    pub fn save_with_updates(
        &mut self,
        root: &Path,
        destination: &Path,
        additional: &[(Vec<u8>, Option<Vec<u8>>)],
    ) -> Result<Value> {
        self.save_checked(root, destination, additional, |_| Ok(()))
    }
    fn save_checked(
        &mut self,
        root: &Path,
        destination: &Path,
        additional: &[(Vec<u8>, Option<Vec<u8>>)],
        validate: impl FnOnce(&Scene) -> Result<()>,
    ) -> Result<Value> {
        if destination.exists() {
            return Err("Save destination already exists".into());
        }
        if self.pending_edits() == 0 && additional.is_empty() {
            return Err("No block changes to save".into());
        }
        if world_edit::tree_hashes(&self.scene.source)? != self.scene.source_hashes {
            return Err("Loaded source world changed; save cancelled".into());
        }
        if self.save_rules.is_none() {
            self.save_rules = Some(SaveRules::load(root)?);
        }
        let rules = self.save_rules.as_mut().ok_or("Save rules missing")?;
        let changes: Vec<_> = self.scene.pending_changes().into_iter().collect();
        if matches!(self.scene.view(), crate::scene::View::Fixed)
            && changes.iter().any(|(p, _)| !self.scene.inside(*p))
        {
            return Err("Edit lies outside the loaded scene".into());
        }
        let prepared = if changes.is_empty() {
            block_edits::Prepared {
                updates: Vec::new(),
                changed_cells: 0,
                repaired_height_entries: 0,
                columns: Vec::new(),
            }
        } else {
            block_edits::prepare_cube_edits(
                &self.scene.snapshot,
                &changes,
                &mut rules.heights,
                &rules.palette,
            )?
        };
        let mut updates = prepared.updates.clone();
        updates.extend_from_slice(additional);
        if updates
            .iter()
            .map(|u| &u.0)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != updates.len()
        {
            return Err("Combined save has conflicting database updates".into());
        }
        let edited = world_edit::edit_copy(&self.scene.source, destination, &updates)?;
        let reloaded = self.scene.reload_like(&destination.join("db"))?;
        let changed_columns: std::collections::BTreeSet<_> = changes
            .iter()
            .map(|(p, _)| [p[0].div_euclid(16), p[2].div_euclid(16)])
            .collect();
        for column in changed_columns {
            let saved =
                mcbe_core::columns::read_geometry(&reloaded.snapshot, column, &rules.palette)?
                    .ok_or("Edited saved column absent")?;
            for (p, expected) in &changes {
                if [p[0].div_euclid(16), p[2].div_euclid(16)] == column
                    && saved.cells.get(p).copied() != *expected
                {
                    return Err("Saved unloaded edit does not reproduce requested cube".into());
                }
            }
        }
        if reloaded.containers != self.scene.containers
            || reloaded.cells != self.scene.cells
            || self
                .scene
                .cells
                .iter()
                .any(|p| self.scene.kind_at(*p) != reloaded.kind_at(*p))
        {
            return Err("Saved scene does not reproduce client edits".into());
        }
        let mut height_comparisons = 0;
        for [cx, cz] in &prepared.columns {
            let stored = ColumnData::decode(
                reloaded
                    .snapshot
                    .get(&heightmaps::column_key(*cx, *cz))
                    .ok_or("Saved column absent")?,
            )?;
            let (rebuilt, _) = heightmaps::rebuild_column(
                &reloaded.snapshot,
                *cx,
                *cz,
                &mut rules.heights,
                &rules.palette,
            )?;
            if stored != rebuilt {
                return Err("Saved height/biome data differs from rebuilt column".into());
            }
            height_comparisons += 256;
        }
        if world_edit::tree_hashes(&self.scene.source)? != self.scene.source_hashes {
            return Err("Source world changed during client save".into());
        }
        let unloaded_edits = changes
            .iter()
            .filter(|(p, _)| !self.scene.inside(*p))
            .count();
        let report = json!({"complete":true,"edit":edited,"changed_cells":prepared.changed_cells,"unloaded_edits":unloaded_edits,"repaired_height_entries":prepared.repaired_height_entries,"height_comparisons":height_comparisons,"changes":changes,"rules":rules.provenance,"scope":"Copied-world measured cube edits and height repair, including unloaded edits/new subchunks; lighting, actor state, neighbors and full lifecycle remain open"});
        let destination = destination.canonicalize()?;
        validate(&reloaded)?;
        self.scene = reloaded;
        self.last_save = Some(destination);
        Ok(report)
    }
    pub fn save_unique(&mut self, root: &Path) -> Result<PathBuf> {
        self.save_unique_with_updates(root, &[])
    }
    pub fn save_unique_with_updates(
        &mut self,
        root: &Path,
        additional: &[(Vec<u8>, Option<Vec<u8>>)],
    ) -> Result<PathBuf> {
        self.save_unique_checked(root, additional, |_| Ok(()))
    }
    pub fn save_unique_checked(
        &mut self,
        root: &Path,
        additional: &[(Vec<u8>, Option<Vec<u8>>)],
        validate: impl FnOnce(&Scene) -> Result<()>,
    ) -> Result<PathBuf> {
        if self.pending_edits() == 0 && additional.is_empty() {
            return Err("No block changes to save".into());
        }
        let directory = root.join("saves");
        fs::create_dir_all(&directory)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        let destination = (0..100)
            .map(|i| directory.join(format!("world-{stamp}-{i}")))
            .find(|p| !p.exists())
            .ok_or("Could not select a new save name")?;
        let report = self.save_checked(root, &destination, additional, validate)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination.with_extension("json"))?;
        file.write_all(&serde_json::to_vec_pretty(&report)?)?;
        file.sync_all()?;
        Ok(destination)
    }
    pub fn reload_saved(&mut self) -> Result<()> {
        self.scene = self.prepare_reload()?;
        Ok(())
    }
    pub fn prepare_reload(&self) -> Result<Scene> {
        let path = self
            .last_save
            .as_ref()
            .ok_or("There is no saved copy in this session")?;
        let mut reloaded = self.scene.reload_like(&path.join("db"))?;
        reloaded.recenter(self.actor.position)?;
        let a = self.actor.position;
        let half = self.actor.body.width * 0.5;
        let low = [a.x - half, a.y, a.z - half];
        let high = [a.x + half, a.y + self.actor.body.height, a.z + half];
        if reloaded
            .motion_scene(&self.actor)?
            .boxes
            .iter()
            .any(|b| (0..3).all(|axis| low[axis] < b.max[axis] && high[axis] > b.min[axis]))
        {
            return Err("Reload would place the actor inside saved geometry; move to a clear location first".into());
        }
        Ok(reloaded)
    }
}

pub fn verify(root: &Path, database: &Path, out: &Path) -> Result<Value> {
    let mut client = Client::new(Scene::load_fixed(database)?)?;
    let original_source = client.scene.source.clone();
    let original_hashes = client.scene.source_hashes.clone();
    let raised = [-44, -31, -24];
    let lowered = [-43, -32, -24];
    if client.scene.cells.contains(&raised) || !client.scene.cells.contains(&lowered) {
        return Err("Unexpected source fixture cells".into());
    }
    client.scene.cells.insert(raised);
    client.scene.cells.remove(&lowered);
    let expected = client.scene.cells.clone();
    let destination = out.join("saved-world");
    let saved = client.save_to(root, &destination)?;
    if saved["changed_cells"] != 2
        || saved["repaired_height_entries"] != 2
        || saved["height_comparisons"] != 256
        || client.pending_edits() != 0
    {
        return Err("Two-cell save did not repair the expected height entries".into());
    }
    let before = world_edit::tree_hashes(&destination)?;
    client.scene.cells.remove(&raised);
    if client.save_to(root, &destination).is_ok()
        || world_edit::tree_hashes(&destination)? != before
    {
        return Err("Existing save was overwritten".into());
    }
    client.reload_saved()?;
    if client.scene.cells != expected || client.pending_edits() != 0 {
        return Err("Saved scene reload did not discard unsaved changes".into());
    }
    client.scene.cells.remove(&raised);
    let second = out.join("saved-world-second");
    let second_saved = client.save_to(root, &second)?;
    if second_saved["changed_cells"] != 1 || second_saved["repaired_height_entries"] != 1 {
        return Err("Second save did not preserve its new base".into());
    }
    let new_cell = [-20, -40, -8];
    let grass = [-20, -61, -8];
    let dirt = [-20, -62, -8];
    let missing_key = mcbe_core::chunks::key(-2, -1, -3);
    if client.scene.snapshot.get(&missing_key).is_some()
        || client.scene.kind_at(grass) != Some(Cube::Grass)
        || client.scene.kind_at(dirt) != Some(Cube::Dirt)
    {
        return Err("Unexpected terrain save fixture".into());
    }
    client.scene.cells.insert(new_cell);
    client.scene.kinds.insert(new_cell, Cube::Stone);
    client.scene.cells.remove(&grass);
    client.scene.kinds.remove(&grass);
    client.scene.kinds.insert(dirt, Cube::Stone);
    let terrain_world = out.join("saved-world-terrain");
    let terrain_save = client.save_to(root, &terrain_world)?;
    if terrain_save["changed_cells"] != 3
        || client.scene.snapshot.get(&missing_key).is_none()
        || client.scene.kind_at(new_cell) != Some(Cube::Stone)
        || client.scene.kind_at(grass).is_some()
        || client.scene.kind_at(dirt) != Some(Cube::Stone)
    {
        return Err("Terrain edit or new-subchunk save failed".into());
    }
    let source_unchanged = world_edit::tree_hashes(&original_source)? == original_hashes;
    let first_save_unchanged = world_edit::tree_hashes(&destination)? == before;
    if !source_unchanged || !first_save_unchanged {
        return Err("Client save modified an earlier world".into());
    }
    let report = json!({"complete":true,"first_save":saved,"second_save":second_saved,"terrain_save":terrain_save,"new_subchunk_created":true,"reload_matches":true,"existing_save_rejected":true,"original_source_unchanged":source_unchanged,"first_save_unchanged":first_save_unchanged,"first_world":destination,"second_world":second,"terrain_world":terrain_world,"scope":"Client cube save/reload integration; native acceptance is audited separately"});
    fs::write(
        out.join("persistence.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
