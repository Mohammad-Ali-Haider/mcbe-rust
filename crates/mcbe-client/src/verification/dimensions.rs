//! Real native dimensions, shared UI/input travel, saves and GPU LOD readback.
use crate::engine::input::gamepad::{Button as B, Pad};
use crate::{
    App, engine, engine_player, engine_ui,
    scene::{Client, Result},
};
use mcbe_core::motion::Vec3;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use winit::keyboard::KeyCode as K;
pub struct Live {
    case: String,
    out: PathBuf,
    started: Instant,
    stable: Option<Instant>,
    step: usize,
    waiting: bool,
    saving: bool,
    done: bool,
    source: serde_json::Value,
    source_instance: u64,
    source_inventory: crate::engine_inventory::Inventory,
    trace: Vec<serde_json::Value>,
    save_meshes: usize,
    save_instance: u64,
    reload_error: Option<String>,
    motion: u8,
    cancel_at: Option<Instant>,
}
fn require(ok: bool, message: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}
fn signature(c: &Client) -> Result<serde_json::Value> {
    let s = c.scene.engine.as_ref().unwrap();
    let stream = s.stream.as_ref().unwrap();
    Ok(
        serde_json::json!({"seed":s.seed,"generator":stream.generator.config,"pose":{"position":c.actor.position,"yaw":c.yaw,"pitch":c.pitch,"movement":c.movement},"edits":stream.edits.iter().map(|(p,id)|serde_json::json!({"position":p,"material":id.map(|id|s.registry.get(id).id.clone())})).collect::<Vec<_>>(),"fluids":s.fluids.amounts.iter().collect::<Vec<_>>(),"machines":engine::persistence::machines::SavedMachines::capture(&s.machines,&s.registry)?,"entities":s.entities.saved()}),
    )
}
fn target(app: &App, id: &str) -> Result<engine_ui::Target> {
    let document = app.palette.document(&app.client, app.palette.viewport)?;
    use engine::ui::framework::{screen::Action, widgets::Kind};
    let (i,_)=document.widgets.iter().enumerate().find(|(_,w)|matches!(&w.widget.kind,Kind::Button {action:Action::TravelDimension {dimension},..} if dimension==id)).ok_or("Dimension picker target missing")?;
    Ok(engine_ui::Target::Button(i as u16))
}
fn choose(app: &mut App, id: &str, pad: bool) -> Result<()> {
    if pad {
        super::controller::sample(
            app,
            Some(Pad {
                buttons: B::RightStick.bit() | B::View.bit(),
                ..Default::default()
            }),
        )?;
        super::controller::neutral(app)?;
        require(
            app.palette.screen_id() == "dimensions" && app.palette.open,
            "Xbox dimension shortcut did not open picker",
        )?;
        let t = target(app, id)?;
        super::controller::focus(app, t)?;
        super::controller::tap(app, B::South)?;
    } else {
        app.engine_key(K::KeyU)?;
        let wanted = target(app, id)?;
        let points = app.palette.control_points(&app.client)?;
        let p = points
            .into_iter()
            .find(|p| app.palette.hit(&app.client, *p, app.palette.viewport) == Some(wanted))
            .ok_or("Dimension focus target missing")?;
        app.palette
            .move_pointer(&app.client, p, app.palette.viewport);
        app.engine_key(K::Enter)?;
    }
    require(
        app.load_busy() && !app.palette.open,
        "Picker did not start background dimension travel",
    )
}
fn look_at(app: &mut App, position: Vec3, target: [f32; 3]) -> Result<()> {
    app.client.set_spawn(position)?;
    app.simulation_previous = position;
    let eye = app.client.eye();
    let d = std::array::from_fn::<_, 3, _>(|i| target[i] as f64 - eye[i]);
    app.client.yaw = d[0].atan2(d[2]) as f32;
    app.client.pitch = d[1].atan2((d[0] * d[0] + d[2] * d[2]).sqrt()) as f32;
    Ok(())
}
pub fn prepare(app: &mut App, out: &Path, case: &str) -> Result<()> {
    require(!out.exists(), "Fresh dimension proof directory required")?;
    std::fs::create_dir_all(out)?;
    app.root = out.to_path_buf();
    app.debug = false;
    app.notice = None;
    require(
        app.client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .dimensions
            .is_some(),
        "Dimension startup registry missing",
    )?;
    engine_player::mode(&mut app.client, engine_player::Mode::Noclip)?;
    let active = app
        .client
        .scene
        .engine
        .as_ref()
        .unwrap()
        .dimensions
        .as_ref()
        .unwrap()
        .active
        .clone();
    if case == "showcase" || case == "underside" || case == "wire" || case == "motion" {
        if active == "islands" || active == "sky_gardens" {
            let g = app
                .client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .stream
                .as_ref()
                .unwrap()
                .generator
                .clone();
            let spawn = g.terrain.as_ref().unwrap().spawn();
            let s = g.terrain.as_ref().unwrap().sample(spawn[0], spawn[2]);
            let y = if case == "underside" {
                s.column.bottom.unwrap() as f32 - 20.
            } else {
                s.height as f32 + 38.
            };
            let target = if case == "underside" {
                s.column.bottom.unwrap() as f32 + 1.
            } else {
                s.height as f32
            };
            look_at(
                app,
                Vec3::new(spawn[0] as f32 + 70., y, spawn[2] as f32 - 70.),
                [spawn[0] as f32, target, spawn[2] as f32],
            )?;
        } else {
            app.client.yaw = 0.7;
            app.client.pitch = -0.12;
        }
    }
    if case == "wire" {
        app.engine_key(K::F4)?;
    }
    if case == "picker" || case == "pages" {
        app.engine_key(K::KeyU)?;
        if case == "pages" {
            app.palette.control_page(&app.client, 1)?;
        }
    }
    if case.starts_with("travel") || case == "controller-return" || case == "cancel" {
        let p = app.client.actor.position;
        let origin = [
            p.x.floor() as i32 + 3,
            p.y.floor() as i32,
            p.z.floor() as i32 + 3,
        ];
        let s = app.client.scene.engine.as_mut().unwrap();
        let glass = s.registry.find("blue_glass")?;
        let water = s.registry.find("water")?;
        let furnace = s.registry.find("furnace")?;
        let mut bucket = s.registry.catalog_stack(s.registry.find("bucket")?);
        bucket.data = Some(engine::items::ItemData::Fluid {
            kind: engine::blocks::components::FluidKind::Water,
            amount: 331,
        });
        s.player.offhand = Some(bucket);
        app.client.scene.change_engine(origin, Some(glass))?;
        let fluid = [origin[0] + 2, origin[1], origin[2]];
        app.client.scene.set_fluid(fluid, water, 333)?;
        let machine = [origin[0] + 4, origin[1], origin[2]];
        app.client.scene.change_engine(machine, Some(furnace))?;
        let s = app.client.scene.engine.as_mut().unwrap();
        s.machines.ensure(machine)?;
        s.machines.clock_ms = 1300;
        s.entities
            .spawn("grazer", [p.x + 8., p.y + 2., p.z + 8.], true)?;
        app.show_character = true;
    }
    if case == "controller-return" {
        app.controls.start()?;
        super::controller::neutral(app)?;
    }
    let source = signature(&app.client)?;
    let s = app.client.scene.engine.as_ref().unwrap();
    let source_inventory = s.player.clone();
    let source_instance = s.instance;
    app.dimension_live = Some(Live {
        case: case.into(),
        out: out.into(),
        started: Instant::now(),
        stable: None,
        step: 0,
        waiting: false,
        saving: false,
        done: false,
        source,
        source_instance,
        source_inventory,
        trace: Vec::new(),
        save_meshes: 0,
        save_instance: 0,
        reload_error: None,
        motion: 0,
        cancel_at: None,
    });
    app.frame_limit = usize::MAX / 2;
    Ok(())
}
fn ready(app: &App) -> bool {
    if app.load_busy() || app.save_busy() {
        return false;
    }
    let Some(gpu) = &app.gpu else {
        return false;
    };
    let s = app
        .client
        .scene
        .engine
        .as_ref()
        .unwrap()
        .stream
        .as_ref()
        .unwrap();
    let resident = s
        .volume
        .as_ref()
        .is_none_or(|v| v.wanted.len() == v.residents.len());
    let renderer = gpu.rendering_observation();
    let ray_ready =
        renderer["requested_method"] != "ray_traced" || renderer["genuine_ray_tracing"] == true;
    resident
        && ray_ready
        && gpu.stats.meshes_ready
        && gpu.stats.distant_ready
        && gpu.stats.distant_previous_vertices == 0
}
pub fn advance(app: &mut App) -> Result<()> {
    let Some(mut live) = app.dimension_live.take() else {
        return Ok(());
    };
    if live.done {
        app.dimension_live = Some(live);
        return Ok(());
    }
    require(
        live.started.elapsed() < Duration::from_secs(180),
        "Dimension native fixture failed to settle",
    )?;
    if live.waiting {
        if live
            .cancel_at
            .is_some_and(|t| t.elapsed() >= Duration::from_millis(50))
        {
            app.engine_key(K::Escape)?;
            live.cancel_at = None;
        }
        if app.load_busy() {
            app.dimension_live = Some(live);
            return Ok(());
        }
        if live.case == "cancel" {
            require(
                app.client.scene.engine.as_ref().unwrap().instance == live.source_instance
                    && signature(&app.client)? == live.source,
                "Cancelled travel mutated source dimension",
            )?;
        } else {
            let active = &app
                .client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .dimensions
                .as_ref()
                .unwrap()
                .active;
            live.trace.push(serde_json::json!({"active":active,"world":signature(&app.client)?,"loading":app.loading_observation()}));
            if live.step == 1 && (live.case == "travel-return" || live.case == "controller-return")
            {
                if live.case == "controller-return" {
                    super::controller::neutral(app)?;
                    super::controller::sample(
                        app,
                        Some(Pad {
                            buttons: B::RightStick.bit() | B::West.bit(),
                            ..Default::default()
                        }),
                    )?;
                    super::controller::neutral(app)?;
                } else {
                    app.engine_key(K::KeyV)?;
                }
                live.source_inventory = app.client.scene.engine.as_ref().unwrap().player.clone();
            }
        }
        live.waiting = false;
        live.stable = None;
    }
    if live.case == "motion"
        && let Some(gpu) = &app.gpu
    {
        live.trace.push(serde_json::json!({"step":live.motion,"vertices":gpu.stats.distant_vertices,"previous":gpu.stats.distant_previous_vertices,"ready":gpu.stats.distant_ready}));
        require(
            gpu.stats.distant_vertices > 0 || live.motion == 0,
            "Layered LOD disappeared during a chunk crossing",
        )?;
    }
    if !ready(app) {
        live.stable = None;
        app.dimension_live = Some(live);
        return Ok(());
    }
    if live.stable.get_or_insert_with(Instant::now).elapsed() < Duration::from_millis(250) {
        app.dimension_live = Some(live);
        return Ok(());
    }
    let route: &[&str] = match live.case.as_str() {
        "travel-cavern" => &["cavern"],
        "travel-islands" => &["cavern", "islands"],
        "travel-return" | "controller-return" => &["cavern", "islands", "surface"],
        "travel-custom" => &["sky_gardens"],
        "cancel" => &["cavern"],
        _ => &[],
    };
    if live.step < route.len() {
        choose(app, route[live.step], live.case == "controller-return")?;
        live.step += 1;
        live.waiting = true;
        live.stable = None;
        if live.case == "cancel" {
            live.cancel_at = Some(Instant::now());
        }
        app.dimension_live = Some(live);
        return Ok(());
    }
    if live.case == "motion" && live.motion < 4 {
        let p = app.client.actor.position;
        app.client.set_spawn(Vec3::new(p.x + 16., p.y, p.z))?;
        app.simulation_previous = app.client.actor.position;
        live.motion += 1;
        live.stable = None;
        app.dimension_live = Some(live);
        return Ok(());
    }
    if live.case == "reload-invalid" && live.reload_error.is_none() {
        let home = app
            .client
            .owned_materials
            .as_ref()
            .ok_or("Authored dimension home missing")?
            .root
            .clone();
        let path = home.join("content/base/world/dimensions/cavern.json");
        let mut definition: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        definition["id"] = serde_json::json!("surface");
        std::fs::write(&path, serde_json::to_vec_pretty(&definition)?)?;
        let before = app.client.scene.engine.as_ref().unwrap().registry_revision;
        let out = app.root.clone();
        app.root = home;
        let error = app
            .engine_key(K::F6)
            .err()
            .ok_or("Invalid dimension reload was accepted")?
            .to_string();
        app.root = out;
        require(
            app.client.scene.engine.as_ref().unwrap().registry_revision == before
                && signature(&app.client)? == live.source,
            "Rejected dimension reload partially changed the world",
        )?;
        live.reload_error = Some(error);
    }
    if !live.saving {
        if live.case == "travel-return" || live.case == "controller-return" {
            require(
                signature(&app.client)? == live.source,
                "Returning dimension state differs from source",
            )?;
            require(
                app.client
                    .scene
                    .engine
                    .as_ref()
                    .unwrap()
                    .player
                    .changed_slots(&live.source_inventory)
                    == 0,
                "Shared hand swap was reverted by dimension travel",
            )?;
        }
        let gpu = app.gpu.as_ref().unwrap();
        live.save_meshes = gpu.stats.mesh_rebuilds;
        live.save_instance = app.client.scene.engine.as_ref().unwrap().instance;
        app.modifiers = winit::keyboard::ModifiersState::CONTROL;
        app.engine_key(K::KeyS)?;
        app.modifiers = winit::keyboard::ModifiersState::empty();
        live.saving = true;
        live.stable = None;
        app.dimension_live = Some(live);
        return Ok(());
    }
    let path = app
        .client
        .last_save
        .clone()
        .ok_or("Dimension Ctrl+S save missing")?;
    let loaded = engine::load(&path)?;
    require(
        signature(&loaded)? == signature(&app.client)?,
        "Active dimension save roundtrip differs",
    )?;
    require(
        loaded
            .scene
            .engine
            .as_ref()
            .unwrap()
            .player
            .changed_slots(&app.client.scene.engine.as_ref().unwrap().player)
            == 0,
        "Dimension save lost inventory or offhand contents",
    )?;
    let s = app.client.scene.engine.as_ref().unwrap();
    let restored = loaded.scene.engine.as_ref().unwrap();
    let a = serde_json::to_value(s.dimensions.as_ref().unwrap().saved(&s.registry)?)?;
    let b = serde_json::to_value(
        restored
            .dimensions
            .as_ref()
            .unwrap()
            .saved(&restored.registry)?,
    )?;
    require(a == b, "Inactive dimension save roundtrip differs")?;
    require(
        s.instance == live.save_instance
            && app.gpu.as_ref().unwrap().stats.mesh_rebuilds == live.save_meshes,
        "Saving replaced/rebuilt dimension terrain",
    )?;
    let geometry = app.gpu.as_ref().unwrap().verify_geometry(&app.client)?;
    require(
        geometry["all_vertex_bytes_match"] == true,
        "Native dimension chunk buffers differ",
    )?;
    let readback = app.gpu.as_ref().unwrap().dump_distant_geometry(&live.out)?;
    let stream = s.stream.as_ref().unwrap();
    let terrain = stream.generator.terrain.as_ref().unwrap();
    let center = [
        app.client.actor.position.x.floor() as i32,
        app.client.actor.position.z.floor() as i32,
    ];
    let mut columns = Vec::new();
    for x in (center[0] - 384..=center[0] + 384).step_by(32) {
        for z in (center[1] - 384..=center[1] + 384).step_by(32) {
            let p = terrain.sample(x, z);
            columns.push(serde_json::json!({"x":x,"z":z,"top":p.height,"bottom":p.column.bottom,"ceiling":p.column.ceiling,"roof_top":p.column.roof_top,"present":p.column.present,"coverage":p.column.coverage,"water":p.water,"biome":terrain.settings.biomes[p.biome].id}));
        }
    }
    std::fs::write(
        live.out.join("dimensions.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"complete":true,"case":live.case,"active":s.dimensions.as_ref().unwrap().active,"seed":s.seed,"generator":stream.generator.config,"base_seed":s.dimensions.as_ref().unwrap().base_seed,"definitions":s.dimensions.as_ref().unwrap().definitions,"source":live.source,"trace":live.trace,"saved_world":path,"all_worlds_roundtrip":true,"shared_inventory_preserved":true,"source_return_equal":matches!(live.case.as_str(),"travel-return"|"controller-return"),"cancellation_source_preserved":live.case=="cancel","reload_rejected":live.reload_error,"save_instance_preserved":true,"save_meshes_unchanged":true,"near_geometry":geometry,"gpu_readback":readback,"columns":columns,"input":app.controls.observation(),"scope":"Actual native dimension startup, shared picker/input/background transitions and snapshot saves. GPU vertex buffers read back for independent analytic profile and seam checks. Physical Xbox controller is not connected/tested."}),
        )?,
    )?;
    app.notice = None;
    live.done = true;
    app.frame_limit = app.frames + 5;
    app.dimension_live = Some(live);
    Ok(())
}
