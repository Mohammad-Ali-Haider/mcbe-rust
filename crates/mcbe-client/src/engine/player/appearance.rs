//! Read-only local skin configuration and a shared validated humanoid model.
use crate::{engine_preview::Model, scene::Result};
use mcbe_core::textures::Texture;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub skin: String,
    #[serde(default)]
    pub slim: bool,
}
pub struct Appearance {
    pub skin: Texture,
    pub model: Model,
    pub provenance: serde_json::Value,
}
impl Appearance {
    pub fn fallback() -> Self {
        let mut rgba = vec![0; 64 * 64 * 4];
        for (rect, color) in [
            ([0, 0, 32, 16], [195, 150, 110, 255]),
            ([16, 16, 24, 16], [44, 100, 150, 255]),
            ([0, 16, 16, 16], [42, 48, 66, 255]),
            ([40, 16, 16, 16], [195, 150, 110, 255]),
            ([16, 48, 16, 16], [42, 48, 66, 255]),
            ([32, 48, 16, 16], [195, 150, 110, 255]),
        ] {
            for y in rect[1]..rect[1] + rect[3] {
                for x in rect[0]..rect[0] + rect[2] {
                    rgba[(y * 64 + x) * 4..(y * 64 + x + 1) * 4].copy_from_slice(&color);
                }
            }
        }
        Self {
            skin: Texture {
                width: 64,
                height: 64,
                rgba,
            },
            model: Model::standard([64, 64], false),
            provenance: serde_json::json!({"source":"engine fallback","size":[64,64],"slim":false}),
        }
    }
    pub fn read_home(home: &Path) -> Result<Self> {
        let file = home.join(".local/player.json");
        if !file.is_file() {
            return Ok(Self::fallback());
        }
        if std::fs::metadata(&file)?.len() > 4096 {
            return Err("Player configuration exceeds 4 KiB".into());
        }
        let config: Config = serde_json::from_slice(&std::fs::read(&file)?)?;
        if config.skin.is_empty() {
            return Ok(Self::fallback());
        }
        let path = Path::new(&config.skin);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            home.join(path)
        };
        if std::fs::metadata(&path)?.len() > 1024 * 1024 {
            return Err("Player skin PNG exceeds 1 MiB".into());
        }
        let bytes = std::fs::read(&path)?;
        let skin = Texture::decode(&bytes, "png")?;
        if !matches!((skin.width, skin.height), (64, 64) | (64, 32))
            || (skin.height == 32 && config.slim)
        {
            return Err("Use a 64x64 skin, or a classic 64x32 legacy skin".into());
        }
        let model = Model::standard([skin.width, skin.height], config.slim);
        let provenance = serde_json::json!({"source":path,"sha256":format!("{:x}",Sha256::digest(&bytes)),"rgba_sha256":format!("{:x}",Sha256::digest(&skin.rgba)),"size":[skin.width,skin.height],"slim":config.slim,"read_only":true});
        Ok(Self {
            skin,
            model,
            provenance,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_and_legacy_models_have_bounded_uvs_and_matching_arm_widths() {
        for (size, slim) in [([64, 64], false), ([64, 64], true), ([64, 32], false)] {
            let model = Model::standard(size, slim);
            assert_eq!(model.cubes.len(), if size[1] == 32 { 7 } else { 12 });
            for c in &model.cubes {
                assert!(c.uv[0] + 2. * (c.size[0] + c.size[2]) <= size[0] as f32);
                assert!(c.uv[1] + c.size[1] + c.size[2] <= size[1] as f32);
            }
            assert_eq!(
                model
                    .cubes
                    .iter()
                    .find(|c| c.bone == "rightArm")
                    .unwrap()
                    .size[0],
                if slim { 3. } else { 4. }
            );
        }
    }
}
