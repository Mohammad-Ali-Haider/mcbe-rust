//! Sparse persistent machines with a bounded active queue independent of chunk residency.
pub(crate) mod furnace;
pub(crate) mod recipes;
use crate::scene::Result;
use furnace::Furnace;
use recipes::Recipes;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
pub const MAX_MACHINES: usize = 4096;
pub const UPDATE_BUDGET: usize = 64;
#[derive(Clone, Default)]
pub struct Machines {
    pub furnaces: BTreeMap<[i32; 3], Furnace>,
    pub clock_ms: u64,
    pub revision: u64,
    fraction_ms: f64,
    queue: VecDeque<[i32; 3]>,
    queued: BTreeSet<[i32; 3]>,
}
impl Machines {
    pub fn at_clock(clock_ms: u64) -> Self {
        Self {
            clock_ms,
            ..Default::default()
        }
    }
    pub fn advance_seconds(&mut self, seconds: f32, r: &Recipes) {
        self.fraction_ms += f64::from(seconds) * 1000.;
        let elapsed = self.fraction_ms.floor() as u64;
        self.fraction_ms -= elapsed as f64;
        self.advance(elapsed, r);
    }
    pub fn ensure(&mut self, p: [i32; 3]) -> Result<()> {
        if !self.furnaces.contains_key(&p) {
            if self.furnaces.len() >= MAX_MACHINES {
                return Err("World machine budget reached".into());
            }
            self.furnaces.insert(
                p,
                Furnace {
                    updated_ms: self.clock_ms,
                    ..Default::default()
                },
            );
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(())
    }
    pub fn wake(&mut self, p: [i32; 3], r: &Recipes) {
        if self.furnaces.get(&p).is_some_and(|f| f.active(r)) && self.queued.insert(p) {
            self.queue.push_back(p);
        }
    }
    pub fn refresh(&mut self, p: [i32; 3], r: &Recipes) {
        if let Some(f) = self.furnaces.get_mut(&p) {
            let old = f.clone();
            f.advance(self.clock_ms, r);
            if *f != old {
                self.revision = self.revision.wrapping_add(1);
            }
        }
    }
    pub fn advance(&mut self, elapsed_ms: u64, r: &Recipes) {
        self.clock_ms = self.clock_ms.saturating_add(elapsed_ms);
        for _ in 0..self.queue.len().min(UPDATE_BUDGET) {
            let p = self.queue.pop_front().unwrap();
            self.queued.remove(&p);
            self.refresh(p, r);
            self.wake(p, r);
        }
    }
    pub fn rehydrate(&mut self, r: &Recipes) {
        self.queue.clear();
        self.queued.clear();
        for p in self.furnaces.keys().copied().collect::<Vec<_>>() {
            self.wake(p, r);
        }
    }
    pub fn changed(&self, old: &Self) -> usize {
        self.furnaces
            .iter()
            .filter(|(p, f)| old.furnaces.get(*p) != Some(*f))
            .count()
            + old
                .furnaces
                .keys()
                .filter(|p| !self.furnaces.contains_key(*p))
                .count()
    }
    pub fn remove(&mut self, p: [i32; 3]) -> Result<()> {
        if self
            .furnaces
            .get(&p)
            .is_some_and(|f| f.slots.iter().any(Option::is_some))
        {
            return Err("Empty the furnace before breaking or replacing it".into());
        }
        if self.furnaces.remove(&p).is_some() {
            self.queue.retain(|q| *q != p);
            self.queued.remove(&p);
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine_blocks::Registry, engine_inventory::Stack};
    fn fixture() -> (Registry, Recipes, Furnace) {
        let r = Registry::built_in();
        let recipes = recipes::Definitions::built_in().compile(&r).unwrap();
        let f = Furnace {
            slots: [
                Some(Stack {
                    id: r.find("sand").unwrap(),
                    count: 6,
                }),
                Some(Stack {
                    id: r.find("oak_planks").unwrap(),
                    count: 4,
                }),
                None,
            ],
            ..Default::default()
        };
        (r, recipes, f)
    }
    #[test]
    fn exact_fuel_boundaries_match_small_steps_and_long_catchup() {
        let (r, recipes, mut fine) = fixture();
        let mut coarse = fine.clone();
        for i in 1..=1200 {
            fine.advance(i * 50, &recipes);
        }
        coarse.advance(60000, &recipes);
        assert_eq!(fine, coarse);
        assert!(fine.slots[0].is_none() && fine.slots[1].is_none());
        assert_eq!(
            fine.slots[2],
            Some(Stack {
                id: r.find("glass").unwrap(),
                count: 6
            })
        );
        assert_eq!(fine.burn_ms, 0);
    }
    #[test]
    fn blocked_outputs_and_invalid_inputs_do_not_consume_new_fuel() {
        let (r, recipes, mut f) = fixture();
        f.slots[2] = Some(Stack {
            id: r.find("glass").unwrap(),
            count: 64,
        });
        let slots = f.slots;
        f.advance(1000000, &recipes);
        assert_eq!(f.slots, slots);
        assert_eq!(f.cook_ms, 0);
        f.slots[2] = Some(Stack {
            id: r.find("slate").unwrap(),
            count: 1,
        });
        f.advance(2000000, &recipes);
        assert_eq!(f.slots[0], slots[0]);
        assert_eq!(f.slots[1], slots[1]);
        f.slots[2] = None;
        f.advance(2004000, &recipes);
        assert_eq!(f.cook_ms, 4000);
        let fuel = f.slots[1];
        f.slots[2] = Some(Stack {
            id: r.find("slate").unwrap(),
            count: 64,
        });
        f.advance(2009000, &recipes);
        assert_eq!(f.cook_ms, 0);
        assert_eq!(f.burn_ms, 6000);
        assert_eq!(f.slots[1], fuel);
        f.slots[0] = Some(Stack {
            id: r.find("cobblestone").unwrap(),
            count: 1,
        });
        f.slots[2] = None;
        f.advance(2010000, &recipes);
        assert_eq!(f.cook_ms, 1000);
    }
    #[test]
    fn scheduler_is_bounded_fair_and_new_inputs_do_not_smelt_during_idle_time() {
        let (_, recipes, f) = fixture();
        let mut m = Machines::default();
        for x in 0..200 {
            let p = [x, 4, 0];
            m.ensure(p).unwrap();
            m.furnaces.insert(p, f.clone());
            m.wake(p, &recipes);
        }
        m.advance(10000, &recipes);
        assert_eq!(
            m.furnaces.values().filter(|f| f.slots[2].is_some()).count(),
            UPDATE_BUDGET
        );
        for _ in 0..3 {
            m.advance(0, &recipes);
        }
        assert!(m.furnaces.values().all(|f| f.slots[2].is_some()));
        let p = [250, 4, 0];
        m.ensure(p).unwrap();
        m.advance(600000, &recipes);
        m.refresh(p, &recipes);
        let clock = m.clock_ms;
        let target = m.furnaces.get_mut(&p).unwrap();
        target.slots = f.slots;
        m.wake(p, &recipes);
        m.advance(1000, &recipes);
        for _ in 0..4 {
            m.advance(0, &recipes);
        }
        assert_eq!(m.furnaces[&p].cook_ms, 1000);
        assert_eq!(m.furnaces[&p].updated_ms, clock + 1000);
    }
    #[test]
    fn high_frame_rates_retain_fractional_simulation_time() {
        let (_, recipes, f) = fixture();
        let mut m = Machines::default();
        m.ensure([0, 4, 0]).unwrap();
        m.furnaces.insert([0, 4, 0], f);
        m.wake([0, 4, 0], &recipes);
        for _ in 0..20000 {
            m.advance_seconds(0.00025, &recipes);
        }
        assert!((4999..=5001).contains(&m.clock_ms));
        assert!((4999..=5001).contains(&m.furnaces[&[0, 4, 0]].cook_ms));
    }
    #[test]
    fn smelting_definitions_reject_duplicates_invalid_timers_and_unknown_items() {
        let r = Registry::built_in();
        let mut d = recipes::Definitions::built_in();
        d.smelting.push(d.smelting[0].clone());
        assert!(d.compile(&r).is_err());
        let mut d = recipes::Definitions::built_in();
        d.fuels[0].burn_ms = 0;
        assert!(d.compile(&r).is_err());
        let mut d = recipes::Definitions::built_in();
        d.smelting[0].result = "missing".into();
        assert!(d.compile(&r).is_err());
    }
}
