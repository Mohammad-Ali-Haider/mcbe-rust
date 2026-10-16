//! Software preview of actual composed GPU quads and atlas pixels. No app window.
use super::*;
fn rasterize(atlas: &Atlas, vertices: &[Vertex], width: usize, height: usize) -> Vec<u32> {
    let mut out = vec![0x18222e; width * height];
    for q in vertices.chunks_exact(6) {
        let point = |v: &Vertex| {
            [
                (v.position[0] + 1.) * 0.5 * width as f32,
                (1. - v.position[1]) * 0.5 * height as f32,
            ]
        };
        let a = point(&q[0]);
        let b = point(&q[2]);
        if b[0] <= a[0] || b[1] <= a[1] {
            continue;
        }
        for y in (a[1].ceil().max(0.) as usize)..((b[1].ceil() as usize).min(height)) {
            for x in (a[0].ceil().max(0.) as usize)..((b[0].ceil() as usize).min(width)) {
                let f = [
                    (x as f32 + 0.5 - a[0]) / (b[0] - a[0]),
                    (y as f32 + 0.5 - a[1]) / (b[1] - a[1]),
                ];
                let uv = std::array::from_fn::<_, 2, _>(|i| {
                    ((q[0].uv[i] + (q[2].uv[i] - q[0].uv[i]) * f[i]) * atlas.size as f32)
                        .floor()
                        .clamp(0., atlas.size as f32 - 1.) as usize
                });
                let src = (uv[1] * atlas.size as usize + uv[0]) * 4;
                let alpha = atlas.rgba[src + 3] as f32 / 255. * q[0].color[3];
                let previous = out[y * width + x];
                let mut pixel = 0;
                for (i, shift) in [16, 8, 0].into_iter().enumerate() {
                    let value = (atlas.rgba[src + i] as f32 * q[0].color[i] * alpha
                        + ((previous >> shift) & 255) as f32 * (1. - alpha))
                        .round()
                        .clamp(0., 255.) as u32;
                    pixel |= value << shift;
                }
                out[y * width + x] = pixel;
            }
        }
    }
    out
}
#[test]
fn window_free_ui_composition_exports_actual_keycaps_and_inventory_when_requested() {
    let c = crate::engine::generate(37).unwrap();
    let atlas = Atlas::build(&c).unwrap();
    let mut palette = Palette::recover(&c);
    let controls = crate::engine::input::runtime::Runtime::default();
    let chat = crate::engine::ui::chat::Chat::default();
    let menu = crate::engine_settings::Menu::default();
    let settings = crate::engine_settings::Settings::default();
    for open in [false, true] {
        palette.open = open;
        let image = atlas.compose(
            &c,
            &palette,
            Context {
                chat: &chat,
                entity_target: None,
                controls: &controls,
                use_kind: Default::default(),
                ray_status: 0,
                menu: &menu,
                settings: &settings,
                fullscreen: false,
                startup: false,
                loading: false,
                width: 1280,
                height: 720,
                scale: 2.,
                wire: false,
                debug: false,
                statistics: "",
                notice: None,
            },
        );
        assert!(image.vertices.len() > 100 && image.vertices.len() < 100_000);
        assert!(image.vertices.iter().all(|v| {
            v.position
                .into_iter()
                .chain(v.uv)
                .chain(v.color)
                .all(f32::is_finite)
        }));
        if let Ok(path) = std::env::var("MCBE_UI_PREVIEW_OUT") {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(path);
            std::fs::create_dir_all(&path).unwrap();
            let pixels = rasterize(&atlas, &image.vertices, 1280, 720);
            crate::render::png(
                &path.join(if open {
                    "inventory.png"
                } else {
                    "controls.png"
                }),
                &pixels,
                1280,
                720,
            )
            .unwrap();
        }
    }
}
