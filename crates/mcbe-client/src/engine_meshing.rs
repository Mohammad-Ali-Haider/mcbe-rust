//! Bounded CPU mesh jobs with owned halos and epoch/per-chunk result validation.
use crate::{
    engine::WorldState,
    engine_mesh::{ChunkMesh, Snapshot},
    scene::Result,
};
use std::{collections::BTreeMap, sync::mpsc, thread, time::Instant};
pub type Key = [i32; 3];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Token {
    epoch: u64,
    key: Key,
    version: u64,
}
enum Job {
    Build {
        token: Token,
        snapshot: Snapshot,
        greedy: bool,
    },
    Stop,
}
pub struct Completed {
    pub key: Key,
    pub mesh: ChunkMesh,
}
struct Built {
    token: Token,
    mesh: ChunkMesh,
    milliseconds: f64,
    off_thread: bool,
}
#[derive(Default)]
pub struct Counters {
    pub submitted: usize,
    pub received: usize,
    pub rejected: usize,
    pub off_thread: usize,
    pub peak_jobs: usize,
    pub snapshot_ms: f64,
    pub max_snapshot_ms: f64,
    pub worker_ms: f64,
    pub max_worker_ms: f64,
}
pub struct Scheduler {
    epoch: u64,
    needed: BTreeMap<Key, u64>,
    pending: BTreeMap<Key, Token>,
    sender: mpsc::Sender<Job>,
    receiver: mpsc::Receiver<Built>,
    thread: Option<thread::JoinHandle<()>>,
    pub counters: Counters,
}
impl Scheduler {
    pub fn new() -> Result<Self> {
        let (sender, jobs) = mpsc::channel();
        let (results, receiver) = mpsc::channel();
        let owner = thread::current().id();
        let thread = thread::Builder::new()
            .name("blockwork-mesh".into())
            .spawn(move || {
                while let Ok(Job::Build {
                    token,
                    snapshot,
                    greedy,
                }) = jobs.recv()
                {
                    let started = Instant::now();
                    let mesh = snapshot.build(greedy);
                    let built = Built {
                        token,
                        mesh,
                        milliseconds: started.elapsed().as_secs_f64() * 1000.,
                        off_thread: thread::current().id() != owner,
                    };
                    if results.send(built).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            epoch: 0,
            needed: BTreeMap::new(),
            pending: BTreeMap::new(),
            sender,
            receiver,
            thread: Some(thread),
            counters: Counters::default(),
        })
    }
    pub fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.needed.clear();
    }
    pub fn mark(&mut self, key: Key, version: u64) {
        self.needed.insert(key, version);
    }
    pub fn retain(&mut self, mut available: impl FnMut(Key) -> bool) {
        self.needed.retain(|k, _| available(*k));
    }
    pub fn jobs(&self) -> usize {
        self.pending.len()
    }
    pub fn remaining(&self) -> usize {
        self.needed.len()
    }
    pub fn ready(&self) -> bool {
        self.needed.is_empty() && self.pending.is_empty()
    }
    /// Receive at most two stale jobs or one accepted job in a frame.
    pub fn poll(&mut self) -> Result<Option<Completed>> {
        for _ in 0..2 {
            let built = match self.receiver.try_recv() {
                Ok(built) => built,
                Err(mpsc::TryRecvError::Empty) => return Ok(None),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("Mesh worker stopped unexpectedly".into());
                }
            };
            self.pending.remove(&built.token.key);
            self.counters.received += 1;
            self.counters.off_thread += usize::from(built.off_thread);
            self.counters.worker_ms += built.milliseconds;
            self.counters.max_worker_ms = self.counters.max_worker_ms.max(built.milliseconds);
            if built.token.epoch == self.epoch
                && self.needed.get(&built.token.key) == Some(&built.token.version)
            {
                self.needed.remove(&built.token.key);
                return Ok(Some(Completed {
                    key: built.token.key,
                    mesh: built.mesh,
                }));
            }
            self.counters.rejected += 1;
        }
        Ok(None)
    }
    pub fn submit(&mut self, state: &WorldState, eye: [f32; 3], greedy: bool) -> Result<()> {
        while self.pending.len() < 2 {
            let distance = |key: &Key| -> f32 {
                (0..3)
                    .map(|a| (key[a] as f32 * 16. + 8. - eye[a]).powi(2))
                    .sum()
            };
            let next = self
                .needed
                .iter()
                .filter(|(k, _)| !self.pending.contains_key(*k))
                .min_by(|(a, _), (b, _)| distance(a).total_cmp(&distance(b)))
                .map(|(k, v)| (*k, *v));
            let Some((key, version)) = next else {
                break;
            };
            let started = Instant::now();
            let snapshot = Snapshot::capture(state, key);
            let elapsed = started.elapsed().as_secs_f64() * 1000.;
            self.counters.snapshot_ms += elapsed;
            self.counters.max_snapshot_ms = self.counters.max_snapshot_ms.max(elapsed);
            let token = Token {
                epoch: self.epoch,
                key,
                version,
            };
            self.sender.send(Job::Build {
                token,
                snapshot,
                greedy,
            })?;
            self.pending.insert(key, token);
            self.counters.submitted += 1;
            self.counters.peak_jobs = self.counters.peak_jobs.max(self.pending.len());
        }
        Ok(())
    }
}
impl Drop for Scheduler {
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
    fn drain(scheduler: &mut Scheduler) -> Vec<Completed> {
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        let mut result = Vec::new();
        while scheduler.jobs() > 0 {
            if let Some(done) = scheduler.poll().unwrap() {
                result.push(done);
            }
            assert!(Instant::now() < deadline, "Mesh worker did not finish");
            thread::sleep(std::time::Duration::from_millis(1));
        }
        result
    }
    #[test]
    fn stale_edits_evictions_and_epochs_are_rejected_with_bounded_jobs() {
        let mut client = crate::engine::generate(360036).unwrap();
        let state = client.scene.engine.as_mut().unwrap();
        let key = [0, 0, 0];
        let mut scheduler = Scheduler::new().unwrap();
        scheduler.mark(key, 1);
        scheduler.submit(state, [0.; 3], true).unwrap();
        state.blocks.insert([2, 4, 2], 3);
        scheduler.mark(key, 2);
        assert!(drain(&mut scheduler).is_empty());
        assert_eq!(scheduler.remaining(), 1);
        scheduler.submit(state, [0.; 3], true).unwrap();
        let fresh = drain(&mut scheduler);
        assert_eq!(fresh.len(), 1);
        let expected = crate::engine_mesh::build(state, key);
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&fresh[0].mesh.vertices),
            bytemuck::cast_slice::<_, u8>(&expected.vertices)
        );
        scheduler.mark(key, 3);
        scheduler.submit(state, [0.; 3], true).unwrap();
        scheduler.retain(|_| false);
        assert!(drain(&mut scheduler).is_empty());
        scheduler.mark(key, 4);
        scheduler.submit(state, [0.; 3], true).unwrap();
        scheduler.invalidate();
        scheduler.mark(key, 4);
        assert!(drain(&mut scheduler).is_empty());
        scheduler.submit(state, [0.; 3], false).unwrap();
        assert_eq!(drain(&mut scheduler).len(), 1);
        assert!(scheduler.ready());
        for x in 0..100 {
            scheduler.mark([x, 0, 0], 5);
        }
        scheduler.submit(state, [0.; 3], true).unwrap();
        assert_eq!(scheduler.jobs(), 2);
        scheduler.invalidate();
        drain(&mut scheduler);
        assert_eq!(scheduler.counters.peak_jobs, 2);
        assert!(scheduler.counters.rejected >= 5);
        assert_eq!(scheduler.counters.off_thread, scheduler.counters.received);
    }
}
