//! Stable authoring identities, independent of registry order and cell positions.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StateIdentity {
    pub block: String,
    pub properties: BTreeMap<String, String>,
}
impl StateIdentity {
    pub fn validate(&self) -> crate::scene::Result<()> {
        if !identifier(&self.block)
            || self.properties.len() > 8
            || self
                .properties
                .iter()
                .any(|(k, v)| !identifier(k) || !identifier(v))
        {
            return Err("Invalid block state identity".into());
        }
        Ok(())
    }
}
pub fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 48
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_-".contains(&c))
}
