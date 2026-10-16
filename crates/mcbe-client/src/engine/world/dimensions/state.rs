//! Sparse inactive worlds. Shared inventory and render assets stay on the client.
use super::{Definition, Definitions};
use crate::{
    engine::{
        self, WorldState, blocks::systems::simulation::Simulation, entities, fluids, machines,
        persistence,
    },
    engine_blocks::{BlockId, Registry},
    engine_player::Movement,
    engine_stream::{self as stream, Generator},
    scene::{Client, Result, Scene},
};
use mcbe_core::motion::Vec3;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};
#[derive(Clone)]
pub struct World {
    pub seed: u64,
    pub generator: stream::Config,
    pub edits: BTreeMap<[i32; 3], Option<BlockId>>,
    pub fluids: fluids::Fluids,
    pub machines: machines::Machines,
    pub entities: entities::World,
    pub simulation: Simulation,
    pub movement: Movement,
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}
#[derive(Clone)]
pub struct State {
    pub definitions: Definitions,
    pub base_seed: u64,
    pub active: String,
    pub inactive: BTreeMap<String, World>,
    pub revision: u64,
    pub baseline: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Saved {
    pub version: u32,
    pub definitions: Definitions,
    pub base_seed: u64,
    pub active: String,
    pub inactive: Vec<SavedWorld>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedWorld {
    id: String,
    seed: u64,
    generator: stream::Config,
    edits: Vec<engine::SavedEdit>,
    fluids: Vec<fluids::SavedAmount>,
    machines: persistence::machines::SavedMachines,
    entities: entities::Saved,
    movement: Movement,
    position: Vec3,
    yaw: f32,
    pitch: f32,
}
impl Saved {
    pub fn has_skin_data(&self) -> bool {
        self.inactive
            .iter()
            .any(|w| w.entities.definitions.has_skin_data())
    }
}
impl World {
    pub fn capture(client: &Client) -> Result<Self> {
        let s = client
            .scene
            .engine
            .as_ref()
            .ok_or("Dimension world missing")?;
        let generator = s
            .stream
            .as_ref()
            .ok_or("Dimensions require a generated world")?;
        Ok(Self {
            seed: s.seed,
            generator: generator.generator.config.clone(),
            edits: generator.edits.clone(),
            fluids: s.fluids.clone(),
            machines: s.machines.clone(),
            entities: s.entities.clone(),
            simulation: s.simulation.clone(),
            movement: client.movement,
            position: client.actor.position,
            yaw: client.yaw,
            pitch: client.pitch,
        })
    }
    fn saved(&self, id: &str, r: &Registry) -> Result<SavedWorld> {
        Ok(SavedWorld {
            id: id.into(),
            seed: self.seed,
            generator: self.generator.clone(),
            edits: self
                .edits
                .iter()
                .map(|(p, id)| engine::SavedEdit {
                    position: *p,
                    material: id.map(|id| r.get(id).id.clone()),
                })
                .collect(),
            fluids: self
                .fluids
                .amounts
                .iter()
                .map(|(p, a)| fluids::SavedAmount {
                    position: *p,
                    amount: *a,
                })
                .collect(),
            machines: persistence::machines::SavedMachines::capture(&self.machines, r)?,
            entities: self.entities.saved(),
            movement: self.movement,
            position: self.position,
            yaw: self.yaw,
            pitch: self.pitch,
        })
    }
    fn into_client(
        self,
        r: Registry,
        recipes: crate::engine_crafting::Recipes,
        smelting: machines::recipes::Recipes,
        radius: i32,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Client> {
        let mut config = self.generator;
        config.radius = radius;
        let generator = Generator::new(self.seed, &r, config)?;
        let mut stream = stream::State::new(generator);
        stream.edits = self.edits;
        stream.original_edits = stream.edits.clone();
        stream.reindex();
        let mut state = WorldState::new(self.seed, r, 0, None);
        state.stream = Some(stream);
        state.fluids = self.fluids;
        state.fluids.original = state.fluids.amounts.clone();
        state.original_machines = self.machines.clone();
        state.machines = self.machines;
        state.original_entity_revision = self.entities.stats.revision;
        state.entities = self.entities;
        state.simulation = self.simulation;
        state.recipes = recipes;
        state.smelting = smelting;
        let mut scene = Scene::from_engine(HashMap::new(), state)?;
        scene.bounds = scene
            .engine
            .as_ref()
            .unwrap()
            .stream
            .as_ref()
            .unwrap()
            .generator
            .bounds();
        let mut client = Client::new(scene)?;
        client.movement = self.movement;
        client.set_spawn_checked(self.position, cancelled)?;
        client.yaw = self.yaw;
        client.pitch = self.pitch;
        Ok(client)
    }
}
impl SavedWorld {
    fn restore(
        self,
        r: &Registry,
        recipes: &machines::recipes::Recipes,
    ) -> Result<(String, World)> {
        if !self.position.finite()
            || !self.yaw.is_finite()
            || !self.pitch.is_finite()
            || !(-1.5..=1.5).contains(&self.pitch)
            || self.edits.len() > stream::MAX_EDITS
        {
            return Err("Invalid saved dimension pose or edit count".into());
        }
        let generator = Generator::new(self.seed, r, self.generator.clone())?;
        let mut streamed = stream::State::new(generator);
        let bounds = streamed.generator.bounds();
        for e in self.edits {
            let id = e.material.as_deref().map(|id| r.find(id)).transpose()?;
            if !bounds.contains(e.position)
                || (self.generator.version == 1 && e.position[1] == 0)
                || id.is_some_and(|id| r.get(id).container.is_some())
                || streamed.generator.block(e.position) == id
                || streamed.edits.insert(e.position, id).is_some()
            {
                return Err("Invalid or duplicate saved dimension edit".into());
            }
        }
        streamed.original_edits = streamed.edits.clone();
        streamed.reindex();
        let mut state = WorldState::new(self.seed, r.clone(), 0, None);
        state.stream = Some(streamed);
        state.entities = entities::World::restore(self.entities, r)?;
        for e in state.entities.records.values() {
            if let Some(physics) = &e.physics {
                let actor = physics.actor(e.transform.position)?;
                let p = actor.position;
                let half = actor.body.width * 0.5;
                for x in (p.x - half).floor() as i32..(p.x + half).ceil() as i32 {
                    for y in p.y.floor() as i32..(p.y + actor.body.height).ceil() as i32 {
                        for z in (p.z - half).floor() as i32..(p.z + half).ceil() as i32 {
                            let position = [x, y, z];
                            if let Some(id) = state.stream.as_ref().unwrap().observed(position)
                                && engine::blocks::systems::collision::intersects_actor(
                                    &state.geometry,
                                    id,
                                    position,
                                    &actor,
                                )
                            {
                                return Err("Saved dimension geometry overlaps an entity".into());
                            }
                        }
                    }
                }
            }
        }
        state.machines = self.machines.restore(r, recipes)?;
        persistence::machines::validate_positions(&state, &state.machines, r)?;
        let mut scene = Scene::from_engine(HashMap::new(), state)?;
        scene.bounds = bounds;
        fluids::restore(&mut scene, Some(self.fluids), engine::SAVE_VERSION)?;
        let mut client = Client::new(scene)?;
        client.movement = self.movement;
        stream::validate_position(&client, self.position)?;
        let s = client.scene.engine.take().unwrap();
        Ok((
            self.id,
            World {
                seed: self.seed,
                generator: self.generator,
                edits: s.stream.unwrap().edits,
                fluids: s.fluids,
                machines: s.machines,
                entities: s.entities,
                simulation: s.simulation,
                movement: self.movement,
                position: self.position,
                yaw: self.yaw,
                pitch: self.pitch,
            },
        ))
    }
}
impl State {
    pub fn new(
        definitions: Definitions,
        base_seed: u64,
        active: String,
        r: &Registry,
    ) -> Result<Self> {
        let definitions = definitions.compiled(r)?;
        definitions.get(&active)?;
        Ok(Self {
            definitions,
            base_seed,
            active,
            inactive: BTreeMap::new(),
            revision: 0,
            baseline: 0,
        })
    }
    pub fn pending(&self) -> bool {
        self.revision != self.baseline
    }
    pub fn saved(&self, r: &Registry) -> Result<Saved> {
        Ok(Saved {
            version: 1,
            definitions: self.definitions.clone(),
            base_seed: self.base_seed,
            active: self.active.clone(),
            inactive: self
                .inactive
                .iter()
                .map(|(id, w)| w.saved(id, r))
                .collect::<Result<_>>()?,
        })
    }
    pub fn restore(
        saved: Saved,
        r: &Registry,
        recipes: &machines::recipes::Recipes,
    ) -> Result<Self> {
        if saved.version != 1 || saved.inactive.len() >= saved.definitions.dimensions.len() {
            return Err("Invalid saved dimension count/version".into());
        }
        let mut result = Self::new(saved.definitions, saved.base_seed, saved.active, r)?;
        for record in saved.inactive {
            result.definitions.get(&record.id)?;
            if record.id == result.active || result.inactive.contains_key(&record.id) {
                return Err("Duplicate active/inactive dimension ID".into());
            }
            let (id, world) = record.restore(r, recipes)?;
            result.inactive.insert(id, world);
        }
        Ok(result)
    }
    pub fn rebind(
        &self,
        old: &Registry,
        new: &Registry,
        previous: &machines::recipes::Recipes,
        recipes: &machines::recipes::Recipes,
        entities: Option<entities::definition::Definitions>,
        definitions: Option<Definitions>,
    ) -> Result<Self> {
        let definitions = definitions.unwrap_or_else(|| self.definitions.clone());
        definitions.validate(new)?;
        definitions.get(&self.active)?;
        let mut next = Self::new(definitions, self.base_seed, self.active.clone(), new)?;
        for (id, w) in &self.inactive {
            next.definitions.get(id)?;
            let generator = Generator::new(w.seed, old, w.generator.clone())?;
            for p in w.fluids.amounts.keys() {
                let id = w
                    .edits
                    .get(p)
                    .copied()
                    .unwrap_or_else(|| generator.block(*p))
                    .ok_or("Inactive fluid lost its material")?;
                let mapped = new.find(&old.get(id).id)?;
                if new.get(mapped).fluid != old.get(id).fluid {
                    return Err("Cannot change an inactive dimension fluid capability".into());
                }
            }
            let mut saved = w.saved(id, old)?;
            saved.machines = persistence::machines::SavedMachines::reconfigured(
                &w.machines,
                old,
                new,
                previous,
                recipes,
            )?;
            if let Some(definitions) = &entities {
                saved.entities = w
                    .entities
                    .reconfigured(definitions.clone().compile(new)?)?
                    .saved();
            }
            let (_, world) = saved.restore(new, recipes)?;
            next.inactive.insert(id.clone(), world);
        }
        next.revision = self.revision.wrapping_add(1);
        next.baseline = self.baseline;
        Ok(next)
    }
}
pub fn attach_legacy(client: &mut Client, mut definitions: Definitions) -> Result<()> {
    let s = client
        .scene
        .engine
        .as_ref()
        .ok_or("Dimension world missing")?;
    if s.dimensions.is_some() {
        return Ok(());
    }
    let Some(stream) = &s.stream else {
        return Ok(());
    };
    let active = if definitions.get("surface").is_ok() {
        "surface".to_owned()
    } else {
        let mut id = "original".to_owned();
        let mut index = 1;
        while definitions.get(&id).is_ok() {
            id = format!("original_{index}");
            index += 1;
        }
        definitions.dimensions.push(Definition {
            environment: Default::default(),
            version: 1,
            id: id.clone(),
            label: "Original World".into(),
            seed_offset: 0,
            terrain: stream
                .generator
                .config
                .terrain
                .clone()
                .unwrap_or_else(super::super::terrain::Settings::built_in),
        });
        id
    };
    let mut context = State::new(definitions, s.seed, active, &s.registry)?;
    context.revision = 1;
    client.scene.engine.as_mut().unwrap().dimensions = Some(context);
    Ok(())
}
/// Immutable request captured on the application thread; workers own preparation.
pub struct Travel {
    pub source: World,
    pub context: State,
    pub destination: String,
    pub registry: Registry,
    pub player: crate::engine_inventory::Inventory,
    pub original_player: crate::engine_inventory::Inventory,
    pub recipes: crate::engine_crafting::Recipes,
    pub smelting: machines::recipes::Recipes,
    pub ui: Arc<crate::engine::ui::framework::Library>,
    pub appearance: Arc<crate::engine::player::appearance::Appearance>,
    pub entity_assets: Arc<crate::engine::entities::assets::Assets>,
    pub camera_mode: crate::engine::player::camera::Mode,
    pub loaded: Option<(std::path::PathBuf, String)>,
    pub last_save: Option<std::path::PathBuf>,
    pub radius: i32,
}
impl Travel {
    pub fn capture(client: &Client, destination: &str) -> Result<Self> {
        let s = client
            .scene
            .engine
            .as_ref()
            .ok_or("Dimension world missing")?;
        let context = s
            .dimensions
            .as_ref()
            .ok_or("Dimension registry is unavailable")?;
        context.definitions.get(destination)?;
        if destination == context.active {
            return Err("Already in that dimension".into());
        }
        if client.view.is_free() {
            return Err("Return from free camera before dimension travel".into());
        }
        if s.player.cursor.is_some()
            || s.player
                .crafting
                .iter()
                .chain(&s.player.table)
                .any(Option::is_some)
        {
            return Err("Close crafting and settle held items before dimension travel".into());
        }
        Ok(Self {
            source: World::capture(client)?,
            context: context.clone(),
            destination: destination.into(),
            registry: s.registry.clone(),
            player: s.player.clone(),
            original_player: s.original_player.clone(),
            recipes: s.recipes.clone(),
            smelting: s.smelting.clone(),
            ui: s.ui.clone(),
            appearance: client.appearance.clone(),
            entity_assets: client.entity_assets.clone(),
            camera_mode: client.view.mode,
            loaded: s.loaded.clone(),
            last_save: client.last_save.clone(),
            radius: s.stream.as_ref().unwrap().generator.config.radius,
        })
    }
    #[cfg(test)]
    pub fn prepare(self) -> Result<Client> {
        self.prepare_with_token(&std::sync::atomic::AtomicU64::new(1), 1)
    }
    pub fn prepare_with_token(
        mut self,
        token: &std::sync::atomic::AtomicU64,
        id: u64,
    ) -> Result<Client> {
        let cancelled = || token.load(std::sync::atomic::Ordering::Acquire) != id;
        let mut client = if let Some(world) = self.context.inactive.remove(&self.destination) {
            world.into_client(
                self.registry.clone(),
                self.recipes.clone(),
                self.smelting.clone(),
                self.radius,
                &cancelled,
            )?
        } else {
            let definition = self.context.definitions.get(&self.destination)?;
            let mut client = stream::generate_config_checked(
                definition.seed(self.context.base_seed),
                self.registry.clone(),
                definition.config(self.radius),
                &cancelled,
            )?;
            let s = client.scene.engine.as_mut().unwrap();
            s.entities.library = self.source.entities.library.clone();
            s.recipes = self.recipes;
            s.smelting = self.smelting;
            client
        };
        self.context
            .inactive
            .insert(self.context.active.clone(), self.source);
        self.context.active = self.destination;
        self.context.revision = self.context.revision.wrapping_add(1);
        let s = client.scene.engine.as_mut().unwrap();
        s.dimensions = Some(self.context);
        s.player = self.player.clone();
        s.original_player = self.original_player;
        s.ui = self.ui;
        s.loaded = self.loaded;
        client.appearance = self.appearance;
        {
            let state = client.scene.engine.as_ref().unwrap();
            let (definitions, assets) = self
                .entity_assets
                .for_definitions(&state.entities.library.definitions, &state.registry)?;
            let entities = state
                .entities
                .reconfigured(definitions.compile(&state.registry)?)?;
            client.scene.engine.as_mut().unwrap().entities = entities;
            client.entity_assets = assets;
        }
        client.view.mode = self.camera_mode;
        client.last_save = self.last_save;
        *client.engine_distant_prepared.borrow_mut() =
            engine::render::distance::prepare(&client, token, id)?;
        Ok(client)
    }
}

#[cfg(test)]
mod tests;
