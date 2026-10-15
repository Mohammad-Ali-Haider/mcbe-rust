//! Deterministic, budgeted neighbor events and scheduled ticks for active blocks.
//! Ordinary terrain incurs no scans or per-cell behavior allocations.
use super::super::{
    BlockId, Registry,
    behavior::{self, BlockEvent, Command, EventContext, Position},
};
use crate::engine::world::storage::Store;
use crate::scene::{Client, Result};
use std::collections::BTreeMap;
const BUDGET: usize = 512;
const QUEUE_LIMIT: usize = 65536;
#[derive(Clone, Copy, Debug)]
struct Pending {
    expected: BlockId,
    event: BlockEvent,
}
#[derive(Clone, Default)]
pub struct Simulation {
    enabled: bool,
    clock: u64,
    active: BTreeMap<Position, BlockId>,
    pending: BTreeMap<Position, Pending>,
    scheduled: BTreeMap<(u64, Position), BlockId>,
    deadlines: BTreeMap<Position, (u64, BlockId)>,
    rescan: bool,
    cursor: Option<Position>,
}
impl Simulation {
    pub fn new(registry: &Registry) -> Self {
        Self {
            enabled: registry.blocks.iter().any(|b| b.behavior.receives_events()),
            ..Self::default()
        }
    }
    fn enqueue(&mut self, p: Position, event: BlockEvent) {
        let Some(&expected) = self.active.get(&p) else {
            return;
        };
        if self.pending.len() >= QUEUE_LIMIT && !self.pending.contains_key(&p) {
            // Resume a bounded sweep later; never permanently lose overflow work.
            self.rescan = true;
            self.cursor = self.cursor.filter(|c| *c < p);
            return;
        }
        self.pending
            .entry(p)
            .and_modify(|v| {
                if v.expected != expected || event == BlockEvent::ScheduledTick {
                    *v = Pending { expected, event };
                }
            })
            .or_insert(Pending { expected, event });
    }
    fn cancel(&mut self, p: Position) {
        if let Some((deadline, _)) = self.deadlines.remove(&p) {
            self.scheduled.remove(&(deadline, p));
        }
        self.pending.remove(&p);
    }
    pub fn changed(&mut self, registry: &Registry, p: Position, id: Option<BlockId>) {
        if !self.enabled {
            return;
        }
        self.cancel(p);
        self.active.remove(&p);
        if let Some(id) = id.filter(|id| registry.get(*id).behavior.receives_events()) {
            self.active.insert(p, id);
            self.enqueue(p, BlockEvent::Placed);
        }
        for n in behavior::neighbors(p) {
            self.enqueue(n, BlockEvent::NeighborChanged);
        }
    }
    pub fn evict_chunk(&mut self, registry: &Registry, key: [i32; 3]) {
        if !self.enabled {
            return;
        }
        let lo = [key[0] * 16, i32::MIN, i32::MIN];
        let hi = [key[0] * 16 + 16, i32::MIN, i32::MIN];
        let positions = self
            .active
            .range(lo..hi)
            .map(|(p, _)| *p)
            .filter(|p| crate::engine::world::storage::key(*p) == key)
            .collect::<Vec<_>>();
        for p in positions {
            self.changed(registry, p, None);
        }
    }
    /// Seed only on load/registry replacement or newly resident columns.
    pub fn hydrate(
        &mut self,
        registry: &Registry,
        blocks: impl IntoIterator<Item = (Position, BlockId)>,
    ) {
        if !self.enabled {
            return;
        }
        for (p, id) in blocks {
            if registry.get(id).signal.is_some() || registry.get(id).behavior.receives_events() {
                self.changed(registry, p, Some(id));
            }
        }
    }
    fn schedule(&mut self, p: Position, expected: BlockId, delay: u16) {
        if self.active.get(&p) != Some(&expected) {
            return;
        }
        let due = self.clock.saturating_add(u64::from(delay));
        if self
            .deadlines
            .get(&p)
            .is_some_and(|(old, id)| *id == expected && *old <= due)
        {
            return;
        }
        if let Some((old, _)) = self.deadlines.insert(p, (due, expected)) {
            self.scheduled.remove(&(old, p));
        }
        self.scheduled.insert((due, p), expected);
    }
    fn refill(&mut self) {
        if !self.rescan || self.pending.len() == QUEUE_LIMIT {
            return;
        }
        let positions: Vec<_> = self
            .active
            .range((
                self.cursor
                    .map_or(std::ops::Bound::Unbounded, std::ops::Bound::Excluded),
                std::ops::Bound::Unbounded,
            ))
            .take((QUEUE_LIMIT - self.pending.len()).min(BUDGET))
            .map(|(p, _)| *p)
            .collect();
        if positions.is_empty() {
            self.rescan = false;
            self.cursor = None;
            return;
        }
        for p in positions {
            self.cursor = Some(p);
            self.enqueue(p, BlockEvent::NeighborChanged);
        }
    }
    pub fn advance(&mut self, registry: &Registry, blocks: &Store) -> Result<Vec<Command>> {
        if !self.enabled {
            return Ok(vec![]);
        }
        self.clock = self.clock.saturating_add(1);
        for _ in 0..BUDGET {
            let Some((&(due, p), &id)) = self.scheduled.first_key_value() else {
                break;
            };
            if due > self.clock {
                break;
            }
            self.scheduled.remove(&(due, p));
            self.deadlines.remove(&p);
            if blocks.get(&p) == Some(&id) {
                self.enqueue(p, BlockEvent::ScheduledTick);
            }
        }
        self.refill();
        let mut result = Vec::new();
        let mut commands = Vec::new();
        for _ in 0..BUDGET {
            let Some((p, pending)) = self.pending.pop_first() else {
                break;
            };
            if blocks.get(&p) != Some(&pending.expected) {
                continue;
            }
            let context = EventContext {
                registry,
                blocks,
                position: p,
                state: pending.expected,
            };
            behavior::get(registry, pending.expected).on_event(
                &context,
                pending.event,
                &mut commands,
            )?;
            for command in commands.drain(..) {
                match command {
                    Command::Schedule {
                        position,
                        expected,
                        delay,
                    } => self.schedule(position, expected, delay),
                    set @ Command::Set { .. } => result.push(set),
                }
            }
        }
        Ok(result)
    }
}
pub fn tick(client: &mut Client) -> Result<()> {
    let Some(state) = client.scene.engine.as_mut() else {
        return Ok(());
    };
    let commands = state.simulation.advance(&state.registry, &state.blocks)?;
    for command in commands {
        if let Command::Set {
            position,
            expected,
            state: id,
        } = command
        {
            let world = client.scene.engine.as_ref().unwrap();
            if client.scene.inside(position)
                && world.blocks.get(&position) == Some(&expected)
                && !super::collision::intersects_actor(&world.geometry, id, position, &client.actor)
            {
                client.scene.change_engine(position, Some(id))?;
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn apply(sim: &mut Simulation, r: &Registry, blocks: &mut Store) -> usize {
        let commands = sim.advance(r, blocks).unwrap();
        let count = commands.len();
        for c in commands {
            if let Command::Set {
                position,
                expected,
                state,
            } = c
                && blocks.get(&position) == Some(&expected)
            {
                blocks.insert(position, state);
                sim.changed(r, position, Some(state));
            }
        }
        count
    }
    #[test]
    fn signal_propagates_decays_and_rehydrates_without_recursion() {
        let r = crate::engine::content::loader::signal_example()
            .unwrap()
            .registry;
        let source = r.find("power_source").unwrap();
        let wire = r.find("wire_0").unwrap();
        let mut blocks = Store::from([
            ([0, 4, 0], source),
            ([1, 4, 0], wire),
            ([2, 4, 0], wire),
            ([3, 4, 0], wire),
        ]);
        let mut sim = Simulation::new(&r);
        sim.hydrate(&r, blocks.iter().map(|(p, id)| (p, *id)));
        for _ in 0..20 {
            apply(&mut sim, &r, &mut blocks);
        }
        for x in 1..=3 {
            assert_eq!(
                behavior::wire::strength(r.get(blocks[&[x, 4, 0]])),
                15 - x as u8
            );
        }
        // Rebuilding after save/residency must re-evaluate persisted state IDs.
        let bytes = serde_json::to_vec(&r).unwrap();
        let r = Registry::parse(&bytes).unwrap();
        blocks.remove(&[0, 4, 0]);
        sim = Simulation::new(&r);
        sim.hydrate(&r, blocks.iter().map(|(p, id)| (p, *id)));
        for _ in 0..100 {
            apply(&mut sim, &r, &mut blocks);
        }
        assert!(
            blocks
                .values()
                .all(|id| behavior::wire::strength(r.get(*id)) == 0)
        );
        assert!(sim.pending.is_empty() && sim.scheduled.is_empty());
    }
    #[test]
    fn scheduled_delay_is_coalesced_and_removed_blocks_cannot_mutate() {
        let r = crate::engine::content::loader::signal_example()
            .unwrap()
            .registry;
        let wire = r.find("wire_0").unwrap();
        let source = r.find("power_source").unwrap();
        let p = [1, 4, 0];
        let mut blocks = Store::from([(p, wire), ([0, 4, 0], source)]);
        let mut sim = Simulation::new(&r);
        sim.hydrate(&r, blocks.iter().map(|(p, id)| (p, *id)));
        assert_eq!(apply(&mut sim, &r, &mut blocks), 0);
        assert_eq!(sim.scheduled.len(), 1);
        for _ in 0..100 {
            sim.enqueue(p, BlockEvent::NeighborChanged);
        }
        assert_eq!(sim.pending.len(), 1);
        assert_eq!(apply(&mut sim, &r, &mut blocks), 0);
        assert_eq!(apply(&mut sim, &r, &mut blocks), 1);
        blocks.remove(&p);
        sim.changed(&r, p, None);
        for _ in 0..4 {
            assert_eq!(apply(&mut sim, &r, &mut blocks), 0);
        }
        assert!(sim.active.is_empty() && sim.scheduled.is_empty());
    }
    #[test]
    fn live_edits_save_load_and_stream_eviction_keep_signal_states_consistent() {
        let r = crate::engine::content::loader::signal_example()
            .unwrap()
            .registry;
        let source = r.find("power_source").unwrap();
        let wire = r.find("wire_0").unwrap();
        let mut client = crate::engine_stream::generate(360036, r.clone()).unwrap();
        client
            .scene
            .change_engine([0, 20, 0], Some(source))
            .unwrap();
        client.scene.change_engine([1, 20, 0], Some(wire)).unwrap();
        for _ in 0..8 {
            client.tick(Default::default()).unwrap();
        }
        let world = client.scene.engine.as_ref().unwrap();
        assert_eq!(
            behavior::wire::strength(r.get(world.blocks[&[1, 20, 0]])),
            14
        );
        assert!(!world.dirty_chunks.is_empty());
        let dir = std::env::temp_dir().join(format!("mre-signal-save-{}", std::process::id()));
        let path = crate::engine::save(&mut client, &dir).unwrap();
        let mut loaded = crate::engine::load(&path).unwrap();
        loaded.scene.change_engine([0, 20, 0], None).unwrap();
        for _ in 0..8 {
            tick(&mut loaded).unwrap();
        }
        assert_eq!(
            loaded.scene.engine.as_ref().unwrap().blocks[&[1, 20, 0]],
            wire
        );
        // A departed chunk drops queued ticks; returning seeds persisted edits.
        crate::engine_stream::jump(&mut client, 3).unwrap();
        assert!(
            client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .simulation
                .active
                .is_empty()
        );
        crate::engine_stream::jump(&mut client, 0).unwrap();
        assert!(
            client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .simulation
                .active
                .contains_key(&[1, 20, 0])
        );
        client.scene.change_engine([0, 20, 0], None).unwrap();
        for _ in 0..8 {
            tick(&mut client).unwrap();
        }
        assert_eq!(
            client.scene.engine.as_ref().unwrap().blocks[&[1, 20, 0]],
            wire
        );
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn budget_bounds_updates_and_overflow_sweep_retains_work() {
        let r = crate::engine::content::loader::signal_example()
            .unwrap()
            .registry;
        let id = r.find("wire_0").unwrap();
        let mut sim = Simulation::new(&r);
        // Exercise overflow without allocating a whole terrain region.
        for i in 0..QUEUE_LIMIT + 7 {
            sim.active.insert([i as i32, 4, 0], id);
            sim.enqueue([i as i32, 4, 0], BlockEvent::Placed);
        }
        assert_eq!(sim.pending.len(), QUEUE_LIMIT);
        assert!(sim.rescan);
        let mut blocks: Store = sim.active.iter().map(|(p, id)| (*p, *id)).collect();
        blocks.insert(
            [(QUEUE_LIMIT + 7) as i32, 4, 0],
            r.find("power_source").unwrap(),
        );
        sim.advance(&r, &blocks).unwrap();
        assert_eq!(sim.scheduled.len(), BUDGET);
        for _ in 0..300 {
            apply(&mut sim, &r, &mut blocks);
        }
        assert!(!sim.rescan);
        assert_eq!(
            behavior::wire::strength(r.get(blocks[&[(QUEUE_LIMIT + 6) as i32, 4, 0]])),
            14
        );
        assert!(sim.pending.len() <= QUEUE_LIMIT);
        // The normal catalog has zero active work even when cells change.
        let r = Registry::built_in();
        let mut idle = Simulation::new(&r);
        idle.changed(&r, [0, 4, 0], Some(0));
        assert!(idle.advance(&r, &Store::new()).unwrap().is_empty());
        assert!(idle.active.is_empty());
    }
}
