use crate::Result;
use mcbe_core::{
    resources::Pack,
    textures::{self, Texture},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};
pub fn inspect(args: &[String], texture: bool) -> Result<bool> {
    if args.len() != 3 {
        return Err("resource|texture PACK LOGICAL_PATH".into());
    }
    let mut pack = Pack::open(Path::new(&args[1]))?;
    let resource = pack.read(&args[2])?;
    let mut report = json!({"pack":pack.root(),"logical_path":args[2],"bytes":resource.bytes.len(),"sha256":format!("{:x}",Sha256::digest(&resource.bytes)),"origins":resource.origins});
    if texture {
        let extension = args[2].rsplit('.').next().ok_or("Missing extension")?;
        let decoded = Texture::decode(&resource.bytes, extension)?;
        report["image"] = json!({"width":decoded.width,"height":decoded.height,"rgba_sha256":decoded.rgba_sha256()});
    }
    pack.verify_unchanged()?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(true)
}
pub fn material(args: &[String]) -> Result<bool> {
    if args.len() != 4 {
        return Err("material PACK ALIAS VARIANT".into());
    }
    let mut pack = Pack::open(Path::new(&args[1]))?;
    let material = textures::terrain_material(&mut pack, &args[2], args[3].parse()?)?;
    println!("{}", serde_json::to_string_pretty(&material.provenance)?);
    Ok(true)
}
pub fn block_material(args: &[String]) -> Result<bool> {
    if args.len() != 5 {
        return Err("block-material PACK BLOCK FACE VARIANT".into());
    }
    let mut pack = Pack::open(Path::new(&args[1]))?;
    let material = textures::block_material(&mut pack, &args[2], &args[3], args[4].parse()?)?;
    println!("{}", serde_json::to_string_pretty(&material.provenance)?);
    Ok(true)
}
pub fn block_material_split(args: &[String]) -> Result<bool> {
    if args.len() != 6 {
        return Err("block-material-split DEFINITIONS_PACK ASSETS_PACK BLOCK FACE VARIANT".into());
    }
    let mut definitions = Pack::open(Path::new(&args[1]))?;
    let mut assets = Pack::open(Path::new(&args[2]))?;
    let material = textures::block_material_split(
        &mut definitions,
        &mut assets,
        &args[3],
        &args[4],
        args[5].parse()?,
    )?;
    println!("{}", serde_json::to_string_pretty(&material.provenance)?);
    Ok(true)
}
pub fn verify(args: &[String]) -> Result<bool> {
    if args.len() != 2 {
        return Err("verify-textures FIXTURE".into());
    }
    let bytes = fs::read(&args[1])?;
    let fixture: Value = serde_json::from_slice(&bytes)?;
    if fixture["complete"] != true {
        return Err("Complete independent fixture required".into());
    }
    let mut pack = Pack::open(Path::new(
        fixture["pack"].as_str().ok_or("Missing fixture pack")?,
    ))?;
    let rows = fixture["rows"].as_array().ok_or("Missing image rows")?;
    if rows.is_empty() {
        return Err("Empty texture fixture".into());
    }
    let mut formats: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures = vec![];
    let mut translucent = 0usize;
    for row in rows {
        let logical = row["logical_path"].as_str().ok_or("Missing logical path")?;
        let resource = pack.read(logical)?;
        let extension = logical
            .rsplit('.')
            .next()
            .ok_or("Missing texture extension")?;
        let texture = Texture::decode(&resource.bytes, extension)?;
        *formats.entry(extension.into()).or_default() += 1;
        translucent += usize::from(texture.rgba.chunks_exact(4).any(|p| p[3] != 255));
        let origin = resource
            .origins
            .iter()
            .find(|v| v.storage == "archive")
            .ok_or("Expected fixture archive origin")?;
        let actual = json!({"width":texture.width,"height":texture.height,"rgba_sha256":texture.rgba_sha256(),"payload_sha256":origin.payload_sha256,"archive_sha256":origin.file_sha256,"payload_offset":origin.payload_offset,"descriptor_offset":origin.descriptor_offset});
        let mut expected = json!({});
        for key in actual
            .as_object()
            .ok_or("Image observation must be object")?
            .keys()
        {
            expected[key] = row[key].clone();
        }
        if actual != expected {
            failures.push(json!({"path":logical,"actual":actual,"expected":expected}));
        }
    }
    pack.verify_unchanged()?;
    let complete = failures.is_empty();
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"complete":complete,"textures":rows.len(),"formats":formats,"textures_with_alpha":translucent,"failures":failures,"source_hashes":pack.source_hashes(),"fixture_sha256":format!("{:x}",Sha256::digest(&bytes)),"scope":"Independent archive-byte and Pillow RGBA comparison; not native texture selection or visual parity"})
        )?
    );
    Ok(complete)
}
