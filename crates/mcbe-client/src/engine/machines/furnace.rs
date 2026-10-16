//! Furnace state advances analytically at fuel/cook boundaries, not per voxel.
use super::recipes::{Recipe, Recipes};
use crate::{
    engine_inventory::{Inventory, Stack},
    scene::Result,
};
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Furnace {
    /// Input, fuel, output. Output accepts withdrawal only.
    pub slots: [Option<Stack>; 3],
    pub burn_ms: u32,
    pub burn_total_ms: u32,
    pub cook_ms: u32,
    pub cooking: Option<u16>,
    pub updated_ms: u64,
}
impl Furnace {
    pub fn validate(&self, count: usize, clock: u64) -> Result<()> {
        let mut p = Inventory::empty();
        p.slots[..3].copy_from_slice(&self.slots);
        p.validate(count)?;
        if self.burn_ms > self.burn_total_ms
            || self.burn_total_ms > 3_600_000
            || self.cook_ms > 3_600_000
            || self.updated_ms > clock
            || self.cooking.is_some_and(|i| i as usize >= count)
        {
            return Err("Invalid saved furnace timers".into());
        }
        Ok(())
    }
    fn recipe(&self, recipes: &Recipes) -> Option<Recipe> {
        let input = self.slots[0]?;
        if input.data.is_some() {
            return None;
        }
        let r = recipes.get(input.id)?;
        self.slots[2]
            .is_none_or(|s| {
                s.data.is_none()
                    && s.id == r.result
                    && u16::from(s.count) + u16::from(r.count) <= 64
            })
            .then_some(r)
    }
    pub fn active(&self, r: &Recipes) -> bool {
        self.burn_ms > 0
            || (self.recipe(r).is_some() && self.slots[1].is_some_and(|s| r.fuel(s.id) > 0))
    }
    fn consume(slot: &mut Option<Stack>) {
        if let Some(s) = slot {
            s.count -= 1;
            if s.count == 0 {
                *slot = None;
            }
        }
    }
    pub fn advance(&mut self, clock: u64, recipes: &Recipes) {
        let mut elapsed = clock.saturating_sub(self.updated_ms);
        self.updated_ms = clock;
        let identity = self.slots[0].map(|s| s.id);
        if identity != self.cooking {
            self.cook_ms = 0;
            self.cooking = identity;
        }
        while elapsed > 0 {
            let recipe = self.recipe(recipes);
            if recipe.is_none() {
                self.cook_ms = 0;
            }
            if self.burn_ms == 0 {
                if recipe.is_none() {
                    break;
                }
                let fuel = self.slots[1].map_or(0, |s| recipes.fuel(s.id));
                if fuel == 0 {
                    break;
                }
                Self::consume(&mut self.slots[1]);
                self.burn_ms = fuel;
                self.burn_total_ms = fuel;
            }
            let step = elapsed
                .min(u64::from(self.burn_ms))
                .min(recipe.map_or(u64::MAX, |r| {
                    u64::from(r.cook_ms.saturating_sub(self.cook_ms))
                }));
            self.burn_ms -= step as u32;
            elapsed -= step;
            if let Some(r) = recipe {
                self.cook_ms += step as u32;
                if self.cook_ms >= r.cook_ms {
                    Self::consume(&mut self.slots[0]);
                    if let Some(s) = &mut self.slots[2] {
                        s.count += r.count;
                    } else {
                        self.slots[2] = Some(Stack {
                            data: None,
                            id: r.result,
                            count: r.count,
                        });
                    }
                    self.cook_ms = 0;
                    self.cooking = self.slots[0].map(|s| s.id);
                }
            }
        }
    }
    pub fn cook_fraction(&self, r: &Recipes) -> f64 {
        self.slots[0]
            .and_then(|s| r.get(s.id))
            .map_or(0., |r| f64::from(self.cook_ms) / f64::from(r.cook_ms))
    }
    pub fn burn_fraction(&self) -> f64 {
        if self.burn_total_ms == 0 {
            0.
        } else {
            f64::from(self.burn_ms) / f64::from(self.burn_total_ms)
        }
    }
}
