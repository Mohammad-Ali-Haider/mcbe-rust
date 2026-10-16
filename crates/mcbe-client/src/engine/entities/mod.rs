//! Persistent entities with typed optional components and bounded fixed-tick systems.
pub(crate) mod ai;
pub(crate) mod components;
pub(crate) mod definition;
pub(crate) mod interaction;
pub(crate) mod mesh;
pub(crate) mod model;
pub(crate) mod navigation;
pub(crate) mod simulation;
use crate::{engine_blocks::Registry, scene::Result};
use components::*;
use definition::{Definitions, Library};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
pub const MAX_ENTITIES: usize = 1024;
pub const ACTIVE_LIMIT: usize = 128;
pub const AI_BUDGET: usize = 24;
pub const PATH_BUDGET: usize = 4;
pub const ACTIVE_RADIUS: f32 = 64.;
#[derive(Clone, Default, Serialize)]
pub struct Statistics {
    pub ticks: u64,
    pub active: usize,
    pub moving: usize,
    pub sleeping: usize,
    pub physics_steps: usize,
    pub decisions: usize,
    pub paths: usize,
    pub path_nodes: usize,
    pub completed_paths: usize,
    pub spawned: usize,
    pub despawned: usize,
    pub damage_events: usize,
    pub revision: u64,
}
#[derive(Clone)]
pub struct World {
    pub library: Arc<Library>,
    pub records: BTreeMap<Id, Entity>,
    pub next_id: u64,
    pub clock: u64,
    pub random: u64,
    pub natural: bool,
    pub next_spawn: u64,
    pub cursor: usize,
    pub stats: Statistics,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Saved {
    pub definitions: Definitions,
    pub records: Vec<Entity>,
    pub next_id: u64,
    pub clock: u64,
    pub random: u64,
    pub next_spawn: u64,
}
impl World {
    pub fn new(r: &Registry) -> Self {
        Self {
            library: Arc::new(
                Definitions::built_in()
                    .available(r)
                    .compile(r)
                    .expect("Built-in entity materials"),
            ),
            records: BTreeMap::new(),
            next_id: 1,
            clock: 0,
            random: 0x9e3779b97f4a7c15,
            natural: false,
            next_spawn: 20,
            cursor: 0,
            stats: Statistics::default(),
        }
    }
    pub fn spawn(&mut self, kind: &str, position: [f32; 3], persistent: bool) -> Result<Id> {
        if self.records.len() >= MAX_ENTITIES
            || self.next_id == u64::MAX
            || !valid_position(position)
        {
            return Err("Entity population/position/ID budget exceeded".into());
        }
        let d = self.library.get(kind)?;
        let id = Id(self.next_id);
        self.next_id += 1;
        let entity = Entity {
            id,
            kind: kind.into(),
            persistent,
            transform: Transform {
                position,
                previous: position,
                yaw: 0.,
            },
            physics: d.body.as_ref().map(|b| Physics {
                width: b.width,
                height: b.height,
                speed: b.speed,
                velocity: [0.; 3],
                grounded: false,
            }),
            health: d.health.map(|h| Health {
                current: h,
                maximum: h,
                hurt_until: 0,
            }),
            brain: d.behavior.map(|behavior| Brain {
                behavior,
                goal: None,
                decide_at: 0,
                random: id.0.wrapping_mul(0x9e3779b97f4a7c15) ^ self.random,
                following: false,
                flee_until: 0,
                path: Vec::new(),
                replan_at: 0,
                activity: Activity::Idle,
            }),
            visual: (!d.model.is_empty()).then(|| Visual {
                model: kind.into(),
                gait: 0.,
            }),
        };
        self.records.insert(id, entity);
        self.stats.spawned += 1;
        self.stats.revision += 1;
        Ok(id)
    }
    pub fn despawn(&mut self, id: Id) -> bool {
        let removed = self.records.remove(&id).is_some();
        if removed {
            self.stats.despawned += 1;
            self.stats.revision += 1;
        }
        removed
    }
    pub fn damage(&mut self, id: Id, amount: u16) -> bool {
        let Some(e) = self.records.get_mut(&id) else {
            return false;
        };
        let Some(h) = &mut e.health else { return false };
        if amount == 0 || self.clock < h.hurt_until {
            return false;
        }
        h.current = h.current.saturating_sub(amount);
        h.hurt_until = self.clock.saturating_add(8);
        if let Some(b) = &mut e.brain {
            b.flee_until = self.clock.saturating_add(60);
            b.decide_at = 0;
            b.path.clear();
        }
        self.stats.damage_events += 1;
        self.stats.revision += 1;
        if h.current == 0 {
            self.despawn(id);
        }
        true
    }
    pub fn interact(&mut self, id: Id) -> bool {
        let Some(e) = self.records.get_mut(&id) else {
            return false;
        };
        let Some(b) = &mut e.brain else { return false };
        b.following = !b.following;
        b.decide_at = 0;
        b.path.clear();
        e.persistent = true;
        self.stats.revision += 1;
        true
    }
    pub fn saved(&self) -> Saved {
        let mut records = self.records.values().cloned().collect::<Vec<_>>();
        for e in &mut records {
            e.transform.previous = e.transform.position;
            if let Some(v) = &mut e.visual {
                v.gait = 0.;
            }
            if let Some(b) = &mut e.brain {
                b.path.clear();
                b.replan_at = 0;
                b.activity = Activity::Idle;
            }
        }
        Saved {
            definitions: self.library.definitions.clone(),
            records,
            next_id: self.next_id,
            clock: self.clock,
            random: self.random,
            next_spawn: self.next_spawn,
        }
    }
    pub fn restore(saved: Saved, r: &Registry) -> Result<Self> {
        if saved.records.len() > MAX_ENTITIES
            || saved.next_id == 0
            || saved.clock > u64::MAX - 10000
            || saved.next_spawn > saved.clock.saturating_add(10000)
        {
            return Err("Saved entity budgets/clock invalid".into());
        }
        let library = Arc::new(saved.definitions.clone().compile(r)?);
        let mut records = BTreeMap::new();
        for mut e in saved.records {
            let d = library.get(&e.kind)?;
            if e.id.0 == 0
                || e.id.0 >= saved.next_id
                || !valid_position(e.transform.position)
                || !e.transform.yaw.is_finite()
                || e.transform.yaw.abs() > std::f32::consts::TAU
                || records.contains_key(&e.id)
            {
                return Err("Saved entity identity/transform invalid".into());
            }
            if let Some(p) = &e.physics {
                let b = d.body.as_ref().ok_or("Unexpected entity physics")?;
                if p.width != b.width
                    || p.height != b.height
                    || p.speed != b.speed
                    || !p
                        .velocity
                        .into_iter()
                        .all(|v| v.is_finite() && v.abs() <= 4.)
                {
                    return Err("Saved entity physics invalid".into());
                }
            } else if d.body.is_some() {
                return Err("Entity physics missing".into());
            }
            if let Some(h) = &e.health {
                if Some(h.maximum) != d.health
                    || h.current == 0
                    || h.current > h.maximum
                    || h.hurt_until > saved.clock + 100
                {
                    return Err("Saved entity health invalid".into());
                }
            } else if d.health.is_some() {
                return Err("Entity health missing".into());
            }
            if let Some(b) = &mut e.brain {
                if Some(b.behavior) != d.behavior
                    || b.goal.is_some_and(|p| !valid_position(p))
                    || b.decide_at > saved.clock + 1000
                    || b.flee_until > saved.clock + 1000
                {
                    return Err("Saved entity brain invalid".into());
                }
                b.path.clear();
                b.activity = Activity::Idle;
            } else if d.behavior.is_some() {
                return Err("Entity brain missing".into());
            }
            if e.visual.as_ref().is_some_and(|v| v.model != e.kind)
                || e.visual.is_some() == d.model.is_empty()
            {
                return Err("Saved entity visual invalid".into());
            }
            e.transform.previous = e.transform.position;
            records.insert(e.id, e);
        }
        Ok(Self {
            library,
            records,
            next_id: saved.next_id,
            clock: saved.clock,
            random: saved.random,
            natural: false,
            next_spawn: saved.next_spawn,
            cursor: 0,
            stats: Statistics::default(),
        })
    }
    pub fn reconfigured(&self, library: Library) -> Result<Self> {
        let mut next = self.clone();
        next.library = Arc::new(library);
        for e in next.records.values_mut() {
            let d = next.library.get(&e.kind)?;
            e.physics = d.body.as_ref().map(|b| Physics {
                width: b.width,
                height: b.height,
                speed: b.speed,
                velocity: e.physics.as_ref().map_or([0.; 3], |p| p.velocity),
                grounded: e.physics.as_ref().is_some_and(|p| p.grounded),
            });
            e.health = d.health.map(|h| Health {
                maximum: h,
                current: e.health.as_ref().map_or(h, |old| old.current.min(h)),
                hurt_until: e.health.as_ref().map_or(0, |old| old.hurt_until),
            });
            e.brain = d.behavior.map(|behavior| {
                if let Some(old) = &e.brain
                    && old.behavior == behavior
                {
                    let mut brain = old.clone();
                    brain.path.clear();
                    brain.replan_at = 0;
                    return brain;
                }
                Brain {
                    behavior,
                    goal: None,
                    decide_at: 0,
                    random: e.brain.as_ref().map_or(e.id.0, |b| b.random),
                    following: e.brain.as_ref().is_some_and(|b| b.following),
                    flee_until: 0,
                    path: Vec::new(),
                    replan_at: 0,
                    activity: Activity::Idle,
                }
            });
            e.visual = (!d.model.is_empty()).then(|| Visual {
                model: e.kind.clone(),
                gait: e.visual.as_ref().map_or(0., |v| v.gait),
            });
        }
        if next.saved() != self.saved() {
            next.stats.revision += 1;
        }
        Ok(next)
    }
    pub fn observation(&self) -> serde_json::Value {
        serde_json::json!({"count":self.records.len(),"clock":self.clock,"natural_spawning":self.natural,"statistics":self.stats,"limits":{"entities":MAX_ENTITIES,"active":ACTIVE_LIMIT,"ai_decisions":AI_BUDGET,"path_plans":PATH_BUDGET,"radius":ACTIVE_RADIUS},"entities":self.records.values().collect::<Vec<_>>()})
    }
}
pub fn valid_position(p: [f32; 3]) -> bool {
    p.into_iter()
        .all(|v| v.is_finite() && v.abs() < 2_000_000_000.)
}
pub fn random(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_add(0x9e3779b97f4a7c15);
    let mut v = *seed;
    v = (v ^ (v >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    v = (v ^ (v >> 27)).wrapping_mul(0x94d049bb133111eb);
    v ^ (v >> 31)
}

#[cfg(test)]
mod tests;
