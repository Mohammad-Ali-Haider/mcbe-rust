//! Bounded PNG/TGA decoding and explicit terrain-atlas variant selection.
use crate::{
    Result,
    resources::{Origin, Pack},
};
use image::{ImageFormat, ImageReader, Limits};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::Cursor;
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
impl Texture {
    pub fn decode(bytes: &[u8], extension: &str) -> Result<Self> {
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("Image payload exceeds 16 MiB".into());
        }
        let format = match extension {
            "png" => ImageFormat::Png,
            "tga" => ImageFormat::Tga,
            _ => return Err("Only PNG/TGA textures are implemented".into()),
        };
        let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
        let mut limits = Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        let decoded = reader.decode().map_err(|e| e.to_string())?.to_rgba8();
        let (width, height) = decoded.dimensions();
        if width == 0 || height == 0 {
            return Err("Empty texture dimensions".into());
        }
        Ok(Self {
            width,
            height,
            rgba: decoded.into_raw(),
        })
    }
    pub fn rgba_sha256(&self) -> String {
        format!("{:x}", Sha256::digest(&self.rgba))
    }
    pub fn sample_repeat(&self, u: f64, v: f64) -> [u8; 4] {
        let x = (u.rem_euclid(1.0) * self.width as f64).floor() as usize;
        let y = (v.rem_euclid(1.0) * self.height as f64).floor() as usize;
        let offset = (y * self.width as usize + x) * 4;
        self.rgba[offset..offset + 4]
            .try_into()
            .expect("Validated RGBA dimensions")
    }
}
pub struct Material {
    pub texture: Texture,
    pub provenance: Value,
}
pub fn block_material(
    pack: &mut Pack,
    block: &str,
    face: &str,
    variant: usize,
) -> Result<Material> {
    let (alias, origins) = block_binding(pack, block, face)?;
    let mut material = terrain_material(pack, &alias, variant)?;
    material.provenance["block"] = json!(block);
    material.provenance["face"] = json!(face);
    material.provenance["block_origins"] = json!(origins);
    material.provenance["source_hashes"] = json!(pack.source_hashes());
    Ok(material)
}
/// Explicit definitions and asset packs; this does not resolve active-pack precedence.
pub fn block_material_split(
    definitions: &mut Pack,
    assets: &mut Pack,
    block: &str,
    face: &str,
    variant: usize,
) -> Result<Material> {
    let (alias, origins) = block_binding(definitions, block, face)?;
    let mut material = terrain_material(assets, &alias, variant)?;
    definitions.verify_unchanged().map_err(|e| e.to_string())?;
    material.provenance["block"] = json!(block);
    material.provenance["face"] = json!(face);
    material.provenance["block_origins"] = json!(origins);
    material.provenance["definition_source_hashes"] = json!(definitions.source_hashes());
    material.provenance["binding_scope"] =
        json!("Explicit definitions/asset packs; native active-pack precedence unverified");
    Ok(material)
}
fn block_binding(pack: &mut Pack, block: &str, face: &str) -> Result<(String, Vec<Origin>)> {
    if !["up", "down", "north", "south", "east", "west"].contains(&face) {
        return Err("Unknown block face".into());
    }
    let resource = pack.read("blocks.json").map_err(|e| e.to_string())?;
    let blocks: Value = crate::resources::json(&resource.bytes)?;
    let key = if blocks.get(block).is_some() {
        block
    } else {
        block.strip_prefix("minecraft:").unwrap_or(block)
    };
    let textures = &blocks[key]["textures"];
    let alias = match textures {
        Value::String(value) => value.as_str(),
        Value::Object(values) => values
            .get(face)
            .or_else(|| {
                if face != "up" && face != "down" {
                    values.get("side")
                } else {
                    None
                }
            })
            .or_else(|| values.get("*"))
            .and_then(Value::as_str)
            .ok_or("Missing explicit face/side/default alias")?,
        _ => return Err("Only scalar or face-map block texture aliases are implemented".into()),
    };
    Ok((alias.to_owned(), resource.origins))
}
pub fn terrain_material(pack: &mut Pack, alias: &str, variant: usize) -> Result<Material> {
    let resource = pack
        .read("textures/terrain_texture.json")
        .map_err(|e| e.to_string())?;
    let table: Value = crate::resources::json(&resource.bytes)?;
    if table["texture_name"] != "atlas.terrain" {
        return Err("Expected a terrain texture atlas".into());
    }
    let value = &table["texture_data"][alias]["textures"];
    let selected = match value {
        Value::Array(values) => values
            .get(variant)
            .ok_or("Texture variant outside declared list")?,
        _ if variant == 0 => value,
        _ => return Err("Scalar texture only supports explicit variant zero".into()),
    };
    let (path, deferred) = match selected {
        Value::String(path) => (path.as_str(), Value::Null),
        Value::Object(object)
            if object
                .keys()
                .all(|key| ["path", "overlay_color"].contains(&key.as_str())) =>
        {
            (
                object
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or("Missing texture object path")?,
                selected.clone(),
            )
        }
        _ => return Err("Texture variation object is not supported".into()),
    };
    let mut found: Option<(Texture, Vec<Origin>, String)> = None;
    for extension in ["png", "tga"] {
        let logical = format!("{path}.{extension}");
        match pack.read(&logical) {
            Ok(resource) => {
                let texture = Texture::decode(&resource.bytes, extension)?;
                if found.is_some() {
                    return Err("Both PNG and TGA exist; format precedence is unverified".into());
                }
                found = Some((texture, resource.origins, logical));
            }
            Err(crate::resources::ResourceError::Missing(_)) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    let (texture, origins, logical) = found.ok_or("Texture asset not found")?;
    pack.verify_unchanged().map_err(|e| e.to_string())?;
    let provenance = json!({"pack":pack.root(),"alias":alias,"variant":variant,"logical_path":logical,"width":texture.width,"height":texture.height,"rgba_sha256":texture.rgba_sha256(),"deferred_modifiers":deferred,"atlas_origins":resource.origins,"texture_origins":origins,"source_hashes":pack.source_hashes(),"scope":"Explicit single-pack/variant lookup; tint/biome modifiers are retained but unapplied; native selection/UVs remain open"});
    Ok(Material {
        texture,
        provenance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rgba_alpha_and_negative_uv_wrapping() {
        let texture = Texture {
            width: 2,
            height: 1,
            rgba: vec![1, 2, 3, 0, 7, 8, 9, 255],
        };
        assert_eq!(texture.sample_repeat(0.0, 0.0), [1, 2, 3, 0]);
        assert_eq!(texture.sample_repeat(-0.25, 0.0), [7, 8, 9, 255]);
        assert_eq!(texture.sample_repeat(1.5, 2.0), [7, 8, 9, 255]);
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(2, 1, texture.rgba.clone()).unwrap(),
        )
        .write_to(&mut encoded, ImageFormat::Png)
        .unwrap();
        let decoded = Texture::decode(encoded.get_ref(), "png").unwrap();
        assert_eq!(decoded.rgba, texture.rgba);
    }
    #[test]
    fn corrupt_and_unsupported_images_fail() {
        assert!(Texture::decode(b"not a png", "png").is_err());
        assert!(Texture::decode(&[], "tga").is_err());
        assert!(Texture::decode(&[], "jpg").is_err());
    }
    #[test]
    fn explicit_variants_and_face_aliases() {
        let root = std::env::current_dir().unwrap().join(format!(
            "../../target/test-resources/material-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("textures")).unwrap();
        std::fs::write(
            root.join("blocks.json"),
            serde_json::to_vec(
                &json!({"test":{"textures":{"up":"top","down":"top","side":"sides"}},"fallback":{"textures":{"*":"top"}}}),
            )
            .unwrap(),
        )
        .unwrap();
        std::fs::write(root.join("textures/terrain_texture.json"),serde_json::to_vec(&json!({"texture_name":"atlas.terrain","texture_data":{"top":{"textures":["textures/a","textures/b"]},"sides":{"textures":"textures/a"}}})).unwrap()).unwrap();
        for (name, color) in [("a", [1, 2, 3, 255]), ("b", [4, 5, 6, 255])] {
            let mut buffer = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, image::Rgba(color)))
                .write_to(&mut buffer, ImageFormat::Png)
                .unwrap();
            std::fs::write(
                root.join(format!("textures/{name}.png")),
                buffer.into_inner(),
            )
            .unwrap();
        }
        let mut pack = Pack::open(&root).unwrap();
        assert_eq!(
            block_material(&mut pack, "minecraft:test", "up", 1)
                .unwrap()
                .texture
                .rgba,
            [4, 5, 6, 255]
        );
        assert_eq!(
            block_material(&mut pack, "test", "west", 0)
                .unwrap()
                .texture
                .rgba,
            [1, 2, 3, 255]
        );
        assert!(block_material(&mut pack, "test", "up", 2).is_err());
        assert!(block_material(&mut pack, "test", "east", 1).is_err());
        assert!(block_material(&mut pack, "test", "diagonal", 0).is_err());
        assert_eq!(
            block_material(&mut pack, "fallback", "down", 0)
                .unwrap()
                .texture
                .rgba,
            [1, 2, 3, 255]
        );
    }
}
