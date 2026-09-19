mod container_world;
mod engine;
mod engine_assets;
mod engine_blocks;
mod engine_crafting;
mod engine_geometry;
mod engine_gpu;
mod engine_gpu_ui;
mod engine_inventory;
mod engine_inventory_study;
mod engine_loading;
mod engine_loading_study;
mod engine_mesh;
mod engine_meshing;
mod engine_player;
mod engine_save_study;
mod engine_saving;
mod engine_stream;
mod engine_stream_study;
mod engine_ui;
mod engine_v1_study;
mod inventory_persistence;
mod inventory_ui;
mod persistence;
mod render;
mod scene;
mod streamed;
mod study;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use scene::{Client, Input, Result, Scene};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

struct App {
    v1_test: bool,
    v1_report: Option<serde_json::Value>,
    load_test: bool,
    load_close_test: bool,
    load_stage: usize,
    load_origin: mcbe_core::motion::Vec3,
    load_report: Option<serde_json::Value>,
    loader: Option<engine_loading::Service>,
    load_events: Vec<engine_loading::Event>,
    load_error: Option<String>,
    loading_frames: usize,
    async_step: usize,
    saver: Option<engine_saving::Service>,
    save_events: Vec<engine_saving::Event>,
    save_error: Option<String>,
    deferred: Option<KeyCode>,
    close_after_save: bool,
    close_completed: bool,
    close_requested_busy: bool,
    save_close_test: bool,
    saving_frames: usize,
    save_test: bool,
    save_start: Option<usize>,
    save_report: Option<serde_json::Value>,
    sync_mesh: bool,
    async_test: bool,
    async_save: Option<PathBuf>,
    stream_start: Option<usize>,
    stream_tour_frames: usize,
    async_mode_trace: Vec<serde_json::Value>,
    travel_index: usize,
    stream_test: bool,
    stream_trace: Vec<serde_json::Value>,
    naive_mesh: bool,
    mesh_toggle_test: bool,
    mesh_toggle_trace: Vec<serde_json::Value>,
    profile_seconds: Option<f32>,
    profile_started: Option<Instant>,
    inventory_test: bool,
    inventory_mesh_check: Option<serde_json::Value>,
    modifiers: winit::keyboard::ModifiersState,
    window_test: bool,
    edit_test: bool,
    window_transitions: Vec<serde_json::Value>,
    gpu: Option<engine_gpu::Renderer>,
    wireframe: bool,
    frame_limit: usize,
    capture_out: Option<PathBuf>,
    simulation_previous: mcbe_core::motion::Vec3,
    palette: engine_ui::Palette,
    cursor: (f64, f64),
    debug: bool,
    client: Client,
    inventory: Option<inventory_ui::Panel>,
    inventory_open: bool,
    world_inventory: bool,
    window: Option<std::sync::Arc<Window>>,
    keys: HashSet<KeyCode>,
    captured: bool,
    previous: Instant,
    next_frame: Instant,
    accumulator: f32,
    frames: usize,
    inventory_frames: usize,
    smoke: bool,
    error: Option<String>,
    notice: Option<String>,
    root: PathBuf,
}
impl App {
    fn new(client: Client, root: PathBuf, inventory_start: bool, smoke: bool) -> Result<Self> {
        let mut palette = engine_ui::Palette::default();
        if client.scene.engine.as_ref().is_some_and(|s| {
            s.player.cursor.is_some() || s.player.crafting.iter().any(Option::is_some)
        }) {
            palette.open = true;
            palette.viewport = [480., 270.];
            palette.pointer = [240., 135.];
        }
        Ok(Self {
            v1_test: false,
            v1_report: None,
            load_test: false,
            load_close_test: false,
            load_stage: 0,
            load_origin: client.actor.position,
            load_report: None,
            loader: None,
            load_events: Vec::new(),
            load_error: None,
            loading_frames: 0,
            async_step: 0,
            saver: None,
            save_events: Vec::new(),
            save_error: None,
            deferred: None,
            close_after_save: false,
            close_completed: false,
            close_requested_busy: false,
            save_close_test: false,
            saving_frames: 0,
            save_test: false,
            save_start: None,
            save_report: None,
            travel_index: 0,
            stream_test: false,
            stream_trace: Vec::new(),
            sync_mesh: false,
            async_test: false,
            async_save: None,
            stream_start: None,
            stream_tour_frames: 420,
            async_mode_trace: Vec::new(),
            naive_mesh: false,
            mesh_toggle_test: false,
            mesh_toggle_trace: Vec::new(),
            profile_seconds: None,
            profile_started: None,
            inventory_test: false,
            inventory_mesh_check: None,
            modifiers: Default::default(),
            window_test: false,
            edit_test: false,
            window_transitions: Vec::new(),
            gpu: None,
            wireframe: false,
            frame_limit: 3,
            capture_out: None,
            simulation_previous: client.actor.position,
            palette,
            cursor: (0., 0.),
            debug: false,
            client,
            inventory: if inventory_start {
                Some(inventory_ui::Panel::load(&root)?)
            } else {
                None
            },
            inventory_open: inventory_start,
            world_inventory: false,
            window: None,
            keys: HashSet::new(),
            captured: false,
            previous: Instant::now(),
            next_frame: Instant::now(),
            accumulator: 0.,
            frames: 0,
            inventory_frames: 0,
            smoke,
            error: None,
            notice: None,
            root,
        })
    }
    fn toggle_fullscreen(&self) {
        if let Some(w) = &self.window {
            if w.fullscreen().is_some() {
                w.set_fullscreen(None);
                w.set_maximized(true);
            } else {
                w.set_fullscreen(Some(winit::window::Fullscreen::Borderless(
                    w.current_monitor(),
                )));
            }
        }
    }
    fn engine_key(&mut self, code: KeyCode) -> Result<bool> {
        if self.client.scene.engine.is_none() || code == KeyCode::F11 {
            return Ok(false);
        }
        if self.load_busy() && matches!(code, KeyCode::F5 | KeyCode::F6) {
            self.deferred = Some(code);
            self.notice = Some("Action will follow destination preparation".into());
            return Ok(true);
        }
        if code == KeyCode::Escape && self.load_busy() {
            self.loader.as_mut().unwrap().cancel();
            self.deferred = None;
            self.capture(false);
            self.notice = Some("Preparation cancelled; current world retained".into());
            return Ok(true);
        }
        if matches!(code, KeyCode::KeyR | KeyCode::KeyT)
            && !self.palette.open
            && self.client.scene.engine.as_ref().unwrap().stream.is_some()
        {
            let destination = if code == KeyCode::KeyR {
                engine_loading::Destination::Exhibit(0)
            } else {
                engine_loading::Destination::Overlook
            };
            self.request_travel(destination)?;
            return Ok(true);
        }
        if self.load_busy()
            && !matches!(
                code,
                KeyCode::F1
                    | KeyCode::F2
                    | KeyCode::F3
                    | KeyCode::F4
                    | KeyCode::F7
                    | KeyCode::F8
                    | KeyCode::F9
                    | KeyCode::F10
                    | KeyCode::F11
            )
        {
            return Ok(true);
        }
        if matches!(code, KeyCode::KeyF | KeyCode::KeyN) && !self.palette.open {
            let current = self.client.movement.mode;
            let next = if code == KeyCode::KeyN {
                if current == engine_player::Mode::Noclip {
                    engine_player::Mode::Flying
                } else {
                    engine_player::Mode::Noclip
                }
            } else if current == engine_player::Mode::Walking {
                engine_player::Mode::Flying
            } else {
                engine_player::Mode::Walking
            };
            engine_player::mode(&mut self.client, next)?;
            self.notice = Some(format!(
                "Movement: {:?} | F fly | N noclip | Space up / Shift down",
                next
            ));
            return Ok(true);
        }
        if code == KeyCode::F4 {
            self.wireframe = !self.wireframe;
            return Ok(true);
        }
        if code == KeyCode::F2 {
            self.sync_mesh = !self.sync_mesh;
            if let Some(gpu) = &mut self.gpu {
                gpu.set_async(!self.sync_mesh);
            }
            return Ok(true);
        }
        if code == KeyCode::F8 {
            self.naive_mesh = !self.naive_mesh;
            if let Some(gpu) = &mut self.gpu {
                gpu.set_greedy(!self.naive_mesh);
            }
            return Ok(true);
        }
        if code == KeyCode::F7 {
            if let Some(gpu) = &mut self.gpu {
                gpu.stats.gui_scale = if gpu.stats.gui_scale >= 3. {
                    1.
                } else {
                    gpu.stats.gui_scale + 1.
                };
            }
            if self.palette.open {
                let (position, viewport) = self.inventory_pointer();
                self.palette.move_pointer(&self.client, position, viewport);
            }
            return Ok(true);
        }
        if code == KeyCode::F5 {
            self.request_save()?;
            self.notice = Some("Saving world snapshot...".into());
            return Ok(true);
        }
        if code == KeyCode::F9 {
            if self.save_busy() {
                self.deferred = Some(code);
                self.notice = Some("Reload will follow the save".into());
                return Ok(true);
            }
            self.ensure_loader()?;
            self.loader.as_mut().unwrap().reload(&self.client)?;
            self.begin_loading("Preparing saved world...");
            return Ok(true);
        }
        if code == KeyCode::F10 {
            self.travel_index = (self.travel_index + 1) % 4;
            self.request_travel(engine_loading::Destination::Exhibit(self.travel_index))?;
            return Ok(true);
        }
        if self.palette.open {
            self.palette.viewport = self.inventory_pointer().1;
            self.palette.key(&mut self.client, code)?;
            if !self.palette.open {
                self.capture(true);
            }
            self.keys.clear();
            return Ok(true);
        }
        match code {
            KeyCode::KeyI | KeyCode::KeyE => {
                self.palette.toggle(&mut self.client)?;
                let (position, viewport) = self.inventory_pointer();
                self.palette.move_pointer(&self.client, position, viewport);
                self.keys.clear();
                self.capture(false);
                self.accumulator = 0.;
                Ok(true)
            }
            KeyCode::F3 => {
                self.debug = !self.debug;
                Ok(true)
            }
            KeyCode::F4 => {
                self.wireframe = !self.wireframe;
                Ok(true)
            }
            KeyCode::F7 => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.stats.gui_scale = if gpu.stats.gui_scale >= 3. {
                        1.
                    } else {
                        gpu.stats.gui_scale + 1.
                    };
                }
                Ok(true)
            }
            KeyCode::F1 => {
                self.notice = if self.notice.is_some() {
                    None
                } else {
                    Some("WASD move | Space jump/up | Shift sneak/down | F fly | N noclip | E blocks | F5 save | F9 reload | F4 wireframe | F11 fullscreen".into())
                };
                Ok(true)
            }
            KeyCode::F6 => {
                if self.save_busy() {
                    self.deferred = Some(code);
                    self.notice = Some("Definitions will reload after saving".into());
                    return Ok(true);
                }
                crate::engine::reload_definitions(
                    &mut self.client,
                    &self.root.join("assets/blocks.json"),
                )?;
                self.notice = Some("Reloaded block definitions and textures".into());
                Ok(true)
            }
            _ => Ok(false),
        }
    }
    fn inventory_pointer(&self) -> ([f32; 2], [f32; 2]) {
        if let Some(gpu) = &self.gpu {
            let (w, h) = gpu.logical_size();
            (
                [
                    self.cursor.0 as f32 / gpu.stats.gui_scale,
                    self.cursor.1 as f32 / gpu.stats.gui_scale,
                ],
                [w as f32, h as f32],
            )
        } else {
            let size = self.window.as_ref().map(|w| w.inner_size()).unwrap_or(
                winit::dpi::PhysicalSize::new(render::WIDTH as u32, render::HEIGHT as u32),
            );
            (
                [
                    self.cursor.0 as f32 * render::WIDTH as f32 / size.width.max(1) as f32,
                    self.cursor.1 as f32 * render::HEIGHT as f32 / size.height.max(1) as f32,
                ],
                [render::WIDTH as f32, render::HEIGHT as f32],
            )
        }
    }
    fn open_target_inventory(&mut self) -> Result<bool> {
        let Some(hit) = self.client.target()? else {
            return Ok(false);
        };
        let p = [hit.position.x, hit.position.y, hit.position.z];
        if !self.client.scene.containers.contains_key(&p) {
            return Ok(false);
        }
        let reuse = self.world_inventory
            && self.inventory.as_ref().is_some_and(|panel| {
                panel.source() == self.client.scene.source && panel.positions() == [p]
            });
        if !reuse {
            if self.world_inventory
                && self
                    .inventory
                    .as_ref()
                    .is_some_and(|panel| panel.pending_slots().is_ok_and(|n| n > 0))
            {
                return Err(
                    "Save or reset container edits before opening another container".into(),
                );
            }
            self.inventory = Some(inventory_ui::Panel::load_target(
                &self.root,
                &self.client.scene.source.join("db"),
                p,
            )?);
        }
        self.world_inventory = true;
        self.inventory_open = true;
        self.keys.clear();
        self.accumulator = 0.;
        self.capture(false);
        Ok(true)
    }
    fn load_busy(&self) -> bool {
        self.loader.as_ref().is_some_and(|s| s.busy())
    }
    fn ensure_loader(&mut self) -> Result<()> {
        if self.loader.is_none() {
            self.loader = Some(engine_loading::Service::new()?);
        }
        Ok(())
    }
    fn begin_loading(&mut self, message: &str) {
        self.load_error = None;
        self.keys.clear();
        self.accumulator = 0.;
        self.capture(false);
        self.notice = Some(message.into());
    }
    fn request_travel(&mut self, destination: engine_loading::Destination) -> Result<()> {
        self.ensure_loader()?;
        self.loader
            .as_mut()
            .unwrap()
            .travel(&self.client, destination)?;
        self.begin_loading("Preparing destination... | Escape cancel | F10 next");
        Ok(())
    }
    fn poll_loads(&mut self) -> Result<()> {
        let event = if let Some(loader) = &mut self.loader {
            loader.poll(&mut self.client)?
        } else {
            None
        };
        if let Some(event) = event {
            if let Some(error) = &event.error {
                self.load_error = Some(error.clone());
                self.notice = Some(format!("Preparation failed: {error}"));
                self.deferred = None;
            } else if event.applied {
                self.simulation_previous = self.client.actor.position;
                self.accumulator = 0.;
                self.previous = Instant::now();
                if event.reload {
                    self.palette.cursor = 0;
                    self.palette.hover = None;
                    self.palette.revision = self.palette.revision.wrapping_add(1);
                    let player = &self.client.scene.engine.as_ref().unwrap().player;
                    self.palette.open =
                        player.cursor.is_some() || player.crafting.iter().any(Option::is_some);
                    self.notice = Some("Reloaded saved world and inventory".into());
                } else {
                    self.notice = Some("Destination ready | F10 next | R return".into());
                }
                if self.async_test {
                    self.notice = None;
                }
            }
            if self.load_events.len() == 64 {
                self.load_events.remove(0);
            }
            self.load_events.push(event);
        }
        Ok(())
    }
    fn loading_observation(&self) -> serde_json::Value {
        serde_json::json!({"busy":self.load_busy(),"counters":self.loader.as_ref().map(|l|&l.counters),"events":self.load_events,"loading_presented_frames":self.loading_frames,"error":self.load_error})
    }
    fn save_busy(&self) -> bool {
        self.saver.as_ref().is_some_and(|s| s.busy())
    }
    fn request_save(&mut self) -> Result<()> {
        if self.saver.is_none() {
            self.saver = Some(engine_saving::Service::new()?);
        }
        self.save_error = None;
        self.saver
            .as_mut()
            .unwrap()
            .request(&self.client, &self.root.join("saves"))?;
        Ok(())
    }
    fn poll_saves(&mut self) -> Result<()> {
        let event = if let Some(saver) = &mut self.saver {
            saver.poll(&mut self.client)?
        } else {
            None
        };
        if let Some(event) = event {
            if let Some(error) = &event.error {
                self.save_error = Some(error.clone());
                self.notice = Some(format!("Save failed: {error}"));
                self.close_after_save = false;
                self.deferred = None;
            } else {
                self.notice = Some(if event.adopted {
                    format!(
                        "Saved snapshot | {} unsaved edits",
                        self.client.pending_edits()
                    )
                } else {
                    "Saved earlier world snapshot; current world unchanged".into()
                });
                if self.async_test && event.adopted {
                    self.async_save = event.path.clone();
                    self.notice = None;
                }
            }
            if self.save_events.len() == 64 {
                self.save_events.remove(0);
            }
            self.save_events.push(event);
        }
        if !self.save_busy()
            && !self.load_busy()
            && !self.close_after_save
            && let Some(code) = self.deferred.take()
            && let Err(e) = self.engine_key(code)
        {
            self.save_error = Some(e.to_string());
            self.notice = Some(e.to_string());
        }
        Ok(())
    }
    fn finish_work(&mut self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.save_busy() || self.load_busy() || self.deferred.is_some() {
            self.poll_saves()?;
            self.poll_loads()?;
            if Instant::now() > deadline {
                return Err("Background save did not finish in verification".into());
            }
            if self.save_busy() || self.load_busy() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        if let Some(error) = self.save_error.as_ref().or(self.load_error.as_ref()) {
            return Err(error.clone().into());
        }
        Ok(())
    }
    fn saving_observation(&self) -> serde_json::Value {
        serde_json::json!({"busy":self.save_busy(),"snapshots":self.saver.as_ref().map_or(0,|s|s.snapshots()),"counters":self.saver.as_ref().map(|s|&s.counters),"events":self.save_events,"saving_presented_frames":self.saving_frames,"close_requested_busy":self.close_requested_busy,"close_completed":self.close_completed,"error":self.save_error})
    }
    fn save_current(&mut self) -> Result<PathBuf> {
        if self.client.scene.engine.is_some() {
            return engine::save(&mut self.client, &self.root.join("saves"));
        }
        if self.world_inventory {
            let panel = self
                .inventory
                .as_mut()
                .ok_or("World inventory panel missing")?;
            if panel.source() != self.client.scene.source {
                return Err("Container and walking view refer to different world copies".into());
            }
            let updates = panel.prepared_updates()?;
            self.client
                .save_unique_checked(&self.root, &updates, |scene| {
                    panel.adopt_saved(&scene.source)
                })?;
            Ok(self.client.scene.source.clone())
        } else {
            self.client.save_unique(&self.root)
        }
    }
    fn reload_current(&mut self) -> Result<()> {
        if self.client.scene.engine.is_some() {
            engine::reload(&mut self.client)?;
            let p = &self.client.scene.engine.as_ref().unwrap().player;
            if p.cursor.is_some() || p.crafting.iter().any(Option::is_some) {
                self.palette.open = true;
                self.capture(false);
            }
            return Ok(());
        }
        if self.world_inventory {
            let scene = self.client.prepare_reload()?;
            let position = *self
                .inventory
                .as_ref()
                .ok_or("World inventory panel missing")?
                .positions()
                .first()
                .ok_or("Selected container missing")?;
            let mut panel =
                inventory_ui::Panel::load_target(&self.root, &scene.source.join("db"), position)?;
            panel.adopt_saved(&scene.source)?;
            self.client.scene = scene;
            self.inventory = Some(panel);
        } else {
            self.client.reload_saved()?;
        }
        Ok(())
    }
    fn inventory_key(&mut self, code: KeyCode) -> Result<()> {
        if self.world_inventory && code == KeyCode::F5 {
            let path = self.save_current()?;
            self.notice = Some(format!(
                "Saved world and container edits to {}",
                path.display()
            ));
        } else if self.world_inventory && code == KeyCode::F9 {
            self.reload_current()?;
            self.notice = Some("Reloaded the latest world copy; unsaved edits discarded.".into());
        } else if let Some(panel) = &mut self.inventory {
            panel.key(code)?;
        }
        Ok(())
    }
    fn capture(&mut self, value: bool) {
        let Some(window) = &self.window else {
            return;
        };
        let mode = if value {
            CursorGrabMode::Confined
        } else {
            CursorGrabMode::None
        };
        match window.set_cursor_grab(mode) {
            Ok(()) => {
                self.captured = value;
                window.set_cursor_visible(!value);
            }
            Err(e) => self.error = Some(format!("Mouse capture: {e}")),
        }
    }
    fn redraw(&mut self) -> Result<()> {
        self.poll_saves()?;
        self.poll_loads()?;
        if self.load_test && !self.load_close_test {
            let ready = self.gpu.as_ref().is_some_and(|g| g.stats.meshes_ready);
            match self.load_stage {
                0 if ready => {
                    for _ in 0..3 {
                        self.engine_key(KeyCode::F10)?;
                    }
                    self.load_stage = 1;
                }
                1 if !self.load_busy() && ready => {
                    self.engine_key(KeyCode::F5)?;
                    self.load_stage = 2;
                }
                2 if !self.save_busy() => {
                    let oak = self
                        .client
                        .scene
                        .engine
                        .as_ref()
                        .unwrap()
                        .registry
                        .find("oak_planks")?;
                    self.client
                        .scene
                        .change_engine([320, 40, -257], Some(oak))?;
                    self.engine_key(KeyCode::F9)?;
                    self.load_stage = 3;
                }
                3 if !self.load_busy() && ready => {
                    self.load_stage = 4;
                    self.notice = None;
                    self.frame_limit = self.frames + 4;
                }
                _ => {}
            }
        }
        if self.async_test && self.load_busy() {
            self.frame_limit += 1;
        }
        if self.load_test
            && self.load_busy()
            && !self.load_close_test
            && self.frames.is_multiple_of(2)
        {
            self.engine_key(KeyCode::F4)?;
            self.engine_key(KeyCode::F4)?;
        }
        if self.save_test {
            if self.save_start.is_none() && self.gpu.as_ref().is_some_and(|g| g.stats.meshes_ready)
            {
                self.save_start = Some(self.frames);
                self.frame_limit = self.frames + 180;
            }
            if self.save_start == Some(self.frames) {
                self.engine_key(KeyCode::F5)?;
                let id = self
                    .client
                    .scene
                    .engine
                    .as_ref()
                    .unwrap()
                    .registry
                    .find("oak_planks")?;
                self.client.scene.change_engine([8, 4, -4], Some(id))?;
                self.client.scene.engine.as_mut().unwrap().player.slots[10] =
                    Some(engine_inventory::Stack { id, count: 5 });
            }
        }
        if self.stream_test {
            if self.stream_start.is_none()
                && self.gpu.as_ref().is_some_and(|g| g.stats.meshes_ready)
            {
                self.stream_start = Some(self.frames);
                self.frame_limit = self.frames + self.stream_tour_frames;
                let gpu = self.gpu.as_mut().unwrap();
                gpu.stats.warmup_frames = self.frames + 10;
                gpu.frame_times.clear();
            }
            let phase = self
                .frames
                .saturating_sub(self.stream_start.unwrap_or(self.frames))
                .min(self.stream_tour_frames - 1);
            let tour = if (phase / 420).is_multiple_of(2) {
                phase % 420
            } else {
                419 - phase % 420
            } as f32;
            let x = (tour * 0.25).floor() as i32;
            let stream = self
                .client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .stream
                .as_ref()
                .unwrap();
            let y = (1..62)
                .rev()
                .find(|y| stream.observed([x, *y, -10]).is_some())
                .unwrap_or(0)
                + 2;
            self.client.actor.position =
                mcbe_core::motion::Vec3::new(tour * 0.25 + 0.5, y as f32, -9.5);
            self.client.yaw = 0.75;
            self.client.pitch = -0.3;
        }
        if self.async_test && !self.load_busy() {
            let phase = self.async_step;
            self.async_step += 1;
            match phase {
                1 => {
                    let id = self
                        .client
                        .scene
                        .engine
                        .as_ref()
                        .unwrap()
                        .registry
                        .find("oak_planks")?;
                    self.client.scene.change_engine([2, 3, -2], Some(id))?;
                }
                3 => {
                    self.engine_key(KeyCode::F10)?;
                }
                5 => {
                    let mut registry = self.client.scene.engine.as_ref().unwrap().registry.clone();
                    registry.blocks.reverse();
                    std::fs::create_dir_all(self.root.join("assets"))?;
                    std::fs::write(
                        self.root.join("assets/blocks.json"),
                        serde_json::to_vec(&registry)?,
                    )?;
                    std::fs::write(
                        self.root.join("assets/recipes.json"),
                        serde_json::to_vec(&self.client.scene.engine.as_ref().unwrap().recipes)?,
                    )?;
                    self.engine_key(KeyCode::F6)?;
                }
                140 | 142 => {
                    self.engine_key(KeyCode::F2)?;
                }
                7 | 9 | 122 => {
                    self.engine_key(KeyCode::F8)?;
                }
                90 => {
                    let id = self
                        .client
                        .scene
                        .engine
                        .as_ref()
                        .unwrap()
                        .registry
                        .find("oak_planks")?;
                    self.client.scene.change_engine([128, 40, 128], Some(id))?;
                }
                120 => {
                    if !self.gpu.as_ref().unwrap().has_mesh([8, 2, 8]) {
                        return Err("Async removal fixture was not uploaded".into());
                    }
                    self.client.scene.change_engine([128, 40, 128], None)?;
                    self.engine_key(KeyCode::F8)?;
                }
                10 | 123 => {
                    self.engine_key(KeyCode::F5)?;
                    self.async_save = self.client.last_save.clone();
                    self.notice = None;
                }
                _ => {}
            }
        }
        self.client.sync_region()?;
        if self.stream_test && self.frames + 1 >= self.frame_limit {
            let stream = engine_stream::observation(&self.client);
            if stream["resident_columns"] != stream["wanted_columns"] || stream["queued_jobs"] != 0
            {
                if self.frame_limit >= 2000 {
                    return Err("Native streaming tour failed to settle".into());
                }
                self.frame_limit += 1;
            }
        }
        let now = Instant::now();
        let dt = (now - self.previous).as_secs_f32().min(0.25);
        self.previous = now;
        if self.window.as_ref().is_some_and(|w| w.has_focus())
            && !self.smoke
            && !self.inventory_open
            && !self.palette.open
            && !self.close_after_save
            && !self.load_busy()
        {
            let down = |key| f32::from(self.keys.contains(&key));
            self.client.look(
                (down(KeyCode::ArrowRight) - down(KeyCode::ArrowLeft)) * dt * 1.8,
                (down(KeyCode::ArrowDown) - down(KeyCode::ArrowUp)) * dt * 1.8,
            );
            self.accumulator += dt;
            while self.accumulator >= 0.05 {
                self.simulation_previous = self.client.actor.position;
                self.client.tick(Input {
                    forward: down(KeyCode::KeyW) - down(KeyCode::KeyS),
                    side: down(KeyCode::KeyD) - down(KeyCode::KeyA),
                    jump: self.keys.contains(&KeyCode::Space),
                    sneak: self.keys.contains(&KeyCode::ShiftLeft)
                        || self.keys.contains(&KeyCode::ShiftRight),
                })?;
                self.accumulator -= 0.05;
            }
        } else {
            self.accumulator = 0.0;
        }
        if self.window_test {
            if self.frames == 1 || self.frames == 4 {
                self.toggle_fullscreen();
            }
            if [0, 3, 6].contains(&self.frames)
                && let Some(w) = &self.window
            {
                let s = w.inner_size();
                self.window_transitions.push(serde_json::json!({"frame":self.frames,"fullscreen":w.fullscreen().is_some(),"maximized":w.is_maximized(),"width":s.width,"height":s.height,"monitor":w.current_monitor().map(|m|[m.size().width,m.size().height])}));
            }
        }
        if self.edit_test && self.frames == 1 {
            self.client
                .set_spawn(mcbe_core::motion::Vec3::new(0.5, 4., -2.5))?;
            self.client.yaw = 0.;
            self.client.pitch = -0.65;
            let selected = self
                .client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .registry
                .find("oak_planks")?;
            {
                let player = &mut self.client.scene.engine.as_mut().unwrap().player;
                player.assign(selected, player.active)?;
            }
            if !self.client.edit(false)? || !self.client.edit(true)? {
                return Err("GPU live editing fixture failed".into());
            }
            self.client.set_spawn(engine::SPAWN)?;
            self.client.yaw = 0.;
            self.client.pitch = -0.08;
        }
        if self.mesh_toggle_test && [1, 3].contains(&self.frames) {
            self.engine_key(KeyCode::F8)?;
        }
        if let Some(seconds) = self.profile_seconds
            && self.frames >= 120
        {
            let started = self.profile_started.get_or_insert_with(Instant::now);
            let elapsed = started.elapsed().as_secs_f32();
            self.client.yaw = elapsed * std::f32::consts::TAU / 6.;
            self.client.pitch = -0.15 + (elapsed * 1.5).sin() * 0.15;
            if elapsed >= seconds {
                self.frame_limit = self.frames + 1;
            }
        }
        if self.smoke && self.frame_limit == 240 && self.profile_seconds.is_none() {
            self.client.yaw = self.frames as f32 * std::f32::consts::TAU / 240.;
            self.client.pitch = -0.15 + (self.frames as f32 * 0.07).sin() * 0.15;
        }
        if self.inventory_test && self.frames == 1 {
            let instance = self.client.scene.engine.as_ref().unwrap().instance;
            let before = self.gpu.as_ref().unwrap().stats.mesh_rebuilds;
            self.engine_key(KeyCode::F5)?;
            let unchanged = instance == self.client.scene.engine.as_ref().unwrap().instance;
            if !unchanged {
                return Err("Saving inventory replaced the live world instance".into());
            }
            self.inventory_mesh_check = Some(
                serde_json::json!({"instance_preserved": unchanged, "mesh_rebuilds_before": before}),
            );
        }
        if self.smoke
            && self.frames + 1 >= self.frame_limit
            && (self.save_busy() || self.load_busy() || self.deferred.is_some())
        {
            return Ok(());
        }
        let save_active = self.save_busy();
        let load_active = self.load_busy();
        if let Some(gpu) = &mut self.gpu {
            let size = self.window.as_ref().unwrap().inner_size();
            if size.width == 0 || size.height == 0 {
                return Ok(());
            }
            if gpu.stats.width != size.width || gpu.stats.height != size.height {
                gpu.resize(size.width, size.height);
            }
            if self.palette.open {
                let (w, h) = gpu.logical_size();
                let viewport = [w as f32, h as f32];
                if self.palette.viewport != viewport {
                    let old = engine_ui::Palette::origin(self.palette.viewport);
                    let new = engine_ui::Palette::origin(viewport);
                    let position = [
                        self.palette.pointer[0] + new[0] - old[0],
                        self.palette.pointer[1] + new[1] - old[1],
                    ];
                    self.palette.move_pointer(&self.client, position, viewport);
                }
            }
            let current = self.client.actor.position;
            let previous = self.simulation_previous;
            let alpha = (self.accumulator / 0.05).clamp(0., 1.);
            let lerp = if !self.smoke
                && !self.palette.open
                && (current.x - previous.x).abs() < 1.
                && (current.z - previous.z).abs() < 1.
                && (current.y - previous.y).abs() < 1.
            {
                alpha
            } else {
                1.
            };
            let eye = [
                previous.x + (current.x - previous.x) * lerp,
                previous.y + (current.y - previous.y) * lerp + self.client.movement.eye_height(),
                previous.z + (current.z - previous.z) * lerp,
            ];
            let capture = if self.smoke && self.frames + 1 >= self.frame_limit {
                self.capture_out.as_ref().map(|o| o.join("gpu.png"))
            } else {
                None
            };
            let presented = gpu.draw(
                &self.client,
                &self.palette,
                engine_gpu::Frame {
                    wire: self.wireframe,
                    debug: self.debug,
                    notice: self.notice.as_deref(),
                    eye,
                    capture: capture.as_deref(),
                },
            )?;
            if presented {
                self.frames += 1;
                self.saving_frames += usize::from(save_active);
                self.loading_frames += usize::from(load_active);
            }
            if presented && self.mesh_toggle_test && [1, 3, 5].contains(&self.frames) {
                self.mesh_toggle_trace.push(serde_json::json!({"frame":self.frames,"greedy":gpu.stats.greedy_meshing,"mesh_quads":gpu.stats.mesh_quads,"exposed_faces":gpu.stats.exposed_faces,"mesh_rebuilds":gpu.stats.mesh_rebuilds}));
            }
            if presented
                && self.stream_test
                && let Some(start) = self.stream_start
            {
                let tour = self.frames - start;
                if [1, 65, 129, 193, 257, 321, 385, 420].contains(&tour)
                    || tour == self.stream_tour_frames
                {
                    self.stream_trace.push(serde_json::json!({"frame":self.frames,"tour_frame":tour,"stream":engine_stream::observation(&self.client),"gpu_chunks":gpu.stats.chunks,"mesh_bytes":gpu.stats.mesh_bytes,"mesh_rebuilds":gpu.stats.mesh_rebuilds,"texture_uploads":gpu.stats.texture_uploads,"mesh_pending":gpu.stats.mesh_pending_chunks,"mesh_jobs":gpu.stats.mesh_jobs_in_flight}));
                }
            }
            if presented
                && self.async_test
                && !load_active
                && [139, 141, 143].contains(&self.async_step)
            {
                self.async_mode_trace.push(serde_json::json!({"frame":self.frames,"async":gpu.stats.async_meshing,"main_builds":gpu.stats.mesh_main_builds,"ready":gpu.stats.meshes_ready}));
            }
            if self.inventory_test && self.frames == 2 {
                let check = self.inventory_mesh_check.as_mut().unwrap();
                check["mesh_rebuilds_after"] = serde_json::json!(gpu.stats.mesh_rebuilds);
                check["complete"] = serde_json::json!(
                    check["mesh_rebuilds_before"] == check["mesh_rebuilds_after"]
                );
                if check["complete"] != true {
                    return Err("Inventory-only save rebuilt terrain meshes".into());
                }
            }
            return Ok(());
        }
        let mut pixels = render::render(&self.client, render::WIDTH, render::HEIGHT);
        if self.client.scene.engine.is_some() {
            engine_ui::draw(
                &self.client,
                &self.palette,
                &mut pixels,
                render::WIDTH,
                render::HEIGHT,
            );
        }
        if let Some(window) = &self.window {
            present(
                window,
                &pixels,
                &self.client,
                self.captured,
                self.debug,
                self.error.as_deref().or(self.notice.as_deref()),
                if self.inventory_open {
                    self.inventory.as_ref()
                } else {
                    None
                },
            )?;
        }
        self.frames += 1;
        if self.inventory_open {
            self.inventory_frames += 1;
        }
        Ok(())
    }
}
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(if self.client.scene.engine.is_some() {
                "Blockwork | Blockwork Version 1 | Checkpoint 44"
            } else {
                "MCBE Reconstruction | Rust research client"
            })
            .with_inner_size(winit::dpi::LogicalSize::new(1100.0, 680.0))
            .with_maximized(true);
        match event_loop.create_window(attributes) {
            Ok(window) => {
                let window = std::sync::Arc::new(window);
                if self.client.scene.engine.is_some() {
                    match engine_gpu::Renderer::new(window.clone()) {
                        Ok(mut gpu) => {
                            gpu.set_greedy(!self.naive_mesh);
                            gpu.set_async(!self.sync_mesh);
                            gpu.collect_timings = self.smoke;
                            if self.profile_seconds.is_some() {
                                gpu.stats.warmup_frames = 120;
                            }
                            self.gpu = Some(gpu);
                        }
                        Err(e) => {
                            self.error = Some(e.to_string());
                            event_loop.exit();
                            return;
                        }
                    }
                }
                self.window = Some(window);
                self.previous = Instant::now();
            }
            Err(e) => {
                self.error = Some(e.to_string());
                event_loop.exit();
            }
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // Automated fixtures drive the same controllers explicitly. Physical input
        // must not move their cameras, edit worlds or capture the user's cursor.
        if self.smoke
            && matches!(
                &event,
                WindowEvent::KeyboardInput { .. }
                    | WindowEvent::MouseInput { .. }
                    | WindowEvent::MouseWheel { .. }
                    | WindowEvent::CursorMoved { .. }
                    | WindowEvent::ModifiersChanged(_)
            )
        {
            return;
        }
        if self.close_after_save
            && matches!(
                &event,
                WindowEvent::KeyboardInput { .. }
                    | WindowEvent::MouseInput { .. }
                    | WindowEvent::MouseWheel { .. }
            )
        {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                let loading = self.load_busy();
                if loading {
                    self.loader.as_mut().unwrap().cancel();
                }
                if self.save_busy() || loading {
                    if self.save_busy()
                        && !self.saver.as_ref().unwrap().latest_matches(&self.client)
                        && let Err(e) = self.request_save()
                    {
                        self.notice = Some(e.to_string());
                        return;
                    }
                    self.close_after_save = true;
                    self.close_requested_busy = true;
                    self.deferred = None;
                    self.keys.clear();
                    self.capture(false);
                    self.notice = Some("Finishing pending work before closing...".into());
                } else {
                    self.close_completed = true;
                    event_loop.exit();
                }
            }
            WindowEvent::Focused(false) => {
                self.keys.clear();
                self.capture(false);
            }
            WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. } if self.load_busy() => {
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if self.palette.open
                        && !self.load_busy()
                        && self.palette.search
                        && event.state == ElementState::Pressed
                        && !self.modifiers.control_key()
                        && !self.modifiers.alt_key()
                        && let Some(text) = &event.text
                    {
                        self.palette.text(text);
                    }
                    if event.state == ElementState::Pressed && !event.repeat {
                        match self.engine_key(code) {
                            Ok(true) => return,
                            Ok(false) => {}
                            Err(e) => {
                                self.notice = Some(e.to_string());
                                return;
                            }
                        }
                    }
                    if code == KeyCode::KeyI
                        && event.state == ElementState::Pressed
                        && !event.repeat
                    {
                        if self.client.scene.engine.is_some() {
                            self.notice = Some("Unlimited creative palette: 1 Meadow / 2 Earth / 3 Slate / 4 Clay / 5 Ivory / 6 Moss / 7 Basalt / 8 Ember".into());
                            return;
                        }
                        if self.inventory.is_none() {
                            match inventory_ui::Panel::load(&self.root) {
                                Ok(panel) => self.inventory = Some(panel),
                                Err(error) => {
                                    self.notice = Some(format!("Inventory: {error}"));
                                    return;
                                }
                            }
                        }
                        self.inventory_open = !self.inventory_open;
                        self.keys.clear();
                        self.accumulator = 0.;
                        self.capture(false);
                        return;
                    }
                    if self.inventory_open {
                        if event.state == ElementState::Pressed && !event.repeat {
                            if code == KeyCode::Escape {
                                self.inventory_open = false;
                            } else if let Err(error) = self.inventory_key(code) {
                                self.notice = Some(format!("Inventory: {error}"));
                            }
                        }
                        return;
                    }
                    if event.state == ElementState::Pressed {
                        self.keys.insert(code);
                        if !event.repeat {
                            match code {
                                KeyCode::F11 => self.toggle_fullscreen(),
                                KeyCode::Escape => self.capture(false),
                                KeyCode::KeyR if self.client.scene.engine.is_some() => {
                                    if let Err(e) = engine::reset(&mut self.client) {
                                        self.notice = Some(e.to_string());
                                    }
                                }
                                KeyCode::KeyR => self.client.reset(),
                                KeyCode::KeyG => self.client.cycle_material(),
                                KeyCode::Digit1
                                | KeyCode::Digit2
                                | KeyCode::Digit3
                                | KeyCode::Digit4
                                | KeyCode::Digit5
                                | KeyCode::Digit6
                                | KeyCode::Digit7
                                | KeyCode::Digit8
                                | KeyCode::Digit9
                                    if self.client.scene.engine.is_some() =>
                                {
                                    let index = match code {
                                        KeyCode::Digit1 => 0,
                                        KeyCode::Digit2 => 1,
                                        KeyCode::Digit3 => 2,
                                        KeyCode::Digit4 => 3,
                                        KeyCode::Digit5 => 4,
                                        KeyCode::Digit6 => 5,
                                        KeyCode::Digit7 => 6,
                                        KeyCode::Digit8 => 7,
                                        _ => 8,
                                    };
                                    let state = self.client.scene.engine.as_mut().unwrap();
                                    let _ = state.player.select(index);
                                }
                                KeyCode::KeyE => {
                                    if let Err(error) = self.open_target_inventory() {
                                        self.notice = Some(format!("Container: {error}"));
                                    }
                                }
                                KeyCode::KeyT if self.client.scene.engine.is_some() => {
                                    match self
                                        .client
                                        .set_spawn(mcbe_core::motion::Vec3::new(20.5, 13., 0.5))
                                    {
                                        Ok(()) => {
                                            self.client.yaw = -1.4;
                                            self.client.pitch = -0.4;
                                        }
                                        Err(e) => self.notice = Some(e.to_string()),
                                    }
                                }
                                KeyCode::KeyT => {
                                    self.client.actor = mcbe_core::motion::Actor::new(
                                        mcbe_core::motion::Vec3::new(-20.5, -60.0, -8.5),
                                        mcbe_core::motion::Body::PROBE,
                                    )
                                    .expect("Constant terrain spawn");
                                    self.client.yaw = -2.25;
                                    self.client.pitch = 0.25;
                                }
                                KeyCode::F5 => {
                                    self.notice = Some(match self.save_current() {
                                        Ok(path) => format!("Saved to {}", path.display()),
                                        Err(e) => format!("Save: {e}"),
                                    });
                                    self.keys.clear();
                                    self.previous = Instant::now();
                                    self.accumulator = 0.0;
                                }
                                KeyCode::F9 => {
                                    self.notice=Some(match self.reload_current() {
                                        Ok(())=>"Reloaded latest saved copy; unsaved block changes discarded.".into(),
                                        Err(e)=>format!("Reload: {e}"),
                                    });
                                    self.keys.clear();
                                    self.previous = Instant::now();
                                    self.accumulator = 0.0;
                                }
                                _ => {}
                            }
                        }
                    } else {
                        self.keys.remove(&code);
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => {
                if self.palette.open {
                    if matches!(button, MouseButton::Left | MouseButton::Right) {
                        let (position, viewport) = self.inventory_pointer();
                        let pointer = engine_ui::Pointer {
                            position,
                            viewport,
                            button: if button == MouseButton::Right {
                                engine_inventory::Button::Right
                            } else {
                                engine_inventory::Button::Left
                            },
                            shift: self.modifiers.shift_key(),
                        };
                        if let Err(e) = self.palette.click(&mut self.client, pointer) {
                            self.notice = Some(e.to_string());
                        }
                    }
                    return;
                }
                if self.inventory_open {
                    return;
                }
                if !self.captured {
                    self.capture(true);
                } else if button == MouseButton::Middle && self.client.scene.engine.is_some() {
                    if let Ok(Some(hit)) = self.client.target() {
                        let p = [hit.position.x, hit.position.y, hit.position.z];
                        let state = self.client.scene.engine.as_mut().unwrap();
                        if let Some(id) = state.blocks.get(&p).copied() {
                            if let Some(i) = state.player.slots[..9]
                                .iter()
                                .position(|s| s.is_some_and(|s| s.id == id))
                            {
                                let _ = state.player.select(i);
                            } else {
                                let _ = state.player.assign(id, state.player.active);
                            }
                        }
                    }
                } else if button == MouseButton::Right
                    && self.client.target().ok().flatten().is_some_and(|hit| {
                        self.client.scene.containers.contains_key(&[
                            hit.position.x,
                            hit.position.y,
                            hit.position.z,
                        ])
                    })
                {
                    if let Err(error) = self.open_target_inventory() {
                        self.notice = Some(format!("Container: {error}"));
                    }
                } else if matches!(button, MouseButton::Left | MouseButton::Right)
                    && let Err(e) = self.client.edit(button == MouseButton::Right)
                {
                    self.notice = Some(e.to_string());
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    self.error = Some(e.to_string());
                    event_loop.exit();
                }
                if self.load_close_test && self.frames == 1 && !self.close_after_save {
                    if let Err(e) = self.engine_key(KeyCode::F10) {
                        self.error = Some(e.to_string());
                        event_loop.exit();
                    }
                    self.window_event(event_loop, _id, WindowEvent::CloseRequested);
                }
                if self.save_close_test && self.frames == 1 && !self.close_after_save {
                    if let Err(e) = self.engine_key(KeyCode::F5) {
                        self.error = Some(e.to_string());
                        event_loop.exit();
                    }
                    let id = self
                        .client
                        .scene
                        .engine
                        .as_ref()
                        .unwrap()
                        .registry
                        .find("oak_planks")
                        .unwrap();
                    if let Err(e) = self.client.scene.change_engine([9, 4, -4], Some(id)) {
                        self.error = Some(e.to_string());
                        event_loop.exit();
                    }
                    self.window_event(event_loop, _id, WindowEvent::CloseRequested);
                }
                if self.close_after_save && !self.save_busy() && !self.load_busy() {
                    self.close_completed = true;
                    event_loop.exit();
                }
                if self.smoke
                    && self.frames >= self.frame_limit
                    && !self.save_busy()
                    && !self.load_busy()
                {
                    event_loop.exit();
                }
            }
            WindowEvent::ModifiersChanged(mods) => self.modifiers = mods.state(),
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(state) = &mut self.client.scene.engine {
                    let y = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                        winit::event::MouseScrollDelta::PixelDelta(p) => p.y,
                    };
                    let step = if y > 0. {
                        -1
                    } else if y < 0. {
                        1
                    } else {
                        0
                    };
                    if self.palette.open {
                        self.palette.scroll(&self.client, step);
                    } else {
                        state.player.cycle(step);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                if self.palette.open {
                    let (position, viewport) = self.inventory_pointer();
                    self.palette.move_pointer(&self.client, position, viewport);
                }
            }
            _ => {}
        }
    }
    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if !self.smoke
            && self.captured
            && let DeviceEvent::MouseMotion { delta: (x, y) } = event
        {
            self.client.look(x as f32 * 0.0025, y as f32 * 0.0025);
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some()
            && (self.smoke || self.window.as_ref().is_some_and(|w| w.has_focus()))
            && let Some(w) = &self.window
        {
            let size = w.inner_size();
            if size.width > 0 && size.height > 0 {
                w.request_redraw();
                event_loop.set_control_flow(ControlFlow::Poll);
                return;
            }
        }
        let now = Instant::now();
        if now >= self.next_frame
            && let Some(window) = &self.window
        {
            window.request_redraw();
            self.next_frame = now + Duration::from_millis(33);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

fn present(
    window: &Window,
    pixels: &[u32],
    client: &Client,
    captured: bool,
    debug: bool,
    error: Option<&str>,
    inventory: Option<&inventory_ui::Panel>,
) -> Result<()> {
    use windows_sys::Win32::Graphics::Gdi::*;
    let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
        return Err("This client currently requires Windows".into());
    };
    let hwnd = handle.hwnd.get() as *mut std::ffi::c_void;
    let size = window.inner_size();
    if size.width == 0 || size.height == 0 {
        return Ok(());
    }
    let bitmap = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: render::WIDTH as i32,
            biHeight: -(render::HEIGHT as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    };
    let target = client
        .target()?
        .map(|h| {
            format!(
                "{}, {}, {} / {:?}",
                h.position.x, h.position.y, h.position.z, h.face
            )
        })
        .unwrap_or_else(|| "none within 6 blocks".into());
    let p = client.actor.position;
    let lines = [
        "MCBE RECONSTRUCTION / RUST DEVELOPMENT CLIENT".to_string(),
        if client.stone_material.is_some() {
            client.scene.residency().map_or_else(||"Fixed research view | provisional grass colors".to_owned(),|r|format!("Saved columns {}/{} | {} unavailable | provisional colors and player input",r.ready,r.requested,r.missing+r.rejected.len()))
        } else {
            "Captured stone scene | explicit procedural material | input under development".into()
        },
        "WASD walk / Space test jump / Mouse or arrows look / R arena / T terrain / I inventory study".into(),
        format!(
            "Click capture / Esc release / Left remove / Right place {} / G study palette",
            client.placement.resource_key()
        ),
        format!(
            "Position {:.2}, {:.2}, {:.2} | target {target}",
            p.x, p.y, p.z
        ),
        format!(
            "{} blocks | {} unsaved changes | {} | source world stays unchanged",
            client.scene.cells.len(),
            client.pending_edits(),
            if captured {
                "mouse captured"
            } else {
                "mouse released"
            }
        ),
        error
            .unwrap_or(
                "F5: save new world copy / F9: reload latest copy / Close: discard unsaved changes",
            )
            .to_owned(),
    ];
    let lines = if client.scene.engine.is_some() && !debug {
        error.map(str::to_owned).into_iter().collect::<Vec<_>>()
    } else if client.scene.engine.is_some() {
        vec![
            format!("BLOCKWORK / RUST ENGINE / CHECKPOINT {}", engine::CHECKPOINT),
            "WASD move  |  Space jump  |  Mouse / arrows look  |  Click to capture  |  Esc release".into(),
            "Left remove  |  Right build  |  1-8 / G material  |  F5 save  |  F9 reload  |  R reset  |  T overlook".into(),
            format!("{}  |  {} blocks  |  {} unsaved edits  |  {:.1}, {:.1}, {:.1}", engine::material(client).map_or("Empty hand",|b|b.label.as_str()), client.scene.cells.len(),client.pending_edits(),p.x,p.y,p.z),
            error.unwrap_or("Build something in the court. Saves are .blockworld files in the saves folder.").into(),
        ]
    } else if let Some(panel) = inventory {
        panel.lines()?
    } else {
        lines.to_vec()
    };
    // SAFETY: HWND belongs to the live winit window on this thread; pixel and
    // bitmap storage remain valid throughout the synchronous GDI calls. Every
    // acquired DC is released before any fallible operation or return.
    unsafe {
        let dc = GetDC(hwnd);
        if dc.is_null() {
            return Err("GetDC failed".into());
        }
        let rows = StretchDIBits(
            dc,
            0,
            0,
            size.width as i32,
            size.height as i32,
            0,
            0,
            render::WIDTH as i32,
            render::HEIGHT as i32,
            pixels.as_ptr().cast(),
            &bitmap,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
        SetBkMode(dc, OPAQUE as i32);
        SetBkColor(dc, 0x00201910);
        SetTextColor(dc, 0x00eaf6e5);
        let old_font = if inventory.is_some() {
            let rectangle = windows_sys::Win32::Foundation::RECT {
                left: 0,
                top: 0,
                right: size.width as i32,
                bottom: (lines.len() as i32 * 22 + 38).min(size.height as i32),
            };
            FillRect(dc, &rectangle, GetStockObject(BLACK_BRUSH) as _);
            SelectObject(dc, GetStockObject(SYSTEM_FIXED_FONT))
        } else {
            std::ptr::null_mut()
        };
        for (i, line) in lines.iter().enumerate() {
            let text: Vec<u16> = line.encode_utf16().collect();
            TextOutW(dc, 18, 16 + i as i32 * 22, text.as_ptr(), text.len() as i32);
        }
        if !old_font.is_null() {
            SelectObject(dc, old_font);
        }
        ReleaseDC(hwnd, dc);
        if rows == 0 || rows == -1 {
            return Err("StretchDIBits failed".into());
        }
    }
    Ok(())
}

fn verify(client: &mut Client, root: &Path, source: &Path, out: &Path) -> Result<()> {
    if out.exists() {
        return Err("Use a new verification output folder".into());
    }
    std::fs::create_dir_all(out)?;
    let before = mcbe_core::world_edit::tree_hashes(source)?;
    let initial = client.scene.cells.clone();
    render::png(
        &out.join("initial.png"),
        &render::render(client, render::WIDTH, render::HEIGHT),
        render::WIDTH,
        render::HEIGHT,
    )?;
    let hit = client.target()?.ok_or("Initial wall target absent")?;
    let position = [hit.position.x, hit.position.y, hit.position.z];
    if !client.edit(true)? || client.scene.cells.len() != initial.len() + 1 {
        return Err("Stone placement action failed".into());
    }
    if !client.edit(false)? || client.scene.cells != initial {
        return Err("Removing newly placed stone failed".into());
    }
    if !client.edit(false)? || client.scene.cells.contains(&position) {
        return Err("Remove action failed".into());
    }
    client.scene.cells.insert(position);
    for _ in 0..40 {
        client.tick(Input {
            forward: 1.0,
            ..Default::default()
        })?;
    }
    let stopped = client.actor.position;
    if stopped.x > -38.29 || stopped.x < -39.5 {
        return Err(format!("Actor did not stop at saved wall: {stopped:?}").into());
    }
    if client.edit(true)? {
        return Err("Placement intersecting actor was accepted".into());
    }
    client.tick(Input {
        jump: true,
        ..Default::default()
    })?;
    let mut peak = client.actor.position.y;
    for _ in 0..40 {
        client.tick(Input::default())?;
        peak = peak.max(client.actor.position.y);
    }
    if peak < -30.0 || (client.actor.position.y + 31.0).abs() > 0.00002 {
        return Err("Test jump failed to rise and land on saved floor".into());
    }
    client.reset();
    client.actor.position.x = -43.5;
    client.yaw = 0.9;
    render::png(
        &out.join("overview.png"),
        &render::render(client, 960, 540),
        960,
        540,
    )?;
    client.actor = mcbe_core::motion::Actor::new(
        mcbe_core::motion::Vec3::new(-20.5, -60.0, -8.5),
        mcbe_core::motion::Body::PROBE,
    )?;
    client.yaw = -2.25;
    client.pitch = 0.25;
    render::png(
        &out.join("terrain.png"),
        &render::render(client, 960, 540),
        960,
        540,
    )?;
    for _ in 0..20 {
        client.tick(Input::default())?;
    }
    if (client.actor.position.y + 60.0).abs() > 0.00002 {
        return Err("Actor did not stand on saved grass terrain".into());
    }
    let hole = [[-21, -61, -9], [-21, -62, -9]];
    let mut removed = Vec::new();
    for p in hole {
        let kind = client
            .scene
            .kind_at(p)
            .ok_or("Missing temporary hole cell")?;
        client.scene.cells.remove(&p);
        client.scene.kinds.remove(&p);
        removed.push((p, kind));
    }
    client.actor = mcbe_core::motion::Actor::new(
        mcbe_core::motion::Vec3::new(-20.5, -62.0, -8.5),
        mcbe_core::motion::Body::PROBE,
    )?;
    client.yaw = std::f32::consts::FRAC_PI_2;
    client.pitch = 0.0;
    for _ in 0..20 {
        client.tick(Input::default())?;
    }
    if (client.actor.position.y + 62.0).abs() > 0.00002 {
        return Err("Actor did not stand in temporary terrain hole".into());
    }
    render::png(
        &out.join("grass-side.png"),
        &render::render(client, 960, 540),
        960,
        540,
    )?;
    for (p, kind) in removed {
        client.scene.cells.insert(p);
        client.scene.kinds.insert(p, kind);
    }
    client.reset();
    client.yaw = std::f32::consts::FRAC_PI_2;
    let unchanged = mcbe_core::world_edit::tree_hashes(source)? == before;
    if !unchanged || client.scene.cells != initial {
        return Err("Verification did not preserve source or restore scene".into());
    }
    let persistence = persistence::verify(root, source, out)?;
    let materials = client.stone_material.as_ref().map(|m| m.provenance.clone());
    let mut cube_materials: Vec<_> = client.cube_materials.iter().collect();
    cube_materials.sort_by_key(|(key, _)| **key);
    std::fs::write(out.join("cube-materials.json"),serde_json::to_vec_pretty(&cube_materials.iter().map(|((kind,face),value)|serde_json::json!({"kind":kind,"face":face,"material":value.provenance})).collect::<Vec<_>>())?)?;
    let mut material_counts = std::collections::BTreeMap::new();
    for p in &initial {
        let kind = client.scene.kind_at(*p).ok_or("Missing loaded cube kind")?;
        *material_counts.entry(kind).or_insert(0usize) += 1;
    }
    let mut grass_tints: Vec<_> = client
        .grass_tints
        .iter()
        .map(|(code, tint)| serde_json::json!({"code":code,"tint":tint.provenance}))
        .collect();
    grass_tints.sort_by_key(|row| row["code"].as_u64().unwrap_or_default());
    let mut biome_counts = std::collections::BTreeMap::new();
    for code in client.scene.biome_codes.values() {
        *biome_counts.entry(code.to_string()).or_insert(0usize) += 1;
    }
    std::fs::write(
        out.join("grass-tints.json"),
        serde_json::to_vec_pretty(&grass_tints)?,
    )?;
    std::fs::write(
        out.join("materials.json"),
        serde_json::to_vec_pretty(&materials)?,
    )?;
    std::fs::write(
        out.join("verification.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"complete":true,"source_unchanged":unchanged,"source_hashes":before,"source":source,"source_files":client.scene.source_files,"loaded_solid_blocks":initial.len(),"material_counts":material_counts,"grass_biome_counts":biome_counts,"grass_tints":grass_tints,"terrain_standing":true,"removed_and_restored":position,"placement_and_removal":true,"actor_intersection_rejected":true,"collision_stop":stopped,"test_jump_peak":peak,"region_min":scene::MIN,"region_max":scene::MAX,"materials":materials,"persistence":persistence,"scope":"Client integration checks; development lighting/input and inferred resource-based grass coloring, no native player/visual parity"}),
        )?,
    )?;
    Ok(())
}
fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.is_empty() || arguments.iter().any(|a| a.starts_with("--engine")) {
        return engine::launch(&arguments);
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut database = root.join(
        "tools/vendor/bds-1.26.52.3/runtime-collision-holdout-04/worlds/reconstruction-probe/db",
    );
    let mut mode = None;
    let mut out = None;
    let mut pack =
        PathBuf::from(r"C:\XboxGames\Minecraft for Windows\Content\data\resource_packs\vanilla");
    let mut procedural = false;
    let mut raw_grass = false;
    let mut inventory_start = false;
    let mut fixed_region = false;
    let mut radius = 1u8;
    let mut spawn = None;
    let mut behavior_pack =
        PathBuf::from(r"C:\XboxGames\Minecraft for Windows\Content\data\behavior_packs\vanilla");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--pack" => {
                i += 1;
                pack = PathBuf::from(args.get(i).ok_or("--pack requires path")?);
            }
            "--procedural" => procedural = true,
            "--raw-grass" => raw_grass = true,
            "--inventory" => inventory_start = true,
            "--fixed-region" => fixed_region = true,
            "--spawn" => {
                let mut xyz = [0f32; 3];
                for value in &mut xyz {
                    i += 1;
                    *value = args.get(i).ok_or("--spawn requires X Y Z")?.parse()?;
                }
                spawn = Some(mcbe_core::motion::Vec3::new(xyz[0], xyz[1], xyz[2]));
            }
            "--radius" => {
                i += 1;
                radius = args.get(i).ok_or("--radius requires an integer")?.parse()?;
            }
            "--behavior-pack" => {
                i += 1;
                behavior_pack = PathBuf::from(args.get(i).ok_or("--behavior-pack requires path")?);
            }
            "--db" => {
                i += 1;
                database = PathBuf::from(args.get(i).ok_or("--db requires path")?);
            }
            "--headless"
            | "--smoke"
            | "--study"
            | "--study-smoke"
            | "--inventory-study"
            | "--inventory-smoke"
            | "--container-study"
            | "--container-smoke"
            | "--container-inventory-smoke" => {
                mode = Some(args[i].clone());
                i += 1;
                out = Some(PathBuf::from(
                    args.get(i).ok_or("Mode requires fresh output folder")?,
                ));
            }
            _ => {
                return Err(
                    "Usage: mcbe-client [--db STOPPED_DB] [--spawn X Y Z] [--radius 1..3 | --fixed-region] [--pack PACK | --procedural] [--behavior-pack PACK] [--raw-grass] [--inventory] [--headless NEW_OUT | --smoke NEW_OUT | --study NEW_OUT | --study-smoke NEW_OUT | --inventory-study NEW_OUT | --inventory-smoke NEW_OUT]"
                        .into(),
                );
            }
        }
        i += 1;
    }
    let mut client = Client::new(if fixed_region {
        Scene::load_fixed(&database)?
    } else {
        Scene::load_streamed(
            &database,
            spawn.map_or([-3, -2], |p| {
                [
                    (p.x.floor() as i32).div_euclid(16),
                    (p.z.floor() as i32).div_euclid(16),
                ]
            }),
            radius,
        )?
    })?;
    if let Some(spawn) = spawn {
        client.set_spawn(spawn)?;
    }
    if !procedural {
        let mut selected = mcbe_core::resources::Pack::open(&pack)?;
        let mut grass_context = None;
        let material =
            mcbe_core::textures::block_material(&mut selected, "minecraft:stone", "up", 0)?;
        if material
            .texture
            .rgba
            .chunks_exact(4)
            .any(|pixel| pixel[3] != 255)
        {
            return Err("This stone renderer requires an opaque texture".into());
        }
        for face in ["down", "north", "south", "east", "west"] {
            let other =
                mcbe_core::textures::block_material(&mut selected, "minecraft:stone", face, 0)?;
            if other.texture.width != material.texture.width
                || other.texture.height != material.texture.height
                || other.texture.rgba != material.texture.rgba
            {
                return Err(
                    "Current stone renderer requires the same texture on every face".into(),
                );
            }
        }
        if !raw_grass {
            let catalog: serde_json::Value = serde_json::from_slice(&std::fs::read(
                root.join("evidence/biome-normal-04/corrected-analysis/analysis.json"),
            )?)?;
            if catalog["complete"] != true
                || catalog["executable_sha256"]
                    != "19c88569af2e4b7d984e999055a31cbcb0799dacf8bbbf7371eda42f5772a443"
            {
                return Err("Unknown biome identity catalog".into());
            }
            let mut behavior = mcbe_core::resources::Pack::open(&behavior_pack)?;
            let codes: std::collections::BTreeSet<_> =
                client.scene.biome_codes.values().copied().collect();
            for code in codes {
                let biome = catalog["mapping"][code.to_string()]
                    .as_str()
                    .ok_or("Unmeasured grass biome identity")?;
                let tint = mcbe_core::grass::load(&mut selected, &mut behavior, biome)?;
                client.grass_tints.insert(code, tint);
            }
            behavior.verify_unchanged()?;
            grass_context = Some((behavior, catalog));
        }
        selected.verify_unchanged()?;
        client.stone_material = Some(material);
        for (kind, key) in [
            (mcbe_core::cubes::Cube::Dirt, "dirt"),
            (mcbe_core::cubes::Cube::Grass, "grass"),
            (mcbe_core::cubes::Cube::Bedrock, "bedrock"),
        ] {
            for (index, face) in ["up", "down", "north", "south", "east", "west"]
                .iter()
                .enumerate()
            {
                let material = mcbe_core::textures::block_material(&mut selected, key, face, 0)?;
                client.cube_materials.insert((kind, index), material);
            }
        }
        selected.verify_unchanged()?;
        if let Some((behavior, catalog)) = grass_context {
            client.grass_resources = Some(scene::GrassResources { behavior, catalog });
        }
        client.material_pack = Some(selected);
    }
    client.sync_region()?;
    if matches!(
        mode.as_deref(),
        Some("--container-study" | "--container-smoke" | "--container-inventory-smoke")
    ) {
        let destination = out
            .as_ref()
            .ok_or("Container verification output missing")?;
        let mut app = App::new(
            client,
            root,
            false,
            mode.as_deref() != Some("--container-study"),
        )?;
        container_world::verify(&mut app, destination)?;
        if mode.as_deref() == Some("--container-study") {
            println!("Container world integration checks passed");
            return Ok(());
        }
        if mode.as_deref() == Some("--container-inventory-smoke") {
            app.open_target_inventory()?;
        }
        return run_window(app, out);
    }
    if let Some(out) = &out {
        if matches!(
            mode.as_deref(),
            Some("--inventory-study" | "--inventory-smoke")
        ) {
            inventory_ui::Panel::verify(&root, out)?;
        } else if matches!(mode.as_deref(), Some("--study" | "--study-smoke")) {
            study::verify(&mut client, &root, out)?;
        } else if fixed_region {
            verify(&mut client, &root, &database, out)?;
        } else {
            streamed::verify(&mut client, &root, &database, out)?;
        }
    }
    if matches!(
        mode.as_deref(),
        Some("--headless" | "--study" | "--inventory-study")
    ) {
        println!("Client integration checks passed");
        return Ok(());
    }
    let inventory_start = inventory_start || mode.as_deref() == Some("--inventory-smoke");
    let app = App {
        v1_test: false,
        v1_report: None,
        load_test: false,
        load_close_test: false,
        load_stage: 0,
        load_origin: client.actor.position,
        load_report: None,
        loader: None,
        load_events: Vec::new(),
        load_error: None,
        loading_frames: 0,
        async_step: 0,
        saver: None,
        save_events: Vec::new(),
        save_error: None,
        deferred: None,
        close_after_save: false,
        close_completed: false,
        close_requested_busy: false,
        save_close_test: false,
        saving_frames: 0,
        save_test: false,
        save_start: None,
        save_report: None,
        travel_index: 0,
        stream_test: false,
        stream_trace: Vec::new(),
        sync_mesh: false,
        async_test: false,
        async_save: None,
        stream_start: None,
        stream_tour_frames: 420,
        async_mode_trace: Vec::new(),
        naive_mesh: false,
        mesh_toggle_test: false,
        mesh_toggle_trace: Vec::new(),
        profile_seconds: None,
        profile_started: None,
        inventory_test: false,
        inventory_mesh_check: None,
        modifiers: Default::default(),
        window_test: false,
        edit_test: false,
        window_transitions: Vec::new(),
        gpu: None,
        wireframe: false,
        frame_limit: 3,
        capture_out: None,
        simulation_previous: client.actor.position,
        palette: engine_ui::Palette::default(),
        cursor: (0., 0.),
        debug: false,
        client,
        inventory: if inventory_start {
            Some(inventory_ui::Panel::load(&root)?)
        } else {
            None
        },
        inventory_open: inventory_start,
        world_inventory: false,
        window: None,
        keys: HashSet::new(),
        captured: false,
        previous: Instant::now(),
        next_frame: Instant::now(),
        accumulator: 0.0,
        frames: 0,
        inventory_frames: 0,
        smoke: matches!(
            mode.as_deref(),
            Some("--smoke" | "--study-smoke" | "--inventory-smoke")
        ),
        error: None,
        notice: None,
        root,
    };
    run_window(app, out)
}
fn run_window(mut app: App, out: Option<PathBuf>) -> Result<()> {
    app.capture_out = out.clone();
    EventLoop::new()?.run_app(&mut app)?;
    if let Some(error) = app.error {
        return Err(error.into());
    }
    if let Some(out) = out {
        if app.v1_test {
            let geometry = app.gpu.as_ref().unwrap().verify_geometry(&app.client)?;
            std::fs::write(
                out.join("version1-native.json"),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"complete":geometry["complete"]==true,"checkpoint":engine::CHECKPOINT,"version":"1.0","movement":app.client.movement,"eye":app.client.eye(),"position":app.client.actor.position,"geometry":geometry,"pending_edits":app.client.pending_edits(),"saved_world":app.client.last_save,"report":app.v1_report}),
                )?,
            )?;
        }
        if app.load_test || app.load_close_test {
            let geometry = if app.load_test && !app.load_close_test {
                Some(app.gpu.as_ref().unwrap().verify_geometry(&app.client)?)
            } else {
                None
            };
            std::fs::write(
                out.join("loading-native.json"),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"complete":!app.load_busy()&&app.load_error.is_none(),"checkpoint":engine::CHECKPOINT,"close_test":app.load_close_test,"loading":app.loading_observation(),"close_completed":app.close_completed,"stage":app.load_stage,"position":app.client.actor.position,"origin":app.load_origin,"pending_edits":app.client.pending_edits(),"saved_world":app.client.last_save,"post_reload_block_present":app.client.scene.engine.as_ref().unwrap().blocks.contains_key(&[320,40,-257]),"geometry":geometry,"headless":app.load_report}),
                )?,
            )?;
        }
        if app.save_test || app.save_close_test {
            let saved = app
                .client
                .last_save
                .clone()
                .ok_or("Background save file missing")?;
            let document: serde_json::Value = serde_json::from_slice(&std::fs::read(&saved)?)?;
            let geometry = if app.save_test {
                app.gpu.as_ref().unwrap().verify_geometry(&app.client)?
            } else {
                serde_json::Value::Null
            };
            std::fs::write(
                out.join("saving-native.json"),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"complete":!app.save_busy()&&app.save_error.is_none(),"checkpoint":engine::CHECKPOINT,"close_test":app.save_close_test,"saving":app.saving_observation(),"saved_world":saved,"saved_document":document,"pending_edits":app.client.pending_edits(),"live_post_request_block":app.client.scene.engine.as_ref().unwrap().blocks.contains_key(&[8,4,-4]),"geometry":geometry,"headless":app.save_report}),
                )?,
            )?;
        }
        if let Some(gpu) = &app.gpu {
            if app.async_test {
                let geometry = gpu.verify_geometry(&app.client)?;
                std::fs::write(
                    out.join("async.json"),
                    serde_json::to_vec_pretty(
                        &serde_json::json!({"complete":geometry["complete"]==true&&gpu.stats.meshes_ready&&gpu.stats.mesh_discarded_jobs>0,"checkpoint":engine::CHECKPOINT,"saved_world":app.async_save,"mode_trace":app.async_mode_trace,"geometry":geometry,"streaming":engine_stream::observation(&app.client),"statistics":gpu.stats,"scope":"Live edits, F10 eviction, F6 registry reload, F8 and F2 round trips, last-block removal and F5 while mesh jobs are pending; final mesh fingerprints match fresh synchronous snapshots."}),
                    )?,
                )?;
            }
            let mut times = gpu.frame_times.clone();
            times.sort_by(f64::total_cmp);
            let timing = if times.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::json!({"samples":times.len(),"median_frame_ms":times[times.len()/2],"p95_frame_ms":times[times.len()*95/100],"mean_presented_fps":1000./(times.iter().sum::<f64>()/times.len()as f64),"scope":"CPU event/present intervals with GPU backpressure; excludes warmup frames (statistics.warmup_frames) and screenshot capture; not physical display refresh or GPU timestamp duration"})
            };
            std::fs::write(
                out.join("gpu.json"),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"complete":app.close_completed||app.frames>=app.frame_limit,"statistics":gpu.stats,"saving":app.saving_observation(),"loading":app.loading_observation(),"timing":timing,"wireframe":app.wireframe,"palette_open":app.palette.open,"window_transitions":app.window_transitions,"live_edit_test":app.edit_test,"pending_edits":app.client.pending_edits(),"inventory_save_mesh_check":app.inventory_mesh_check,"mesh_toggle_trace":app.mesh_toggle_trace,"streaming":engine_stream::observation(&app.client),"stream_trace":app.stream_trace,"profile_seconds":app.profile_seconds,"profile_elapsed_seconds":app.profile_started.map(|t|t.elapsed().as_secs_f32()),"target":app.client.target()?.map(|h|[h.position.x,h.position.y,h.position.z]),"uncapped_event_loop":true,"back_face_culling":true,"frustum_culling":true,"internal_faces_removed":true,"depth_testing":true,"dirty_chunk_rebuilds":true}),
                )?,
            )?;
        }
        std::fs::write(
            out.join("window.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"complete":app.close_completed||app.frames>=app.frame_limit,"presented_frames":app.frames,"inventory_presented_frames":app.inventory_frames,"inventory_open":app.inventory_open,"palette_open":app.palette.open,"maximized":app.window.as_ref().is_some_and(|w|w.is_maximized()),"fullscreen":app.window.as_ref().is_some_and(|w|w.fullscreen().is_some()),"world_inventory":app.world_inventory,"selected_containers":app.inventory.as_ref().map(|p|p.positions()),"inventory_state":app.inventory.as_ref().map(|p|p.containers.observation()),"backend":if app.gpu.is_some(){"wgpu Direct3D 12 native-resolution meshes + winit events"}else{"Windows GDI framebuffer + winit events"},"width":app.gpu.as_ref().map_or(render::WIDTH as u32,|g|g.stats.width),"height":app.gpu.as_ref().map_or(render::HEIGHT as u32,|g|g.stats.height)}),
            )?,
        )?;
    }
    Ok(())
}
