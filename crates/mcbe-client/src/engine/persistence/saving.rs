//! One active save and one coalesced pending snapshot; live baselines adopt snapshots.
use crate::{
    engine::{SaveSnapshot, SaveTiming},
    scene::{Client, Result},
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Instant,
};
struct Request {
    id: u64,
    snapshot: SaveSnapshot,
    directory: PathBuf,
}
enum Job {
    Save(Box<Request>),
    Stop,
}
struct Finished {
    id: u64,
    result: std::result::Result<crate::engine::CommittedSave, String>,
    off_thread: bool,
}
#[derive(Default, Serialize)]
pub struct Counters {
    pub requests: usize,
    pub completed: usize,
    pub failed: usize,
    pub coalesced: usize,
    pub cancelled: usize,
    pub stale: usize,
    pub worker_jobs: usize,
    pub peak_snapshots: usize,
    pub capture_ms: f64,
    pub max_capture_ms: f64,
    pub serialize_ms: f64,
    pub validate_ms: f64,
    pub io_ms: f64,
    pub bytes_written: usize,
}
#[derive(Serialize)]
pub struct Event {
    pub id: u64,
    pub path: Option<PathBuf>,
    pub sha256: Option<String>,
    pub adopted: bool,
    pub error: Option<String>,
    pub off_thread: bool,
    pub timing: SaveTiming,
}
struct Stamp {
    movement: crate::engine_player::Movement,
    instance: u64,
    revision: u64,
    registry: u64,
    player: crate::engine_inventory::Inventory,
    position: mcbe_core::motion::Vec3,
    yaw: f32,
    pitch: f32,
}
impl Stamp {
    fn capture(client: &Client) -> Self {
        let s = client.scene.engine.as_ref().unwrap();
        Self {
            movement: client.movement,
            instance: s.instance,
            revision: s.revision,
            registry: s.registry_revision,
            player: s.player.clone(),
            position: client.actor.position,
            yaw: client.yaw,
            pitch: client.pitch,
        }
    }
    fn matches(&self, client: &Client) -> bool {
        client.scene.engine.as_ref().is_some_and(|s| {
            s.instance == self.instance
                && s.revision == self.revision
                && s.registry_revision == self.registry
                && s.player.changed_slots(&self.player) == 0
                && s.player.active == self.player.active
        }) && client.movement == self.movement
            && client.actor.position == self.position
            && client.yaw == self.yaw
            && client.pitch == self.pitch
    }
}
pub struct Service {
    latest: Option<Stamp>,
    sender: mpsc::Sender<Job>,
    receiver: mpsc::Receiver<Finished>,
    thread: Option<thread::JoinHandle<()>>,
    active: bool,
    pending: Option<Request>,
    next: u64,
    pub counters: Counters,
}
impl Service {
    pub fn new() -> Result<Self> {
        let (sender, jobs) = mpsc::channel();
        let (completed, receiver) = mpsc::channel();
        let owner = thread::current().id();
        let thread = thread::Builder::new()
            .name("blockwork-save".into())
            .spawn(move || {
                while let Ok(Job::Save(request)) = jobs.recv() {
                    let request = *request;
                    let result = request
                        .snapshot
                        .write(&request.directory)
                        .map_err(|e| e.to_string());
                    if completed
                        .send(Finished {
                            id: request.id,
                            result,
                            off_thread: thread::current().id() != owner,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self {
            latest: None,
            sender,
            receiver,
            thread: Some(thread),
            active: false,
            pending: None,
            next: 0,
            counters: Counters::default(),
        })
    }
    pub fn latest_matches(&self, client: &Client) -> bool {
        self.latest.as_ref().is_some_and(|s| s.matches(client))
    }
    pub fn busy(&self) -> bool {
        self.active || self.pending.is_some()
    }
    pub fn snapshots(&self) -> usize {
        usize::from(self.active) + usize::from(self.pending.is_some())
    }
    pub fn request(&mut self, client: &Client, directory: &Path) -> Result<u64> {
        let started = Instant::now();
        let snapshot = SaveSnapshot::capture(client)?;
        self.latest = Some(Stamp::capture(client));
        let elapsed = started.elapsed().as_secs_f64() * 1000.;
        self.counters.capture_ms += elapsed;
        self.counters.max_capture_ms = self.counters.max_capture_ms.max(elapsed);
        self.next = self.next.wrapping_add(1);
        let id = self.next;
        self.counters.requests += 1;
        let request = Request {
            id,
            snapshot,
            directory: directory.to_path_buf(),
        };
        if self.active {
            if self.pending.replace(request).is_some() {
                self.counters.coalesced += 1;
            }
        } else {
            self.sender.send(Job::Save(Box::new(request)))?;
            self.active = true;
        }
        self.counters.peak_snapshots = self.counters.peak_snapshots.max(self.snapshots());
        Ok(id)
    }
    pub fn poll(&mut self, client: &mut Client) -> Result<Option<Event>> {
        let finished = match self.receiver.try_recv() {
            Ok(v) => v,
            Err(mpsc::TryRecvError::Empty) => return Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.active = false;
                self.pending = None;
                return Err("Save worker stopped unexpectedly".into());
            }
        };
        self.active = false;
        self.counters.worker_jobs += usize::from(finished.off_thread);
        let event = match finished.result {
            Ok(committed) => {
                let path = committed.path.clone();
                let sha256 = committed.sha256.clone();
                let timing = committed.timing.clone();
                let adopted = committed.adopt(client)?;
                self.counters.completed += 1;
                self.counters.stale += usize::from(!adopted);
                self.counters.serialize_ms += timing.serialize_ms;
                self.counters.validate_ms += timing.validate_ms;
                self.counters.io_ms += timing.io_ms;
                self.counters.bytes_written += timing.bytes;
                Event {
                    id: finished.id,
                    path: Some(path),
                    sha256: Some(sha256),
                    adopted,
                    error: None,
                    off_thread: finished.off_thread,
                    timing,
                }
            }
            Err(error) => {
                self.counters.failed += 1;
                if self.pending.take().is_some() {
                    self.counters.cancelled += 1;
                }
                Event {
                    id: finished.id,
                    path: None,
                    sha256: None,
                    adopted: false,
                    error: Some(error),
                    off_thread: finished.off_thread,
                    timing: SaveTiming::default(),
                }
            }
        };
        if let Some(mut pending) = self.pending.take() {
            if client
                .scene
                .engine
                .as_ref()
                .is_some_and(|s| s.instance == pending.snapshot.instance)
            {
                pending.snapshot.refresh_source(client);
                self.sender.send(Job::Save(Box::new(pending)))?;
                self.active = true;
            } else {
                self.counters.cancelled += 1;
            }
        }
        Ok(Some(event))
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.sender.send(Job::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine, engine_blocks::Registry, engine_inventory::Stack, engine_stream};
    use std::{
        fs,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "blockwork-save-worker-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            for file in fs::read_dir(&self.0).unwrap().flatten() {
                fs::remove_file(file.path()).unwrap();
            }
            fs::remove_dir(&self.0).unwrap();
        }
    }
    fn drain(service: &mut Service, client: &mut Client) -> Vec<Event> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut result = Vec::new();
        while service.busy() {
            if let Some(event) = service.poll(client).unwrap() {
                result.push(event);
            }
            assert!(Instant::now() < deadline, "Save worker did not complete");
            thread::sleep(Duration::from_millis(1));
        }
        result
    }
    #[test]
    fn coalesced_snapshots_preserve_post_request_world_and_stack_changes() {
        let dir = Temp::new();
        let mut c = engine_stream::generate(360036, Registry::built_in()).unwrap();
        let mut service = Service::new().unwrap();
        let oak = c
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .find("oak_planks")
            .unwrap();
        c.scene.change_engine([2, 3, -2], Some(oak)).unwrap();
        c.scene.engine.as_mut().unwrap().player.slots[9] = Some(Stack { id: oak, count: 10 });
        service.request(&c, &dir.0).unwrap();
        c.scene.change_engine([3, 3, -2], None).unwrap();
        service.request(&c, &dir.0).unwrap();
        c.scene.change_engine([4, 40, -2], Some(oak)).unwrap();
        c.scene.engine.as_mut().unwrap().player.slots[9] = Some(Stack { id: oak, count: 8 });
        service.request(&c, &dir.0).unwrap();
        c.scene.change_engine([5, 4, -2], Some(oak)).unwrap();
        c.scene.engine.as_mut().unwrap().player.slots[9] = Some(Stack { id: oak, count: 7 });
        let instance = c.scene.engine.as_ref().unwrap().instance;
        let revision = c.scene.engine.as_ref().unwrap().revision;
        let events = drain(&mut service, &mut c);
        assert_eq!(events.len(), 2);
        assert!(
            events
                .iter()
                .all(|e| e.adopted && e.off_thread && e.error.is_none())
        );
        assert_eq!(service.counters.peak_snapshots, 2);
        assert_eq!(service.counters.coalesced, 1);
        assert_eq!(service.counters.worker_jobs, 2);
        let first: serde_json::Value =
            serde_json::from_slice(&fs::read(events[0].path.as_ref().unwrap()).unwrap()).unwrap();
        let last: serde_json::Value =
            serde_json::from_slice(&fs::read(events[1].path.as_ref().unwrap()).unwrap()).unwrap();
        assert_eq!(first["edits"].as_array().unwrap().len(), 1);
        assert_eq!(last["edits"].as_array().unwrap().len(), 3);
        assert_eq!(last["inventory"]["slots"][9]["count"], 8);
        assert_eq!(
            c.scene.engine.as_ref().unwrap().player.slots[9]
                .unwrap()
                .count,
            7
        );
        assert_eq!(c.pending_edits(), 2);
        assert_eq!(c.scene.engine.as_ref().unwrap().instance, instance);
        assert_eq!(c.scene.engine.as_ref().unwrap().revision, revision);
        assert!(fs::read_dir(&dir.0).unwrap().all(|f| {
            f.unwrap()
                .path()
                .extension()
                .is_some_and(|e| e == "blockworld")
        }));
    }
    #[test]
    fn failed_or_stale_saves_do_not_adopt_a_false_baseline() {
        let dir = Temp::new();
        let mut c = engine_stream::generate(360036, Registry::built_in()).unwrap();
        let mut service = Service::new().unwrap();
        c.scene.change_engine([2, 3, -2], Some(3)).unwrap();
        let pending = c.pending_edits();
        let blocker = dir.0.join("not-a-directory");
        fs::write(&blocker, b"keep").unwrap();
        service.request(&c, &blocker).unwrap();
        let failed = drain(&mut service, &mut c);
        assert!(failed[0].error.is_some());
        assert_eq!(c.pending_edits(), pending);
        assert!(c.last_save.is_none());
        assert_eq!(fs::read(&blocker).unwrap(), b"keep");
        service.request(&c, &dir.0).unwrap();
        c = engine_stream::generate(360037, Registry::built_in()).unwrap();
        let stale = drain(&mut service, &mut c);
        assert!(!stale[0].adopted);
        assert!(stale[0].error.is_none());
        assert!(c.last_save.is_none());
        assert_eq!(c.pending_edits(), 0);
        service.request(&c, &dir.0).unwrap();
        let success = drain(&mut service, &mut c);
        let path = success[0].path.as_ref().unwrap();
        let original = fs::read(path).unwrap();
        let mut altered: serde_json::Value = serde_json::from_slice(&original).unwrap();
        altered["seed"] = serde_json::json!(999);
        let changed = serde_json::to_vec(&altered).unwrap();
        fs::write(path, &changed).unwrap();
        c.scene.change_engine([2, 3, -2], Some(3)).unwrap();
        let pending = c.pending_edits();
        service.request(&c, &dir.0).unwrap();
        assert!(drain(&mut service, &mut c)[0].error.is_some());
        assert_eq!(c.pending_edits(), pending);
        assert_eq!(fs::read(path).unwrap(), changed);
        assert!(engine::reload(&mut c).is_err());
        fs::write(path, original).unwrap();
    }
}
