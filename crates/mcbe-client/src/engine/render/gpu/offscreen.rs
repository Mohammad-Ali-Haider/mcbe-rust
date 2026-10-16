//! Full production world-frame checks without any window or surface.
use super::*;
use crate::engine::{
    input::runtime::Runtime,
    ui::chat::Chat,
    world::{generation, terrain::Settings as TerrainSettings},
};
use crate::engine_blocks::Registry;
use std::sync::atomic::AtomicU64;
fn capture(renderer: &Renderer, path: &Path) -> Vec<u8> {
    let texture = renderer.offscreen_target.as_ref().unwrap();
    let w = renderer.config.width;
    let h = renderer.config.height;
    let stride = (w * 4).div_ceil(256) * 256;
    let buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("offscreen world readback"),
        size: (stride * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut e = renderer.device.create_command_encoder(&Default::default());
    e.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    renderer.queue.submit(Some(e.finish()));
    let (tx, rx) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |v| tx.send(v).unwrap());
    renderer.device.poll(wgpu::PollType::wait()).unwrap();
    rx.recv().unwrap().unwrap();
    let bytes = buffer.slice(..).get_mapped_range();
    let mut out = Vec::new();
    let mut pixels = Vec::new();
    for row in bytes.chunks_exact(stride as usize) {
        for p in row[..w as usize * 4].chunks_exact(4) {
            out.extend([p[2], p[1], p[0], p[3]]);
            pixels.push((u32::from(p[2]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[0]));
        }
    }
    crate::render::png(path, &pixels, w as usize, h as usize).unwrap();
    out
}
fn frame<'a>(
    settings: &'a crate::engine_settings::Settings,
    menu: &'a crate::engine_settings::Menu,
    chat: &'a Chat,
    controls: &'a Runtime,
    eye: [f32; 3],
    direction: [f32; 3],
    actor_position: [f32; 3],
) -> Frame<'a> {
    Frame {
        settings,
        menu,
        chat,
        controls,
        eye,
        direction,
        actor_position,
        entity_blend: 1.,
        fullscreen: false,
        startup: false,
        wire: false,
        debug: false,
        notice: None,
        show_character: false,
        capture: None,
    }
}
#[test]
#[ignore = "Explicit production offscreen LOD frames; no window or surface"]
fn production_island_frames_keep_coverage_during_detail_arrival_and_eviction() {
    let out = std::env::var_os("MCBE_LOD_PREVIEW_OUT")
        .map(std::path::PathBuf::from)
        .expect("Set MCBE_LOD_PREVIEW_OUT to an ignored capture directory");
    std::fs::create_dir_all(&out).unwrap();
    let mut terrain = TerrainSettings::built_in();
    terrain.version = 2;
    terrain.generator = Some(Box::new(generation::Definition {
        kind: "islands".into(),
        options: serde_json::json!({}),
        climate: Default::default(),
        materials: Default::default(),
    }));
    let mut c = crate::engine_stream::generate_config(
        713,
        Registry::built_in(),
        crate::engine_stream::Config {
            version: 2,
            terrain: Some(terrain),
            radius: 2,
            exhibits: vec![],
        },
    )
    .unwrap();
    crate::engine_stream::set_radius(&mut c, 9).unwrap();
    c.view.mode = crate::engine::player::camera::Mode::Free;
    *c.engine_distant_prepared.borrow_mut() =
        crate::engine_distance::prepare(&c, &AtomicU64::new(1), 1).unwrap();
    let actor = [c.actor.position.x, c.actor.position.y, c.actor.position.z];
    let eye = [actor[0], actor[1] + 52., actor[2] + 95.];
    let direction = (Vec3::from(actor) - Vec3::from(eye)).normalize().to_array();
    let mut renderer = Renderer::offscreen(512, 288).unwrap();
    renderer.set_async(false);
    renderer.stats.gui_scale = 1.;
    renderer
        .device
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let palette = Palette::recover(&c);
    let mut settings = crate::engine_settings::Settings {
        render_distance: 9,
        gui_scale: 1,
        ..Default::default()
    };
    let menu = crate::engine_settings::Menu::default();
    let chat = Chat::default();
    let controls = Runtime::default();
    assert!(
        renderer
            .draw(
                &c,
                &palette,
                frame(&settings, &menu, &chat, &controls, eye, direction, actor)
            )
            .unwrap()
    );
    let baseline = capture(&renderer, &out.join("fallback.png"));
    assert!(renderer.distant.vertices > 0);
    let residents = c
        .scene
        .engine
        .as_ref()
        .unwrap()
        .stream
        .as_ref()
        .unwrap()
        .volume
        .as_ref()
        .unwrap()
        .residents
        .clone();
    let initial_chunks = renderer.meshes.len();
    assert!(initial_chunks > 0);
    let background = [baseline[0], baseline[1], baseline[2]];
    let is_sky = |data: &[u8], x: usize, y: usize| {
        let p = &data[(y * 512 + x) * 4..];
        (0..3).all(|a| (p[a] as i32 - background[a] as i32).abs() < 8)
    };
    let mut reference = Vec::new();
    for y in 4..208 {
        for x in 4..508 {
            if (y - 2..=y + 2).all(|yy| (x - 2..=x + 2).all(|xx| !is_sky(&baseline, xx, yy))) {
                reference.push((x, y));
            }
        }
    }
    assert!(
        reference.len() > 500,
        "real island coverage missing: {}",
        reference.len()
    );
    let mut rows = Vec::new();
    for (name, time, evict) in [
        ("detailed", 10., false),
        ("evict-half", 10.12, true),
        ("fallback-restored", 10.3, true),
    ] {
        if evict {
            c.scene
                .engine
                .as_mut()
                .unwrap()
                .stream
                .as_mut()
                .unwrap()
                .volume
                .as_mut()
                .unwrap()
                .residents
                .clear();
        }
        renderer.coverage_clock = Instant::now() - std::time::Duration::from_secs_f64(time);
        assert!(
            renderer
                .draw(
                    &c,
                    &palette,
                    frame(&settings, &menu, &chat, &controls, eye, direction, actor)
                )
                .unwrap()
        );
        let image = capture(&renderer, &out.join(format!("{name}.png")));
        let missing = reference
            .iter()
            .filter(|(x, y)| is_sky(&image, *x, *y))
            .count();
        rows.push(serde_json::json!({"stage":name,"reference_pixels":reference.len(),"missing_pixels":missing,"chunks":renderer.meshes.len(),"coverage_uploads":renderer.stats.detail_coverage_uploads}));
        assert!(
            missing * 1000 < reference.len() * 5,
            "{name}: {missing} interior pixels lost from {}",
            reference.len()
        );
    }
    assert!(
        renderer.meshes.is_empty(),
        "outgoing meshes must retire after their fade"
    );
    // A new distant snapshot must overlap the retained one while its center moves.
    let center = c
        .scene
        .engine
        .as_ref()
        .unwrap()
        .stream
        .as_ref()
        .unwrap()
        .center;
    c.scene
        .engine
        .as_mut()
        .unwrap()
        .stream
        .as_mut()
        .unwrap()
        .center = [center[0] + 4, center[1]];
    *c.engine_distant_prepared.borrow_mut() =
        crate::engine_distance::prepare(&c, &AtomicU64::new(1), 1).unwrap();
    for stage in ["recenter-start", "recenter-mid", "recenter-end"] {
        if stage == "recenter-mid" {
            std::thread::sleep(std::time::Duration::from_millis(90));
        }
        if stage == "recenter-end" {
            std::thread::sleep(std::time::Duration::from_millis(110));
        }
        assert!(
            renderer
                .draw(
                    &c,
                    &palette,
                    frame(&settings, &menu, &chat, &controls, eye, direction, actor)
                )
                .unwrap()
        );
        let image = capture(&renderer, &out.join(format!("{stage}.png")));
        let missing = reference
            .iter()
            .filter(|(x, y)| is_sky(&image, *x, *y))
            .count();
        rows.push(serde_json::json!({"stage":stage,"reference_pixels":reference.len(),"missing_pixels":missing}));
        assert!(
            missing * 1000 < reference.len() * 5,
            "recenter dropped island interior: {missing}"
        );
    }
    // Restore the same resident geometry before testing lighting, so a ray
    // request cannot pass only by falling back on an empty acceleration scene.
    {
        let state = c.scene.engine.as_mut().unwrap();
        state
            .stream
            .as_mut()
            .unwrap()
            .volume
            .as_mut()
            .unwrap()
            .residents = residents;
        state.revision += 1;
        let revision = state.revision;
        for key in state.blocks.chunk_keys() {
            state.dirty_chunks.insert(key, revision);
        }
    }
    let light_eye = [actor[0], actor[1] + 8., actor[2] + 18.];
    let light_direction = (Vec3::from(actor) - Vec3::from(light_eye))
        .normalize()
        .to_array();
    for (method, name) in [
        (super::super::methods::Method::Deferred, "deferred"),
        (super::super::methods::Method::RayTraced, "ray"),
    ] {
        settings.rendering.method = method;
        settings.rendering.quality = super::super::methods::Quality::Low;
        for _ in 0..32 {
            assert!(
                renderer
                    .draw(
                        &c,
                        &palette,
                        frame(
                            &settings,
                            &menu,
                            &chat,
                            &controls,
                            light_eye,
                            light_direction,
                            actor
                        )
                    )
                    .unwrap()
            );
            renderer.device.poll(wgpu::PollType::wait()).unwrap();
            if method != super::super::methods::Method::RayTraced
                || renderer
                    .ray
                    .as_ref()
                    .is_some_and(|s| s.ready && s.pending == 0)
            {
                break;
            }
        }
        if method == super::super::methods::Method::RayTraced && renderer.stats.hardware_ray_tracing
        {
            assert!(
                renderer
                    .ray
                    .as_ref()
                    .is_some_and(|s| s.ready && s.pending == 0),
                "hardware ray scene must actually become ready"
            );
        }
        capture(&renderer, &out.join(format!("{name}.png")));
    }
    // Rough terrain must not acquire a mirror image simply by enabling reflections.
    settings.rendering.ray_indirect = false;
    settings.rendering.ray_reflections = false;
    renderer.coverage_clock = Instant::now() - std::time::Duration::from_secs(30);
    assert!(
        renderer
            .draw(
                &c,
                &palette,
                frame(
                    &settings,
                    &menu,
                    &chat,
                    &controls,
                    light_eye,
                    light_direction,
                    actor
                )
            )
            .unwrap()
    );
    let matte_off = capture(&renderer, &out.join("rough-reflections-off.png"));
    settings.rendering.ray_reflections = true;
    assert!(
        renderer
            .draw(
                &c,
                &palette,
                frame(
                    &settings,
                    &menu,
                    &chat,
                    &controls,
                    light_eye,
                    light_direction,
                    actor
                )
            )
            .unwrap()
    );
    let matte_on = capture(&renderer, &out.join("rough-reflections-on.png"));
    let different = matte_off
        .chunks_exact(4)
        .zip(matte_on.chunks_exact(4))
        .take(512 * 208)
        .filter(|(a, b)| (0..3).any(|i| (a[i] as i32 - b[i] as i32).abs() > 1))
        .count();
    assert_eq!(
        different, 0,
        "rough terrain launched visible mirror reflections"
    );
    let ray_rendering = renderer.rendering_observation();
    // Move the actual streaming camera across multiple chunk boundaries. At each
    // pose compare the full fallback against partly and fully published detail,
    // rather than comparing unrelated pixels from different camera positions.
    settings.rendering.method = super::super::methods::Method::Forward;
    let mut motion_clock = 40.;
    for offset in [-16., -48., -96., -144.] {
        let moving_eye = [actor[0] + offset, actor[1] + 52., actor[2] + 95.];
        let moving_direction = (Vec3::from(actor) - Vec3::from(moving_eye))
            .normalize()
            .to_array();
        c.view.free = moving_eye;
        c.view.previous_free = moving_eye;
        for _ in 0..4 {
            crate::engine_stream::update(&mut c).unwrap();
        }
        *c.engine_distant_prepared.borrow_mut() =
            crate::engine_distance::prepare(&c, &AtomicU64::new(1), 1).unwrap();
        renderer.coverage.clear();
        renderer.coverage_clock = Instant::now() - std::time::Duration::from_secs_f64(motion_clock);
        assert!(
            renderer
                .draw(
                    &c,
                    &palette,
                    frame(
                        &settings,
                        &menu,
                        &chat,
                        &controls,
                        moving_eye,
                        moving_direction,
                        actor
                    )
                )
                .unwrap()
        );
        let base = capture(
            &renderer,
            &out.join(format!("moving-{offset}-fallback.png")),
        );
        let refs: Vec<_> = (4..208)
            .flat_map(|y| (4..508).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                (y - 2..=y + 2).all(|yy| (x - 2..=x + 2).all(|xx| !is_sky(&base, xx, yy)))
            })
            .collect();
        assert!(
            refs.len() > 500,
            "moving camera lost baseline island at {offset}"
        );
        for (phase, delta) in [("half", 0.12), ("full", 0.3)] {
            renderer.coverage_clock =
                Instant::now() - std::time::Duration::from_secs_f64(motion_clock + delta);
            assert!(
                renderer
                    .draw(
                        &c,
                        &palette,
                        frame(
                            &settings,
                            &menu,
                            &chat,
                            &controls,
                            moving_eye,
                            moving_direction,
                            actor
                        )
                    )
                    .unwrap()
            );
            let pixels = capture(&renderer, &out.join(format!("moving-{offset}-{phase}.png")));
            let missing = refs.iter().filter(|&&(x, y)| is_sky(&pixels, x, y)).count();
            rows.push(serde_json::json!({"stage":format!("moving-{offset}-{phase}"),"reference_pixels":refs.len(),"missing_pixels":missing,"stream_center":c.scene.engine.as_ref().unwrap().stream.as_ref().unwrap().center}));
            assert!(
                missing * 1000 < refs.len() * 5,
                "moving camera {offset} {phase}: {missing} island pixels vanished"
            );
        }
        motion_clock += 1.;
    }
    assert!(pollster::block_on(renderer.device.pop_error_scope()).is_none());
    std::fs::write(out.join("coverage.json"),serde_json::to_vec_pretty(&serde_json::json!({"initial_chunks":initial_chunks,"frames":rows,"ray_rendering":ray_rendering,"rough_mirror_changed_pixels":different,"rendering":renderer.rendering_observation(),"statistics":renderer.stats})).unwrap()).unwrap();
    eprintln!("Production offscreen island coverage: {rows:?}");
}

#[test]
#[ignore = "Explicit production offscreen UI animation/cache check; no window"]
fn production_ui_animation_settles_and_stops_uploading_after_skipped_frames() {
    let out = std::env::var_os("MCBE_ANIMATION_PREVIEW_OUT")
        .or_else(|| std::env::var_os("MCBE_LOD_PREVIEW_OUT"))
        .map(std::path::PathBuf::from)
        .expect("Set MCBE_ANIMATION_PREVIEW_OUT");
    std::fs::create_dir_all(&out).unwrap();
    let mut c = crate::engine::generate(713).unwrap();
    c.view.mode = crate::engine::player::camera::Mode::Free;
    let mut renderer = Renderer::offscreen(512, 288).unwrap();
    renderer.set_async(false);
    renderer.stats.gui_scale = 1.;
    renderer
        .device
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let actor = [c.actor.position.x, c.actor.position.y, c.actor.position.z];
    let eye = [actor[0], actor[1] + 6., actor[2] + 12.];
    let direction = (Vec3::from(actor) - Vec3::from(eye)).normalize().to_array();
    let palette = Palette::recover(&c);
    let settings = crate::engine_settings::Settings::default();
    let menu = crate::engine_settings::Menu::default();
    let chat = Chat::default();
    let controls = Runtime::default();
    let draw = |r: &mut Renderer, c: &crate::scene::Client, t: f64| {
        r.ui_epoch = Instant::now() - std::time::Duration::from_secs_f64(t);
        assert!(
            r.draw(
                c,
                &palette,
                frame(&settings, &menu, &chat, &controls, eye, direction, actor)
            )
            .unwrap()
        );
    };
    draw(&mut renderer, &c, 1.);
    let start = capture(&renderer, &out.join("ui-initial.png"));
    // Skip the entire animation interval: the final state must still be drawn.
    draw(&mut renderer, &c, 10.);
    let settled = capture(&renderer, &out.join("ui-settled.png"));
    assert_ne!(start, settled);
    draw(&mut renderer, &c, 10.);
    let idle = renderer.stats.ui_uploads;
    draw(&mut renderer, &c, 10.);
    assert_eq!(renderer.stats.ui_uploads, idle);
    c.scene.engine.as_mut().unwrap().player.active = 1;
    draw(&mut renderer, &c, 11.);
    let before = capture(&renderer, &out.join("selection-before.png"));
    draw(&mut renderer, &c, 11.07);
    let mid = capture(&renderer, &out.join("selection-mid.png"));
    draw(&mut renderer, &c, 11.3);
    let end = capture(&renderer, &out.join("selection-end.png"));
    assert_ne!(before, mid);
    assert_ne!(mid, end);
    draw(&mut renderer, &c, 12.);
    let idle = renderer.stats.ui_uploads;
    draw(&mut renderer, &c, 12.);
    assert_eq!(renderer.stats.ui_uploads, idle);
    let mut disabled = (*c.animations).clone();
    disabled.enabled = false;
    c.animations = std::sync::Arc::new(disabled.compile().unwrap());
    c.scene.engine.as_mut().unwrap().player.active = 2;
    draw(&mut renderer, &c, 13.);
    let idle = renderer.stats.ui_uploads;
    draw(&mut renderer, &c, 14.);
    assert_eq!(
        renderer.stats.ui_uploads, idle,
        "disabled animations must stay cached"
    );
    // Exercise the real inventory overlay and its interleaved 3D preview, whose
    // draw ranges change while zero-alpha animation quads appear/disappear.
    let mut enabled = (*c.animations).clone();
    enabled.enabled = true;
    c.animations = std::sync::Arc::new(enabled.compile().unwrap());
    let mut palette = palette;
    palette.open = true;
    palette.hover = Some(crate::engine_ui::Target::Slot(
        crate::engine_inventory::Slot::Player(0),
    ));
    palette.revision += 1;
    let inventory_draw = |r: &mut Renderer, t: f64| {
        r.ui_epoch = Instant::now() - std::time::Duration::from_secs_f64(t);
        assert!(
            r.draw(
                &c,
                &palette,
                frame(&settings, &menu, &chat, &controls, eye, direction, actor)
            )
            .unwrap()
        );
    };
    inventory_draw(&mut renderer, 15.);
    let opening = capture(&renderer, &out.join("inventory-initial.png"));
    inventory_draw(&mut renderer, 15.07);
    let middle = capture(&renderer, &out.join("inventory-mid.png"));
    inventory_draw(&mut renderer, 15.3);
    let complete = capture(&renderer, &out.join("inventory-end.png"));
    assert_ne!(opening, middle);
    assert_ne!(middle, complete);
    assert!(renderer.stats.preview_vertices > 0 && renderer.ui_preview_at.is_some());
    inventory_draw(&mut renderer, 16.);
    let idle = renderer.stats.ui_uploads;
    let preview_idle = renderer.stats.preview_uploads;
    inventory_draw(&mut renderer, 17.);
    assert_eq!(renderer.stats.ui_uploads, idle);
    assert_eq!(renderer.stats.preview_uploads, preview_idle);
    assert!(pollster::block_on(renderer.device.pop_error_scope()).is_none());
    std::fs::write(out.join("ui-animation.json"),serde_json::to_vec_pretty(&serde_json::json!({"ui_uploads":renderer.stats.ui_uploads,"skipped_frame_settled":true,"idle_uploads_stopped":true,"disabled_uploads_stopped":true,"inventory_preview_verified":true,"renderer":renderer.rendering_observation()})).unwrap()).unwrap();
}
