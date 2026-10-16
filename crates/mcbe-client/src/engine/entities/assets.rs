//! Validated creature skins and optional local cuboid resources, read outside draw calls.
use super::definition::{Definitions, Part};
use crate::{
    engine_blocks::Registry,
    scene::{Client, Result},
};
use glam::{Mat4, Vec3};
use mcbe_core::{resources::Pack, textures::Texture};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum Skin {
    Owned { path: String },
    LocalPack { pack: String, path: String },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelResource {
    pub pack: String,
    pub path: String,
    pub geometry: String,
    pub material: String,
    #[serde(default)]
    pub swings: BTreeMap<String, f32>,
    #[serde(default)]
    pub poses: BTreeMap<String, [f32; 3]>,
}
fn pack_path(pack: &str, path: &str) -> Result<()> {
    mcbe_core::resources::validate_path(pack)?;
    mcbe_core::resources::validate_path(path)?;
    if pack.contains('/') {
        return Err("Pack name must be one directory".into());
    }
    Ok(())
}
impl Skin {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Owned { path } => {
                if !crate::engine_materials::valid_path(path) {
                    return Err("Entity skin must be a relative PNG path".into());
                }
            }
            Self::LocalPack { pack, path } => {
                pack_path(pack, path)?;
                if !path.ends_with(".png") {
                    return Err("Entity pack skin must be a PNG".into());
                }
            }
        }
        Ok(())
    }
}
impl ModelResource {
    pub fn validate(&self) -> Result<()> {
        pack_path(&self.pack, &self.path)?;
        if !self.path.ends_with(".json")
            || self.geometry.is_empty()
            || self.geometry.len() > 128
            || self.swings.len() > 64
            || self.poses.len() > 64
            || self
                .swings
                .iter()
                .any(|(k, v)| k.len() > 64 || !v.is_finite() || v.abs() > 1.5)
            || self
                .poses
                .iter()
                .any(|(k, v)| k.len() > 64 || v.iter().any(|x| !x.is_finite() || x.abs() > 360.))
        {
            return Err("Entity model source or bone pose invalid".into());
        }
        Ok(())
    }
}
pub fn validate_part(p: &Part) -> Result<()> {
    if p.uv.as_ref().is_some_and(|uv| {
        uv.iter().any(|r| {
            !r.iter().all(|x| x.is_finite())
                || r[2].abs() <= 0.
                || r[3].abs() <= 0.
                || (0..2)
                    .any(|a| r[a].min(r[a] + r[a + 2]) < 0. || r[a].max(r[a] + r[a + 2]) > 1.00001)
        })
    }) {
        return Err("Entity face UV must remain within its skin".into());
    }
    if let Some(t) = p.transform {
        let m = Mat4::from_cols_array_2d(&t);
        if !m.is_finite()
            || m.x_axis.w != 0.
            || m.y_axis.w != 0.
            || m.z_axis.w != 0.
            || m.w_axis.w != 1.
            || m.determinant() <= 1e-8
        {
            return Err("Entity transform must be finite, affine and preserve winding".into());
        }
        for i in 0..8 {
            let q = m.transform_point3(Vec3::new(
                (i & 1) as f32,
                ((i >> 1) & 1) as f32,
                ((i >> 2) & 1) as f32,
            ));
            if (0..3).any(|a| q[a] < p.bounds[a] - 0.0001 || q[a] > p.bounds[a + 3] + 0.0001) {
                return Err("Entity transform exceeds declared cuboid bounds".into());
            }
        }
    }
    Ok(())
}
pub struct Assets {
    pub id: u64,
    pub textures: Vec<Arc<Texture>>,
    pub kinds: BTreeMap<String, u32>,
    pub provenance: Vec<serde_json::Value>,
    pub home: PathBuf,
    pub root: Option<PathBuf>,
    signature: Vec<(String, Option<Skin>)>,
}
impl Default for Assets {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            textures: vec![],
            kinds: BTreeMap::new(),
            provenance: vec![],
            home: PathBuf::new(),
            root: None,
            signature: vec![],
        }
    }
}
impl Assets {
    pub fn layer(&self, kind: &str) -> u32 {
        self.kinds.get(kind).copied().unwrap_or(0)
    }
    pub fn read(
        definitions: &Definitions,
        r: &Registry,
        home: &Path,
        root: Option<&Path>,
    ) -> Result<(Definitions, Self)> {
        // Validate before opening resources or publishing compiled model state.
        definitions.clone().compile(r)?;
        let mut definitions = definitions.clone();
        let mut packs = BTreeMap::<String, Pack>::new();
        let mut assets = Self {
            home: home.to_path_buf(),
            root: root.map(Path::to_path_buf),
            ..Default::default()
        };
        let mut decoded = BTreeMap::<String, u32>::new();
        for d in &mut definitions.kinds {
            if let Some(source) = &d.model_resource
                && let Some(root) = root
                && (d.model.is_empty() || d.model.iter().any(|p| p.uv.is_none()))
            {
                if !packs.contains_key(&source.pack) {
                    packs.insert(source.pack.clone(), Pack::open(&root.join(&source.pack))?);
                }
                let resource = packs.get_mut(&source.pack).unwrap().read(&source.path)?;
                if resource.bytes.len() > 4 * 1024 * 1024 {
                    return Err("Entity resource model exceeds 4 MiB".into());
                }
                d.model =
                    super::import::model(&mcbe_core::resources::json(&resource.bytes)?, source)?;
                assets.provenance.push(serde_json::json!({"kind":d.id,"model":source.path,"origins":resource.origins,"parts":d.model.len()}));
            }
            let Some(skin) = &d.skin else {
                continue;
            };
            let key = serde_json::to_string(skin)?;
            if let Some(layer) = decoded.get(&key) {
                if d.model.iter().any(|p| p.uv.is_none()) {
                    return Err("A skinned entity requires UVs for each visible cuboid".into());
                }
                assets.kinds.insert(d.id.clone(), *layer);
                continue;
            }
            let bytes = match skin {
                Skin::Owned { path } => {
                    let file = home.join(path);
                    if std::fs::metadata(&file)?.len() > 1024 * 1024 {
                        return Err("Entity skin exceeds 1 MiB".into());
                    }
                    std::fs::read(file)?
                }
                Skin::LocalPack { pack, path } => {
                    let Some(root) = root else {
                        continue;
                    };
                    if !packs.contains_key(pack) {
                        packs.insert(pack.clone(), Pack::open(&root.join(pack))?);
                    }
                    packs.get_mut(pack).unwrap().read(path)?.bytes
                }
            };
            if bytes.len() > 1024 * 1024 {
                return Err("Entity skin exceeds 1 MiB".into());
            }
            let texture = Texture::decode(&bytes, "png")?;
            if texture.width > 256 || texture.height > 256 {
                return Err("Entity skin dimensions exceed 256x256".into());
            }
            if d.model.iter().any(|p| p.uv.is_none()) {
                return Err("A skinned entity requires UVs for each visible cuboid".into());
            }
            let layer = assets.textures.len() as u32 + 1;
            assets.provenance.push(serde_json::json!({"kind":d.id,"skin":skin,"size":[texture.width,texture.height],"rgba_sha256":texture.rgba_sha256()}));
            assets.textures.push(Arc::new(texture));
            assets.kinds.insert(d.id.clone(), layer);
            decoded.insert(key, layer);
        }
        for p in packs.values() {
            p.verify_unchanged()?;
        }
        definitions.clone().compile(r)?;
        assets.signature = definitions
            .kinds
            .iter()
            .map(|d| (d.id.clone(), d.skin.clone()))
            .collect();
        Ok((definitions, assets))
    }
    pub fn for_definitions(
        self: &Arc<Self>,
        definitions: &Definitions,
        r: &Registry,
    ) -> Result<(Definitions, Arc<Self>)> {
        let needs_models = self.root.is_some()
            && definitions.kinds.iter().any(|d| {
                d.model_resource.is_some()
                    && (d.model.is_empty() || d.model.iter().any(|p| p.uv.is_none()))
            });
        if !needs_models
            && self.signature
                == definitions
                    .kinds
                    .iter()
                    .map(|d| (d.id.clone(), d.skin.clone()))
                    .collect::<Vec<_>>()
        {
            return Ok((definitions.clone(), self.clone()));
        }
        let (definitions, assets) = Self::read(definitions, r, &self.home, self.root.as_deref())?;
        Ok((definitions, Arc::new(assets)))
    }
}
pub fn prepare(client: &mut Client, home: &Path, root: Option<&Path>) -> Result<()> {
    let state = client.scene.engine.as_ref().unwrap();
    let (definitions, assets) = Assets::read(
        &state.entities.library.definitions,
        &state.registry,
        home,
        root,
    )?;
    let library = definitions.compile(&state.registry)?;
    let world = state.entities.reconfigured(library)?;
    client.scene.engine.as_mut().unwrap().entities = world;
    client.entity_assets = Arc::new(assets);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_skin_is_shared_bounded_and_prepared_without_installed_resources() {
        let home = std::env::temp_dir().join(format!(
            "mc-rust-owned-mob-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&home).unwrap();
        crate::render::png(&home.join("skin.png"), &[0x37b8dc; 64], 8, 8).unwrap();
        let bytes = std::fs::read(home.join("skin.png")).unwrap();
        let r = Registry::built_in();
        let mut definitions = Definitions::built_in();
        definitions.kinds.truncate(1);
        definitions.kinds[0].skin = Some(Skin::Owned {
            path: "skin.png".into(),
        });
        for p in &mut definitions.kinds[0].model {
            p.uv = Some([[0., 0., 1., 1.]; 6]);
            let low = Vec3::new(p.bounds[0], p.bounds[1], p.bounds[2]);
            let high = Vec3::new(p.bounds[3], p.bounds[4], p.bounds[5]);
            p.transform = Some(
                (Mat4::from_translation(low) * Mat4::from_scale(high - low)).to_cols_array_2d(),
            );
        }
        let mut twin = definitions.kinds[0].clone();
        twin.id = "twin".into();
        definitions.kinds.push(twin);
        let (definitions, assets) = Assets::read(&definitions, &r, &home, None).unwrap();
        assert_eq!(assets.textures.len(), 1);
        assert_eq!(assets.layer("grazer"), 1);
        assert_eq!(assets.layer("twin"), 1);
        assert_eq!(
            assets.textures[0].sample_repeat(0.25, 0.25),
            [55, 184, 220, 255]
        );
        let mut c = super::super::tests::flat();
        c.scene.engine.as_mut().unwrap().entities.library =
            Arc::new(definitions.compile(&r).unwrap());
        c.entity_assets = Arc::new(assets);
        let plan = crate::engine_texture::Prepared::build(&c).unwrap();
        assert_eq!(plan.skin_width, 8);
        assert_eq!(plan.skin_layers, 1);
        let mut count = 0;
        for step in &plan.steps {
            if let crate::engine_texture::Step::Skin { rgba, .. } = step {
                assert!(rgba.len() <= crate::engine_texture::BATCH_BYTES);
                assert_eq!(&rgba[..4], &[55, 184, 220, 255]);
                count += 1;
            }
        }
        assert_eq!(count, 4);
        assert_eq!(std::fs::read(home.join("skin.png")).unwrap(), bytes);
        let assets = c.entity_assets.clone();
        let defs = &c
            .scene
            .engine
            .as_ref()
            .unwrap()
            .entities
            .library
            .definitions;
        assert!(Arc::ptr_eq(
            &assets,
            &assets.for_definitions(defs, &r).unwrap().1
        ));
        {
            let w = &mut c.scene.engine.as_mut().unwrap().entities;
            w.spawn("grazer", [3., 1., 0.], true).unwrap();
            let saved = w.saved();
            let restored = super::super::World::restore(saved.clone(), &r).unwrap();
            assert_eq!(restored.saved(), saved);
        }
        let value = serde_json::to_vec(
            &crate::engine::SaveSnapshot::capture(&c)
                .unwrap()
                .document()
                .unwrap(),
        )
        .unwrap();
        let mut copy = crate::engine::decode(&value, None).unwrap();
        prepare(&mut copy, &home, None).unwrap();
        assert_eq!(
            copy.scene.engine.as_ref().unwrap().entities.saved(),
            c.scene.engine.as_ref().unwrap().entities.saved()
        );
        assert_eq!(
            copy.entity_assets.textures[0].rgba,
            c.entity_assets.textures[0].rgba
        );
        std::fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn invalid_skin_paths_uvs_and_singular_transforms_fail_before_resource_use() {
        assert!(
            Skin::Owned {
                path: "../skin.png".into()
            }
            .validate()
            .is_err()
        );
        assert!(
            Skin::LocalPack {
                pack: "../other".into(),
                path: "skin.png".into()
            }
            .validate()
            .is_err()
        );
        let r = Registry::built_in();
        let mut defs = Definitions::built_in();
        let part = &mut defs.kinds[0].model[0];
        part.uv = Some([[0., 0., 2., 1.]; 6]);
        assert!(defs.clone().compile(&r).is_err());
        let part = &mut defs.kinds[0].model[0];
        part.uv = None;
        part.transform = Some([[0.; 4]; 4]);
        assert!(defs.clone().compile(&r).is_err());
        defs.kinds[0].model[0].transform = Some(Mat4::IDENTITY.to_cols_array_2d());
        assert!(defs.compile(&r).is_err());
    }
}
