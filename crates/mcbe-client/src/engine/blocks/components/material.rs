//! Linear metallic/roughness parameters, validated at registration.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Material {
    pub roughness: f32,
    pub metallic: f32,
    pub emission: f32,
}
impl Default for Material {
    fn default() -> Self {
        Self {
            roughness: 0.82,
            metallic: 0.,
            emission: 0.,
        }
    }
}
impl Material {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn validate(self) -> crate::scene::Result<()> {
        if !self.roughness.is_finite()
            || !self.metallic.is_finite()
            || !self.emission.is_finite()
            || !(0.04..=1.).contains(&self.roughness)
            || !(0. ..=1.).contains(&self.metallic)
            || !(0. ..=8.).contains(&self.emission)
        {
            return Err(
                "Material requires roughness .04..1, metallic 0..1 and emission 0..8".into(),
            );
        }
        Ok(())
    }
    pub fn row(self) -> [f32; 4] {
        [self.roughness, self.metallic, self.emission, 0.]
    }
}
