//! Conservative voxel fluid amounts, sparse persistent overrides and bounded work.
pub(crate) mod transport;
use crate::{
    engine::WorldState,
    engine_blocks::{BlockId, Registry},
    scene::{Result, Scene},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
pub type Position = [i32; 3];
pub const FULL: u16 = 1024;
pub const MAX_AMOUNTS: usize = 200_000;
pub const MAX_QUEUE: usize = 65_536;
pub const STEP_BUDGET: usize = 256;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedAmount {
    pub position: Position,
    pub amount: u16,
}
#[derive(Clone, Default, Serialize)]
pub struct Counters {
    pub last_step_ms: f64,
    pub max_step_ms: f64,
    pub processed: u64,
    pub transfers: u64,
    pub last_processed: usize,
    pub peak_queue: usize,
    pub deferred_steps: u64,
}
#[derive(Clone, Default)]
pub struct Fluids {
    pub amounts: BTreeMap<Position, u16>,
    tick: u64,
    currents: BTreeMap<Position, (u64, [f32; 3])>,
    expiry: VecDeque<(u64, Position)>,
    pub original: BTreeMap<Position, u16>,
    queue: VecDeque<Position>,
    queued: BTreeSet<Position>,
    deferred: BTreeSet<Position>,
    pub counters: Counters,
}
pub fn kind(
    r: &Registry,
    id: Option<BlockId>,
) -> Option<crate::engine::blocks::components::FluidKind> {
    id.and_then(|id| r.get(id).fluid)
}
pub fn block(state: &WorldState, p: Position) -> Option<BlockId> {
    if state.stream.as_ref().is_some_and(|s| !s.ready(p)) {
        state.stream.as_ref().unwrap().observed(p)
    } else {
        state.blocks.get(&p).copied()
    }
}
pub fn editable(scene: &Scene, p: Position) -> bool {
    scene.inside(p)
        && !(scene
            .engine
            .as_ref()
            .and_then(|s| s.stream.as_ref())
            .is_none_or(|s| s.generator.config.version == 1)
            && p[1] == crate::engine::BOUNDS.min[1])
}
pub fn amount(state: &WorldState, p: Position) -> u16 {
    if kind(&state.registry, block(state, p)).is_some() {
        state.fluids.amounts.get(&p).copied().unwrap_or(FULL)
    } else {
        0
    }
}
pub fn neighbors(p: Position) -> impl Iterator<Item = Position> {
    [
        [0, 0, 0],
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
    ]
    .into_iter()
    .map(move |d| std::array::from_fn(|i| p[i] + d[i]))
}
impl Fluids {
    pub fn begin_step(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        while self
            .expiry
            .front()
            .is_some_and(|(t, _)| self.tick.saturating_sub(*t) >= 4)
        {
            let (t, p) = self.expiry.pop_front().unwrap();
            if self.currents.get(&p).is_some_and(|(time, _)| *time == t) {
                self.currents.remove(&p);
            }
        }
    }
    pub fn flux(&mut self, p: Position, direction: [f32; 3], amount: u16) {
        let entry = self.currents.entry(p).or_insert((self.tick, [0.; 3]));
        if entry.0 != self.tick {
            entry.0 = self.tick;
            entry.1 = [0.; 3];
            self.expiry.push_back((self.tick, p));
        } else if entry.1 == [0.; 3] {
            self.expiry.push_back((self.tick, p));
        }
        for (i, d) in direction.iter().enumerate() {
            entry.1[i] = (entry.1[i] + d * amount as f32 / FULL as f32).clamp(-1., 1.);
        }
    }
    pub fn current(&self, p: Position) -> [f32; 3] {
        self.currents.get(&p).map_or([0.; 3], |(time, v)| {
            let f = (1. - self.tick.saturating_sub(*time) as f32 / 4.).max(0.);
            v.map(|n| n * f)
        })
    }
    pub fn pending(&self) -> usize {
        self.amounts
            .iter()
            .filter(|(p, n)| self.original.get(*p) != Some(*n))
            .count()
            + self
                .original
                .keys()
                .filter(|p| !self.amounts.contains_key(*p))
                .count()
    }
    pub fn wake(&mut self, p: Position) {
        for q in neighbors(p) {
            if self.queued.contains(&q) {
                continue;
            }
            if self.queue.len() < MAX_QUEUE {
                self.queued.insert(q);
                self.queue.push_back(q);
            } else {
                self.deferred.insert(crate::engine::world::storage::key(q));
            }
        }
        self.counters.peak_queue = self.counters.peak_queue.max(self.queue.len());
    }
    pub fn queued(&self) -> usize {
        self.queue.len()
    }
    pub fn take(&mut self) -> Option<Position> {
        let p = self.queue.pop_front()?;
        self.queued.remove(&p);
        Some(p)
    }
    pub fn evict(&mut self, key: Position) {
        self.queue
            .retain(|p| crate::engine::world::storage::key(*p) != key);
        self.queued
            .retain(|p| crate::engine::world::storage::key(*p) != key);
        self.deferred.remove(&key);
        self.currents
            .retain(|p, _| crate::engine::world::storage::key(*p) != key);
    }
    pub fn rescan(&mut self, keys: impl IntoIterator<Item = Position>) {
        self.queue.clear();
        self.queued.clear();
        self.currents.clear();
        self.expiry.clear();
        self.deferred = keys.into_iter().collect();
    }
    pub fn deferred(&mut self) -> Option<Position> {
        self.deferred.pop_first()
    }
}
/// Scan one inserted chunk and its boundary. Generator halos prevent false ocean leaks.
pub fn hydrate_chunk(state: &mut WorldState, key: Position) {
    let mut candidates = Vec::new();
    for x in -1..=16 {
        for y in -1..=16 {
            for z in -1..=16 {
                let p = [key[0] * 16 + x, key[1] * 16 + y, key[2] * 16 + z];
                if state.stream.as_ref().is_some_and(|s| !s.ready(p)) {
                    continue;
                }
                let Some(k) = kind(&state.registry, state.blocks.get(&p).copied()) else {
                    continue;
                };
                let a = amount(state, p);
                let down = [p[0], p[1] - 1, p[2]];
                let falling = block(state, down).is_none()
                    || (kind(&state.registry, block(state, down)) == Some(k)
                        && amount(state, down) < FULL);
                let side = [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]]
                    .iter()
                    .any(|d| {
                        let q = std::array::from_fn(|i| p[i] + d[i]);
                        let b = block(state, q);
                        (b.is_none() || kind(&state.registry, b) == Some(k))
                            && a > amount(state, q) + 1
                    });
                if falling || side {
                    candidates.push(p);
                }
            }
        }
    }
    for p in candidates {
        state.fluids.wake(p);
    }
}
pub fn restore(scene: &mut Scene, saved: Option<Vec<SavedAmount>>, version: u32) -> Result<()> {
    if version < 8 && saved.is_some() {
        return Err("Fluid state requires save version 8".into());
    }
    let state = scene.engine.as_mut().ok_or("Fluid world missing")?;
    let entries = saved.unwrap_or_default();
    if entries.len() > MAX_AMOUNTS {
        return Err("Fluid amount budget exceeded".into());
    }
    let legacy = state
        .stream
        .as_ref()
        .is_none_or(|s| s.generator.config.version == 1);
    let mut amounts = BTreeMap::new();
    for e in entries {
        if e.amount == 0
            || e.amount >= FULL
            || !scene.bounds.contains(e.position)
            || (legacy && e.position[1] == crate::engine::BOUNDS.min[1])
            || kind(&state.registry, block(state, e.position)).is_none()
            || amounts.insert(e.position, e.amount).is_some()
        {
            return Err("Duplicate, out-of-bounds or invalid fluid amount".into());
        }
    }
    state.fluids = Fluids {
        original: amounts.clone(),
        amounts,
        ..Default::default()
    };
    let keys = state.blocks.chunk_keys().collect::<Vec<_>>();
    for k in keys {
        hydrate_chunk(state, k);
    }
    Ok(())
}
impl Scene {
    pub fn set_fluid(&mut self, p: Position, id: BlockId, value: u16) -> Result<()> {
        if value > FULL || !editable(self, p) {
            return Err("Invalid fluid volume or unavailable position".into());
        }
        let state = self.engine.as_ref().ok_or("Fluid world missing")?;
        let k = kind(&state.registry, Some(id)).ok_or("Registered fluid required")?;
        let old = state.blocks.get(&p).copied();
        if old.is_some() && kind(&state.registry, old) != Some(k) {
            return Err("Fluid cannot replace solid material".into());
        }
        if value > 0
            && value < FULL
            && !state.fluids.amounts.contains_key(&p)
            && state.fluids.amounts.len() >= MAX_AMOUNTS
        {
            return Err("Fluid amount budget reached".into());
        }
        let previous = amount(state, p);
        if previous == value {
            return Ok(());
        }
        let next = (value > 0).then_some(id);
        if old != next {
            self.change_engine(p, next)?;
        }
        let state = self.engine.as_mut().unwrap();
        if value > 0 && value < FULL {
            state.fluids.amounts.insert(p, value);
        } else {
            state.fluids.amounts.remove(&p);
        }
        if old == next {
            mark_changed(state, p);
        }
        state.fluids.wake(p);
        Ok(())
    }
}
pub fn changed(state: &mut WorldState, p: Position, old: Option<BlockId>, next: Option<BlockId>) {
    if kind(&state.registry, old).is_none()
        || kind(&state.registry, old) != kind(&state.registry, next)
    {
        state.fluids.amounts.remove(&p);
    }
    if kind(&state.registry, old).is_some() || kind(&state.registry, next).is_some() {
        mark_changed(state, p);
    }
    if kind(&state.registry, old).is_some()
        || kind(&state.registry, next).is_some()
        || neighbors(p).any(|q| kind(&state.registry, block(state, q)).is_some())
    {
        state.fluids.wake(p);
    }
}
pub fn mark_changed(state: &mut WorldState, p: Position) {
    state.revision = state.revision.wrapping_add(1);
    state.edit_revision = state.edit_revision.wrapping_add(1);
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                let k = crate::engine::world::storage::key([p[0] + x, p[1] + y, p[2] + z]);
                if state.stream.as_ref().is_none_or(|s| s.ready_chunk(k)) {
                    state.dirty_chunks.insert(k, state.revision);
                }
            }
        }
    }
}

pub fn observation(c: &crate::scene::Client) -> serde_json::Value {
    let Some(s) = &c.scene.engine else {
        return serde_json::Value::Null;
    };
    let e = crate::engine::player::locomotion::Environment::sample(c);
    serde_json::json!({"queued":s.fluids.queued(),"overrides":s.fluids.amounts.len(),"counters":s.fluids.counters,"immersion":e.immersion,"head_underwater":e.head_underwater,"current":e.current,"swimming":c.movement.swimming})
}

#[cfg(test)]
mod tests;
