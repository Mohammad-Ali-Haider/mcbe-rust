//! Engine terrain, player inventory, recipes and versioned saves.
//! Optional installed Minecraft texture bindings are handled in engine_assets.
use crate::engine_blocks::{Block, BlockId, Pattern, Registry};
use crate::{
    App, render,
    scene::{Client, Input, Result, Scene},
};
use mcbe_core::{motion::Vec3, streaming::Bounds};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const CHECKPOINT: u32 = 48;
pub const BOUNDS: Bounds = Bounds {
    min: [-40, 0, -40],
    max: [40, 40, 40],
};
pub const SPAWN: Vec3 = Vec3::new(0.5, 4., -9.5);
#[derive(Clone)]
pub struct WorldState {
    pub geometry: std::sync::Arc<crate::engine_geometry::Library>,
    pub instance: u64,
    pub registry_revision: u64,
    pub dirty_chunks: BTreeMap<[i32; 3], u64>,
    pub revision: u64,
    pub edit_revision: u64,
    pub seed: u64,
    pub loaded: Option<(PathBuf, String)>,
    pub registry: Registry,
    pub player: crate::engine_inventory::Inventory,
    pub original_player: crate::engine_inventory::Inventory,
    pub recipes: crate::engine_crafting::Recipes,
    pub blocks: HashMap<[i32; 3], BlockId>,
    pub original_blocks: HashMap<[i32; 3], BlockId>,
    pub stream: Option<crate::engine_stream::State>,
}
impl WorldState {
    pub fn new(
        seed: u64,
        registry: Registry,
        selected: BlockId,
        loaded: Option<(PathBuf, String)>,
    ) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let player = crate::engine_inventory::Inventory::starter(registry.blocks.len(), selected);
        let recipes = crate::engine_crafting::Recipes::built_in().available(&registry);
        Self {
            geometry: std::sync::Arc::new(
                crate::engine_geometry::Library::compile(&registry)
                    .expect("Validated geometry registry"),
            ),
            instance: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            registry_revision: 0,
            dirty_chunks: BTreeMap::new(),
            revision: 0,
            edit_revision: 0,
            seed,
            registry,
            original_player: player.clone(),
            player,
            recipes,
            loaded,
            blocks: HashMap::new(),
            original_blocks: HashMap::new(),
            stream: None,
        }
    }
    pub fn pending(&self) -> usize {
        if let Some(stream) = &self.stream {
            return stream.pending() + self.player.changed_slots(&self.original_player);
        }
        self.blocks
            .iter()
            .filter(|(p, id)| self.original_blocks.get(*p) != Some(*id))
            .count()
            + self
                .original_blocks
                .keys()
                .filter(|p| !self.blocks.contains_key(*p))
                .count()
            + self.player.changed_slots(&self.original_player)
    }
}
pub fn material(client: &Client) -> Option<&Block> {
    let state = client.scene.engine.as_ref().expect("Engine client");
    state.player.selected().map(|id| state.registry.get(id))
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cell {
    position: [i32; 3],
    material: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    movement: Option<crate::engine_player::Movement>,
    format: String,
    version: u32,
    seed: u64,
    player: Vec3,
    yaw: f32,
    pitch: f32,
    selected: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    cells: Vec<Cell>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    registry: Option<Registry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    inventory: Option<SavedInventory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recipes: Option<crate::engine_crafting::Recipes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generator: Option<crate::engine_stream::Config>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edits: Option<Vec<SavedEdit>>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedEdit {
    position: [i32; 3],
    material: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedStack {
    block: String,
    count: u8,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedInventory {
    slots: Vec<Option<SavedStack>>,
    crafting: Vec<Option<SavedStack>>,
    cursor: Option<SavedStack>,
    active: usize,
}
impl SavedInventory {
    fn capture(p: &crate::engine_inventory::Inventory, r: &Registry) -> Self {
        let convert = |s: Option<crate::engine_inventory::Stack>| {
            s.map(|s| SavedStack {
                block: r.get(s.id).id.clone(),
                count: s.count,
            })
        };
        Self {
            slots: p.slots.iter().copied().map(convert).collect(),
            crafting: p.crafting.iter().copied().map(convert).collect(),
            cursor: convert(p.cursor),
            active: p.active,
        }
    }
    fn restore(self, r: &Registry) -> Result<crate::engine_inventory::Inventory> {
        if self.slots.len() != 36 || self.crafting.len() != 4 {
            return Err("Saved player slot count differs".into());
        }
        let convert = |s: Option<SavedStack>| -> Result<Option<crate::engine_inventory::Stack>> {
            s.map(|s| {
                Ok(crate::engine_inventory::Stack {
                    id: r.find(&s.block)?,
                    count: s.count,
                })
            })
            .transpose()
        };
        let mut p = crate::engine_inventory::Inventory::empty();
        for (i, s) in self.slots.into_iter().enumerate() {
            p.slots[i] = convert(s)?;
        }
        for (i, s) in self.crafting.into_iter().enumerate() {
            p.crafting[i] = convert(s)?;
        }
        p.cursor = convert(self.cursor)?;
        p.active = self.active;
        p.validate(r.blocks.len())?;
        Ok(p)
    }
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn noise(x: i32, z: i32, seed: u64) -> u64 {
    let mut n = seed
        ^ (x as u64).wrapping_mul(0x9e3779b97f4a7c15)
        ^ (z as u64).wrapping_mul(0xbf58476d1ce4e5b9);
    n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
    n ^ (n >> 31)
}
#[cfg(test)]
pub fn generate(seed: u64) -> Result<Client> {
    generate_with_registry(seed, Registry::built_in())
}
pub fn generate_with_registry(seed: u64, registry: Registry) -> Result<Client> {
    let kinds = playground_blocks(seed, &registry)?;
    let clay = registry.find("clay")?;
    let mut client = Client::new(Scene::from_engine(
        kinds,
        WorldState::new(seed, registry, clay, None),
    )?)?;
    client.set_spawn(SPAWN)?;
    client.yaw = 0.;
    client.pitch = -0.08;
    Ok(client)
}
pub fn playground_blocks(seed: u64, registry: &Registry) -> Result<HashMap<[i32; 3], BlockId>> {
    registry.validate()?;
    let meadow = registry.find("meadow")?;
    let earth = registry.find("earth")?;
    let slate = registry.find("slate")?;
    let clay = registry.find("clay")?;
    let ivory = registry.find("ivory")?;
    let moss = registry.find("moss")?;
    let basalt = registry.find("basalt")?;
    let mut kinds = HashMap::new();
    for x in -40i32..40 {
        for z in -40i32..40 {
            let flat = x.abs() <= 16 && z.abs() <= 18;
            let h = if flat {
                4
            } else {
                5 + (noise(x.div_euclid(5), z.div_euclid(5), seed) % 5) as i32
            };
            for y in 0..h {
                let cube = if y == 0 {
                    basalt
                } else if y == h - 1 {
                    meadow
                } else if y >= h - 3 {
                    earth
                } else {
                    slate
                };
                kinds.insert([x, y, z], cube);
            }
        }
    }
    // A building court, arch, stepped overlook and eight material exhibits.
    for x in -9..=9 {
        for z in -3..=10 {
            kinds.insert([x, 3, z], slate);
        }
    }
    for x in [-6, 6] {
        for y in 4..=9 {
            for z in 6..=7 {
                kinds.insert([x, y, z], clay);
            }
        }
    }
    for x in -6..=6 {
        for z in 6..=7 {
            kinds.insert([x, 10, z], ivory);
        }
    }
    for i in 0..8 {
        for x in 10 + i..=19 {
            for z in -2..=2 {
                kinds.insert([x, 4 + i, z], slate);
            }
        }
    }
    for x in 18..=23 {
        for z in -4..=4 {
            kinds.insert([x, 12, z], ivory);
        }
    }
    for (i, _) in registry.blocks.iter().take(15).enumerate() {
        let x = -11 + (i % 8) as i32 * 3;
        let z = 14 + (i / 8) as i32 * 4;
        for y in 4..=6 {
            kinds.insert([x, y, z], i as BlockId);
        }
        kinds.insert([x, 7, z], ivory);
    }
    for [x, z] in [
        [-12, -11],
        [-13, 7],
        [13, -12],
        [11, 20],
        [-21, -20],
        [25, 17],
    ] {
        let ground = (0..20)
            .rev()
            .find(|y| kinds.contains_key(&[x, *y, z]))
            .unwrap()
            + 1;
        for y in ground..ground + 4 {
            kinds.insert([x, y, z], clay);
        }
        for dx in -2i32..=2 {
            for dz in -2i32..=2 {
                for dy in 2..=4 {
                    if dx.abs() + dz.abs() < 4 {
                        kinds.insert([x + dx, ground + dy, z + dz], moss);
                    }
                }
            }
        }
    }
    Ok(kinds)
}
#[derive(Clone)]
enum SavedTerrain {
    Finite(HashMap<[i32; 3], BlockId>),
    Stream(
        crate::engine_stream::Config,
        BTreeMap<[i32; 3], Option<BlockId>>,
    ),
}
pub struct SaveSnapshot {
    movement: crate::engine_player::Movement,
    pub instance: u64,
    registry_revision: u64,
    seed: u64,
    registry: Registry,
    player: crate::engine_inventory::Inventory,
    recipes: crate::engine_crafting::Recipes,
    terrain: SavedTerrain,
    position: Vec3,
    yaw: f32,
    pitch: f32,
    loaded: Option<(PathBuf, String)>,
}
#[derive(Serialize, Clone, Default)]
pub struct SaveTiming {
    pub serialize_ms: f64,
    pub validate_ms: f64,
    pub io_ms: f64,
    pub bytes: usize,
}
pub struct CommittedSave {
    pub snapshot: SaveSnapshot,
    pub path: PathBuf,
    pub sha256: String,
    pub timing: SaveTiming,
}
impl SaveSnapshot {
    pub fn capture(client: &Client) -> Result<Self> {
        let s = client.scene.engine.as_ref().ok_or("Engine world missing")?;
        Ok(Self {
            movement: client.movement,
            instance: s.instance,
            registry_revision: s.registry_revision,
            seed: s.seed,
            registry: s.registry.clone(),
            player: s.player.clone(),
            recipes: s.recipes.clone(),
            terrain: match &s.stream {
                Some(stream) => {
                    SavedTerrain::Stream(stream.generator.config.clone(), stream.edits.clone())
                }
                None => SavedTerrain::Finite(s.blocks.clone()),
            },
            position: client.actor.position,
            yaw: client.yaw,
            pitch: client.pitch,
            loaded: s.loaded.clone(),
        })
    }
    pub fn refresh_source(&mut self, client: &Client) {
        if let Some(state) = &client.scene.engine
            && state.instance == self.instance
        {
            self.loaded = state.loaded.clone();
        }
    }
    fn document(&self) -> Result<Document> {
        self.registry.validate()?;
        self.player.validate(self.registry.blocks.len())?;
        let id = |id: BlockId| -> Result<String> {
            Ok(self
                .registry
                .blocks
                .get(usize::from(id))
                .ok_or("Unknown snapshot block")?
                .id
                .clone())
        };
        let (cells, generator, edits) = match &self.terrain {
            SavedTerrain::Finite(blocks) => {
                let sorted: BTreeMap<_, _> = blocks.iter().map(|(p, id)| (*p, *id)).collect();
                (
                    sorted
                        .into_iter()
                        .map(|(position, block)| {
                            Ok(Cell {
                                position,
                                material: id(block)?,
                            })
                        })
                        .collect::<Result<Vec<_>>>()?,
                    None,
                    None,
                )
            }
            SavedTerrain::Stream(config, edits) => (
                Vec::new(),
                Some(config.clone()),
                Some(
                    edits
                        .iter()
                        .map(|(position, block)| {
                            Ok(SavedEdit {
                                position: *position,
                                material: block.map(id).transpose()?,
                            })
                        })
                        .collect::<Result<Vec<_>>>()?,
                ),
            ),
        };
        Ok(Document {
            movement: Some(self.movement),
            format: "blockwork-world".into(),
            version: 5,
            seed: self.seed,
            player: self.position,
            yaw: self.yaw,
            pitch: self.pitch,
            selected: self
                .player
                .selected()
                .map(id)
                .transpose()?
                .unwrap_or_default(),
            cells,
            registry: Some(self.registry.clone()),
            inventory: Some(SavedInventory::capture(&self.player, &self.registry)),
            recipes: Some(self.recipes.clone()),
            generator,
            edits,
        })
    }
    fn check_source(&self) -> Result<()> {
        if let Some((path, expected)) = &self.loaded
            && (fs::metadata(path)?.len() > 32 * 1024 * 1024 || hash(&fs::read(path)?) != *expected)
        {
            return Err("Loaded save changed on disk; save cancelled".into());
        }
        Ok(())
    }
    pub fn write(self, directory: &Path) -> Result<CommittedSave> {
        use std::time::Instant;
        self.check_source()?;
        let started = Instant::now();
        let bytes = serde_json::to_vec(&self.document()?)?;
        let serialize_ms = started.elapsed().as_secs_f64() * 1000.;
        let started = Instant::now();
        {
            let _validated = decode(&bytes, None)?;
        }
        let validate_ms = started.elapsed().as_secs_f64() * 1000.;
        let digest = hash(&bytes);
        let started = Instant::now();
        fs::create_dir_all(directory)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        for i in 0..100 {
            let path = directory.join(format!("world-{stamp}-{i}.blockworld"));
            let staged = directory.join(format!(".world-{stamp}-{i}.part"));
            let mut file = match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staged)
            {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            };
            let _cleanup = PendingFile(staged.clone());
            if path.exists() {
                drop(file);
                continue;
            }
            let wrote = (|| -> std::io::Result<()> {
                file.write_all(&bytes)?;
                file.sync_all()
            })();
            drop(file);
            wrote?;
            if hash(&fs::read(&staged)?) != digest {
                return Err("Staged world bytes differ; save cancelled".into());
            }
            self.check_source()?;
            match publish_unique(&staged, &path) {
                Ok(()) => {
                    let path = path.canonicalize()?;
                    let timing = SaveTiming {
                        serialize_ms,
                        validate_ms,
                        io_ms: started.elapsed().as_secs_f64() * 1000.,
                        bytes: bytes.len(),
                    };
                    return Ok(CommittedSave {
                        snapshot: self,
                        path,
                        sha256: digest,
                        timing,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Err("No unique engine save name available".into())
    }
}
struct PendingFile(PathBuf);
impl Drop for PendingFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
#[cfg(windows)]
fn publish_unique(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let from: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<_> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both UTF-16 paths are terminated and remain alive for the call.
    // No replace/copy flags: same-directory publication fails if the name exists.
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::MoveFileExW(from.as_ptr(), to.as_ptr(), 0)
    } == 0
    {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
fn publish_unique(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::hard_link(source, destination)?;
    fs::remove_file(source)
}
impl CommittedSave {
    pub fn adopt(self, client: &mut Client) -> Result<bool> {
        let Some(state) = client.scene.engine.as_mut() else {
            return Ok(false);
        };
        if state.instance != self.snapshot.instance
            || state.registry_revision != self.snapshot.registry_revision
        {
            return Ok(false);
        }
        match (self.snapshot.terrain, &mut state.stream) {
            (SavedTerrain::Stream(_, edits), Some(stream)) => stream.original_edits = edits,
            (SavedTerrain::Finite(blocks), None) => state.original_blocks = blocks,
            _ => return Ok(false),
        }
        state.original_player = self.snapshot.player;
        state.loaded = Some((self.path.clone(), self.sha256));
        client.last_save = Some(self.path);
        Ok(true)
    }
}
#[cfg(test)]
fn document(client: &Client) -> Result<Document> {
    SaveSnapshot::capture(client)?.document()
}
fn decode(bytes: &[u8], loaded: Option<(PathBuf, String)>) -> Result<Client> {
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("Engine save exceeds 32 MiB".into());
    }
    let d: Document = serde_json::from_slice(bytes)?;
    if d.format != "blockwork-world"
        || ![1, 2, 3, 4, 5].contains(&d.version)
        || d.cells.len() > BOUNDS.volume()?
        || !d.yaw.is_finite()
        || !d.pitch.is_finite()
        || !(-1.5..=1.5).contains(&d.pitch)
    {
        return Err("Unsupported or invalid engine save".into());
    }
    let registry = match (d.version, d.registry) {
        (1, None) => {
            let mut r = Registry::built_in();
            r.blocks.truncate(8);
            r
        }
        (2..=5, Some(r)) => {
            r.validate()?;
            r
        }
        _ => return Err("Save registry/version mismatch".into()),
    };
    let player = match (d.version, d.inventory) {
        (1 | 2, None) => crate::engine_inventory::Inventory::starter(
            registry.blocks.len(),
            registry.find(&d.selected)?,
        ),
        (3..=5, Some(p)) => p.restore(&registry)?,
        _ => return Err("Save inventory/version mismatch".into()),
    };
    if d.version >= 3
        && player
            .selected()
            .map_or_else(String::new, |id| registry.get(id).id.clone())
            != d.selected
    {
        return Err("Saved hotbar selection header differs".into());
    }
    let recipes = match (d.version, d.recipes) {
        (1 | 2, None) => crate::engine_crafting::Recipes::built_in().available(&registry),
        (3..=5, Some(r)) => {
            r.validate_registry(&registry)?;
            r
        }
        _ => return Err("Save recipe/version mismatch".into()),
    };
    let movement = match (d.version, d.movement) {
        (5, Some(m)) => m,
        (1..=4, None) => Default::default(),
        _ => return Err("Save movement/version mismatch".into()),
    };
    if d.version == 4 || d.version == 5 && d.generator.is_some() {
        if !d.cells.is_empty() {
            return Err("Streamed saves contain sparse edits, not full cells".into());
        }
        let config = d.generator.ok_or("Streamed generator missing")?;
        let generator = crate::engine_stream::Generator::new(d.seed, &registry, config)?;
        let mut stream = crate::engine_stream::State::new(generator);
        let edits = d.edits.ok_or("Streamed edits missing")?;
        if edits.len() > crate::engine_stream::MAX_EDITS {
            return Err("Streamed save edit budget exceeded".into());
        }
        for edit in edits {
            let id = edit.material.map(|id| registry.find(&id)).transpose()?;
            if !crate::engine_stream::BOUNDS.contains(edit.position)
                || edit.position[1] == 0
                || stream.generator.block(edit.position) == id
                || stream.edits.insert(edit.position, id).is_some()
            {
                return Err(
                    "Duplicate, redundant, foundation or out-of-bounds streamed edit".into(),
                );
            }
        }
        stream.original_edits = stream.edits.clone();
        let mut state = WorldState::new(d.seed, registry, player.selected().unwrap_or(0), loaded);
        state.player = player.clone();
        state.original_player = player;
        state.recipes = recipes;
        state.stream = Some(stream);
        let mut client = Client::new(Scene::from_engine(HashMap::new(), state)?)?;
        client.scene.bounds = crate::engine_stream::BOUNDS;
        client.movement = movement;
        client.set_spawn(d.player)?;
        client.yaw = d.yaw;
        client.pitch = d.pitch;
        return Ok(client);
    }
    if d.generator.is_some() || d.edits.is_some() {
        return Err("Legacy save cannot contain streamed fields".into());
    }
    let basalt = registry.find("basalt")?;
    let mut kinds = HashMap::new();
    for cell in d.cells {
        if !BOUNDS.contains(cell.position)
            || kinds
                .insert(cell.position, registry.find(&cell.material)?)
                .is_some()
        {
            return Err("Duplicate or out-of-bounds saved block".into());
        }
    }
    for x in -40..40 {
        for z in -40..40 {
            if kinds.get(&[x, 0, z]) != Some(&basalt) {
                return Err("Engine foundation missing".into());
            }
        }
    }
    let mut client = Client::new(Scene::from_engine(
        kinds,
        WorldState::new(d.seed, registry, player.selected().unwrap_or(0), loaded),
    )?)?;
    {
        let state = client.scene.engine.as_mut().unwrap();
        state.original_player = player.clone();
        state.player = player;
        state.recipes = recipes;
    }
    client.movement = movement;
    client.set_spawn(d.player)?;
    client.yaw = d.yaw;
    client.pitch = d.pitch;
    Ok(client)
}
pub fn load(path: &Path) -> Result<Client> {
    if fs::metadata(path)?.len() > 32 * 1024 * 1024 {
        return Err("Engine save exceeds 32 MiB".into());
    }
    let bytes = fs::read(path)?;
    let path = path.canonicalize()?;
    let mut client = decode(&bytes, Some((path.clone(), hash(&bytes))))?;
    client.last_save = Some(path);
    Ok(client)
}
pub fn save(client: &mut Client, directory: &Path) -> Result<PathBuf> {
    let committed = SaveSnapshot::capture(client)?.write(directory)?;
    let path = committed.path.clone();
    if !committed.adopt(client)? {
        return Err("Save baseline changed before adoption".into());
    }
    Ok(path)
}
pub fn reload(client: &mut Client) -> Result<()> {
    if let Some((path, expected)) = client.scene.engine.as_ref().and_then(|s| s.loaded.as_ref())
        && (fs::metadata(path)?.len() > 32 * 1024 * 1024 || hash(&fs::read(path)?) != *expected)
    {
        return Err("Loaded save changed on disk; reload cancelled".into());
    }
    let path = client
        .last_save
        .as_ref()
        .ok_or("Press F5 to create your first save")?;
    let mut prepared = load(path)?;
    if let Some(assets) = &client.engine_assets {
        prepared.engine_assets = Some(crate::engine_assets::load(
            &prepared.scene.engine.as_ref().unwrap().registry,
            &assets.root,
        )?);
    }
    *client = prepared;
    Ok(())
}
pub fn reset(client: &mut Client) -> Result<()> {
    if client
        .scene
        .engine
        .as_ref()
        .and_then(|s| s.stream.as_ref())
        .is_some()
    {
        return crate::engine_stream::jump(client, 0);
    }
    for z in -12..=0 {
        for x in -4..=4 {
            let y = (0..36)
                .rev()
                .find(|y| client.scene.cells.contains(&[x, *y, z]))
                .unwrap_or(0)
                + 1;
            if client
                .set_spawn(Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5))
                .is_ok()
            {
                return Ok(());
            }
        }
    }
    Err("No clear reset location found".into())
}
pub fn color(m: &Block, face: usize, u: f64, v: f64) -> [f64; 3] {
    let mut rgb = m.color.map(f64::from);
    if matches!(m.pattern, Pattern::Meadow) && face != 0 {
        rgb = m.side_color.unwrap_or(m.color).map(f64::from);
        if face != 1 && v > 0.82 {
            rgb = m.color.map(f64::from);
        }
    }
    let tx = (u * 16.).floor() as i32;
    let ty = (v * 16.).floor() as i32;
    let seed =
        m.id.bytes()
            .fold(0u64, |a, b| a.wrapping_mul(31) + u64::from(b));
    let grain = if matches!(m.pattern, Pattern::Solid) {
        0.
    } else {
        (noise(tx, ty, seed) % 13) as f64 - 6.
    };
    let shade = match m.pattern {
        Pattern::Bricks if ty % 8 == 0 || (tx + (ty / 8 % 2) * 8) % 16 == 0 => 0.78,
        Pattern::Checker if (tx / 4 + ty / 4) % 2 == 0 => 0.8,
        _ => 1.,
    };
    rgb.map(|c| (c + grain) * shade)
}
pub fn apply_registry(client: &mut Client, registry: Registry) -> Result<()> {
    registry.validate()?;
    let state = client.scene.engine.as_ref().ok_or("Engine world missing")?;
    let remap = |blocks: &HashMap<[i32; 3], BlockId>| -> Result<HashMap<[i32; 3], BlockId>> {
        blocks
            .iter()
            .map(|(p, id)| Ok((*p, registry.find(&state.registry.get(*id).id)?)))
            .collect()
    };
    let stream = if let Some(old) = &state.stream {
        let mut next = old.clone();
        next.generator = std::sync::Arc::new(crate::engine_stream::Generator::new(
            state.seed,
            &registry,
            old.generator.config.clone(),
        )?);
        let remap_edits=|edits:&BTreeMap<[i32;3],Option<BlockId>>|->Result<BTreeMap<[i32;3],Option<BlockId>>> { edits.iter().map(|(p,id)|Ok((*p,id.map(|id|registry.find(&state.registry.get(id).id)).transpose()?))).collect() };
        next.edits = remap_edits(&old.edits)?;
        next.original_edits = remap_edits(&old.original_edits)?;
        Some(next)
    } else {
        None
    };
    let worker = stream
        .as_ref()
        .map(|s| crate::engine_stream::Worker::new(s.generator.clone()))
        .transpose()?;
    let blocks = remap(&state.blocks)?;
    let original_blocks = remap(&state.original_blocks)?;
    let remap_player =
        |old: &crate::engine_inventory::Inventory| -> Result<crate::engine_inventory::Inventory> {
            let mut next = old.clone();
            for stack in next
                .slots
                .iter_mut()
                .chain(&mut next.crafting)
                .chain(std::iter::once(&mut next.cursor))
                .flatten()
            {
                stack.id = registry.find(&state.registry.get(stack.id).id)?;
            }
            next.revision = next.revision.wrapping_add(1);
            next.validate(registry.blocks.len())?;
            Ok(next)
        };
    let player = remap_player(&state.player)?;
    let original_player = remap_player(&state.original_player)?;
    let recipes = state.recipes.clone().available(&registry);
    let shapes = registry.shapes()?;
    let geometry = std::sync::Arc::new(crate::engine_geometry::Library::compile(&registry)?);
    if client.movement.mode != crate::engine_player::Mode::Noclip {
        let p = client.actor.position;
        let half = client.actor.body.width * 0.5;
        let low = [p.x - half, p.y, p.z - half];
        let high = [p.x + half, p.y + client.actor.body.height, p.z + half];
        if blocks.iter().any(|(p, id)| {
            geometry.collision(*id).iter().any(|b| {
                (0..3).all(|a| low[a] < p[a] as f32 + b[a + 3] && high[a] > p[a] as f32 + b[a])
            })
        }) {
            return Err("Updated model overlaps the player; move to clear space before F6".into());
        }
    }
    let state = client.scene.engine.as_mut().unwrap();
    state.blocks = blocks;
    state.geometry = geometry;
    state.stream = stream;
    client.engine_worker = worker;
    state.original_blocks = original_blocks;
    state.player = player;
    state.original_player = original_player;
    state.recipes = recipes;
    state.registry = registry;
    state.revision = state.revision.wrapping_add(1);
    state.registry_revision = state.revision;
    client.scene.shapes = shapes;
    Ok(())
}
pub fn reload_definitions(client: &mut Client, path: &Path) -> Result<()> {
    let registry = Registry::read(path)?;
    let recipe_path = path
        .parent()
        .ok_or("Block definition directory missing")?
        .join("recipes.json");
    let recipes = if recipe_path.is_file() {
        crate::engine_crafting::Recipes::read(&recipe_path)?
    } else {
        client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .recipes
            .clone()
            .available(&registry)
    };
    recipes.validate_registry(&registry)?;
    let assets = match &client.engine_assets {
        Some(a) => Some(crate::engine_assets::load(&registry, &a.root)?),
        None => None,
    };
    let home = client
        .owned_materials
        .as_ref()
        .map(|a| a.root.as_path())
        .unwrap_or(path.parent().unwrap().parent().unwrap_or(Path::new(".")));
    let owned = crate::engine_materials::load(&registry, home)?;
    apply_registry(client, registry)?;
    client.owned_materials = Some(owned);
    client.scene.engine.as_mut().unwrap().recipes = recipes;
    client.engine_assets = assets;
    Ok(())
}

pub fn launch(args: &[String]) -> Result<()> {
    let mut home = std::env::current_exe()?
        .parent()
        .ok_or("Executable directory missing")?
        .to_path_buf();
    let mut source = None;
    let mut definitions = None;
    let mut original = false;
    let mut pack_root = PathBuf::from(crate::engine_assets::DEFAULT_ROOT);
    let mut resume = false;
    let mut out = None;
    let mut smoke = false;
    let mut seed = 360036u64;
    let mut camera = 0u8;
    let mut preview_case = "neutral".to_owned();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--engine" => {},
            "--engine-original" => original=true,
            "--engine-naive" => {},
            "--engine-stream" | "--engine-finite" | "--engine-sync-mesh" => {},
            "--engine-continue" => resume = true,
            "--engine-home" | "--engine-load" | "--engine-study" | "--engine-features-smoke" | "--engine-settings-smoke" | "--engine-preview-smoke" | "--engine-preview-case" | "--engine-startup-gpu-close-smoke" | "--engine-startup-retry-smoke" | "--engine-startup-smoke" | "--engine-startup-close-smoke" | "--engine-startup-error-smoke" | "--engine-v1-study" | "--engine-v1-smoke" | "--engine-v1-wire-smoke" | "--engine-v1-fly-smoke" | "--engine-v1-noclip-smoke" | "--engine-v1-sneak-smoke" | "--engine-loading-study" | "--engine-loading-smoke" | "--engine-loading-close-smoke" | "--engine-save-study" | "--engine-save-smoke" | "--engine-save-close-smoke" | "--engine-async-smoke" | "--engine-stream-profile" | "--engine-stream-study" | "--engine-stream-smoke" | "--engine-stream-wire-smoke" | "--engine-smoke" | "--engine-palette-smoke" | "--engine-wire-smoke" | "--engine-window-smoke" | "--engine-edit-smoke" | "--engine-benchmark" | "--engine-profile" | "--engine-render-smoke" | "--engine-mesh-smoke" | "--engine-camera" | "--engine-inventory-study" | "--engine-inventory-smoke" | "--engine-seed" | "--engine-blocks" | "--engine-pack-root" => {
                let flag = &args[i]; i+=1; let value = args.get(i).ok_or("Engine option requires a value")?;
                match flag.as_str() {
                    "--engine-home" => home=PathBuf::from(value),
                    "--engine-blocks" => definitions=Some(PathBuf::from(value)),
                    "--engine-pack-root" => pack_root=PathBuf::from(value),
                    "--engine-load" => source=Some(PathBuf::from(value)),
                    "--engine-seed" => seed=value.parse()?,
                    "--engine-preview-case" => preview_case=value.clone(),
                    "--engine-camera" => { camera=value.parse()?; if camera>3 { return Err("Engine camera must be 0..3".into()); } },
                    _ => { out=Some(PathBuf::from(value)); smoke = !flag.ends_with("-study"); },
                }
            },
            _ => return Err("Engine: --engine [--engine-home DIR] [--engine-load FILE] [--engine-seed INTEGER] [--engine-study NEW_OUT | --engine-smoke NEW_OUT]".into()),
        }
        i += 1;
    }
    let startup_test = args.iter().any(|a| a.starts_with("--engine-startup-"));
    let streamed = !args.iter().any(|a| a == "--engine-finite")
        && (out.is_none()
            || startup_test
            || args.iter().any(|a| {
                a == "--engine-stream"
                    || a.starts_with("--engine-save-")
                    || a.starts_with("--engine-loading-")
                    || a.starts_with("--engine-v1-")
                    || a == "--engine-async-smoke"
                    || a.starts_with("--engine-stream-")
            }));
    let options = crate::engine_startup::Options {
        home: home.clone(),
        source,
        definitions,
        original,
        pack_root,
        resume,
        streamed,
        seed,
    };
    let background = out.is_none() || startup_test;
    let client = if background {
        crate::engine_startup::placeholder()?
    } else {
        options.prepare(&Default::default())?
    };
    let mut app = App::new(client, home, false, smoke)?;
    if !smoke {
        match crate::engine_settings::Settings::read(&app.root) {
            Ok(s) => app.settings = s,
            Err(e) => app.notice = Some(format!("Settings: {e}")),
        }
    }
    if background {
        app.startup = Some(crate::engine_startup::Service::new(options));
        app.notice = Some("Opening Blockwork... | Escape close".into());
    }
    if startup_test {
        let destination = out.as_ref().ok_or("Startup proof output missing")?;
        if destination.exists() {
            return Err("Fresh startup proof directory required".into());
        }
        fs::create_dir_all(destination)?;
        app.frame_limit = 180;
        app.startup_gpu_close_test = args.iter().any(|a| a == "--engine-startup-gpu-close-smoke");
        app.startup_close_test = args.iter().any(|a| a == "--engine-startup-close-smoke");
        app.startup_error_test = args.iter().any(|a| a == "--engine-startup-error-smoke");
        app.startup_retry_test = args.iter().any(|a| a == "--engine-startup-retry-smoke");
        if app.startup_retry_test {
            let path = destination.join("retry-blocks.json");
            fs::write(&path, b"invalid JSON")?;
            app.startup.as_mut().unwrap().options.definitions = Some(path);
        }
    }
    app.naive_mesh = args.iter().any(|a| a == "--engine-naive");
    app.sync_mesh = args.iter().any(|a| a == "--engine-sync-mesh");
    if args.iter().any(|a| a == "--engine-mesh-smoke") {
        app.mesh_toggle_test = true;
        app.frame_limit = 6;
        app.wireframe = true;
    }
    if args.iter().any(|a| a == "--engine-window-smoke") {
        app.window_test = true;
        app.frame_limit = 8;
    }
    if args.iter().any(|a| a == "--engine-edit-smoke") {
        app.edit_test = true;
        app.frame_limit = 4;
    }
    if let Some(out) = &out {
        if args
            .iter()
            .any(|a| a == "--engine-features-smoke" || a == "--engine-settings-smoke")
        {
            crate::engine_features_study::prepare(
                &mut app,
                out,
                args.iter().any(|a| a == "--engine-settings-smoke"),
            )?;
        } else if args.iter().any(|a| a == "--engine-preview-smoke") {
            if out.exists() {
                return Err("Fresh preview proof directory required".into());
            }
            fs::create_dir_all(out)?;
            app.engine_key(winit::keyboard::KeyCode::KeyE)?;
            let [left, top] = crate::engine_ui::Palette::origin(app.palette.viewport);
            let point = match preview_case.as_str() {
                "left" => [left - 90., top + 50.],
                "right" => [left + 416., top + 50.],
                "up" => [left + 202., top - 90.],
                "down" => [left + 202., top + 256.],
                "neutral" | "held" | "scaled" | "sneak" | "live" => [left + 202., top + 25.5],
                _ => return Err("Unknown player preview case".into()),
            };
            app.palette
                .move_pointer(&app.client, point, app.palette.viewport);
            if preview_case == "held" {
                let id = app
                    .client
                    .scene
                    .engine
                    .as_ref()
                    .unwrap()
                    .registry
                    .find("oak_log")?;
                app.client.scene.engine.as_mut().unwrap().player.cursor =
                    Some(crate::engine_inventory::Stack { id, count: 3 });
            }
            if preview_case == "sneak" {
                app.client.tick(Input {
                    sneak: true,
                    ..Default::default()
                })?;
            }
            app.preview_test = Some(preview_case);
            app.frame_limit = 60;
            app.notice = None;
        } else if startup_test {
            // The production startup service is exercised inside the native event loop.
        } else if args.iter().any(|a| a.starts_with("--engine-v1-")) {
            crate::engine_v1_study::verify(&mut app, out)?;
            app.v1_test = smoke;
            app.frame_limit = 180;
            if args.iter().any(|a| a == "--engine-v1-wire-smoke") {
                app.engine_key(winit::keyboard::KeyCode::F4)?;
            }
            if args.iter().any(|a| a == "--engine-v1-fly-smoke") {
                app.engine_key(winit::keyboard::KeyCode::KeyF)?;
                for _ in 0..5 {
                    app.client.tick(Input {
                        jump: true,
                        ..Default::default()
                    })?;
                }
            }
            if args.iter().any(|a| a == "--engine-v1-noclip-smoke") {
                app.engine_key(winit::keyboard::KeyCode::KeyN)?;
            }
            if args.iter().any(|a| a == "--engine-v1-sneak-smoke") {
                app.client.tick(Input {
                    sneak: true,
                    ..Default::default()
                })?;
            }
            app.notice = None;
        } else if args.iter().any(|a| a == "--engine-loading-close-smoke") {
            if out.exists() {
                return Err("Fresh loading-close verification folder required".into());
            }
            fs::create_dir_all(out)?;
            app.root = out.clone();
            app.load_close_test = true;
            app.frame_limit = usize::MAX;
        } else if args
            .iter()
            .any(|a| a == "--engine-loading-study" || a == "--engine-loading-smoke")
        {
            crate::engine_loading_study::verify(&mut app, out)?;
            app.load_test = smoke;
            app.frame_limit = usize::MAX;
            app.load_origin = app.client.actor.position;
        } else if args.iter().any(|a| a == "--engine-save-close-smoke") {
            if out.exists() {
                return Err("Fresh close-save verification folder required".into());
            }
            fs::create_dir_all(out)?;
            app.root = out.clone();
            app.save_close_test = true;
            app.frame_limit = usize::MAX;
        } else if args
            .iter()
            .any(|a| a == "--engine-save-study" || a == "--engine-save-smoke")
        {
            crate::engine_save_study::verify(&mut app, out)?;
            app.save_test = smoke;
            app.frame_limit = 180;
        } else if args.iter().any(|a| a == "--engine-async-smoke") {
            if out.exists() {
                return Err("Fresh asynchronous mesh fixture folder required".into());
            }
            fs::create_dir_all(out)?;
            app.root = out.clone();
            app.async_test = true;
            app.frame_limit = 180;
        } else if args.iter().any(|a| {
            a == "--engine-stream-study"
                || a == "--engine-stream-profile"
                || a == "--engine-stream-smoke"
                || a == "--engine-stream-wire-smoke"
        }) {
            crate::engine_stream_study::verify(&mut app, out)?;
            app.stream_test = smoke;
            if args.iter().any(|a| a == "--engine-stream-profile") {
                app.stream_tour_frames = 1680;
            }
            app.frame_limit = app.stream_tour_frames;
            app.debug = true;
            if args.iter().any(|a| a == "--engine-stream-wire-smoke") {
                app.engine_key(winit::keyboard::KeyCode::F4)?;
            }
        } else if args
            .iter()
            .any(|a| a == "--engine-inventory-study" || a == "--engine-inventory-smoke")
        {
            crate::engine_inventory_study::verify(&mut app, out)?;
            app.inventory_test = smoke;
            app.frame_limit = 4;
        } else if args.iter().any(|a| {
            a == "--engine-benchmark" || a == "--engine-profile" || a == "--engine-render-smoke"
        }) {
            if out.exists() {
                return Err("Fresh GPU benchmark folder required".into());
            }
            fs::create_dir_all(out)?;
            app.frame_limit = if args.iter().any(|a| a == "--engine-render-smoke") {
                3
            } else {
                240
            };
            if args.iter().any(|a| a == "--engine-profile") {
                app.profile_seconds = Some(10.);
                app.frame_limit = usize::MAX;
            }
        } else {
            verify(&mut app, out)?;
        }
        if args.iter().any(|a| a == "--engine-wire-smoke") {
            app.engine_key(winit::keyboard::KeyCode::F4)?;
        }
        if args.iter().any(|a| a == "--engine-palette-smoke") {
            app.engine_key(winit::keyboard::KeyCode::KeyE)?;
        }
        if camera != 0 {
            let (position, yaw, pitch) = match camera {
                1 => (Vec3::new(0.5, 4., -2.5), 0., -0.65),
                2 => (Vec3::new(20.5, 13., 0.5), -1.4, -0.4),
                _ => (Vec3::new(-30.5, 12., -30.5), 0.8, -0.5),
            };
            app.client.set_spawn(position)?;
            app.client.yaw = yaw;
            app.client.pitch = pitch;
        }
        if !smoke {
            return Ok(());
        }
    }
    crate::run_window(app, out)
}

fn image(client: &Client, out: &Path, name: &str) -> Result<()> {
    Ok(render::png(
        &out.join(name),
        &render::render(client, 960, 540),
        960,
        540,
    )?)
}
fn performance(client: &mut Client) -> Result<serde_json::Value> {
    use std::time::Instant;
    let yaw = client.yaw;
    let pitch = client.pitch;
    let mut modes = Vec::new();
    let mut reference = Vec::new();
    let mut identical = true;
    for cached in [false, true] {
        client.engine_cache_enabled = cached;
        *client.engine_render_cache.borrow_mut() = None;
        let mut times = Vec::new();
        let mut hashes = Vec::new();
        for i in 0..86 {
            client.yaw = (i % 8) as f32 * 0.73;
            client.pitch = -0.1 - (i % 3) as f32 * 0.14;
            let start = Instant::now();
            let pixels = render::render(client, render::WIDTH, render::HEIGHT);
            let ms = start.elapsed().as_secs_f64() * 1000.;
            if i >= 6 {
                times.push(ms);
                let bytes: Vec<u8> = pixels.iter().flat_map(|p| p.to_le_bytes()).collect();
                hashes.push(hash(&bytes));
            }
        }
        times.sort_by(f64::total_cmp);
        let median = times[times.len() / 2];
        let p95 = times[times.len() * 95 / 100];
        let stats = client.engine_render_cache.borrow().as_ref().unwrap().stats;
        if cached {
            identical = hashes == reference;
            if stats.rebuilds != 1 || stats.culled_voxels == 0 {
                return Err("Render cache did not reuse/cull static geometry".into());
            }
        } else {
            reference = hashes;
        }
        modes.push(json!({"cached":cached,"frames":times.len(),"median_ms":median,"p95_ms":p95,"median_render_fps":1000./median,"cache":stats}));
    }
    client.yaw = yaw;
    client.pitch = pitch;
    client.engine_cache_enabled = true;
    if !identical {
        return Err("Culled/cached frames differ from full-voxel reference".into());
    }
    Ok(
        json!({"complete":true,"width":render::WIDTH,"height":render::HEIGHT,"frames_identical":identical,"modes":modes,"scope":"CPU framebuffer render time on this PC across eight headings; excludes event loop, GDI scaling and display latency"}),
    )
}
fn verify(app: &mut App, out: &Path) -> Result<()> {
    if out.exists() {
        return Err("Fresh engine verification directory required".into());
    }
    fs::create_dir_all(out)?;
    if let Some(assets) = &app.client.engine_assets {
        fs::write(
            out.join("assets.json"),
            serde_json::to_vec_pretty(&assets.provenance)?,
        )?;
    }
    let initial = app.client.scene.engine.as_ref().unwrap().blocks.clone();
    image(&app.client, out, "playground.png")?;
    for _ in 0..12 {
        app.client.tick(Input::default())?;
    }
    let start = app.client.actor.position;
    app.client.tick(Input {
        jump: true,
        ..Default::default()
    })?;
    let mut peak = start.y;
    for _ in 0..40 {
        app.client.tick(Input::default())?;
        peak = peak.max(app.client.actor.position.y);
    }
    if peak < start.y + 1. || (app.client.actor.position.y - start.y).abs() > 2e-5 {
        return Err("Engine jump/landing failed".into());
    }
    for _ in 0..20 {
        app.client.tick(Input {
            forward: 1.,
            ..Default::default()
        })?;
    }
    if app.client.actor.position.z < start.z + 2. {
        return Err("Engine walking failed".into());
    }
    let walking_distance = app.client.actor.position.z - start.z;
    app.client.set_spawn(Vec3::new(0.5, 4., -2.5))?;
    app.client.yaw = 0.;
    app.client.pitch = -0.65;
    let hit = app.client.target()?.ok_or("Engine edit target missing")?;
    let p = [hit.position.x, hit.position.y, hit.position.z];
    if !app.client.edit(false)? || app.client.scene.cells.contains(&p) {
        return Err("Engine remove failed".into());
    }
    // The same walking-client action places into the removed cell from below.
    let ember = app
        .client
        .scene
        .engine
        .as_ref()
        .unwrap()
        .registry
        .find("ember")?;
    {
        let state = app.client.scene.engine.as_mut().unwrap();
        state.player.assign(ember, state.player.active)?;
    }
    if !app.client.edit(true)?
        || app.client.scene.engine.as_ref().unwrap().blocks.get(&p) != Some(&ember)
    {
        return Err("Engine place failed".into());
    }
    image(&app.client, out, "edited.png")?;
    let old_root = app.root.clone();
    app.root = out.to_path_buf();
    let saved = app.save_current()?;
    app.root = old_root;
    let saved_bytes = fs::read(&saved)?;
    let expected = app.client.scene.engine.as_ref().unwrap().blocks.clone();
    app.client.scene.change_engine(p, None)?;
    app.reload_current()?;
    if app.client.scene.engine.as_ref().unwrap().blocks != expected
        || app.client.pending_edits() != 0
        || fs::read(&saved)? != saved_bytes
    {
        return Err("Engine reload changed or lost saved blocks".into());
    }
    image(&app.client, out, "reloaded.png")?;
    app.client.set_spawn(Vec3::new(20.5, 13., 0.5))?;
    app.client.yaw = -1.4;
    app.client.pitch = -0.4;
    image(&app.client, out, "overlook.png")?;
    // Return the window to the creative court rather than the test camera.
    app.client.set_spawn(SPAWN)?;
    app.client.yaw = 0.;
    app.client.pitch = -0.08;
    app.engine_key(winit::keyboard::KeyCode::KeyE)?;
    if !app.palette.open {
        return Err("Creative palette did not open through E".into());
    }
    app.engine_key(winit::keyboard::KeyCode::End)?;
    let mut palette_pixels = render::render(&app.client, render::WIDTH, render::HEIGHT);
    crate::engine_ui::draw(
        &app.client,
        &app.palette,
        &mut palette_pixels,
        render::WIDTH,
        render::HEIGHT,
    );
    render::png(
        &out.join("palette.png"),
        &palette_pixels,
        render::WIDTH,
        render::HEIGHT,
    )?;
    app.engine_key(winit::keyboard::KeyCode::Enter)?;
    if app.palette.open
        || usize::from(
            app.client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .player
                .selected()
                .ok_or("Expected assigned hotbar block")?,
        ) != app
            .client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .blocks
            .len()
            - 1
    {
        return Err("Creative palette selection failed".into());
    }
    let perf = performance(&mut app.client)?;
    fs::write(
        out.join("performance.json"),
        serde_json::to_vec_pretty(&perf)?,
    )?;
    let report = json!({"complete":true,"checkpoint":CHECKPOINT,"seed":app.client.scene.engine.as_ref().unwrap().seed,"materials":app.client.scene.engine.as_ref().unwrap().registry.blocks.iter().map(|m|m.id.clone()).collect::<Vec<_>>(),"blocks":app.client.scene.cells.len(),"initial_blocks":initial.len(),"jump_peak":peak,"walking_distance":walking_distance,"edited_cell":p,"saved_world":saved,"saved_sha256":hash(&saved_bytes),"save_reload_equal":true,"no_external_game_files":app.client.engine_assets.is_none(),"minecraft_textures":app.client.engine_assets.is_some(),"palette_selection":material(&app.client).map(|b|b.id.clone()),"performance":perf,"scope":"Engine-owned block registry and versioned saves, local Minecraft texture/HUD bindings, creative palette and cached/culled CPU renderer; native interface/lighting parity not fully verified"});
    fs::write(out.join("engine.json"), serde_json::to_vec_pretty(&report)?)?;
    app.frames = 0;
    app.inventory_frames = 0;
    println!("Engine playground checks passed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn movement_and_partial_models_roundtrip_with_crouched_and_noclip_spawns() {
        let mut c = generate(360036).unwrap();
        let slab = c
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .find("stone_slab_upper")
            .unwrap();
        c.tick(Input {
            sneak: true,
            ..Default::default()
        })
        .unwrap();
        c.scene.change_engine([0, 5, -10], Some(slab)).unwrap();
        let bytes = serde_json::to_vec(&document(&c).unwrap()).unwrap();
        let restored = decode(&bytes, None).unwrap();
        assert!(restored.movement.crouched);
        assert_eq!(restored.actor.body.height, 1.5);
        assert_eq!(
            restored.scene.engine.as_ref().unwrap().geometry.boxes(slab)[0][1],
            0.5
        );
        let mut bad: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        bad["movement"]["crouched"] = json!(false);
        assert!(decode(&serde_json::to_vec(&bad).unwrap(), None).is_err());
        c.movement.mode = crate::engine_player::Mode::Noclip;
        c.actor.position = Vec3::new(0.5, 0.2, -9.5);
        let bytes = serde_json::to_vec(&document(&c).unwrap()).unwrap();
        let restored = decode(&bytes, None).unwrap();
        assert_eq!(restored.movement.mode, crate::engine_player::Mode::Noclip);
        assert_eq!(restored.actor.position.y, 0.2);
        let mut bad: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        bad["player"]["x"] = json!(f32::MAX);
        assert!(decode(&serde_json::to_vec(&bad).unwrap(), None).is_err());
        for version in [3, 4] {
            let old = if version == 4 {
                crate::engine_stream::generate(360036, Registry::built_in()).unwrap()
            } else {
                generate(360036).unwrap()
            };
            let mut old = serde_json::to_value(document(&old).unwrap()).unwrap();
            old["version"] = json!(version);
            old.as_object_mut().unwrap().remove("movement");
            let restored = decode(&serde_json::to_vec(&old).unwrap(), None).unwrap();
            assert_eq!(restored.movement, crate::engine_player::Movement::default());
        }
    }
    #[test]
    fn atomic_publication_never_replaces_an_existing_world() {
        let directory = std::env::temp_dir().join(format!(
            "blockwork-publish-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let source = directory.join("pending.part");
        let destination = directory.join("existing.blockworld");
        fs::write(&source, b"new complete file").unwrap();
        fs::write(&destination, b"old save").unwrap();
        assert!(publish_unique(&source, &destination).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"old save");
        assert_eq!(fs::read(&source).unwrap(), b"new complete file");
        fs::remove_file(&source).unwrap();
        fs::remove_file(&destination).unwrap();
        fs::remove_dir(&directory).unwrap();
    }
    #[test]
    fn streamed_save_is_sparse_and_preserves_unloaded_edits_and_inventory() {
        let mut c = crate::engine_stream::generate(360036, Registry::built_in()).unwrap();
        let oak = c
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .find("oak_planks")
            .unwrap();
        c.scene.change_engine([2, 3, -2], Some(oak)).unwrap();
        c.scene.change_engine([3, 3, -2], None).unwrap();
        c.scene.change_engine([4, 40, -2], Some(oak)).unwrap();
        c.scene.engine.as_mut().unwrap().player.cursor =
            Some(crate::engine_inventory::Stack { id: oak, count: 7 });
        crate::engine_stream::jump(&mut c, 1).unwrap();
        assert!(
            !c.scene
                .engine
                .as_ref()
                .unwrap()
                .blocks
                .contains_key(&[4, 40, -2])
        );
        let bytes = serde_json::to_vec(&document(&c).unwrap()).unwrap();
        assert!(bytes.len() < 16_384);
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["version"], 5);
        assert!(value.get("cells").is_none());
        assert_eq!(value["edits"].as_array().unwrap().len(), 3);
        let mut copy = decode(&bytes, None).unwrap();
        assert_eq!(
            copy.scene
                .engine
                .as_ref()
                .unwrap()
                .player
                .cursor
                .unwrap()
                .count,
            7
        );
        assert_eq!(copy.pending_edits(), 0);
        copy.set_spawn(SPAWN).unwrap();
        assert_eq!(
            copy.scene.engine.as_ref().unwrap().blocks.get(&[4, 40, -2]),
            Some(&oak)
        );
        assert!(
            !copy
                .scene
                .engine
                .as_ref()
                .unwrap()
                .blocks
                .contains_key(&[3, 3, -2])
        );
        for edit in [
            json!({"position":[0,0,0],"material":null}),
            json!({"position":[8192,4,0],"material":"slate"}),
            json!({"position":[0,60,0],"material":"unknown"}),
        ] {
            let mut bad = value.clone();
            bad["edits"][0] = edit;
            assert!(decode(&serde_json::to_vec(&bad).unwrap(), None).is_err());
        }
        let mut bad = value.clone();
        bad["generator"]["version"] = json!(99);
        assert!(decode(&serde_json::to_vec(&bad).unwrap(), None).is_err());
        let mut reordered = c.scene.engine.as_ref().unwrap().registry.clone();
        reordered.blocks.reverse();
        apply_registry(&mut c, reordered).unwrap();
        c.set_spawn(SPAWN).unwrap();
        let state = c.scene.engine.as_ref().unwrap();
        assert_eq!(
            state
                .registry
                .get(*state.blocks.get(&[4, 40, -2]).unwrap())
                .id,
            "oak_planks"
        );
    }
    #[test]
    fn inventory_cursor_and_empty_hand_survive_save_and_legacy_migration() {
        use crate::engine_inventory::Stack;
        let mut client = generate(38).unwrap();
        {
            let state = client.scene.engine.as_mut().unwrap();
            state.player.active = 0;
            state.player.slots[0] = None;
            state.player.crafting[3] = Some(Stack {
                id: state.registry.find("oak_log").unwrap(),
                count: 2,
            });
            state.player.cursor = Some(Stack {
                id: state.registry.find("stone_bricks").unwrap(),
                count: 5,
            });
        }
        let bytes = serde_json::to_vec(&document(&client).unwrap()).unwrap();
        let copy = decode(&bytes, None).unwrap();
        let a = &client.scene.engine.as_ref().unwrap().player;
        let b = &copy.scene.engine.as_ref().unwrap().player;
        assert_eq!(a.slots, b.slots);
        assert_eq!(a.crafting, b.crafting);
        assert_eq!(a.cursor, b.cursor);
        assert_eq!(b.selected(), None);
        let mut bad: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        bad["inventory"]["cursor"]["count"] = json!(65);
        assert!(decode(&serde_json::to_vec(&bad).unwrap(), None).is_err());
        let base = generate(37).unwrap();
        let mut old = serde_json::to_value(document(&base).unwrap()).unwrap();
        old["version"] = json!(2);
        old["selected"] = json!("cobblestone");
        old.as_object_mut().unwrap().remove("movement");
        old.as_object_mut().unwrap().remove("inventory");
        old.as_object_mut().unwrap().remove("recipes");
        old["registry"]["blocks"]
            .as_array_mut()
            .unwrap()
            .truncate(15);
        let migrated = decode(&serde_json::to_vec(&old).unwrap(), None).unwrap();
        let state = migrated.scene.engine.as_ref().unwrap();
        assert_eq!(
            state.player.selected(),
            Some(state.registry.find("cobblestone").unwrap())
        );
        assert_eq!(state.player.slots.len(), 36);
    }
    #[test]
    fn registry_reordering_preserves_carried_items_and_rejects_removing_them() {
        let mut client = generate(38).unwrap();
        let mut registry = client.scene.engine.as_ref().unwrap().registry.clone();
        registry.blocks.push(crate::engine_blocks::Block {
            texture_files: None,
            placement: None,
            collision: None,
            render_layer: crate::engine_blocks::RenderLayer::Opaque,
            cull_group: None,
            hidden: false,
            face_textures: None,
            geometry: None,
            id: "carried_only".into(),
            label: "Carried Only".into(),
            color: [1, 2, 3],
            pattern: Pattern::Solid,
            side_color: None,
            minecraft: None,
        });
        apply_registry(&mut client, registry).unwrap();
        {
            let state = client.scene.engine.as_mut().unwrap();
            state.player.cursor = Some(crate::engine_inventory::Stack {
                id: state.registry.find("carried_only").unwrap(),
                count: 7,
            });
        }
        let before = client.scene.engine.as_ref().unwrap().registry.blocks.len();
        assert!(apply_registry(&mut client, Registry::built_in()).is_err());
        assert_eq!(
            client.scene.engine.as_ref().unwrap().registry.blocks.len(),
            before
        );
        let mut reversed = client.scene.engine.as_ref().unwrap().registry.clone();
        reversed.blocks.reverse();
        apply_registry(&mut client, reversed).unwrap();
        let state = client.scene.engine.as_ref().unwrap();
        assert_eq!(
            state.registry.get(state.player.cursor.unwrap().id).id,
            "carried_only"
        );
        assert_eq!(state.player.cursor.unwrap().count, 7);
    }
    #[test]
    fn save_roundtrip_and_malformed_input_are_strict() {
        let client = generate(36).unwrap();
        let bytes = serde_json::to_vec(&document(&client).unwrap()).unwrap();
        let copy = decode(&bytes, None).unwrap();
        assert_eq!(
            client.scene.engine.as_ref().unwrap().blocks,
            copy.scene.engine.as_ref().unwrap().blocks
        );
        assert_eq!(client.actor.position, copy.actor.position);
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let cell = value["cells"][0].clone();
        value["cells"].as_array_mut().unwrap().push(cell);
        assert!(decode(&serde_json::to_vec(&value).unwrap(), None).is_err());
        value["cells"].as_array_mut().unwrap().pop();
        value["cells"][0]["material"] = json!("unknown");
        assert!(decode(&serde_json::to_vec(&value).unwrap(), None).is_err());
    }
    #[test]
    fn seed_changes_hills_while_spawn_stays_clear() {
        let a = generate(36).unwrap();
        let b = generate(37).unwrap();
        assert_ne!(
            a.scene.engine.as_ref().unwrap().blocks,
            b.scene.engine.as_ref().unwrap().blocks
        );
        assert_eq!(a.actor.position, SPAWN);
        assert!(a.scene.motion_scene(&a.actor).is_ok());
        let mut value = serde_json::to_value(document(&a).unwrap()).unwrap();
        value["player"] = json!({"x":0.5,"y":0.,"z":0.5});
        assert!(decode(&serde_json::to_vec(&value).unwrap(), None).is_err());
    }
    #[test]
    fn saves_preserve_previous_files_and_reject_external_changes() {
        let dir = std::env::temp_dir().join(format!(
            "blockwork-save-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut client = generate(36).unwrap();
        let first = save(&mut client, &dir).unwrap();
        let bytes = fs::read(&first).unwrap();
        client.scene.change_engine([1, 5, 1], Some(3)).unwrap();
        let second = save(&mut client, &dir).unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(&first).unwrap(), bytes);
        let expected = client.scene.engine.as_ref().unwrap().blocks.clone();
        fs::write(&second, b"external modification").unwrap();
        assert!(save(&mut client, &dir).is_err());
        assert!(reload(&mut client).is_err());
        assert_eq!(client.scene.engine.as_ref().unwrap().blocks, expected);
        for path in [first, second] {
            fs::remove_file(path).unwrap();
        }
        fs::remove_dir(dir).unwrap();
    }
}
