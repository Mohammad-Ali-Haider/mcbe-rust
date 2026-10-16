//! Explicit renderer selection and bounded lighting quality.
pub(crate) mod deferred;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    #[default]
    Forward,
    Deferred,
    RayTraced,
}
impl Method {
    pub fn cycle(self, delta: i32) -> Self {
        let index = match self {
            Self::Forward => 0,
            Self::Deferred => 1,
            Self::RayTraced => 2,
        };
        [Self::Forward, Self::Deferred, Self::RayTraced][(index + delta).rem_euclid(3) as usize]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Forward => "Forward (fast)",
            Self::Deferred => "Deferred",
            Self::RayTraced => "Ray-traced lighting",
        }
    }
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    Low,
    #[default]
    Balanced,
    High,
}
impl Quality {
    pub fn index(self) -> u32 {
        match self {
            Self::Low => 0,
            Self::Balanced => 1,
            Self::High => 2,
        }
    }
    pub fn shadow_size(self) -> u32 {
        1024 << self.index()
    }
    pub fn radius(self) -> f32 {
        [32., 64., 96.][self.index() as usize]
    }
    pub fn samples(self) -> u32 {
        [4, 8, 12][self.index() as usize]
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Balanced => "Balanced",
            Self::High => "High",
        }
    }
    pub fn ray_samples(self) -> u32 {
        [1, 2, 4][self.index() as usize]
    }
    pub fn cycle(self, delta: i32) -> Self {
        [Self::Low, Self::Balanced, Self::High]
            [(self.index() as i32 + delta).rem_euclid(3) as usize]
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rendering {
    pub ray_reflections: bool,
    pub ray_indirect: bool,
    pub method: Method,
    pub shadows: bool,
    pub ambient_occlusion: bool,
    pub exposure: u32,
    pub quality: Quality,
}
impl Default for Rendering {
    fn default() -> Self {
        Self {
            ray_reflections: true,
            ray_indirect: true,
            method: Method::Forward,
            shadows: true,
            ambient_occlusion: true,
            exposure: 100,
            quality: Quality::Balanced,
        }
    }
}
impl Rendering {
    pub fn validate(&self) -> crate::scene::Result<()> {
        if !(50..=200).contains(&self.exposure) {
            return Err("Exposure must be 50..200 percent".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

pub(crate) mod ray;
