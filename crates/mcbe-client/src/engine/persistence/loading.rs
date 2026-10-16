//! Bounded background travel and reload preparation; the latest request wins.
use crate::{
    engine_assets,
    engine_blocks::BlockId,
    engine_inventory::Inventory,
    engine_stream::{self as stream, Column, Generator, Position},
    scene::{Client, Result},
};
use mcbe_core::motion::{Actor, Vec3};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Instant,
};

#[derive(Clone, Copy)]
pub enum Destination {
    Exhibit(usize),
    Overlook,
}
struct Guard {
    dimensions: Option<u64>,
    entities: u64,
    movement: crate::engine_player::Movement,
    instance: u64,
    registry: u64,
    edits: u64,
    player: Inventory,
    machines: u64,
}
impl Guard {
    fn capture(client: &Client) -> Result<Self> {
        let s = client.scene.engine.as_ref().ok_or("Engine world missing")?;
        Ok(Self {
            dimensions: s.dimensions.as_ref().map(|d| d.revision),
            entities: s.entities.stats.revision,
            movement: client.movement,
            instance: s.instance,
            registry: s.registry_revision,
            edits: s.edit_revision,
            player: s.player.clone(),
            machines: s.machines.revision,
        })
    }
    fn matches(&self, client: &Client, reload: bool) -> bool {
        client.scene.engine.as_ref().is_some_and(|s| {
            s.dimensions.as_ref().map(|d| d.revision) == self.dimensions
                && s.instance == self.instance
                && client.movement == self.movement
                && s.registry_revision == self.registry
                && s.edit_revision == self.edits
                && (!reload
                    || (s.entities.stats.revision == self.entities
                        && s.machines.revision == self.machines
                        && s.player.changed_slots(&self.player) == 0
                        && s.player.active == self.player.active))
        })
    }
}
enum Operation {
    Dimension(Box<crate::engine::world::dimensions::Travel>),
    Travel {
        generator: Arc<Generator>,
        geometry: Arc<crate::engine_geometry::Library>,
        movement: crate::engine_player::Movement,
        edits: BTreeMap<Position, Option<BlockId>>,
        destination: Destination,
    },
    Reload {
        path: PathBuf,
        expected: String,
        root: Option<PathBuf>,
        owned_root: Option<PathBuf>,
    },
}
struct Request {
    id: u64,
    guard: Guard,
    operation: Operation,
}
struct Region {
    volume: Option<crate::engine::world::volume::Volume>,
    blocks: crate::engine::world::storage::Store,
    cells: HashSet<Position>,
    residents: BTreeMap<Column, Vec<Position>>,
    wanted: BTreeSet<Column>,
    position: Vec3,
    yaw: f32,
    pitch: f32,
    chunks: BTreeSet<[i32; 3]>,
}
enum Prepared {
    Dimension(Box<Client>),
    Travel(Box<Region>),
    Reload(Box<Client>),
}
enum Retired {
    Prepared(Prepared),
    Client(Box<Client>),
    Region(Box<Region>),
}
enum Job {
    Prepare(Box<Request>),
    Dispose(Box<Retired>),
    Stop,
}
struct Finished {
    dimension: Option<String>,
    id: u64,
    guard: Guard,
    reload: bool,
    result: std::result::Result<Prepared, String>,
    off_thread: bool,
    milliseconds: f64,
}
#[derive(Default, Serialize)]
pub struct Counters {
    pub requests: usize,
    pub completed: usize,
    pub applied: usize,
    pub failed: usize,
    pub superseded: usize,
    pub stale: usize,
    pub coalesced: usize,
    pub cancelled: usize,
    pub worker_jobs: usize,
    pub peak_requests: usize,
    pub capture_ms: f64,
    pub worker_ms: f64,
    pub max_apply_ms: f64,
}
#[derive(Serialize)]
pub struct Event {
    pub dimension: Option<String>,
    pub id: u64,
    pub reload: bool,
    pub applied: bool,
    pub off_thread: bool,
    pub error: Option<String>,
}
pub struct Service {
    sender: mpsc::Sender<Job>,
    receiver: mpsc::Receiver<Finished>,
    thread: Option<thread::JoinHandle<()>>,
    newest: Arc<AtomicU64>,
    active: bool,
    pending: Option<Request>,
    next: u64,
    pub counters: Counters,
}
fn check_file(path: &std::path::Path, expected: &str) -> Result<()> {
    if fs::metadata(path)?.len() > 32 * 1024 * 1024
        || format!("{:x}", Sha256::digest(fs::read(path)?)) != expected
    {
        return Err("Loaded save changed on disk; reload cancelled".into());
    }
    Ok(())
}
fn prepare_region(
    generator: Arc<Generator>,
    geometry: Arc<crate::engine_geometry::Library>,
    movement: crate::engine_player::Movement,
    edits: BTreeMap<Position, Option<BlockId>>,
    destination: Destination,
    id: u64,
    newest: &AtomicU64,
) -> Result<Region> {
    let observed = |p| edits.get(&p).copied().unwrap_or_else(|| generator.block(p));
    let (position, yaw, pitch) = match destination {
        Destination::Exhibit(index) => {
            let [x, z] = if let Some(terrain) = &generator.terrain {
                let p = terrain.landmark(index);
                [p[0], p[2]]
            } else {
                [[0, -10], [128, 128], [-161, 65], [320, -257]][index % 4]
            };
            let top = generator.visible_surface(x, z).0;
            let y = if generator.terrain.is_some() {
                (top..=top + 6)
                    .rev()
                    .find(|y| observed([x, *y, z]).is_some())
                    .unwrap_or(top - 1)
                    + 1
            } else {
                (1..62)
                    .rev()
                    .find(|y| observed([x, *y, z]).is_some())
                    .unwrap_or(0)
                    + 1
            };
            (
                Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5),
                0.8,
                -0.25,
            )
        }
        Destination::Overlook => (
            Vec3::new(
                20.5,
                if generator.terrain.is_some() {
                    generator.visible_surface(20, 0).0 as f32 + 8.
                } else {
                    13.
                },
                0.5,
            ),
            -1.4,
            -0.4,
        ),
    };
    Actor::new(position, movement.body())?;
    let low = [position.x - 0.3, position.y, position.z - 0.3];
    let high = [
        position.x + 0.3,
        position.y + movement.body().height,
        position.z + 0.3,
    ];
    if (0..3).any(|a| {
        low[a] < generator.bounds().min[a] as f32 || high[a] > generator.bounds().max[a] as f32
    }) {
        return Err("Travel body exceeds world bounds".into());
    }
    for x in low[0].floor() as i32..high[0].ceil() as i32 {
        for y in low[1].floor() as i32..high[1].ceil() as i32 {
            for z in low[2].floor() as i32..high[2].ceil() as i32 {
                if movement.mode != crate::engine_player::Mode::Noclip
                    && let Some(id) = observed([x, y, z])
                {
                    for b in geometry.collision(id) {
                        if (0..3).all(|a| {
                            low[a] < [x, y, z][a] as f32 + b[a + 3]
                                && high[a] > [x, y, z][a] as f32 + b[a]
                        }) {
                            return Err("Travel destination is obstructed".into());
                        }
                    }
                }
            }
        }
    }
    let wanted = stream::wanted(stream::center(position), generator.config.radius);
    let mut region = Region {
        volume: None,
        blocks: crate::engine::world::storage::Store::new(),
        cells: HashSet::new(),
        residents: BTreeMap::new(),
        chunks: BTreeSet::new(),
        wanted,
        position,
        yaw,
        pitch,
    };
    if generator.terrain.is_some() {
        use crate::engine::world::{
            storage::{self, AIR},
            volume,
        };
        let mut window = volume::Volume::wanted(&generator, position);
        let mut indexed: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for (p, id) in &edits {
            indexed.entry(storage::key(*p)).or_default().push((*p, *id));
        }
        for key in &window.wanted {
            if newest.load(Ordering::Acquire) != id {
                return Err("Preparation superseded".into());
            }
            let mut chunk = volume::generate(&generator, *key);
            if let Some(edits) = indexed.get(key) {
                let c = Arc::make_mut(&mut chunk);
                for (p, id) in edits {
                    let i = storage::index(*p);
                    let old = c.ids[i];
                    let next = id.unwrap_or(AIR);
                    if old == AIR && next != AIR {
                        c.count += 1;
                    }
                    if old != AIR && next == AIR {
                        c.count -= 1;
                    }
                    c.ids[i] = next;
                }
            }
            if chunk.count > 0 {
                region.chunks.insert(*key);
            }
            region.blocks.insert_chunk(*key, chunk);
            window.residents.insert(*key);
            region.residents.entry([key[0], key[2]]).or_default();
        }
        region.wanted = window.wanted.iter().map(|k| [k[0], k[2]]).collect();
        region.volume = Some(window);
        return Ok(region);
    }
    for c in &region.wanted {
        if newest.load(Ordering::Acquire) != id {
            return Err("Preparation superseded".into());
        }
        let mut blocks = generator.generate(*c);
        for (p, value) in edits
            .range([c[0] * 16, 0, i32::MIN]..[(c[0] + 1) * 16, 0, i32::MIN])
            .filter(|(p, _)| stream::column(**p) == *c)
        {
            if let Some(value) = value {
                blocks.insert(*p, *value);
            } else {
                blocks.remove(p);
            }
        }
        let positions: Vec<_> = blocks.keys().copied().collect();
        region
            .chunks
            .extend(positions.iter().copied().map(crate::engine_mesh::chunk));
        region.cells.extend(positions.iter().copied());
        region.residents.insert(*c, positions);
        region.blocks.extend(blocks);
    }
    Ok(region)
}
impl Service {
    pub fn new() -> Result<Self> {
        let (sender, jobs) = mpsc::channel();
        let (completed, receiver) = mpsc::channel();
        let newest = Arc::new(AtomicU64::new(0));
        let token = newest.clone();
        let owner = thread::current().id();
        let thread = thread::Builder::new()
            .name("blockwork-loading".into())
            .spawn(move || {
                while let Ok(job) = jobs.recv() {
                    match job {
                        Job::Stop => break,
                        Job::Dispose(retired) => match *retired {
                            Retired::Prepared(v) => drop(v),
                            Retired::Client(v) => drop(v),
                            Retired::Region(v) => drop(v),
                        },
                        Job::Prepare(request) => {
                            let Request {
                                id,
                                guard,
                                operation,
                            } = *request;
                            let started = Instant::now();
                            let dimension=match &operation {Operation::Dimension(t)=>Some(t.destination.clone()),_=>None};
                            let reload=matches!(&operation,Operation::Reload {..});
                            let result = (|| -> Result<Prepared> {
                                if token.load(Ordering::Acquire) != id {
                                    return Err("Preparation superseded".into());
                                }
                                match operation {
                                    Operation::Dimension(travel)=>{
                                        if token.load(Ordering::Acquire)!=id {return Err("Dimension travel superseded".into());}
                                        let client=travel.prepare_with_token(&token,id)?;
                                        if token.load(Ordering::Acquire)!=id {return Err("Dimension travel superseded".into());}
                                        Ok(Prepared::Dimension(Box::new(client)))
                                    },
                                    Operation::Travel {
                                        generator,
                                        geometry,
                                        movement,
                                        edits,
                                        destination,
                                    } => Ok(Prepared::Travel(Box::new(prepare_region(
                                        generator,
                                        geometry,
                                        movement,
                                        edits,
                                        destination,
                                        id,
                                        &token,
                                    )?))),
                                    Operation::Reload {
                                        path,
                                        expected,
                                        root,
                                        owned_root,
                                    } => {
                                        check_file(&path, &expected)?;
                                        let mut client = crate::engine::load(&path)?;
                                        if client
                                            .scene
                                            .engine
                                            .as_ref()
                                            .and_then(|s| s.loaded.as_ref())
                                            .is_none_or(|(_, hash)| hash != &expected)
                                        {
                                            return Err(
                                                "Save changed while being read; reload cancelled"
                                                    .into(),
                                            );
                                        }
                                        if token.load(Ordering::Acquire) != id {
                                            return Err("Preparation superseded".into());
                                        }
                                        if let Some(registry) = client
                                            .scene
                                            .engine
                                            .as_ref()
                                            .unwrap()
                                            .registry
                                            .with_new_builtins()?
                                        {
                                            crate::engine::apply_registry(&mut client, registry)?;
                                        }
                                        if let Some(home) = owned_root {
                                            client.appearance=Arc::new(crate::engine::player::appearance::Appearance::read_home(&home)?);
                                            client.scene.engine.as_mut().unwrap().ui = Arc::new(
                                                crate::engine::ui::framework::Library::read_home(
                                                    &home,
                                                )?,
                                            );
                                            {let state=client.scene.engine.as_mut().unwrap();state.player.configure(crate::engine_inventory::profile::Profile::read_home(&home)?,&state.registry)?;state.ui.validate_inventory(&state.player.profile)?;}
                                            client.owned_materials =
                                                Some(crate::engine_materials::load(
                                                    &client.scene.engine.as_ref().unwrap().registry,
                                                    &home,
                                                )?);
                                        }
                                        if let Some(root)=root {let state=client.scene.engine.as_ref().unwrap();client.engine_assets=Some(engine_assets::load_for(&state.registry,&root,state.ui.theme.legacy_ui)?);}
                                        check_file(&path, &expected)?;
                                        Ok(Prepared::Reload(Box::new(client)))
                                    }
                                }
                            })()
                            .map_err(|e| e.to_string());
                            if completed
                                .send(Finished {
                                    dimension,
                                    id,
                                    guard,
                                    reload,
                                    result,
                                    off_thread: thread::current().id() != owner,
                                    milliseconds: started.elapsed().as_secs_f64() * 1000.,
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            })?;
        Ok(Self {
            sender,
            receiver,
            thread: Some(thread),
            newest,
            active: false,
            pending: None,
            next: 0,
            counters: Counters::default(),
        })
    }
    fn request(&mut self, client: &Client, operation: Operation, started: Instant) -> Result<()> {
        let guard = Guard::capture(client)?;
        self.next = self.next.wrapping_add(1);
        self.newest.store(self.next, Ordering::Release);
        self.counters.requests += 1;
        self.counters.capture_ms += started.elapsed().as_secs_f64() * 1000.;
        let request = Request {
            id: self.next,
            guard,
            operation,
        };
        if self.active {
            if self.pending.replace(request).is_some() {
                self.counters.coalesced += 1;
            }
        } else {
            self.sender.send(Job::Prepare(Box::new(request)))?;
            self.active = true;
        }
        self.counters.peak_requests = self
            .counters
            .peak_requests
            .max(usize::from(self.active) + usize::from(self.pending.is_some()));
        Ok(())
    }
    pub fn dimension(&mut self, client: &Client, id: &str) -> Result<()> {
        let started = Instant::now();
        let travel = crate::engine::world::dimensions::Travel::capture(client, id)?;
        self.request(client, Operation::Dimension(Box::new(travel)), started)
    }
    pub fn travel(&mut self, client: &Client, destination: Destination) -> Result<()> {
        let started = Instant::now();
        let state = client.scene.engine.as_ref().ok_or("Engine world missing")?;
        let s = state
            .stream
            .as_ref()
            .ok_or("Start a streamed world to explore distant terrain")?;
        self.request(
            client,
            Operation::Travel {
                generator: s.generator.clone(),
                geometry: state.geometry.clone(),
                movement: client.movement,
                edits: s.edits.clone(),
                destination,
            },
            started,
        )
    }
    pub fn reload(&mut self, client: &Client) -> Result<()> {
        let started = Instant::now();
        let s = client.scene.engine.as_ref().ok_or("Engine world missing")?;
        let (path, expected) = s
            .loaded
            .as_ref()
            .ok_or("Press F5 to create your first save")?;
        self.request(
            client,
            Operation::Reload {
                path: path.clone(),
                expected: expected.clone(),
                root: client.engine_assets.as_ref().map(|a| a.root.clone()),
                owned_root: client.owned_materials.as_ref().map(|a| a.root.clone()),
            },
            started,
        )
    }
    pub fn busy(&self) -> bool {
        self.active || self.pending.is_some()
    }
    pub fn cancel(&mut self) {
        self.next = self.next.wrapping_add(1);
        self.newest.store(self.next, Ordering::Release);
        self.counters.cancelled +=
            usize::from(self.active) + usize::from(self.pending.take().is_some());
    }
    pub fn poll(&mut self, client: &mut Client) -> Result<Option<Event>> {
        let done = match self.receiver.try_recv() {
            Ok(v) => v,
            Err(mpsc::TryRecvError::Empty) => return Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("Loading worker stopped unexpectedly".into());
            }
        };
        self.active = false;
        self.counters.completed += 1;
        self.counters.worker_jobs += usize::from(done.off_thread);
        self.counters.worker_ms += done.milliseconds;
        let mut event = Event {
            dimension: done.dimension.clone(),
            id: done.id,
            reload: done.reload,
            applied: false,
            off_thread: done.off_thread,
            error: None,
        };
        if done.id != self.newest.load(Ordering::Acquire) {
            self.counters.superseded += 1;
            if let Ok(prepared) = done.result {
                self.sender
                    .send(Job::Dispose(Box::new(Retired::Prepared(prepared))))?;
            }
        } else if !done
            .guard
            .matches(client, done.reload || done.dimension.is_some())
        {
            self.counters.stale += 1;
            event.error = Some("World changed while preparing; request cancelled".into());
            if let Ok(prepared) = done.result {
                self.sender
                    .send(Job::Dispose(Box::new(Retired::Prepared(prepared))))?;
            }
        } else {
            match done.result {
                Err(error) => {
                    self.counters.failed += 1;
                    event.error = Some(error);
                }
                Ok(prepared) => {
                    let started = Instant::now();
                    match prepared {
                        Prepared::Dimension(mut prepared) => {
                            // Registry guards prove that these shared decoded assets still match.
                            prepared.engine_assets = client.engine_assets.take();
                            prepared.owned_materials = client.owned_materials.take();
                            *prepared.engine_gpu_prepared.borrow_mut() =
                                client.engine_gpu_prepared.borrow_mut().take();
                            let old = std::mem::replace(client, *prepared);
                            self.sender
                                .send(Job::Dispose(Box::new(Retired::Client(Box::new(old)))))?;
                        }
                        Prepared::Reload(prepared) => {
                            let old = std::mem::replace(client, *prepared);
                            self.sender
                                .send(Job::Dispose(Box::new(Retired::Client(Box::new(old)))))?;
                        }
                        Prepared::Travel(mut region) => {
                            let state = client.scene.engine.as_mut().unwrap();
                            let s = state.stream.as_mut().unwrap();
                            let old_chunks = state.blocks.chunk_keys().filter(|k| {
                                region.volume.as_ref().map_or_else(
                                    || region.wanted.contains(&[k[0], k[2]]),
                                    |v| v.residents.contains(k),
                                )
                            });
                            region.chunks.extend(old_chunks);
                            state.revision = state.revision.wrapping_add(1);
                            state.dirty_chunks =
                                region.chunks.iter().map(|k| (*k, state.revision)).collect();
                            s.evicted += s
                                .residents
                                .keys()
                                .filter(|c| !region.wanted.contains(*c))
                                .count();
                            s.generated += region.residents.len();
                            s.generation_ms += done.milliseconds;
                            std::mem::swap(&mut state.blocks, &mut region.blocks);
                            std::mem::swap(&mut client.scene.cells, &mut region.cells);
                            std::mem::swap(&mut s.residents, &mut region.residents);
                            std::mem::swap(&mut s.wanted, &mut region.wanted);
                            s.center = stream::center(region.position);
                            std::mem::swap(&mut s.volume, &mut region.volume);
                            state.simulation =
                                crate::engine::blocks::systems::simulation::Simulation::new(
                                    &state.registry,
                                );
                            state.simulation.hydrate(
                                &state.registry,
                                state.blocks.iter().map(|(p, id)| (p, *id)),
                            );
                            state.fluids.rescan(state.blocks.chunk_keys());
                            client.actor = Actor::new(region.position, client.movement.body())?;
                            client.spawn = region.position;
                            client.yaw = region.yaw;
                            client.pitch = region.pitch;
                            self.sender
                                .send(Job::Dispose(Box::new(Retired::Region(region))))?;
                        }
                    }
                    self.counters.max_apply_ms = self
                        .counters
                        .max_apply_ms
                        .max(started.elapsed().as_secs_f64() * 1000.);
                    self.counters.applied += 1;
                    event.applied = true;
                }
            }
        }
        if let Some(request) = self.pending.take() {
            self.sender.send(Job::Prepare(Box::new(request)))?;
            self.active = true;
        }
        Ok(Some(event))
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.cancel();
        let _ = self.sender.send(Job::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine, engine_blocks::Registry, engine_inventory::Stack};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    fn client() -> Client {
        stream::generate(360036, Registry::built_in()).unwrap()
    }
    fn settle(service: &mut Service, client: &mut Client) -> Vec<Event> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut events = Vec::new();
        while service.busy() {
            if let Some(e) = service.poll(client).unwrap() {
                events.push(e);
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        events
    }
    #[test]
    fn modern_travel_matches_sync_chunks_and_cancellation_preserves_height_edits() {
        let mut settings = crate::engine::world::terrain::Settings::built_in();
        settings.caves = false;
        settings.sea_level = -128;
        let mut live =
            stream::generate_modern(360036, Registry::built_in(), settings.clone()).unwrap();
        crate::engine_player::mode(&mut live, crate::engine_player::Mode::Noclip).unwrap();
        live.set_spawn(Vec3::new(0.5, 10002., 0.5)).unwrap();
        live.scene.change_engine([0, 10000, 0], Some(0)).unwrap();
        let original = live.actor.position;
        let mut loader = Service::new().unwrap();
        loader.travel(&live, Destination::Exhibit(1)).unwrap();
        loader.cancel();
        assert!(settle(&mut loader, &mut live).iter().all(|e| !e.applied));
        assert_eq!(live.actor.position, original);
        loader.travel(&live, Destination::Exhibit(1)).unwrap();
        assert!(
            settle(&mut loader, &mut live)
                .iter()
                .any(|e| e.applied && e.off_thread)
        );
        let mut reference =
            stream::generate_modern(360036, Registry::built_in(), settings).unwrap();
        crate::engine_player::mode(&mut reference, crate::engine_player::Mode::Noclip).unwrap();
        stream::jump(&mut reference, 1).unwrap();
        assert_eq!(live.actor.position, reference.actor.position);
        assert_eq!(
            live.scene.engine.as_ref().unwrap().blocks,
            reference.scene.engine.as_ref().unwrap().blocks
        );
        assert!(live.scene.cells.is_empty());
        live.set_spawn(original).unwrap();
        assert_eq!(
            live.scene
                .engine
                .as_ref()
                .unwrap()
                .blocks
                .get(&[0, 10000, 0]),
            Some(&0)
        );
    }
    #[test]
    fn latest_travel_matches_synchronous_terrain_and_preserves_unsaved_state() {
        let mut live = client();
        let oak = live
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .find("oak_planks")
            .unwrap();
        live.scene.change_engine([2, 3, -2], Some(oak)).unwrap();
        live.scene.change_engine([3, 3, -2], None).unwrap();
        live.scene.change_engine([4, 40, -2], Some(oak)).unwrap();
        live.scene.engine.as_mut().unwrap().player.slots[10] = Some(Stack {
            data: None,
            id: oak,
            count: 17,
        });
        let instance = live.scene.engine.as_ref().unwrap().instance;
        let original = live.actor.position;
        let mut loader = Service::new().unwrap();
        for i in 1..=3 {
            loader.travel(&live, Destination::Exhibit(i)).unwrap();
        }
        assert_eq!(live.actor.position, original);
        assert_eq!(loader.counters.peak_requests, 2);
        assert_eq!(loader.counters.coalesced, 1);
        let events = settle(&mut loader, &mut live);
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| e.off_thread));
        assert!(!events[0].applied);
        assert!(events[1].applied);
        assert_eq!(loader.counters.applied, 1);
        assert_eq!(loader.counters.superseded, 1);
        let mut reference = client();
        reference
            .scene
            .change_engine([2, 3, -2], Some(oak))
            .unwrap();
        reference.scene.change_engine([3, 3, -2], None).unwrap();
        reference
            .scene
            .change_engine([4, 40, -2], Some(oak))
            .unwrap();
        stream::jump(&mut reference, 3).unwrap();
        assert_eq!(live.scene.cells, reference.scene.cells);
        assert_eq!(
            live.scene.engine.as_ref().unwrap().blocks,
            reference.scene.engine.as_ref().unwrap().blocks
        );
        assert_eq!(live.actor.position, reference.actor.position);
        assert_eq!(live.scene.engine.as_ref().unwrap().instance, instance);
        assert_eq!(live.pending_edits(), 4);
        assert_eq!(
            live.scene.engine.as_ref().unwrap().player.slots[10]
                .unwrap()
                .count,
            17
        );
        loader.travel(&live, Destination::Exhibit(0)).unwrap();
        settle(&mut loader, &mut live);
        assert_eq!(
            live.scene.engine.as_ref().unwrap().blocks.get(&[4, 40, -2]),
            Some(&oak)
        );
        assert!(live.scene.engine.as_ref().unwrap().dirty_chunks.len() <= 196);
    }
    #[test]
    fn cancellation_and_stale_edits_or_registries_never_replace_current_region() {
        let mut live = client();
        let original = live.actor.position;
        let mut loader = Service::new().unwrap();
        loader.travel(&live, Destination::Exhibit(1)).unwrap();
        loader.cancel();
        let events = settle(&mut loader, &mut live);
        assert!(!events[0].applied);
        assert!(events[0].error.is_none());
        assert_eq!(live.actor.position, original);
        loader.travel(&live, Destination::Exhibit(2)).unwrap();
        live.scene.change_engine([2, 3, -2], None).unwrap();
        let events = settle(&mut loader, &mut live);
        assert!(events[0].error.is_some());
        assert_eq!(live.actor.position, original);
        loader.travel(&live, Destination::Exhibit(3)).unwrap();
        let mut registry = live.scene.engine.as_ref().unwrap().registry.clone();
        registry.blocks.reverse();
        engine::apply_registry(&mut live, registry).unwrap();
        let events = settle(&mut loader, &mut live);
        assert!(events[0].error.is_some());
        assert_eq!(loader.counters.stale, 2);
        assert_eq!(live.actor.position, original);
        loader.travel(&live, Destination::Exhibit(1)).unwrap();
        live = client();
        let current_instance = live.scene.engine.as_ref().unwrap().instance;
        let events = settle(&mut loader, &mut live);
        assert!(events[0].error.is_some());
        assert_eq!(loader.counters.stale, 3);
        assert_eq!(
            live.scene.engine.as_ref().unwrap().instance,
            current_instance
        );
    }
    #[test]
    fn reload_checks_source_bytes_preserves_state_on_failure_and_restores_valid_snapshot() {
        let directory = std::env::temp_dir().join(format!(
            "blockwork-loading-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut live = client();
        let path = engine::save(&mut live, &directory).unwrap();
        let bytes = fs::read(&path).unwrap();
        let mut loader = Service::new().unwrap();
        let instance = live.scene.engine.as_ref().unwrap().instance;
        let mut changed = bytes.clone();
        changed.push(b' ');
        fs::write(&path, &changed).unwrap();
        loader.reload(&live).unwrap();
        let e = settle(&mut loader, &mut live);
        assert!(e[0].error.is_some());
        assert!(!e[0].applied);
        assert_eq!(live.scene.engine.as_ref().unwrap().instance, instance);
        assert_eq!(fs::read(&path).unwrap(), changed);
        fs::write(&path, &bytes).unwrap();
        loader.reload(&live).unwrap();
        live.scene.engine.as_mut().unwrap().player.slots[10] = Some(Stack {
            data: None,
            id: 0,
            count: 11,
        });
        let e = settle(&mut loader, &mut live);
        assert!(e[0].error.is_some());
        assert_eq!(
            live.scene.engine.as_ref().unwrap().player.slots[10]
                .unwrap()
                .count,
            11
        );
        loader.reload(&live).unwrap();
        let e = settle(&mut loader, &mut live);
        assert!(e[0].applied && e[0].reload && e[0].off_thread);
        assert_ne!(live.scene.engine.as_ref().unwrap().instance, instance);
        assert!(live.scene.engine.as_ref().unwrap().player.slots[10].is_none());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        drop(loader);
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}

#[cfg(test)]
mod dimension_tests {
    use super::*;
    use crate::engine::world::dimensions;
    use std::time::Duration;
    fn client() -> Client {
        let mut c = stream::generate(713, crate::engine_blocks::Registry::built_in()).unwrap();
        stream::set_radius(&mut c, 2).unwrap();
        dimensions::attach_legacy(&mut c, dimensions::Definitions::built_in()).unwrap();
        c
    }
    fn settle(service: &mut Service, c: &mut Client) -> Vec<Event> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut events = Vec::new();
        while service.busy() {
            if let Some(e) = service.poll(c).unwrap() {
                events.push(e);
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        events
    }
    #[test]
    fn dimensions_prepare_off_thread_and_latest_request_wins() {
        let mut c = client();
        let mut s = Service::new().unwrap();
        let original = c.scene.engine.as_ref().unwrap().instance;
        s.dimension(&c, "cavern").unwrap();
        s.dimension(&c, "islands").unwrap();
        let events = settle(&mut s, &mut c);
        assert_ne!(c.scene.engine.as_ref().unwrap().instance, original);
        assert_eq!(
            c.scene
                .engine
                .as_ref()
                .unwrap()
                .dimensions
                .as_ref()
                .unwrap()
                .active,
            "islands"
        );
        assert!(
            events
                .iter()
                .any(|e| e.applied && e.off_thread && e.dimension.as_deref() == Some("islands"))
        );
        assert_eq!(s.counters.applied, 1);
        assert_eq!(s.counters.superseded, 1);
        assert!(s.counters.peak_requests <= 2);
        assert!(
            !c.scene
                .engine
                .as_ref()
                .unwrap()
                .dimensions
                .as_ref()
                .unwrap()
                .inactive
                .contains_key("cavern")
        );
    }
    #[test]
    fn dimension_cancel_and_inventory_staleness_preserve_the_source() {
        let mut c = client();
        let instance = c.scene.engine.as_ref().unwrap().instance;
        let mut s = Service::new().unwrap();
        s.dimension(&c, "cavern").unwrap();
        s.cancel();
        settle(&mut s, &mut c);
        assert_eq!(c.scene.engine.as_ref().unwrap().instance, instance);
        assert_eq!(s.counters.applied, 0);
        s.dimension(&c, "islands").unwrap();
        let state = c.scene.engine.as_mut().unwrap();
        state.player.swap_hands().unwrap();
        let events = settle(&mut s, &mut c);
        assert!(events.iter().any(|e| !e.applied && e.error.is_some()));
        assert_eq!(s.counters.stale, 1);
        assert_eq!(c.scene.engine.as_ref().unwrap().instance, instance);
    }
}
