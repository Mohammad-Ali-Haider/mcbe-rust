use mcbe_core::{
    cells::scene_from_shapes,
    chunks::World,
    container_geometry::{ContainerBlock, Geometry},
    cubes::{self, Cube},
    leveldb,
    motion::{Actor, Body, Vec3},
    raycast::{self, Face, Hit, Options, RayBlock, Shapes},
    streaming::{Bounds, Cache, Report},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub const MIN: [i32; 3] = [-48, -64, -32];
pub const MAX: [i32; 3] = [-16, -16, 0];
pub const START: Vec3 = Vec3::new(-39.5, -31.0, -23.5);
#[derive(Clone, Copy)]
pub enum View {
    Fixed,
    Stream { center: [i32; 2], radius: u8 },
}
pub struct Scene {
    pub containers: HashMap<[i32; 3], ContainerBlock>,
    pub geometry: Geometry,
    pub cells: HashSet<[i32; 3]>,
    pub shapes: Shapes,
    pub source_files: usize,
    pub source: PathBuf,
    pub source_hashes: BTreeMap<String, String>,
    pub snapshot: leveldb::Snapshot,
    pub original_cells: HashSet<[i32; 3]>,
    pub kinds: HashMap<[i32; 3], Cube>,
    pub original_kinds: HashMap<[i32; 3], Cube>,
    pub biome_codes: HashMap<[i32; 3], u32>,
    pub bounds: Bounds,
    cache: Option<Cache>,
    pub palette: Option<Arc<mcbe_core::blocks::Palette>>,
    pub engine: Option<crate::engine::WorldState>,
}
impl Scene {
    pub fn load_fixed(path: &Path) -> Result<Self> {
        Self::load_view(path, View::Fixed)
    }
    pub fn load_streamed(path: &Path, center: [i32; 2], radius: u8) -> Result<Self> {
        if radius == 0 {
            return Err(
                "Interactive streaming requires a neighbor-column radius of at least one".into(),
            );
        }
        Self::load_view(path, View::Stream { center, radius })
    }
    pub fn reload_like(&self, path: &Path) -> Result<Self> {
        Self::load_view(path, self.view())
    }
    pub fn view(&self) -> View {
        self.cache
            .as_ref()
            .map_or(View::Fixed, |cache| View::Stream {
                center: cache.center().expect("Initialized cache"),
                radius: cache.radius(),
            })
    }
    fn load_view(path: &Path, view: View) -> Result<Self> {
        let database = path.canonicalize()?;
        if database.file_name().is_none_or(|s| s != "db") {
            return Err("Client expects a stopped world/db folder".into());
        }
        let source = database
            .parent()
            .ok_or("Database has no world parent")?
            .to_path_buf();
        let source_hashes = mcbe_core::world_edit::tree_hashes(&source)?;
        let snapshot = leveldb::read_snapshot(path)?;
        let source_files = snapshot.files.len();
        let mut world = World::new(snapshot);
        static PALETTE: OnceLock<Arc<mcbe_core::blocks::Palette>> = OnceLock::new();
        if PALETTE.get().is_none() {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let full: serde_json::Value = serde_json::from_slice(&std::fs::read(
                root.join("evidence/bds-1.26.52.3/blocks-05/registry.json"),
            )?)?;
            let partial: serde_json::Value = serde_json::from_slice(&std::fs::read(
                root.join("evidence/bds-1.26.52.3/block-partial-02/registry.json"),
            )?)?;
            let palette = mcbe_core::blocks::Palette::from_reports(&full, &partial)?;
            if palette.reference()
                != "19c88569af2e4b7d984e999055a31cbcb0799dacf8bbbf7371eda42f5772a443"
            {
                return Err("Unknown cube palette reference".into());
            }
            let _ = PALETTE.set(Arc::new(palette));
        }
        let palette = PALETTE.get().ok_or("Missing cube palette")?.clone();
        let (kinds, bounds, cache) = match view {
            View::Fixed => (
                cubes::region(&mut world, MIN, MAX, &palette)?,
                Bounds { min: MIN, max: MAX },
                None,
            ),
            View::Stream { center, radius } => {
                let mut cache = Cache::new(radius)?;
                cache.refresh(center, |p| {
                    mcbe_core::columns::read_geometry(&world.snapshot, p, &palette)
                })?;
                if !cache.ready([center[0] * 16, -60, center[1] * 16]) {
                    return Err("Initial saved column is absent or unsupported".into());
                }
                (cache.cells(), cache.bounds()?, Some(cache))
            }
        };
        let cells: HashSet<_> = kinds.keys().copied().collect();
        let containers = cache.as_ref().map_or_else(HashMap::new, Cache::containers);
        let biome_codes = {
            let mut biomes = mcbe_core::biomes::World::new(&world.snapshot);
            kinds
                .iter()
                .filter(|(_, kind)| **kind == Cube::Grass)
                .map(|(p, _)| Ok((*p, biomes.at(*p)?)))
                .collect::<mcbe_core::Result<HashMap<_, _>>>()?
        };
        let shapes = Shapes::read(&mcbe_core::container_geometry::catalog()["rows"])?;
        let geometry = Geometry::measured()?;
        if mcbe_core::world_edit::tree_hashes(&source)? != source_hashes {
            return Err("World changed during client loading".into());
        }
        Ok(Self {
            containers,
            geometry,
            original_cells: cells.clone(),
            original_kinds: kinds.clone(),
            bounds,
            cache,
            palette: Some(palette),
            engine: None,
            biome_codes,
            kinds,
            source,
            source_hashes,
            snapshot: world.snapshot,
            cells,
            shapes,
            source_files,
        })
    }
    pub fn inside(&self, p: [i32; 3]) -> bool {
        self.bounds.contains(p)
            && self.cache.as_ref().is_none_or(|cache| cache.ready(p))
            && self
                .engine
                .as_ref()
                .and_then(|s| s.stream.as_ref())
                .is_none_or(|s| s.residents.contains_key(&crate::engine_stream::column(p)))
    }
    pub fn from_engine(
        blocks: HashMap<[i32; 3], crate::engine_blocks::BlockId>,
        state: crate::engine::WorldState,
    ) -> Result<Self> {
        let cells: HashSet<_> = blocks.keys().copied().collect();
        let shapes = state.registry.shapes()?;
        let mut state = state;
        state.original_blocks = blocks.clone();
        state.blocks = blocks;
        Ok(Self {
            containers: HashMap::new(),
            geometry: Geometry::measured()?,
            shapes,
            original_cells: cells.clone(),
            original_kinds: HashMap::new(),
            cells,
            kinds: HashMap::new(),
            bounds: crate::engine::BOUNDS,
            cache: None,
            palette: None,
            engine: Some(state),
            biome_codes: HashMap::new(),
            source_files: 0,
            source: PathBuf::new(),
            source_hashes: BTreeMap::new(),
            snapshot: leveldb::Snapshot::default(),
        })
    }
    pub fn residency(&self) -> Option<Report> {
        self.cache.as_ref().map(|cache| cache.report(0, 0))
    }
    pub fn pending_changes(&self) -> BTreeMap<[i32; 3], Option<Cube>> {
        let mut changes = self
            .cache
            .as_ref()
            .map_or_else(BTreeMap::new, |cache| cache.edits().clone());
        for p in self.cells.union(&self.original_cells) {
            let value = self.kind_at(*p);
            if value != self.original_kinds.get(p).copied() {
                changes.insert(*p, value);
            } else {
                changes.remove(p);
            }
        }
        changes
    }
    pub fn change(&mut self, p: [i32; 3], kind: Option<Cube>) -> Result<()> {
        if self.engine.is_some() {
            return Err("Engine editing requires an engine block identifier".into());
        }
        if self.containers.contains_key(&p) {
            return Err("Container block changes require block-entity lifecycle support".into());
        }
        if !self.inside(p) {
            return Err("Cannot edit outside available saved columns".into());
        }
        if let Some(cache) = &mut self.cache {
            cache.edit(p, kind)?;
        }
        if let Some(kind) = kind {
            self.cells.insert(p);
            self.kinds.insert(p, kind);
        } else {
            self.cells.remove(&p);
            self.kinds.remove(&p);
        }
        Ok(())
    }
    pub fn change_engine(
        &mut self,
        p: [i32; 3],
        id: Option<crate::engine_blocks::BlockId>,
    ) -> Result<()> {
        if !self.inside(p) || p[1] == crate::engine::BOUNDS.min[1] {
            return Err("Outside loaded terrain or protected foundation".into());
        }
        let state = self.engine.as_mut().ok_or("Engine world missing")?;
        if id.is_some_and(|id| usize::from(id) >= state.registry.blocks.len()) {
            return Err("Unknown engine block".into());
        }
        if let Some(stream) = &mut state.stream {
            stream.record(p, id)?;
            let positions = stream
                .residents
                .get_mut(&crate::engine_stream::column(p))
                .ok_or("Column unavailable")?;
            if id.is_some() && !state.blocks.contains_key(&p) {
                positions.push(p);
            }
            if id.is_none()
                && let Some(i) = positions.iter().position(|v| *v == p)
            {
                positions.swap_remove(i);
            }
        }
        match id {
            Some(id) => {
                state.blocks.insert(p, id);
                self.cells.insert(p);
            }
            None => {
                state.blocks.remove(&p);
                self.cells.remove(&p);
            }
        }
        state.edit_revision = state.edit_revision.wrapping_add(1);
        state.revision = state.revision.wrapping_add(1);
        for d in [
            [0, 0, 0],
            [1, 0, 0],
            [-1, 0, 0],
            [0, 1, 0],
            [0, -1, 0],
            [0, 0, 1],
            [0, 0, -1],
        ] {
            state.dirty_chunks.insert(
                crate::engine_mesh::chunk([p[0] + d[0], p[1] + d[1], p[2] + d[2]]),
                state.revision,
            );
        }
        if let Some(stream) = &state.stream {
            state.dirty_chunks.retain(|k, _| {
                (0..4).contains(&k[1]) && stream.residents.contains_key(&[k[0], k[2]])
            });
        }
        Ok(())
    }
    pub fn recenter(&mut self, position: Vec3) -> Result<bool> {
        let Some(cache) = self.cache.as_ref() else {
            return Ok(false);
        };
        if ![position.x, position.y, position.z]
            .iter()
            .all(|n| n.is_finite())
        {
            return Err("Nonfinite actor position".into());
        }
        let center = [
            (position.x.floor() as i32).div_euclid(16),
            (position.z.floor() as i32).div_euclid(16),
        ];
        if cache.center() == Some(center) {
            return Ok(false);
        }
        // Preflight the destination before changing the resident window.
        let mut prepared = if matches!(
            cache.residents.get(&center),
            Some(mcbe_core::streaming::State::Ready(_))
        ) {
            None
        } else {
            Some(
                mcbe_core::columns::read_geometry(
                    &self.snapshot,
                    center,
                    self.palette
                        .as_deref()
                        .ok_or("Saved-world palette missing")?,
                )?
                .ok_or("Destination saved column is absent")?,
            )
        };
        let changes: Vec<_> = self
            .cells
            .union(&self.original_cells)
            .map(|p| (*p, self.kind_at(*p)))
            .collect();
        let cache = self.cache.as_mut().ok_or("Missing streaming cache")?;
        for (p, kind) in changes {
            cache.edit(p, kind)?;
        }
        cache.refresh(center, |p| {
            if p == center && prepared.is_some() {
                Ok(prepared.take())
            } else {
                mcbe_core::columns::read_geometry(
                    &self.snapshot,
                    p,
                    self.palette
                        .as_deref()
                        .ok_or("Saved-world palette missing")?,
                )
            }
        })?;
        let original = cache.base_cells();
        let kinds = cache.cells();
        self.containers = cache.containers();
        let biomes = kinds
            .iter()
            .filter(|(_, kind)| **kind == Cube::Grass)
            .map(|(p, _)| Ok((*p, cache.biome(*p)?)))
            .collect::<mcbe_core::Result<HashMap<_, _>>>()?;
        self.bounds = cache.bounds()?;
        self.original_cells = original.keys().copied().collect();
        self.original_kinds = original;
        self.cells = kinds.keys().copied().collect();
        self.kinds = kinds;
        self.biome_codes = biomes;
        Ok(true)
    }
    pub fn kind_at(&self, p: [i32; 3]) -> Option<Cube> {
        if self.engine.is_some() {
            return None;
        }
        self.cells
            .contains(&p)
            .then(|| self.kinds.get(&p).copied().unwrap_or(Cube::Stone))
    }
    pub fn target(&self, eye: [f64; 3], direction: [f64; 3]) -> Result<Option<Hit>> {
        const UNAVAILABLE: &str = "client-selection-unavailable-saved-column";
        let result = raycast::cast(
            eye,
            direction,
            Options {
                max_distance: 6.0,
                include_passable: false,
                include_liquids: false,
            },
            &self.shapes,
            |position| {
                if !self.inside(position) {
                    return Err(UNAVAILABLE.into());
                }
                if let Some(container) = self.containers.get(&position) {
                    return Ok(Some(container.block(position)));
                }
                if let Some(state) = &self.engine {
                    return Ok(state.blocks.get(&position).map(|id| RayBlock {
                        position,
                        id: format!("blockwork:{}", state.registry.get(*id).id),
                        states: Vec::new(),
                    }));
                }
                Ok(self.kind_at(position).map(|kind| RayBlock {
                    position,
                    id: kind.id().into(),
                    states: kind.states().into_iter().collect(),
                }))
            },
        );
        match result {
            Ok(hit) => Ok(hit),
            Err(error) if error == UNAVAILABLE => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    pub fn motion_scene(&self, actor: &Actor) -> Result<mcbe_core::motion::SolidScene> {
        Ok(scene_from_shapes(actor, |p| {
            if !self.inside(p) {
                return Ok(vec![[0., 0., 0., 1., 1., 1.]]);
            }
            if let Some(state) = &self.engine {
                return Ok(state
                    .blocks
                    .get(&p)
                    .map_or_else(Vec::new, |id| state.geometry.boxes(*id).to_vec()));
            }
            if self.cells.contains(&p) {
                return Ok(vec![[0., 0., 0., 1., 1., 1.]]);
            }
            match self.containers.get(&p) {
                Some(kind) => self.geometry.collision_for(&kind.block(p), actor),
                None => Ok(Vec::new()),
            }
        })?)
    }
}
#[derive(Default, Clone, Copy)]
pub struct Input {
    pub forward: f32,
    pub side: f32,
    pub jump: bool,
    pub sneak: bool,
}
pub struct Client {
    pub movement: crate::engine_player::Movement,
    pub engine_render_cache: std::cell::RefCell<Option<crate::render::EngineCache>>,
    pub engine_cache_enabled: bool,
    pub engine_assets: Option<crate::engine_assets::Assets>,
    pub engine_worker: Option<crate::engine_stream::Worker>,
    pub scene: Scene,
    pub actor: Actor,
    pub yaw: f32,
    pub pitch: f32,
    pub changed: usize,
    pub spawn: Vec3,
    pub placement: Cube,
    pub available_materials: Vec<Cube>,
    pub save_rules: Option<crate::persistence::SaveRules>,
    pub last_save: Option<PathBuf>,
    pub stone_material: Option<mcbe_core::textures::Material>,
    pub cube_materials: HashMap<(Cube, usize), mcbe_core::textures::Material>,
    pub grass_tints: HashMap<u32, mcbe_core::grass::Tint>,
    pub material_pack: Option<mcbe_core::resources::Pack>,
    pub extra_material_packs: HashMap<&'static str, mcbe_core::resources::Pack>,
    pub grass_resources: Option<GrassResources>,
}
pub struct GrassResources {
    pub behavior: mcbe_core::resources::Pack,
    pub catalog: serde_json::Value,
}
impl Client {
    pub fn new(scene: Scene) -> Result<Self> {
        let mut available = std::collections::BTreeSet::new();
        let mut classifier = cubes::Classifier::default();
        if scene.engine.is_some() {
            available.insert(Cube::Stone);
        }
        for (key, entry) in &scene.snapshot.entries {
            if let Some(y) = mcbe_core::chunks::key_y(key)
                && let Some(raw) = &entry.value
            {
                let chunk = mcbe_core::chunks::decode(raw, Some(y))?;
                for block in &chunk.storages[0].palette {
                    if let Ok(Some(kind)) = classifier.saved(
                        block,
                        scene
                            .palette
                            .as_deref()
                            .ok_or("Saved-world palette missing")?,
                    ) {
                        available.insert(kind);
                    }
                }
            }
        }
        let available_materials: Vec<_> = available.into_iter().collect();
        let placement = if available_materials.contains(&Cube::Stone) {
            Cube::Stone
        } else {
            *available_materials
                .first()
                .ok_or("Source world has no supported persistent cube material")?
        };
        Ok(Self {
            movement: Default::default(),
            scene,
            engine_assets: None,
            engine_worker: None,
            engine_render_cache: std::cell::RefCell::new(None),
            engine_cache_enabled: true,
            actor: Actor::new(START, Body::PROBE)?,
            yaw: std::f32::consts::FRAC_PI_2,
            pitch: -0.12,
            changed: 0,
            spawn: START,
            placement,
            available_materials,
            save_rules: None,
            last_save: None,
            stone_material: None,
            cube_materials: HashMap::new(),
            grass_tints: HashMap::new(),
            material_pack: None,
            extra_material_packs: HashMap::new(),
            grass_resources: None,
        })
    }
    pub fn reset(&mut self) {
        self.actor = Actor::new(self.spawn, Body::PROBE).expect("Validated actor body and spawn");
    }
    pub fn cycle_material(&mut self) {
        if let Some(state) = &mut self.scene.engine {
            state.player.cycle(1);
            return;
        }
        let index = self
            .available_materials
            .iter()
            .position(|kind| *kind == self.placement)
            .unwrap_or(0);
        self.placement = self.available_materials[(index + 1) % self.available_materials.len()];
    }
    pub fn set_spawn(&mut self, position: Vec3) -> Result<()> {
        let actor = Actor::new(position, self.movement.body())?;
        if self
            .scene
            .engine
            .as_ref()
            .and_then(|s| s.stream.as_ref())
            .is_some()
        {
            crate::engine_stream::prepare(self, position)?;
        }
        self.scene.recenter(position)?;
        if self.scene.engine.is_some() {
            let bounds = self.scene.bounds;
            let half = actor.body.width * 0.5;
            let low = [position.x - half, position.y, position.z - half];
            let high = [
                position.x + half,
                position.y + actor.body.height,
                position.z + half,
            ];
            if (0..3).any(|a| low[a] < bounds.min[a] as f32 || high[a] > bounds.max[a] as f32) {
                return Err("Player body exceeds engine world bounds".into());
            }
        }
        let half = actor.body.width * 0.5;
        let low = [position.x - half, position.y, position.z - half];
        let high = [
            position.x + half,
            position.y + actor.body.height,
            position.z + half,
        ];
        if self.movement.mode != crate::engine_player::Mode::Noclip
            && self
                .scene
                .motion_scene(&actor)?
                .boxes
                .iter()
                .any(|b| (0..3).all(|a| low[a] < b.max[a] && high[a] > b.min[a]))
        {
            return Err("Spawn body must fit in clear, available saved geometry".into());
        }
        self.spawn = position;
        self.actor = actor;
        Ok(())
    }
    pub fn sync_region(&mut self) -> Result<bool> {
        let streamed = crate::engine_stream::update(self)?;
        let changed = self.scene.recenter(self.actor.position)? || streamed;
        if let Some(pack) = &mut self.material_pack {
            let kinds: std::collections::BTreeSet<_> = self.scene.kinds.values().copied().collect();
            for kind in kinds {
                if kind == Cube::Stone {
                    continue;
                }
                let group = match kind {
                    Cube::Granite | Cube::Diorite | Cube::Andesite => Some("vanilla_1.21.10"),
                    Cube::Deepslate
                    | Cube::CobbledDeepslate
                    | Cube::Tuff
                    | Cube::Calcite
                    | Cube::CopperOre
                    | Cube::DeepslateCoalOre
                    | Cube::DeepslateIronOre
                    | Cube::DeepslateCopperOre
                    | Cube::DeepslateGoldOre
                    | Cube::DeepslateLapisOre
                    | Cube::DeepslateRedstoneOre
                    | Cube::LitDeepslateRedstoneOre
                    | Cube::DeepslateDiamondOre
                    | Cube::DeepslateEmeraldOre => Some("vanilla_1.17.0"),
                    _ => None,
                };
                if let Some(group) = group
                    && !self.extra_material_packs.contains_key(group)
                {
                    let parent = pack
                        .root()
                        .parent()
                        .ok_or("Base material pack has no parent")?;
                    self.extra_material_packs.insert(
                        group,
                        mcbe_core::resources::Pack::open(&parent.join(group))?,
                    );
                }
                for (face, name) in ["up", "down", "north", "south", "east", "west"]
                    .iter()
                    .enumerate()
                {
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        self.cube_materials.entry((kind, face))
                    {
                        let material = match group {
                            Some("vanilla_1.21.10") => mcbe_core::textures::block_material_split(
                                self.extra_material_packs
                                    .get_mut("vanilla_1.21.10")
                                    .ok_or("Missing stone variant definitions")?,
                                pack,
                                kind.resource_key(),
                                name,
                                0,
                            )?,
                            Some(group) => mcbe_core::textures::block_material(
                                self.extra_material_packs
                                    .get_mut(group)
                                    .ok_or("Missing terrain material pack")?,
                                kind.resource_key(),
                                name,
                                0,
                            )?,
                            None => mcbe_core::textures::block_material(
                                pack,
                                kind.resource_key(),
                                name,
                                0,
                            )?,
                        };
                        if kind != Cube::Grass
                            && material
                                .texture
                                .rgba
                                .chunks_exact(4)
                                .any(|pixel| pixel[3] != 255)
                        {
                            return Err(
                                format!("Opaque cube texture required for {}", kind.id()).into()
                            );
                        }
                        entry.insert(material);
                    }
                }
            }
            pack.verify_unchanged()?;
        }
        if let Some(resources) = &mut self.grass_resources {
            let codes: std::collections::BTreeSet<_> = self
                .scene
                .biome_codes
                .values()
                .copied()
                .filter(|code| !self.grass_tints.contains_key(code))
                .collect();
            for code in codes {
                let biome = resources.catalog["mapping"][code.to_string()]
                    .as_str()
                    .ok_or("Unmeasured streamed grass biome identity")?;
                let tint = mcbe_core::grass::load(
                    self.material_pack
                        .as_mut()
                        .ok_or("Grass coloring requires a resource pack")?,
                    &mut resources.behavior,
                    biome,
                )?;
                self.grass_tints.insert(code, tint);
            }
        }
        Ok(changed)
    }
    pub fn look(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx;
        self.pitch = (self.pitch - dy).clamp(-1.5, 1.5);
    }
    pub fn eye(&self) -> [f64; 3] {
        [
            self.actor.position.x as f64,
            (self.actor.position.y + self.movement.eye_height()) as f64,
            self.actor.position.z as f64,
        ]
    }
    pub fn direction(&self) -> [f64; 3] {
        let (s, c) = self.yaw.sin_cos();
        let (p, q) = self.pitch.sin_cos();
        [(s * q) as f64, p as f64, (c * q) as f64]
    }
    pub fn tick(&mut self, input: Input) -> Result<()> {
        crate::engine_player::tick(self, input)
    }
    pub fn target(&self) -> Result<Option<Hit>> {
        self.scene.target(self.eye(), self.direction())
    }
    pub fn edit(&mut self, place: bool) -> Result<bool> {
        let Some(hit) = self.target()? else {
            return Ok(false);
        };
        let mut p = [hit.position.x, hit.position.y, hit.position.z];
        if place {
            let (axis, sign) = match hit.face {
                Face::West => (0, -1),
                Face::East => (0, 1),
                Face::Down => (1, -1),
                Face::Up => (1, 1),
                Face::North => (2, -1),
                Face::South => (2, 1),
            };
            p[axis] += sign;
            let a = self.actor.position;
            let half = self.actor.body.width * 0.5;
            let low = [a.x - half, a.y, a.z - half];
            let high = [a.x + half, a.y + self.actor.body.height, a.z + half];
            let intersects = if let Some(state) = &self.scene.engine {
                state.player.selected().is_some_and(|id| {
                    state.geometry.boxes(id).iter().any(|b| {
                        (0..3).all(|axis| {
                            low[axis] < p[axis] as f32 + b[axis + 3]
                                && high[axis] > p[axis] as f32 + b[axis]
                        })
                    })
                })
            } else {
                (0..3).all(|axis| low[axis] < (p[axis] + 1) as f32 && high[axis] > p[axis] as f32)
            };
            if !self.scene.inside(p) || intersects {
                return Ok(false);
            }
            if self.scene.cells.contains(&p) || self.scene.containers.contains_key(&p) {
                return Ok(false);
            }
            if let Some(state) = &self.scene.engine {
                let Some(id) = state.player.selected() else {
                    return Ok(false);
                };
                self.scene.change_engine(p, Some(id))?;
            } else {
                self.scene.change(p, Some(self.placement))?;
            }
        } else if !self.scene.cells.contains(&p) {
            return Ok(false);
        } else {
            if self.scene.engine.is_some() {
                self.scene.change_engine(p, None)?;
            } else {
                self.scene.change(p, None)?;
            }
        }
        self.changed += 1;
        Ok(true)
    }
}
