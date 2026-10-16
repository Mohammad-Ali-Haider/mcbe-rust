//! Portable inventory layouts and named reusable containers.
use crate::{engine_blocks::Registry, scene::Result};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Container {
    pub id: String,
    pub label: String,
    pub slots: usize,
    #[serde(default)]
    pub accepts: Vec<String>,
    #[serde(default)]
    pub output_only: bool,
    #[serde(default)]
    pub quick_move_to: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub version: u32,
    pub player_slots: usize,
    pub hotbar_slots: usize,
    pub craft_side: usize,
    pub table_side: usize,
    pub containers: Vec<Container>,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            version: 1,
            player_slots: 36,
            hotbar_slots: 9,
            craft_side: 2,
            table_side: 3,
            containers: vec![],
        }
    }
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || !(1..=256).contains(&self.player_slots)
            || !(1..=16).contains(&self.hotbar_slots)
            || self.hotbar_slots > self.player_slots
            || !(1..=8).contains(&self.craft_side)
            || !(1..=8).contains(&self.table_side)
            || self.containers.len() > 16
        {
            return Err("Inventory profile sizes or version invalid".into());
        }
        let mut names = std::collections::BTreeSet::new();
        let mut total = self.player_slots
            + self.craft_side * self.craft_side
            + self.table_side * self.table_side;
        for c in &self.containers {
            if !crate::engine::blocks::state::identifier(&c.id)
                || c.id == "player"
                || !names.insert(c.id.as_str())
                || c.label.len() > 96
                || !(1..=256).contains(&c.slots)
                || c.accepts.len() > 256
                || c.accepts
                    .iter()
                    .any(|id| !crate::engine::blocks::state::identifier(id))
            {
                return Err("Container ID, label, slot count or filter invalid".into());
            }
            total += c.slots;
        }
        if total > 1024
            || self.containers.iter().any(|c| {
                c.quick_move_to.as_ref().is_some_and(|id| {
                    id == &c.id || (id != "player" && !names.contains(id.as_str()))
                })
            })
        {
            return Err("Inventory slot budget or transfer target invalid".into());
        }
        Ok(())
    }
    pub fn read_home(home: &std::path::Path) -> Result<Self> {
        let path = home.join("content/base/inventory.json");
        if !path.is_file() {
            return Ok(Self::default());
        }
        if std::fs::metadata(&path)?.len() > 256 * 1024 {
            return Err("Inventory profile exceeds 256 KiB".into());
        }
        let p: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        p.validate()?;
        Ok(p)
    }
    pub fn filters(&self, r: &Registry) -> Result<Vec<Vec<u16>>> {
        self.validate()?;
        self.containers
            .iter()
            .map(|c| c.accepts.iter().map(|id| r.find(id)).collect())
            .collect()
    }
}

impl Profile {
    pub fn write_model(&self, m: &mut crate::engine::ui::framework::screen::Model) {
        for (k, v) in [
            ("player_slots", self.player_slots),
            ("hotbar_slots", self.hotbar_slots),
            ("storage_slots", self.player_slots - self.hotbar_slots),
            ("craft_side", self.craft_side),
            ("craft_slots", self.craft_side.pow(2)),
            ("table_side", self.table_side),
            ("table_slots", self.table_side.pow(2)),
        ] {
            m.numbers.insert(k.into(), v as f64);
        }
        for i in 0..16 {
            let c = self.containers.get(i);
            m.numbers.insert(
                format!("container_{i}_slots"),
                c.map_or(0, |c| c.slots) as f64,
            );
            m.texts.insert(
                format!("container_{i}_label"),
                c.map_or_else(String::new, |c| c.label.clone()),
            );
        }
    }
}
