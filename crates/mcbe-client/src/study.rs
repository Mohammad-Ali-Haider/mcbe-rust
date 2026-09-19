//! Integration exercise for the independently measured terrain fixture.
use crate::{
    render,
    scene::{Client, Result},
};
use mcbe_core::{
    cubes::Cube,
    motion::{Actor, Body, Vec3},
    world_edit,
};
use serde_json::{Value, json};
use std::{fs, path::Path};

pub fn verify(client: &mut Client, root: &Path, out: &Path) -> Result<()> {
    if out.exists() {
        return Err("Fresh terrain-study output required".into());
    }
    fs::create_dir_all(out)?;
    let original_source = client.scene.source.clone();
    let original_hashes = client.scene.source_hashes.clone();
    let cases: Value = serde_json::from_slice(&fs::read(
        root.join("evidence/terrain-collision-01/preparation/cases.json"),
    )?)?;
    let materials: Value = serde_json::from_slice(&fs::read(
        root.join("evidence/terrain-collision-01/preparation/materials.json"),
    )?)?;
    let rows = materials.as_array().ok_or("Missing terrain materials")?;
    let mut atlas = vec![0u32; 1440 * 270];
    let mut observations = vec![];
    let mut kinds = vec![];
    for (index, row) in rows.iter().enumerate() {
        let kind: Cube = serde_json::from_value(row["kind"].clone())?;
        if !client.available_materials.contains(&kind) {
            return Err("Terrain source lacks a material prototype".into());
        }
        kinds.push(kind);
        let case = &cases[index * 8];
        let x = case["x"].as_i64().ok_or("Case X")? as i32;
        let z = case["z"].as_i64().ok_or("Case Z")? as i32;
        client.actor = Actor::new(Vec3::new(x as f32 + 0.5, 65., z as f32 + 0.5), Body::PROBE)?;
        client.yaw = 0.72;
        client.pitch = -0.38;
        client.sync_region()?;
        if client.scene.kind_at([x - 4, 64, z - 4]) != Some(kind) {
            return Err("Saved terrain material differs from case".into());
        }
        if client.material_pack.is_some()
            && (0..6).any(|face| !client.cube_materials.contains_key(&(kind, face)))
        {
            return Err("Resident terrain material has no loaded texture".into());
        }
        let pixels = render::render(client, 160, 90);
        for y in 0..90 {
            let offset = (index / 9 * 90 + y) * 1440 + index % 9 * 160;
            atlas[offset..offset + 160].copy_from_slice(&pixels[y * 160..(y + 1) * 160]);
        }
        observations
            .push(json!({"kind":kind,"position":[x,64,z],"residency":client.scene.residency()}));
    }
    render::png(&out.join("materials.png"), &atlas, 1440, 270)?;
    client.reset();
    client.sync_region()?;
    client.yaw = 0.72;
    client.pitch = -0.38;
    render::png(
        &out.join("overview.png"),
        &render::render(client, render::WIDTH, render::HEIGHT),
        render::WIDTH,
        render::HEIGHT,
    )?;
    let initial = client.placement;
    let mut cycled = std::collections::BTreeSet::new();
    for _ in 0..client.available_materials.len() {
        cycled.insert(client.placement);
        client.cycle_material();
    }
    if cycled.len() != client.available_materials.len() || client.placement != initial {
        return Err("Study palette cycle differs from available prototypes".into());
    }
    for (index, kind) in kinds.iter().enumerate() {
        let p = [-120, 80 + index as i32, -120];
        if client.scene.kind_at(p).is_some() {
            return Err("Study edit target must start empty".into());
        }
        client.scene.change(p, Some(*kind))?;
    }
    client.scene.change([-120, 64, -120], None)?;
    let save = client.save_to(root, &out.join("saved-world"))?;
    client.reload_saved()?;
    for (index, kind) in kinds.iter().enumerate() {
        if client.scene.kind_at([-120, 80 + index as i32, -120]) != Some(*kind) {
            return Err("Terrain save failed to preserve an exact material".into());
        }
    }
    if client.scene.kind_at([-120, 64, -120]).is_some() || client.pending_edits() != 0 {
        return Err("Terrain removal/reload mismatch".into());
    }
    client.sync_region()?;
    let mut faces: Vec<_> = client.cube_materials.iter().collect();
    faces.sort_by_key(|(key, _)| **key);
    fs::write(
        out.join("cube-materials.json"),
        serde_json::to_vec_pretty(
            &faces
                .iter()
                .map(|((kind, face), m)| json!({"kind":kind,"face":face,"material":m.provenance}))
                .collect::<Vec<_>>(),
        )?,
    )?;
    let unchanged = world_edit::tree_hashes(&original_source)? == original_hashes
        && world_edit::tree_hashes(&client.scene.source)? == client.scene.source_hashes;
    if !unchanged {
        return Err("Terrain study source changed".into());
    }
    fs::write(
        out.join("study.json"),
        serde_json::to_vec_pretty(
            &json!({"complete":true,"materials":rows.len(),"views":observations,"available_materials":client.available_materials,"palette_cycle":true,"source_unchanged":unchanged,"save":save,"saved_materials":kinds,"scope":"Development rendering, source-backed placement palette and copied-world round trip. Native input, lighting, UVs and redstone activation remain unimplemented."}),
        )?,
    )?;
    Ok(())
}
