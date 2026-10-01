use crate::{
    render,
    scene::{Client, Input, Result, View},
};
use mcbe_core::{
    cubes::Cube,
    motion::{Actor, Body, Vec3},
    world_edit,
};
use serde_json::{Value, json};
use std::{collections::HashMap, fs, path::Path};

fn check_resident(client: &Client, fixture: &Value) -> Result<Value> {
    let mut expected = HashMap::new();
    let bounds = client.scene.bounds;
    let mut columns = 0;
    for row in fixture["columns"]
        .as_array()
        .ok_or("Missing independent column rows")?
    {
        let cx = row["position"][0]
            .as_i64()
            .ok_or("Invalid fixture column X")? as i32;
        let cz = row["position"][1]
            .as_i64()
            .ok_or("Invalid fixture column Z")? as i32;
        if !bounds.contains([cx * 16, -64, cz * 16]) {
            continue;
        }
        columns += 1;
        for cell in row["cells"].as_array().ok_or("Missing fixture cells")? {
            let p = [
                cell[0].as_i64().ok_or("Invalid cell X")? as i32,
                cell[1].as_i64().ok_or("Invalid cell Y")? as i32,
                cell[2].as_i64().ok_or("Invalid cell Z")? as i32,
            ];
            let kind: Cube = serde_json::from_value(cell[3].clone())?;
            expected.insert(p, kind);
        }
    }
    if expected != client.scene.original_kinds {
        return Err("Resident columns differ from independent saved-state fixture".into());
    }
    let residency = client.scene.residency().ok_or("Streaming cache required")?;
    if residency.ready != columns
        || residency.requested != (usize::from(residency.radius) * 2 + 1).pow(2)
        || !residency.rejected.is_empty()
    {
        return Err("Unexpected residency coverage".into());
    }
    Ok(json!({"residency":residency,"bounds":bounds,"base_solid_cells":expected.len()}))
}
fn move_to(client: &mut Client, p: Vec3) -> Result<()> {
    client.actor = Actor::new(p, Body::PROBE)?;
    client.sync_region()?;
    Ok(())
}
pub fn verify(client: &mut Client, root: &Path, database: &Path, out: &Path) -> Result<()> {
    if out.exists() {
        return Err("Use a new streaming verification folder".into());
    }
    if !matches!(client.scene.view(), View::Stream { .. }) {
        return Err("Streaming view required".into());
    }
    fs::create_dir_all(out)?;
    let source = database.parent().ok_or("Missing world directory")?;
    let before = world_edit::tree_hashes(source)?;
    let fixture: Value =
        serde_json::from_slice(&fs::read(root.join("evidence/columns-01/fixture.json"))?)?;
    let mut views = vec![check_resident(client, &fixture)?];
    // The northeast notch has an unavailable column between two known ones.
    move_to(client, Vec3::new(0.7, -60.0, 239.5))?;
    let frontier_marker = [-1, -59, 240];
    if client.scene.kind_at(frontier_marker).is_some() {
        return Err("Unexpected frontier selection fixture".into());
    }
    client.scene.change(frontier_marker, Some(Cube::Stone))?;
    if client
        .scene
        .target([0.7, -58.5, 239.5], [-1.0, 0.0, 1.0])?
        .is_some()
    {
        return Err("Selection crossed unavailable saved data".into());
    }
    client.scene.change(frontier_marker, None)?;
    if client.pending_edits() != 0 {
        return Err("Frontier fixture did not restore its original air cell".into());
    }
    move_to(client, Vec3::new(-32.5, -60.0, -8.5))?;
    client.yaw = std::f32::consts::FRAC_PI_2;
    client.pitch = 0.2;
    let old = client
        .scene
        .residency()
        .ok_or("Missing starting cache")?
        .center;
    for _ in 0..12 {
        client.tick(Input {
            forward: 1.0,
            ..Default::default()
        })?;
    }
    let crossed = client
        .scene
        .residency()
        .ok_or("Missing crossing cache")?
        .center;
    let walk_position = client.actor.position;
    if old != [-3, -1]
        || crossed != [-2, -1]
        || client.actor.position.x <= -32.0
        || (client.actor.position.y + 60.0).abs() > 0.00002
    {
        return Err("Movement did not cross the negative column boundary on saved grass".into());
    }
    views.push(check_resident(client, &fixture)?);
    render::png(
        &out.join("walk.png"),
        &render::render(client, 960, 540),
        960,
        540,
    )?;
    let changes = [
        ([-20, -40, -8], Some(Cube::Stone)),
        ([-20, -61, -8], None),
        ([-20, -62, -8], Some(Cube::Stone)),
    ];
    if client.scene.kind_at(changes[0].0).is_some()
        || client.scene.kind_at(changes[1].0) != Some(Cube::Grass)
        || client.scene.kind_at(changes[2].0) != Some(Cube::Dirt)
    {
        return Err("Unexpected streaming edit fixture".into());
    }
    for (p, kind) in changes {
        client.scene.change(p, kind)?;
    }
    let far = Vec3::new(8.5, -60.0, 136.5);
    move_to(client, far)?;
    views.push(check_resident(client, &fixture)?);
    if changes.iter().any(|(p, _)| client.scene.inside(*p)) || client.pending_edits() != 3 {
        return Err("Eviction lost dirty edits or retained the old columns".into());
    }
    render::png(
        &out.join("far.png"),
        &render::render(client, 960, 540),
        960,
        540,
    )?;
    move_to(client, Vec3::new(-24.5, -60.0, -10.5))?;
    views.push(check_resident(client, &fixture)?);
    if changes
        .iter()
        .any(|(p, kind)| client.scene.kind_at(*p) != *kind)
    {
        return Err("Returning to evicted columns did not restore edits".into());
    }
    move_to(client, far)?;
    let saved = client.save_to(root, &out.join("saved-world"))?;
    if saved["changed_cells"] != 3 || saved["unloaded_edits"] != 3 || client.pending_edits() != 0 {
        return Err("Save did not include unloaded edits".into());
    }
    let saved_hashes = world_edit::tree_hashes(&out.join("saved-world"))?;
    let unsaved = [1, -61, 129];
    if client.scene.kind_at(unsaved) != Some(Cube::Grass) {
        return Err("Unexpected unsaved reload fixture".into());
    }
    client.scene.change(unsaved, None)?;
    move_to(client, Vec3::new(-24.5, -60.0, -10.5))?;
    if client.pending_edits() != 1 || client.scene.inside(unsaved) {
        return Err("Unsaved unloaded change did not survive eviction".into());
    }
    client.reload_saved()?;
    if client.pending_edits() != 0 {
        return Err("Reload did not discard unloaded unsaved edits".into());
    }
    if changes
        .iter()
        .any(|(p, kind)| client.scene.kind_at(*p) != *kind)
    {
        return Err("Reload did not reproduce saved terrain changes".into());
    }
    let column = mcbe_core::columns::read(
        &client.scene.snapshot,
        [0, 8],
        client
            .scene
            .palette
            .as_deref()
            .ok_or("Saved-world palette missing")?,
    )?
    .ok_or("Saved far column absent")?;
    if column.cells.get(&unsaved) != Some(&Cube::Grass) {
        return Err("Discarded unloaded edit changed saved data".into());
    }
    if saved_hashes != world_edit::tree_hashes(&out.join("saved-world"))?
        || before != world_edit::tree_hashes(source)?
    {
        return Err("Streaming checks changed source or saved files".into());
    }
    render::png(
        &out.join("saved.png"),
        &render::render(client, 960, 540),
        960,
        540,
    )?;
    let report = json!({"complete":true,"views":views,"unavailable_selection_stops":true,"boundary_crossing":true,"crossed_from":old,"crossed_to":crossed,"walk_position":walk_position,"eviction_preserves_edits":true,"reentry_matches":true,"saved_unloaded_edits":3,"reload_discards_unloaded_edits":true,"source_unchanged":true,"saved_unchanged":true,"saved":saved,"final_residency":client.scene.residency(),"final_bounds":client.scene.bounds,"scope":"Movable saved-column cache, development movement adapter and typed copied-world edits; no native streaming timing, generation or player-input parity"});
    fs::write(
        out.join("streaming.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
