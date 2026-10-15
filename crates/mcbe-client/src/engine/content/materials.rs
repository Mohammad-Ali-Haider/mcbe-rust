//! Engine-owned per-face PNGs, resolved against the portable game folder.
use crate::{
    engine_blocks::{BlockId, Registry},
    scene::{Client, Result},
};
use mcbe_core::textures::Texture;
use std::{
    collections::HashMap,
    fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
};
pub struct Owned {
    pub root: PathBuf,
    pub textures: HashMap<(BlockId, usize), Arc<Texture>>,
}
pub fn valid_path(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.contains(['\\', ':'])
        && name.ends_with(".png")
        && Path::new(name)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}
pub fn load(registry: &Registry, home: &Path) -> Result<Owned> {
    let mut files = HashMap::<String, Arc<Texture>>::new();
    let mut textures = HashMap::new();
    let mut total = 0;
    for (id, b) in registry.blocks.iter().enumerate() {
        if let Some(names) = &b.texture_files {
            for (face, name) in names.iter().enumerate() {
                if !valid_path(name) {
                    return Err(
                        "Texture paths must be relative PNG files without parent traversal".into(),
                    );
                }
                if !files.contains_key(name) {
                    let path = home.join(name);
                    if fs::metadata(&path)?.len() > 1024 * 1024 {
                        return Err("Block PNG exceeds 1 MiB".into());
                    }
                    let t = Texture::decode(&fs::read(&path)?, "png")?;
                    if t.width > 256 || t.height > 256 {
                        return Err("Block PNG dimensions must be at most 256x256".into());
                    }
                    total += t.rgba.len();
                    if total > 16 * 1024 * 1024 {
                        return Err("Unique block textures exceed 16 MiB decoded".into());
                    }
                    files.insert(name.clone(), Arc::new(t));
                }
                let t = files[name].clone();
                if b.render_layer == crate::engine_blocks::RenderLayer::Opaque
                    && t.rgba.chunks_exact(4).any(|p| p[3] != 255)
                {
                    return Err(
                        "Opaque block PNG must have full alpha; choose cutout or translucent"
                            .into(),
                    );
                }
                textures.insert((id as u16, face), t);
            }
        }
    }
    Ok(Owned {
        root: home.to_path_buf(),
        textures,
    })
}
pub fn tint(rgb: [u8; 3], tint: Option<[u8; 3]>) -> [u8; 3] {
    tint.map_or(rgb, |t| {
        std::array::from_fn(|i| (u16::from(rgb[i]) * u16::from(t[i]) / 255) as u8)
    })
}
pub fn color(client: &Client, id: BlockId, face: usize, u: f64, v: f64) -> Option<[f64; 3]> {
    if let Some(t) = client
        .owned_materials
        .as_ref()
        .and_then(|a| a.textures.get(&(id, face)))
    {
        let p = t.sample_repeat(u, if face <= 1 { v } else { 1. - v });
        let b = client.scene.engine.as_ref()?.registry.get(id);
        return Some(tint([p[0], p[1], p[2]], b.texture_tint).map(f64::from));
    }
    let state = client.scene.engine.as_ref()?;
    client
        .engine_assets
        .as_ref()?
        .color(&state.registry, id, face, u, v)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_texture_tint_preserves_raw_colors_and_multiplies_channels() {
        assert_eq!(tint([255, 128, 64], None), [255, 128, 64]);
        assert_eq!(tint([255, 128, 64], Some([80, 160, 40])), [80, 80, 10]);
        assert_eq!(tint([20, 80, 255], Some([255; 3])), [20, 80, 255]);
    }
    #[test]
    fn own_png_paths_are_portable_and_bounded() {
        assert!(valid_path("assets/textures/my_stone.png"));
        for p in [
            "../other.png",
            "C:/secret.png",
            "/root.png",
            "a\\b.png",
            "a.png:stream",
            "a.jpg",
        ] {
            assert!(!valid_path(p), "{p}");
        }
    }
}
