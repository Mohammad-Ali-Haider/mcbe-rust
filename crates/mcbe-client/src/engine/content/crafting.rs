//! Bounded engine 2x2 shaped recipes, translated inside the input grid.
use crate::{
    engine_blocks::Registry,
    engine_inventory::{Inventory, Stack},
    scene::Result,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub id: String,
    pub pattern: Vec<Vec<String>>,
    pub result: String,
    pub count: u8,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipes {
    pub version: u32,
    pub recipes: Vec<Recipe>,
}
pub struct Match {
    pub stack: Stack,
    pub consumed: Vec<usize>,
}
impl Recipes {
    pub fn read(path: &std::path::Path) -> Result<Self> {
        if std::fs::metadata(path)?.len() > 256 * 1024 {
            return Err("Recipe file exceeds 256 KiB".into());
        }
        Self::parse(&std::fs::read(path)?)
    }
    pub fn available(mut self, registry: &Registry) -> Self {
        self.recipes.retain(|r| {
            registry.find(&r.result).is_ok()
                && r.pattern
                    .iter()
                    .flatten()
                    .all(|s| s.is_empty() || registry.find(s).is_ok())
        });
        self
    }
    pub fn validate_registry(&self, registry: &Registry) -> Result<()> {
        self.validate()?;
        for r in &self.recipes {
            registry.find(&r.result)?;
            for id in r.pattern.iter().flatten().filter(|s| !s.is_empty()) {
                registry.find(id)?;
            }
        }
        Ok(())
    }
    pub fn built_in() -> Self {
        Self::parse(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../content/base/recipes/crafting.json"
        )))
        .expect("Built-in recipes")
    }
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 256 * 1024 {
            return Err("Recipe file exceeds 256 KiB".into());
        }
        let r: Self = serde_json::from_slice(bytes)?;
        r.validate()?;
        Ok(r)
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 || self.recipes.len() > 512 {
            return Err("Unsupported recipe version or count".into());
        }
        let mut names = std::collections::HashSet::new();
        for r in &self.recipes {
            let cols = r.pattern.first().map_or(0, Vec::len);
            if r.id.is_empty()
                || !names.insert(&r.id)
                || r.pattern.is_empty()
                || r.pattern.len() > 2
                || cols == 0
                || cols > 2
                || r.pattern.iter().any(|row| row.len() != cols)
                || r.pattern.iter().flatten().all(String::is_empty)
                || r.count == 0
                || r.count > 64
                || r.result.is_empty()
            {
                return Err("Invalid 2x2 recipe definition".into());
            }
        }
        Ok(())
    }
    pub fn find(&self, registry: &Registry, player: &Inventory) -> Option<Match> {
        for r in &self.recipes {
            let result = registry.find(&r.result).ok();
            let width = r.pattern[0].len();
            let height = r.pattern.len();
            for oy in 0..=2 - height {
                for ox in 0..=2 - width {
                    for mirror in [false, true] {
                        let mut consumed = Vec::new();
                        let mut valid = true;
                        for i in 0..4 {
                            let x = i % 2;
                            let y = i / 2;
                            let expected =
                                if x >= ox && y >= oy && x < ox + width && y < oy + height {
                                    let cx = x - ox;
                                    let key = &r.pattern[y - oy]
                                        [if mirror { width - 1 - cx } else { cx }];
                                    if key.is_empty() {
                                        None
                                    } else {
                                        match registry.find(key) {
                                            Ok(id) => Some(id),
                                            Err(_) => {
                                                valid = false;
                                                None
                                            }
                                        }
                                    }
                                } else {
                                    None
                                };
                            if player.crafting[i].map(|s| s.id) != expected {
                                valid = false;
                            }
                            if expected.is_some() {
                                consumed.push(i);
                            }
                        }
                        if valid && let Some(id) = result {
                            return Some(Match {
                                stack: Stack { id, count: r.count },
                                consumed,
                            });
                        }
                    }
                }
            }
        }
        None
    }
    pub fn take(&self, registry: &Registry, player: &mut Inventory, all: bool) -> usize {
        let mut made = 0;
        for _ in 0..256 {
            let Some(m) = self.find(registry, player) else {
                break;
            };
            if !player.accept_output(m.stack, &m.consumed, all) {
                break;
            }
            made += usize::from(m.stack.count);
            if !all {
                break;
            }
        }
        made
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn translated_input_and_atomic_output_capacity() {
        let r = Recipes::built_in();
        let registry = Registry::built_in();
        let mut p = Inventory::empty();
        let log = registry.find("oak_log").unwrap();
        let planks = registry.find("oak_planks").unwrap();
        p.crafting[3] = Some(Stack { id: log, count: 2 });
        assert_eq!(
            r.find(&registry, &p).unwrap().stack,
            Stack {
                id: planks,
                count: 4
            }
        );
        assert_eq!(r.take(&registry, &mut p, false), 4);
        assert_eq!(p.crafting[3].unwrap().count, 1);
        assert_eq!(p.cursor.unwrap().count, 4);
        p.cursor = Some(Stack {
            id: planks,
            count: 63,
        });
        assert_eq!(r.take(&registry, &mut p, false), 0);
        assert_eq!(p.crafting[3].unwrap().count, 1);
        p.cursor = None;
        assert_eq!(r.take(&registry, &mut p, true), 4);
        assert!(p.crafting[3].is_none());
        assert_eq!(p.slots[0].unwrap().count, 4);
    }
}
