//! User-authored colors and owned PNG sprite bindings, loaded outside draw calls.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Style {
    pub fill: [f32; 4],
    pub text: [f32; 4],
    pub border: [f32; 4],
    pub sprite: Option<String>,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            fill: [1.; 4],
            text: [0.25, 0.25, 0.25, 1.],
            border: [0.1, 0.12, 0.15, 1.],
            sprite: None,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub version: u32,
    #[serde(default)]
    pub textures: BTreeMap<String, String>,
    pub styles: BTreeMap<String, Style>,
}
impl Theme {
    pub fn validate(&self) -> crate::scene::Result<()> {
        if self.version != 1 || self.textures.len() > 8 || self.styles.len() > 64 {
            return Err("UI theme version/count invalid".into());
        }
        for style in self.styles.values() {
            if style
                .fill
                .iter()
                .chain(&style.text)
                .chain(&style.border)
                .any(|v| !v.is_finite() || *v < 0. || *v > 1.)
                || style
                    .sprite
                    .as_ref()
                    .is_some_and(|s| !self.textures.contains_key(s))
            {
                return Err("UI theme color or sprite reference invalid".into());
            }
        }
        Ok(())
    }
    pub fn style(&self, name: &str) -> Style {
        self.styles.get(name).cloned().unwrap_or_default()
    }
}
