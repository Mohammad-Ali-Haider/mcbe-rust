//! Fixed-tick population, AI and collision scheduling; distant entities sleep.
use super::{components::*, navigation, *};
use crate::{
    engine_player,
    scene::{Client, Result},
};
use mcbe_core::motion::{Body, SolidBox, Vec3};
use std::collections::BTreeMap;
#[derive(Clone)]
struct Collider {
    id: Id,
    p: [f32; 3],
    body: Body,
}
struct Spatial {
    cells: BTreeMap<[i32; 3], Vec<Id>>,
    bodies: BTreeMap<Id, Collider>,
}
fn cell(p: [f32; 3]) -> [i32; 3] {
    p.map(|v| (v / 4.).floor() as i32)
}
impl Spatial {
    fn new(w: &World, c: &Client) -> Self {
        let mut s = Self {
            cells: BTreeMap::new(),
            bodies: BTreeMap::new(),
        };
        for e in w.records.values() {
            if let Some(p) = &e.physics {
                s.put(Collider {
                    id: e.id,
                    p: e.transform.position,
                    body: Body {
                        width: p.width,
                        height: p.height,
                    },
                });
            }
        }
        if c.movement.mode != engine_player::Mode::Noclip {
            s.put(Collider {
                id: Id(0),
                p: position(c.actor.position),
                body: c.actor.body,
            });
        }
        s
    }
    fn put(&mut self, b: Collider) {
        if let Some(old) = self.bodies.insert(b.id, b.clone())
            && let Some(v) = self.cells.get_mut(&cell(old.p))
        {
            v.retain(|id| *id != b.id);
        }
        self.cells.entry(cell(b.p)).or_default().push(b.id);
    }
    fn append(&self, id: Id, p: [f32; 3], scene: &mut mcbe_core::motion::SolidScene) -> Result<()> {
        let c = cell(p);
        for x in -1..=1 {
            for y in -2..=2 {
                for z in -1..=1 {
                    if let Some(ids) = self.cells.get(&[c[0] + x, c[1] + y, c[2] + z]) {
                        for other in ids {
                            if *other == id {
                                continue;
                            }
                            let b = &self.bodies[other];
                            let half = b.body.width * 0.5;
                            scene.boxes.push(SolidBox::new(
                                Vec3::new(b.p[0] - half, b.p[1], b.p[2] - half),
                                Vec3::new(b.p[0] + half, b.p[1] + b.body.height, b.p[2] + half),
                            )?);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
fn ambient(c: &Client, w: &mut World) -> Result<()> {
    if !w.natural || w.clock < w.next_spawn {
        return Ok(());
    }
    w.next_spawn = w.clock + 20;
    let player = position(c.actor.position);
    if let Some(id) = w
        .records
        .values()
        .find(|e| !e.persistent && distance(e.transform.position, player) > 128.)
        .map(|e| e.id)
    {
        w.despawn(id);
    }
    if w.records
        .values()
        .filter(|e| distance(e.transform.position, player) < 48.)
        .count()
        >= 24
        || w.records.len() >= MAX_ENTITIES
    {
        return Ok(());
    }
    let total = w
        .library
        .definitions
        .kinds
        .iter()
        .map(|d| u32::from(d.ambient_weight))
        .sum::<u32>();
    if total == 0 {
        return Ok(());
    }
    let pick = (random(&mut w.random) % u64::from(total)) as u32;
    let mut at = 0;
    let kind = w
        .library
        .definitions
        .kinds
        .iter()
        .find(|d| {
            at += u32::from(d.ambient_weight);
            pick < at
        })
        .unwrap()
        .clone();
    let Some(body) = &kind.body else {
        return Ok(());
    };
    let physics = Physics {
        width: body.width,
        height: body.height,
        speed: body.speed,
        velocity: [0.; 3],
        grounded: false,
    };
    for _ in 0..4 {
        let angle = (random(&mut w.random) % 65536) as f32 / 65536. * std::f32::consts::TAU;
        let radius = 12. + (random(&mut w.random) % 16) as f32;
        let x = (player[0] + angle.sin() * radius).floor() + 0.5;
        let z = (player[2] + angle.cos() * radius).floor() + 0.5;
        let y = c
            .scene
            .engine
            .as_ref()
            .unwrap()
            .stream
            .as_ref()
            .map_or(player[1], |s| {
                s.generator.height(x as i32, z as i32) as f32 + 1.
            });
        let Some(p) = navigation::surface(c, x, z, y, &physics) else {
            continue;
        };
        if (p[1] - player[1]).abs() > 24.
            || w.records
                .values()
                .any(|e| distance(e.transform.position, p) < 2.)
        {
            continue;
        }
        w.spawn(&kind.id, p, false)?;
        break;
    }
    Ok(())
}
fn update(c: &Client, w: &mut World) -> Result<()> {
    if w.records.is_empty() && !w.natural {
        return Ok(());
    }
    w.clock = w.clock.saturating_add(1);
    w.stats.ticks += 1;
    ambient(c, w)?;
    let player = position(c.actor.position);
    let keys = w.records.keys().copied().collect::<Vec<_>>();
    let mut active = Vec::new();
    for i in 0..keys.len() {
        let id = keys[(w.cursor + i) % keys.len()];
        let e = w.records.get_mut(&id).unwrap();
        e.transform.previous = e.transform.position;
        if distance(e.transform.position, player) <= ACTIVE_RADIUS
            && c.scene
                .inside(e.transform.position.map(|v| v.floor() as i32))
            && active.len() < ACTIVE_LIMIT
        {
            active.push(id);
        } else if let Some(b) = &mut e.brain {
            b.activity = Activity::Sleeping;
        }
    }
    w.cursor = if keys.is_empty() {
        0
    } else {
        (w.cursor + AI_BUDGET) % keys.len()
    };
    w.stats.active = active.len();
    w.stats.sleeping = keys.len() - active.len();
    w.stats.moving = 0;
    w.stats.physics_steps = 0;
    w.stats.decisions = 0;
    w.stats.paths = 0;
    w.stats.path_nodes = 0;
    let mut spatial = Spatial::new(w, c);
    for id in active {
        let e = w.records.get_mut(&id).unwrap();
        let d = w.library.get(&e.kind)?;
        if let Some(b) = &mut e.brain
            && w.clock >= b.decide_at
            && w.stats.decisions < AI_BUDGET
        {
            ai::decide(
                b,
                &ai::Context {
                    tick: w.clock,
                    position: e.transform.position,
                    player,
                    sense: d.sense,
                },
            );
            w.stats.decisions += 1;
        }
        let Some(physics) = &mut e.physics else {
            continue;
        };
        let old = e.transform.position;
        let mut target = None;
        if let Some(b) = &mut e.brain {
            if let Some(goal) = b.goal {
                if !b.path.is_empty() && distance(old, b.path[0]) < 0.35 {
                    b.path.remove(0);
                }
                if (b.path.is_empty() || w.clock >= b.replan_at) && w.stats.paths < PATH_BUDGET {
                    let path = navigation::plan(c, old, goal, physics);
                    w.stats.paths += 1;
                    w.stats.path_nodes += path.visited;
                    w.stats.completed_paths += usize::from(path.complete);
                    b.path = path.points;
                    b.replan_at = w.clock + 20;
                    if b.path.is_empty() {
                        b.activity = Activity::Blocked;
                    }
                }
                target = b.path.first().copied();
            } else {
                b.path.clear();
            }
        }
        let mut actor = physics.actor(old)?;
        if let Some(target) = target {
            let x = target[0] - old[0];
            let z = target[2] - old[2];
            let len = (x * x + z * z).sqrt();
            if len > 0.05 {
                let speed = physics.speed * 0.05;
                actor.velocity.x = x / len * speed;
                actor.velocity.z = z / len * speed;
                e.transform.yaw = x.atan2(z);
            }
            if target[1] > old[1] + 0.45 && actor.on_ground {
                actor.velocity.y = 0.42;
            }
        } else {
            actor.velocity.x = 0.;
            actor.velocity.z = 0.;
        }
        let mut scene = c.scene.motion_scene(&actor)?;
        spatial.append(id, old, &mut scene)?;
        let actor = actor.tick_solid(&scene)?;
        let next = position(actor.position);
        w.stats.moving += usize::from(next != old);
        e.transform.previous = old;
        e.transform.position = next;
        physics.velocity = position(actor.velocity).map(|v| v.clamp(-4., 4.));
        physics.grounded = actor.on_ground;
        if let Some(v) = &mut e.visual {
            v.gait = (v.gait + distance(old, next) * 7.).rem_euclid(std::f32::consts::TAU);
        }
        spatial.put(Collider {
            id,
            p: next,
            body: actor.body,
        });
        w.stats.physics_steps += 1;
    }
    w.stats.revision = w.stats.revision.wrapping_add(1);
    Ok(())
}
pub fn tick(c: &mut Client) -> Result<()> {
    let Some(s) = &mut c.scene.engine else {
        return Ok(());
    };
    let placeholder = World {
        library: s.entities.library.clone(),
        records: BTreeMap::new(),
        next_id: 1,
        clock: 0,
        random: 1,
        natural: false,
        next_spawn: 20,
        cursor: 0,
        stats: Statistics::default(),
    };
    let mut world = std::mem::replace(&mut s.entities, placeholder);
    let result = update(c, &mut world);
    c.scene.engine.as_mut().unwrap().entities = world;
    result
}
pub fn append_colliders(
    w: &World,
    actor: &mcbe_core::motion::Actor,
    scene: &mut mcbe_core::motion::SolidScene,
) -> Result<()> {
    let p = position(actor.position);
    for e in w.records.values() {
        if distance(e.transform.position, p) > 6. {
            continue;
        }
        if let Some(b) = &e.physics {
            let q = e.transform.position;
            let half = b.width * 0.5;
            scene.boxes.push(SolidBox::new(
                Vec3::new(q[0] - half, q[1], q[2] - half),
                Vec3::new(q[0] + half, q[1] + b.height, q[2] + half),
            )?);
        }
    }
    Ok(())
}
