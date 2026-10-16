//! Bounded shaped recipes, translated/mirrored in shared 2x2 and 3x3 grids.
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
        for r in &self.recipes {
            if registry.get(registry.find(&r.result)?).container.is_some() && r.count != 1 {
                return Err("Container recipes produce one vessel".into());
            }
        }
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
                || r.pattern.len() > 8
                || cols == 0
                || cols > 8
                || r.pattern.iter().any(|row| row.len() != cols)
                || r.pattern.iter().flatten().all(String::is_empty)
                || r.count == 0
                || r.count > 64
                || r.result.is_empty()
            {
                return Err("Invalid shaped recipe definition".into());
            }
        }
        Ok(())
    }
    pub fn find(&self, registry: &Registry, player: &Inventory) -> Option<Match> {
        self.find_grid(registry, &player.crafting, player.profile.craft_side)
    }
    pub fn find_grid(
        &self,
        registry: &Registry,
        grid: &[Option<Stack>],
        side: usize,
    ) -> Option<Match> {
        if grid.iter().flatten().any(
            |s| matches!(s.data,Some(crate::engine::items::ItemData::Fluid{amount,..}) if amount>0),
        ) {
            return None;
        }
        if !(1..=8).contains(&side) || grid.len() != side * side {
            return None;
        }
        for r in &self.recipes {
            let result = registry.find(&r.result).ok();
            let width = r.pattern[0].len();
            let height = r.pattern.len();
            if width > side || height > side {
                continue;
            }
            for oy in 0..=side - height {
                for ox in 0..=side - width {
                    for mirror in [false, true] {
                        let mut consumed = Vec::new();
                        let mut valid = true;
                        for (i, slot) in grid.iter().enumerate() {
                            let x = i % side;
                            let y = i / side;
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
                            if slot.map(|s| s.id) != expected {
                                valid = false;
                            }
                            if expected.is_some() {
                                consumed.push(i);
                            }
                        }
                        if valid && let Some(id) = result {
                            return Some(Match {
                                stack: Stack {
                                    count: r.count,
                                    ..registry.catalog_stack(id)
                                },
                                consumed,
                            });
                        }
                    }
                }
            }
        }
        None
    }
    pub fn take_grid(
        &self,
        registry: &Registry,
        player: &mut Inventory,
        all: bool,
        table: bool,
    ) -> usize {
        let mut made = 0;
        for _ in 0..256 {
            let found = if table {
                self.find_grid(registry, &player.table, player.profile.table_side)
            } else {
                self.find(registry, player)
            };
            let Some(m) = found else {
                break;
            };
            if !player.accept_grid_output(m.stack, &m.consumed, all, table) {
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
    fn table_matches_three_wide_and_mirrored_recipes_but_inventory_cannot() {
        let r = Registry::built_in();
        let recipes = Recipes::built_in();
        let mut p = Inventory::empty();
        let cobble = r.find("cobblestone").unwrap();
        for i in [0, 1, 2, 3, 5, 6, 7, 8] {
            p.table[i] = Some(Stack {
                data: None,
                id: cobble,
                count: 2,
            });
        }
        let found = recipes.find_grid(&r, &p.table, 3).unwrap();
        assert_eq!(found.stack.id, r.find("furnace").unwrap());
        assert_eq!(found.consumed.len(), 8);
        p.cursor = Some(Stack {
            data: None,
            id: cobble,
            count: 64,
        });
        assert_eq!(recipes.take_grid(&r, &mut p, false, true), 0);
        assert_eq!(p.table[0].unwrap().count, 2);
        p.cursor = None;
        assert_eq!(recipes.take_grid(&r, &mut p, true, true), 2);
        assert!(p.table.iter().all(Option::is_none));
        let oak = r.find("oak_planks").unwrap();
        for i in [2, 4, 5, 6, 7, 8] {
            p.table[i] = Some(Stack {
                data: None,
                id: oak,
                count: 1,
            });
        }
        assert_eq!(
            recipes.find_grid(&r, &p.table, 3).unwrap().stack.id,
            r.find("oak_stairs_north").unwrap()
        );
        p.crafting = vec![
            Some(Stack {
                data: None,
                id: cobble,
                count: 1,
            });
            4
        ];
        assert!(recipes.find(&r, &p).is_none());
        p.table = vec![None; 9];
        for i in [6, 7, 8] {
            p.table[i] = Some(Stack {
                data: None,
                id: oak,
                count: 1,
            });
        }
        assert_eq!(recipes.find_grid(&r, &p.table, 3).unwrap().stack.count, 6);
    }
    #[test]
    fn translated_input_and_atomic_output_capacity() {
        let r = Recipes::built_in();
        let registry = Registry::built_in();
        let mut p = Inventory::empty();
        let log = registry.find("oak_log").unwrap();
        let planks = registry.find("oak_planks").unwrap();
        p.crafting[3] = Some(Stack {
            data: None,
            id: log,
            count: 2,
        });
        assert_eq!(
            r.find(&registry, &p).unwrap().stack,
            Stack {
                data: None,
                id: planks,
                count: 4
            }
        );
        assert_eq!(r.take_grid(&registry, &mut p, false, false), 4);
        assert_eq!(p.crafting[3].unwrap().count, 1);
        assert_eq!(p.cursor.unwrap().count, 4);
        p.cursor = Some(Stack {
            data: None,
            id: planks,
            count: 63,
        });
        assert_eq!(r.take_grid(&registry, &mut p, false, false), 0);
        assert_eq!(p.crafting[3].unwrap().count, 1);
        p.cursor = None;
        assert_eq!(r.take_grid(&registry, &mut p, true, false), 4);
        assert!(p.crafting[3].is_none());
        assert_eq!(p.slots[0].unwrap().count, 4);
    }
}
