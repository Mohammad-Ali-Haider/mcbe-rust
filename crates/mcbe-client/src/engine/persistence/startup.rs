//! Visible startup before CPU world/asset preparation, with bounded retry/cancel.
use crate::{
    engine,
    engine_blocks::Registry,
    scene::{Client, Result, Scene},
};
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc,
    },
    thread,
    time::Instant,
};

#[derive(Clone)]
pub struct Options {
    pub home: PathBuf,
    pub source: Option<PathBuf>,
    pub definitions: Option<PathBuf>,
    pub original: bool,
    pub pack_root: PathBuf,
    pub resume: bool,
    pub streamed: bool,
    pub modern: bool,
    pub seed: u64,
}
#[derive(Default)]
pub struct Progress {
    phase: AtomicU8,
    cancelled: AtomicBool,
}
impl Progress {
    fn phase(&self, phase: u8) -> Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err("Startup cancelled".into());
        }
        self.phase.store(phase, Ordering::Release);
        Ok(())
    }
}
impl Options {
    pub fn prepare(&self, progress: &Progress) -> Result<Client> {
        progress.phase(1)?;
        let mut source = self.source.clone();
        if source.is_none() && self.resume && self.home.join("saves").is_dir() {
            source = fs::read_dir(self.home.join("saves"))?
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path().extension().is_some_and(|s| s == "blockworld") && e.path().is_file()
                })
                .max_by_key(|e| (e.metadata().and_then(|m| m.modified()).ok(), e.file_name()))
                .map(|e| e.path());
        }
        let new_world = source.is_none();
        let local = crate::engine::content::loader::entry(&self.home);
        let pack = if new_world && local.file_name().is_some_and(|n| n == "pack.json") {
            Some(crate::engine::content::loader::read(&local)?)
        } else {
            None
        };
        progress.phase(2)?;
        let mut client = match source {
            Some(path) => engine::load(&path)?,
            None => {
                let registry = if let Some(pack) = &pack {
                    pack.registry.clone()
                } else if local.is_file() {
                    Registry::read(&local)?
                } else {
                    Registry::built_in()
                };
                if self.streamed {
                    if self.modern {
                        crate::engine_stream::generate_modern(
                            self.seed,
                            registry,
                            crate::engine::world::terrain::Settings::read_home(&self.home)?,
                        )?
                    } else {
                        crate::engine_stream::generate(self.seed, registry)?
                    }
                } else {
                    engine::generate_with_registry(self.seed, registry)?
                }
            }
        };
        if !new_world
            && self.definitions.is_none()
            && let Some(registry) = client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .registry
                .with_new_builtins()?
        {
            engine::apply_registry(&mut client, registry)?;
        }
        progress.phase(3)?;
        if new_world {
            if let Some(pack) = pack {
                client.scene.engine.as_mut().unwrap().recipes = pack.recipes;
            } else {
                let path = self.home.join("assets/recipes.json");
                if path.is_file() {
                    let recipes = crate::engine_crafting::Recipes::read(&path)?;
                    recipes.validate_registry(&client.scene.engine.as_ref().unwrap().registry)?;
                    client.scene.engine.as_mut().unwrap().recipes = recipes;
                }
            }
        }
        if let Some(path) = &self.definitions {
            let pack = if path.is_dir() || path.file_name().is_some_and(|n| n == "pack.json") {
                Some(crate::engine::content::loader::read(path)?)
            } else {
                None
            };
            let registry = pack
                .as_ref()
                .map(|p| p.registry.clone())
                .map_or_else(|| Registry::read(path), Ok)?;
            engine::apply_registry(&mut client, registry)?;
            if let Some(pack) = pack {
                client.scene.engine.as_mut().unwrap().recipes = pack.recipes;
            }
        }
        if new_world {
            let state = client.scene.engine.as_mut().unwrap();
            let definitions = crate::engine::machines::recipes::Definitions::read_home(&self.home)?;
            let definitions = if self
                .home
                .join("content/base/recipes/smelting.json")
                .is_file()
            {
                definitions
            } else {
                definitions.available(&state.registry)
            };
            state.smelting = definitions.compile(&state.registry)?;
        }
        client.scene.engine.as_mut().unwrap().ui = std::sync::Arc::new(
            crate::engine::ui::framework::Library::read_home(&self.home)?,
        );
        client.appearance = Arc::new(crate::engine::player::appearance::Appearance::read_home(
            &self.home,
        )?);
        progress.phase(4)?;
        if !self.original {
            client.engine_assets = Some(crate::engine_assets::load(
                &client.scene.engine.as_ref().unwrap().registry,
                &self.pack_root,
            )?);
        }
        client.owned_materials = Some(crate::engine_materials::load(
            &client.scene.engine.as_ref().unwrap().registry,
            &self.home,
        )?);
        progress.phase(5)?;
        // Loading a save is a snapshot operation, including the slower texture decode.
        if let Some((path, expected)) = &client.scene.engine.as_ref().unwrap().loaded {
            use sha2::{Digest, Sha256};
            if fs::metadata(path)?.len() > 32 * 1024 * 1024
                || format!("{:x}", Sha256::digest(fs::read(path)?)) != *expected
            {
                return Err("Loaded save changed on disk during startup".into());
            }
        }
        Ok(client)
    }
}
pub fn placeholder() -> Result<Client> {
    let mut registry = Registry::built_in();
    registry.blocks = vec![registry.blocks[2].clone()];
    let mut client = Client::new(Scene::from_engine(
        HashMap::new(),
        engine::WorldState::new(0, registry, 0, None),
    )?)?;
    client.set_spawn(engine::SPAWN)?;
    Ok(client)
}
#[derive(Default, Serialize)]
pub struct Counters {
    pub jobs: usize,
    pub completed: usize,
    pub applied: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub off_thread: bool,
    pub worker_ms: f64,
    pub apply_ms: f64,
    pub graphics_prepare_ms: f64,
    pub graphics_bytes: usize,
    pub graphics_writes: usize,
    pub graphics_cancelled: bool,
}
struct Finished {
    result: std::result::Result<Box<Client>, String>,
    off_thread: bool,
    milliseconds: f64,
    graphics_ms: f64,
    graphics_bytes: usize,
    graphics_writes: usize,
}
pub struct Service {
    pub options: Options,
    progress: Arc<Progress>,
    receiver: Option<mpsc::Receiver<Finished>>,
    retire: Option<mpsc::SyncSender<Option<Box<Client>>>>,
    thread: Option<thread::JoinHandle<()>>,
    pub ready: bool,
    pub graphics_ready: bool,
    pub error: Option<String>,
    pub counters: Counters,
    pub presented: usize,
    pub phase_trace: Vec<u8>,
}
impl Service {
    pub fn new(options: Options) -> Self {
        Self {
            options,
            progress: Arc::new(Progress::default()),
            receiver: None,
            retire: None,
            thread: None,
            ready: false,
            graphics_ready: false,
            error: None,
            counters: Default::default(),
            presented: 0,
            phase_trace: Vec::new(),
        }
    }
    pub fn blocked(&self) -> bool {
        !self.ready || !self.graphics_ready
    }
    pub fn running(&self) -> bool {
        self.receiver.is_some()
    }
    pub fn phase(&self) -> u8 {
        self.progress.phase.load(Ordering::Acquire)
    }
    pub fn message(&self) -> String {
        if let Some(error) = &self.error {
            return format!("Could not open world | Enter retry | Escape close | {error}");
        }
        if self.ready {
            return "Uploading graphics... | Escape close".into();
        }
        let phase = match self.phase() {
            0 => "Opening Blockwork",
            1 => "Finding saved world",
            2 => "Preparing terrain",
            3 => "Reading block definitions",
            4 => "Loading local textures",
            5 => "Checking prepared world",
            6 => "Preparing textured models",
            _ => "Preparing graphics",
        };
        format!("{phase}... | Escape close")
    }
    pub fn start(&mut self) -> Result<()> {
        if self.running() || self.ready {
            return Ok(());
        }
        self.join();
        self.progress = Arc::new(Progress::default());
        self.error = None;
        let progress = self.progress.clone();
        let options = self.options.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let (retire, garbage) = mpsc::sync_channel(1);
        let caller = thread::current().id();
        self.thread = Some(
            thread::Builder::new()
                .name("blockwork-startup".into())
                .spawn(move || {
                    let start = Instant::now();
                    let mut graphics_ms = 0.;
                    let mut graphics_bytes = 0;
                    let mut graphics_writes = 0;
                    let result = options
                        .prepare(&progress)
                        .and_then(|client| {
                            progress.phase(6)?;
                            let graphics_start = Instant::now();
                            let prepared = crate::engine_texture::Prepared::build(&client)?;
                            graphics_ms = graphics_start.elapsed().as_secs_f64() * 1000.;
                            graphics_bytes = prepared
                                .steps
                                .iter()
                                .map(|s| s.bytes(&prepared.atlas).len())
                                .sum();
                            graphics_writes = prepared.steps.len();
                            *client.engine_gpu_prepared.borrow_mut() = Some(prepared);
                            progress.phase(7)?;
                            if let Some((path, expected)) =
                                &client.scene.engine.as_ref().unwrap().loaded
                            {
                                use sha2::{Digest, Sha256};
                                if fs::metadata(path)?.len() > 32 * 1024 * 1024
                                    || format!("{:x}", Sha256::digest(fs::read(path)?)) != *expected
                                {
                                    return Err(
                                        "Loaded save changed on disk during graphics preparation"
                                            .into(),
                                    );
                                }
                            }
                            Ok(client)
                        })
                        .map(Box::new)
                        .map_err(|e| e.to_string());
                    let finished = Finished {
                        result,
                        off_thread: thread::current().id() != caller,
                        milliseconds: start.elapsed().as_secs_f64() * 1000.,
                        graphics_ms,
                        graphics_bytes,
                        graphics_writes,
                    };
                    if sender.send(finished).is_ok() {
                        let _ = garbage.recv();
                    } // Retired clients drop here, on the worker.
                })?,
        );
        self.retire = Some(retire);
        self.receiver = Some(receiver);
        self.counters.jobs += 1;
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.progress.cancelled.store(true, Ordering::Release);
    }
    pub fn poll(&mut self, client: &mut Client, closing: bool) -> Result<bool> {
        let phase = self.phase();
        if self.phase_trace.last() != Some(&phase) {
            self.phase_trace.push(phase);
        }
        let Some(receiver) = &self.receiver else {
            return Ok(false);
        };
        let finished = match receiver.try_recv() {
            Ok(v) => v,
            Err(mpsc::TryRecvError::Empty) => return Ok(false),
            Err(_) => return Err("Startup worker disconnected".into()),
        };
        self.receiver = None;
        self.counters.completed += 1;
        self.counters.off_thread = finished.off_thread;
        self.counters.worker_ms += finished.milliseconds;
        self.counters.graphics_prepare_ms += finished.graphics_ms;
        self.counters.graphics_bytes += finished.graphics_bytes;
        self.counters.graphics_writes += finished.graphics_writes;
        let mut retired = None;
        let mut applied = false;
        if closing || self.progress.cancelled.load(Ordering::Acquire) {
            self.counters.cancelled += 1;
            if let Ok(v) = finished.result {
                retired = Some(v);
            }
        } else {
            match finished.result {
                Ok(prepared) => {
                    let start = Instant::now();
                    retired = Some(Box::new(std::mem::replace(client, *prepared)));
                    self.counters.apply_ms = start.elapsed().as_secs_f64() * 1000.;
                    self.counters.applied += 1;
                    self.ready = true;
                    applied = true;
                }
                Err(error) => {
                    self.counters.failed += 1;
                    self.error = Some(error);
                }
            }
        }
        if let Some(sender) = self.retire.take() {
            let _ = sender.send(retired);
        }
        Ok(applied)
    }
    fn join(&mut self) {
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.cancel();
        self.receiver.take();
        self.retire.take();
        self.join();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_startup_retries_after_definition_file_is_repaired() {
        let home = std::env::temp_dir().join(format!(
            "blockwork-startup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&home).unwrap();
        let definitions = home.join("blocks.json");
        fs::write(&definitions, b"invalid JSON").unwrap();
        let options = Options {
            home: home.clone(),
            source: None,
            definitions: Some(definitions.clone()),
            original: true,
            pack_root: PathBuf::new(),
            resume: false,
            streamed: false,
            modern: false,
            seed: 1,
        };
        let mut service = Service::new(options);
        let mut client = placeholder().unwrap();
        let identity = client.scene.engine.as_ref().unwrap().instance;
        let finish = |s: &mut Service, c: &mut Client| {
            let start = Instant::now();
            while s.running() {
                s.poll(c, false).unwrap();
                assert!(start.elapsed().as_secs() < 10);
                thread::yield_now();
            }
        };
        service.start().unwrap();
        finish(&mut service, &mut client);
        assert!(!service.ready);
        assert!(service.error.is_some());
        assert_eq!(client.scene.engine.as_ref().unwrap().instance, identity);
        fs::write(
            &definitions,
            serde_json::to_vec(&Registry::built_in()).unwrap(),
        )
        .unwrap();
        service.start().unwrap();
        finish(&mut service, &mut client);
        assert!(service.ready);
        assert!(service.error.is_none());
        assert_eq!(service.counters.jobs, 2);
        assert_eq!(service.counters.failed, 1);
        assert_eq!(service.counters.applied, 1);
        assert!(service.counters.off_thread);
        assert!(client.scene.engine.as_ref().unwrap().blocks.len() > 40_000);
        assert_eq!(client.scene.engine.as_ref().unwrap().seed, 1);
        fs::remove_file(definitions).unwrap();
        fs::remove_dir(home).unwrap();
    }
    #[test]
    fn cancelled_startup_never_prepares_or_adopts_a_world() {
        let mut client = placeholder().unwrap();
        let identity = client.scene.engine.as_ref().unwrap().instance;
        let options = Options {
            home: PathBuf::new(),
            source: Some(PathBuf::from("missing-startup-world.blockworld")),
            definitions: None,
            original: true,
            pack_root: PathBuf::new(),
            resume: false,
            streamed: true,
            modern: false,
            seed: 1,
        };
        let progress = Progress::default();
        progress.cancelled.store(true, Ordering::Release);
        assert!(
            options
                .prepare(&progress)
                .err()
                .unwrap()
                .to_string()
                .contains("cancelled")
        );
        let mut service = Service::new(options);
        service.start().unwrap();
        service.cancel();
        let start = Instant::now();
        while service.running() {
            service.poll(&mut client, true).unwrap();
            assert!(start.elapsed().as_secs() < 5);
            thread::yield_now();
        }
        assert_eq!(client.scene.engine.as_ref().unwrap().instance, identity);
        assert!(!service.ready);
        assert_eq!(service.counters.cancelled, 1);
        assert!(service.counters.off_thread);
    }
}
