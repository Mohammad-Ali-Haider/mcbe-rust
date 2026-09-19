//! Versioned procedural terrain, bounded column residency, and sparse edits.
use crate::{
    engine::{self, WorldState},
    engine_blocks::{BlockId, Registry},
    scene::{Client, Result, Scene},
};
use mcbe_core::{motion::Vec3, streaming::Bounds};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Arc, mpsc},
    thread,
    time::Instant,
};
pub type Column = [i32; 2];
pub type Position = [i32; 3];
pub const BOUNDS: Bounds = Bounds {
    min: [-8192, 0, -8192],
    max: [8192, 64, 8192],
};
pub const MAX_EDITS: usize = 200_000;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub radius: i32,
    pub exhibits: Vec<String>,
}
impl Config {
    pub fn new(registry: &Registry) -> Self {
        Self {
            version: 1,
            radius: 3,
            exhibits: registry
                .blocks
                .iter()
                .take(15)
                .map(|b| b.id.clone())
                .collect(),
        }
    }
    fn validate(&self, registry: &Registry) -> Result<()> {
        if self.version != 1
            || !(2..=4).contains(&self.radius)
            || self.exhibits.len() > 15
            || self.exhibits.iter().collect::<BTreeSet<_>>().len() != self.exhibits.len()
        {
            return Err("Unsupported terrain generator configuration".into());
        }
        for id in &self.exhibits {
            registry.find(id)?;
        }
        Ok(())
    }
}
#[derive(Clone)]
pub struct Generator {
    pub config: Config,
    pub seed: u64,
    court: HashMap<Position, BlockId>,
    ids: [BlockId; 6],
}
pub fn column(p: Position) -> Column {
    [p[0].div_euclid(16), p[2].div_euclid(16)]
}
pub fn center(p: Vec3) -> Column {
    [
        (p.x.floor() as i32).div_euclid(16),
        (p.z.floor() as i32).div_euclid(16),
    ]
}
impl Generator {
    pub fn new(seed: u64, registry: &Registry, config: Config) -> Result<Self> {
        config.validate(registry)?;
        let ids = [
            registry.find("meadow")?,
            registry.find("earth")?,
            registry.find("slate")?,
            registry.find("basalt")?,
            registry.find("moss")?,
            registry
                .find("oak_log")
                .or_else(|_| registry.find("clay"))?,
        ];
        let mut court = engine::playground_blocks(seed, registry)?;
        // Exhibit positions depend on saved stable IDs, never the current registry order.
        let ivory = registry.find("ivory")?;
        for i in 0..15 {
            let x = -11 + (i % 8) as i32 * 3;
            let z = 14 + (i / 8) as i32 * 4;
            for y in 4..=7 {
                court.remove(&[x, y, z]);
            }
            if let Some(id) = config.exhibits.get(i) {
                let id = registry.find(id)?;
                for y in 4..=6 {
                    court.insert([x, y, z], id);
                }
                court.insert([x, 7, z], ivory);
            }
        }
        Ok(Self {
            config,
            seed,
            court,
            ids,
        })
    }
    pub fn height(&self, x: i32, z: i32) -> i32 {
        let gx = x.div_euclid(32);
        let gz = z.div_euclid(32);
        let u = x.rem_euclid(32);
        let v = z.rem_euclid(32);
        let h = |a, b| 8 + (engine::noise(a, b, self.seed) % 10) as i32;
        let smooth = (h(gx, gz) * (32 - u) * (32 - v)
            + h(gx + 1, gz) * u * (32 - v)
            + h(gx, gz + 1) * (32 - u) * v
            + h(gx + 1, gz + 1) * u * v)
            / 1024;
        let distance = (x - 39).max(-40 - x).max(z - 39).max(-40 - z).clamp(0, 24);
        let edge = 5 + (engine::noise(x.div_euclid(5), z.div_euclid(5), self.seed) % 5) as i32;
        (edge * (24 - distance) + smooth * distance) / 24
    }
    fn tree(&self, c: Column) -> Option<[i32; 3]> {
        let n = engine::noise(c[0], c[1], self.seed ^ 0x38c6f01c);
        if !n.is_multiple_of(3) {
            return None;
        }
        let x = c[0] * 16 + 4 + ((n >> 8) % 8) as i32;
        let z = c[1] * 16 + 4 + ((n >> 16) % 8) as i32;
        if (-43..=42).contains(&x) && (-43..=42).contains(&z) {
            return None;
        }
        Some([x, self.height(x, z), z])
    }
    pub fn block(&self, p: Position) -> Option<BlockId> {
        if !BOUNDS.contains(p) {
            return None;
        }
        let [x, y, z] = p;
        if (-40..40).contains(&x) && (-40..40).contains(&z) {
            return self.court.get(&p).copied();
        }
        let h = self.height(x, z);
        let mut value = if y == 0 {
            Some(self.ids[3])
        } else if y == h - 1 {
            Some(self.ids[0])
        } else if y >= h - 3 && y < h {
            Some(self.ids[1])
        } else if y < h {
            Some(self.ids[2])
        } else {
            None
        };
        if let Some([tx, ty, tz]) = self.tree(column(p)) {
            if x == tx && z == tz && (ty..ty + 4).contains(&y) {
                value = Some(self.ids[5]);
            }
            if (x - tx).abs() <= 2
                && (z - tz).abs() <= 2
                && (x - tx).abs() + (z - tz).abs() < 4
                && (ty + 2..=ty + 4).contains(&y)
            {
                value = Some(self.ids[4]);
            }
        }
        value
    }
    pub(crate) fn generate(&self, c: Column) -> HashMap<Position, BlockId> {
        let mut blocks = HashMap::new();
        for x in c[0] * 16..c[0] * 16 + 16 {
            for z in c[1] * 16..c[1] * 16 + 16 {
                for y in 0..24 {
                    let p = [x, y, z];
                    if let Some(id) = self.block(p) {
                        blocks.insert(p, id);
                    }
                }
            }
        }
        blocks
    }
}
#[derive(Clone)]
pub struct State {
    pub generator: Arc<Generator>,
    pub residents: BTreeMap<Column, Vec<Position>>,
    pub wanted: BTreeSet<Column>,
    pub center: Column,
    pub edits: BTreeMap<Position, Option<BlockId>>,
    pub original_edits: BTreeMap<Position, Option<BlockId>>,
    pub generated: usize,
    pub evicted: usize,
    pub discarded: usize,
    pub generation_ms: f64,
}
impl State {
    pub fn new(generator: Generator) -> Self {
        Self {
            generator: Arc::new(generator),
            residents: BTreeMap::new(),
            wanted: BTreeSet::new(),
            center: [i32::MIN; 2],
            edits: BTreeMap::new(),
            original_edits: BTreeMap::new(),
            generated: 0,
            evicted: 0,
            discarded: 0,
            generation_ms: 0.,
        }
    }
    pub fn pending(&self) -> usize {
        self.edits
            .iter()
            .filter(|(p, id)| self.original_edits.get(*p) != Some(*id))
            .count()
            + self
                .original_edits
                .keys()
                .filter(|p| !self.edits.contains_key(*p))
                .count()
    }
    pub fn record(&mut self, p: Position, value: Option<BlockId>) -> Result<()> {
        if self.generator.block(p) == value {
            self.edits.remove(&p);
        } else {
            if !self.edits.contains_key(&p) && self.edits.len() >= MAX_EDITS {
                return Err("Streamed edit budget reached".into());
            }
            self.edits.insert(p, value);
        }
        Ok(())
    }
    pub fn observed(&self, p: Position) -> Option<BlockId> {
        self.edits
            .get(&p)
            .copied()
            .unwrap_or_else(|| self.generator.block(p))
    }
}
enum Job {
    Generate(Column),
    Stop,
}
struct Generated {
    column: Column,
    blocks: HashMap<Position, BlockId>,
    milliseconds: f64,
}
pub struct Worker {
    sender: mpsc::Sender<Job>,
    receiver: mpsc::Receiver<Generated>,
    thread: Option<thread::JoinHandle<()>>,
    pending: BTreeSet<Column>,
}
impl Worker {
    pub fn new(generator: Arc<Generator>) -> Result<Self> {
        let (sender, jobs) = mpsc::channel();
        let (completed, receiver) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("blockwork-terrain".into())
            .spawn(move || {
                while let Ok(Job::Generate(column)) = jobs.recv() {
                    let started = Instant::now();
                    let blocks = generator.generate(column);
                    if completed
                        .send(Generated {
                            column,
                            blocks,
                            milliseconds: started.elapsed().as_secs_f64() * 1000.,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self {
            sender,
            receiver,
            thread: Some(thread),
            pending: BTreeSet::new(),
        })
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.sender.send(Job::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
pub(crate) fn wanted(c: Column, radius: i32) -> BTreeSet<Column> {
    let mut result = BTreeSet::new();
    for x in c[0] - radius..=c[0] + radius {
        for z in c[1] - radius..=c[1] + radius {
            if BOUNDS.contains([x * 16, 0, z * 16]) {
                result.insert([x, z]);
            }
        }
    }
    result
}
fn dirty(scene: &mut Scene, c: Column, ys: BTreeSet<i32>) {
    let state = scene.engine.as_mut().unwrap();
    state.revision = state.revision.wrapping_add(1);
    for y in ys {
        for [dx, dz] in [[0, 0], [1, 0], [-1, 0], [0, 1], [0, -1]] {
            state
                .dirty_chunks
                .insert([c[0] + dx, y, c[1] + dz], state.revision);
        }
    }
    let stream = state.stream.as_ref().unwrap();
    state
        .dirty_chunks
        .retain(|k, _| stream.residents.contains_key(&[k[0], k[2]]));
}
fn insert(scene: &mut Scene, generated: Generated) {
    let c = generated.column;
    let state = scene.engine.as_mut().unwrap();
    let stream = state.stream.as_mut().unwrap();
    if stream.residents.contains_key(&c) || !stream.wanted.contains(&c) {
        stream.discarded += 1;
        return;
    }
    let mut blocks = generated.blocks;
    for (p, id) in stream
        .edits
        .range([c[0] * 16, 0, i32::MIN]..[(c[0] + 1) * 16, 0, i32::MIN])
        .filter(|(p, _)| column(**p) == c)
    {
        if let Some(id) = id {
            blocks.insert(*p, *id);
        } else {
            blocks.remove(p);
        }
    }
    let positions: Vec<_> = blocks.keys().copied().collect();
    let ys = positions.iter().map(|p| p[1].div_euclid(16)).collect();
    scene.cells.extend(positions.iter().copied());
    state.blocks.extend(blocks);
    stream.residents.insert(c, positions);
    stream.generated += 1;
    stream.generation_ms += generated.milliseconds;
    dirty(scene, c, ys);
}
fn recenter(scene: &mut Scene, c: Column) {
    let state = scene.engine.as_mut().unwrap();
    let stream = state.stream.as_mut().unwrap();
    if stream.center == c {
        return;
    }
    stream.center = c;
    stream.wanted = wanted(c, stream.generator.config.radius);
    let evict: Vec<_> = stream
        .residents
        .keys()
        .filter(|c| !stream.wanted.contains(*c))
        .copied()
        .collect();
    for c in evict {
        let state = scene.engine.as_mut().unwrap();
        let stream = state.stream.as_mut().unwrap();
        let positions = stream.residents.remove(&c).unwrap();
        let ys = positions.iter().map(|p| p[1].div_euclid(16)).collect();
        for p in positions {
            state.blocks.remove(&p);
            scene.cells.remove(&p);
        }
        stream.evicted += 1;
        dirty(scene, c, ys);
    }
}
/// Teleports and save loading prepare the whole bounded destination before movement.
pub fn prepare(client: &mut Client, position: Vec3) -> Result<()> {
    let state = client.scene.engine.as_ref().ok_or("Engine missing")?;
    let stream = state.stream.as_ref().ok_or("Streamed world missing")?;
    if !position.finite() {
        return Err("Finite streamed position required".into());
    }
    let body = client.movement.body();
    let half = body.width * 0.5;
    let low = [position.x - half, position.y, position.z - half];
    let high = [
        position.x + half,
        position.y + body.height,
        position.z + half,
    ];
    if (0..3).any(|a| low[a] < BOUNDS.min[a] as f32 || high[a] > BOUNDS.max[a] as f32) {
        return Err("Streamed body exceeds world coordinate bounds".into());
    }
    for x in (position.x - half).floor() as i32..(position.x + half).ceil() as i32 {
        for z in (position.z - half).floor() as i32..(position.z + half).ceil() as i32 {
            for y in position.y.floor() as i32..(position.y + body.height).ceil() as i32 {
                if client.movement.mode != crate::engine_player::Mode::Noclip
                    && let Some(id) = stream.observed([x, y, z])
                {
                    for b in state.geometry.boxes(id) {
                        if (0..3).all(|a| {
                            low[a] < [x, y, z][a] as f32 + b[a + 3]
                                && high[a] > [x, y, z][a] as f32 + b[a]
                        }) {
                            return Err("Streamed spawn must fit the block geometry".into());
                        }
                    }
                }
            }
        }
    }
    recenter(&mut client.scene, center(position));
    let stream = client
        .scene
        .engine
        .as_ref()
        .unwrap()
        .stream
        .as_ref()
        .unwrap();
    let missing: Vec<_> = stream
        .wanted
        .iter()
        .filter(|c| !stream.residents.contains_key(*c))
        .copied()
        .collect();
    let generator = stream.generator.clone();
    for c in missing {
        let started = Instant::now();
        let blocks = generator.generate(c);
        insert(
            &mut client.scene,
            Generated {
                column: c,
                blocks,
                milliseconds: started.elapsed().as_secs_f64() * 1000.,
            },
        );
    }
    if client.engine_worker.is_none() {
        client.engine_worker = Some(Worker::new(generator)?);
    }
    Ok(())
}
/// Normal walking accepts one completed column per poll, with at most two queued jobs.
pub fn update(client: &mut Client) -> Result<bool> {
    if client
        .scene
        .engine
        .as_ref()
        .and_then(|s| s.stream.as_ref())
        .is_none()
    {
        return Ok(false);
    }
    let before = client.scene.engine.as_ref().unwrap().revision;
    recenter(&mut client.scene, center(client.actor.position));
    let worker = client
        .engine_worker
        .as_mut()
        .ok_or("Terrain worker missing")?;
    match worker.receiver.try_recv() {
        Ok(generated) => {
            worker.pending.remove(&generated.column);
            insert(&mut client.scene, generated);
        }
        Err(mpsc::TryRecvError::Empty) => {}
        Err(mpsc::TryRecvError::Disconnected) => {
            return Err("Terrain worker stopped unexpectedly".into());
        }
    }
    let stream = client
        .scene
        .engine
        .as_ref()
        .unwrap()
        .stream
        .as_ref()
        .unwrap();
    while worker.pending.len() < 2 {
        let next = stream
            .wanted
            .iter()
            .filter(|c| !stream.residents.contains_key(*c) && !worker.pending.contains(*c))
            .min_by_key(|c| (c[0] - stream.center[0]).pow(2) + (c[1] - stream.center[1]).pow(2))
            .copied();
        let Some(c) = next else {
            break;
        };
        worker.sender.send(Job::Generate(c))?;
        worker.pending.insert(c);
    }
    Ok(before != client.scene.engine.as_ref().unwrap().revision)
}
pub fn generate(seed: u64, registry: Registry) -> Result<Client> {
    let generator = Generator::new(seed, &registry, Config::new(&registry))?;
    let clay = registry.find("clay")?;
    let mut state = WorldState::new(seed, registry, clay, None);
    state.stream = Some(State::new(generator));
    let mut client = Client::new(Scene::from_engine(HashMap::new(), state)?)?;
    client.scene.bounds = BOUNDS;
    client.set_spawn(engine::SPAWN)?;
    client.yaw = 0.;
    client.pitch = -0.08;
    Ok(client)
}
pub fn jump(client: &mut Client, index: usize) -> Result<()> {
    let [x, z] = [[0, -10], [128, 128], [-161, 65], [320, -257]][index % 4];
    let stream = client
        .scene
        .engine
        .as_ref()
        .and_then(|s| s.stream.as_ref())
        .ok_or("Start a new streamed world to explore distant terrain")?;
    let y = (1..62)
        .rev()
        .find(|y| stream.observed([x, *y, z]).is_some())
        .unwrap_or(0)
        + 1;
    client.set_spawn(Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5))?;
    client.yaw = 0.8;
    client.pitch = -0.25;
    Ok(())
}
pub fn observation(client: &Client) -> serde_json::Value {
    let Some(state) = client.scene.engine.as_ref() else {
        return serde_json::Value::Null;
    };
    let Some(s) = &state.stream else {
        return serde_json::Value::Null;
    };
    serde_json::json!({"generator_version":s.generator.config.version,"radius":s.generator.config.radius,"center":s.center,"resident_columns":s.residents.len(),"wanted_columns":s.wanted.len(),"generated_columns":s.generated,"evicted_columns":s.evicted,"discarded_jobs":s.discarded,"generation_ms":s.generation_ms,"resident_blocks":state.blocks.len(),"sparse_edits":s.edits.len(),"pending_edits":state.pending(),"queued_jobs":client.engine_worker.as_ref().map_or(0,|w|w.pending.len()),"dirty_keys":state.dirty_chunks.len()})
}
#[cfg(test)]
mod tests {
    use super::*;
    fn settle(client: &mut Client) {
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        loop {
            client.sync_region().unwrap();
            let s = client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .stream
                .as_ref()
                .unwrap();
            assert!(s.residents.len() <= 49);
            assert!(client.scene.engine.as_ref().unwrap().dirty_chunks.len() <= 196);
            if s.residents.len() == s.wanted.len()
                && client.engine_worker.as_ref().unwrap().pending.is_empty()
            {
                break;
            }
            assert!(Instant::now() < deadline, "Terrain worker did not settle");
            thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    #[test]
    fn worker_crossings_are_bounded_and_evicted_edits_return() {
        let mut c = generate(360036, Registry::built_in()).unwrap();
        let id = c
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .find("oak_planks")
            .unwrap();
        c.scene.change_engine([2, 3, -2], Some(id)).unwrap();
        c.scene.change_engine([3, 3, -2], None).unwrap();
        c.scene.change_engine([4, 40, -2], Some(id)).unwrap();
        for x in [16, 32, 48, 64, 80, 96, -16, -32, -48, -64, -80, -96] {
            let s = c.scene.engine.as_ref().unwrap().stream.as_ref().unwrap();
            let y = (1..62)
                .rev()
                .find(|y| s.observed([x, *y, -10]).is_some())
                .unwrap()
                + 2;
            c.actor.position = Vec3::new(x as f32 + 0.5, y as f32, -9.5);
            settle(&mut c);
            assert_eq!(c.pending_edits(), 3);
        }
        assert!(
            !c.scene
                .engine
                .as_ref()
                .unwrap()
                .blocks
                .contains_key(&[2, 3, -2])
        );
        assert_eq!(c.scene.engine.as_ref().unwrap().original_blocks.len(), 0);
        assert!(
            c.scene
                .engine
                .as_ref()
                .unwrap()
                .stream
                .as_ref()
                .unwrap()
                .evicted
                > 49
        );
        c.set_spawn(engine::SPAWN).unwrap();
        settle(&mut c);
        let s = c.scene.engine.as_ref().unwrap();
        assert_eq!(s.blocks.get(&[2, 3, -2]), Some(&id));
        assert!(!s.blocks.contains_key(&[3, 3, -2]));
        assert_eq!(s.blocks.get(&[4, 40, -2]), Some(&id));
        assert_eq!(s.stream.as_ref().unwrap().edits.len(), 3);
    }
    #[test]
    fn unloaded_columns_are_collision_barriers_and_generator_is_order_independent() {
        let mut c = generate(360036, Registry::built_in()).unwrap();
        let s = c.scene.engine.as_ref().unwrap().stream.as_ref().unwrap();
        let far = mcbe_core::motion::Actor::new(
            Vec3::new(512.5, 30., 512.5),
            mcbe_core::motion::Body::PROBE,
        )
        .unwrap();
        assert!(!c.scene.inside([512, 30, 512]));
        assert!(!c.scene.motion_scene(&far).unwrap().boxes.is_empty());
        let config = s.generator.config.clone();
        let original = s.generator.clone();
        let mut reordered = c.scene.engine.as_ref().unwrap().registry.clone();
        reordered.blocks.reverse();
        let generator = Generator::new(360036, &reordered, config).unwrap();
        let old = &c.scene.engine.as_ref().unwrap().registry;
        for p in [[-11, 4, 14], [128, 10, 128], [-161, 0, 65], [3, 3, -2]] {
            assert_eq!(
                original.block(p).map(|id| old.get(id).id.clone()),
                generator.block(p).map(|id| reordered.get(id).id.clone())
            );
        }
        let before = c.scene.engine.as_ref().unwrap().revision;
        assert!(c.set_spawn(Vec3::new(9000., 30., 0.)).is_err());
        assert!(c.set_spawn(Vec3::new(f32::MAX, 30., f32::MAX)).is_err());
        assert!(c.set_spawn(Vec3::new(0.5, f32::MAX, 0.5)).is_err());
        assert_eq!(c.scene.engine.as_ref().unwrap().revision, before);
    }
}
