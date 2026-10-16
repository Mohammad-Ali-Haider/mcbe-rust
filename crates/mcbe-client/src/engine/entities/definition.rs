//! Strict JSON entity definitions and cached material bindings.
use super::components::Behavior;
use crate::{engine_blocks::Registry, scene::Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv: Option<[[f32; 4]; 6]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<[[f32; 4]; 4]>,
    pub bounds: [f32; 6],
    pub material: String,
    #[serde(default)]
    pub swing: f32,
    #[serde(default)]
    pub pivot: [f32; 3],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Body {
    pub width: f32,
    pub height: f32,
    pub speed: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin: Option<super::assets::Skin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_resource: Option<super::assets::ModelResource>,
    pub id: String,
    pub label: String,
    pub body: Option<Body>,
    pub health: Option<u16>,
    pub behavior: Option<Behavior>,
    pub sense: f32,
    pub ambient_weight: u16,
    pub model: Vec<Part>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definitions {
    pub version: u32,
    pub kinds: Vec<Definition>,
}
#[derive(Clone)]
pub struct Library {
    pub definitions: Definitions,
    pub kinds: BTreeMap<String, usize>,
    pub materials: Vec<Vec<u16>>,
    pub radii: Vec<f32>,
}
impl Definitions {
    pub fn has_skin_data(&self) -> bool {
        self.kinds.iter().any(|k| {
            k.skin.is_some()
                || k.model_resource.is_some()
                || k.model
                    .iter()
                    .any(|p| p.uv.is_some() || p.transform.is_some())
        })
    }
    pub fn built_in() -> Self {
        serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../content/base/entities.json"
        )))
        .expect("Built-in entity definitions")
    }
    pub fn read_home(home: &Path) -> Result<Self> {
        let p = home.join("content/base/entities.json");
        if !p.is_file() {
            return Ok(Self::built_in());
        }
        if std::fs::metadata(&p)?.len() > 256 * 1024 {
            return Err("Entity definitions exceed 256 KiB".into());
        }
        Ok(serde_json::from_slice(&std::fs::read(p)?)?)
    }
    pub fn available(mut self, r: &Registry) -> Self {
        self.kinds.retain(|d| {
            d.model.iter().all(|p| r.find(&p.material).is_ok())
                && d.model_resource
                    .as_ref()
                    .is_none_or(|m| r.find(&m.material).is_ok())
        });
        self
    }
    pub fn compile(self, r: &Registry) -> Result<Library> {
        if self.version != 1 || self.kinds.len() > 64 {
            return Err("Entity definition version/count invalid".into());
        }
        let mut kinds = BTreeMap::new();
        let mut materials = Vec::new();
        let mut radii = Vec::new();
        for (i, d) in self.kinds.iter().enumerate() {
            if d.id.is_empty()
                || d.id.len() > 64
                || !d
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_:-".contains(&b))
                || kinds.insert(d.id.clone(), i).is_some()
                || d.label.is_empty()
                || d.label.len() > 96
                || !d.sense.is_finite()
                || !(0. ..=32.).contains(&d.sense)
                || d.ambient_weight > 1000
                || d.model.len() > 32
                || d.health == Some(0)
            {
                return Err("Invalid/duplicate entity definition".into());
            }
            if let Some(b) = &d.body
                && (![b.width, b.height, b.speed].into_iter().all(f32::is_finite)
                    || !(0.1..=2.).contains(&b.width)
                    || !(0.1..=4.).contains(&b.height)
                    || !(0. ..=6.).contains(&b.speed))
            {
                return Err("Entity body/speed invalid".into());
            }
            if d.behavior.is_some() && d.body.is_none() {
                return Err("Moving AI requires a physics body".into());
            }
            if let Some(s) = &d.skin {
                s.validate()?;
            }
            if let Some(m) = &d.model_resource {
                m.validate()?;
                r.find(&m.material)?;
            }
            let mut ids = Vec::new();
            for p in &d.model {
                super::assets::validate_part(p)?;
                if !p
                    .bounds
                    .into_iter()
                    .chain(p.pivot)
                    .chain([p.swing])
                    .all(f32::is_finite)
                    || (0..3).any(|a| {
                        p.bounds[a] >= p.bounds[a + 3]
                            || p.bounds[a] < -4.
                            || p.bounds[a + 3] > 8.
                            || p.pivot[a].abs() > 8.
                    })
                    || p.swing.abs() > 1.5
                {
                    return Err("Entity cuboid/pivot invalid".into());
                }
                let id = r.find(&p.material)?;
                if !matches!(
                    r.get(id).render_layer,
                    crate::engine_blocks::RenderLayer::Opaque
                ) || r.get(id).container.is_some()
                {
                    return Err("Entity material must be an opaque registered block".into());
                }
                ids.push(id);
            }
            materials.push(ids);
            radii.push(
                super::model::radius(&d.model).max(d.body.as_ref().map_or(0., |b| {
                    (b.height * b.height + b.width * b.width * 0.5).sqrt()
                })),
            );
        }
        Ok(Library {
            definitions: self,
            kinds,
            materials,
            radii,
        })
    }
}
impl Library {
    pub fn get(&self, id: &str) -> Result<&Definition> {
        self.kinds
            .get(id)
            .map(|i| &self.definitions.kinds[*i])
            .ok_or_else(|| format!("Unknown entity kind {id}").into())
    }
}
