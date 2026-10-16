//! Batched, independently scaled GPU HUD and palette quads.
use crate::{
    engine_ui::Palette,
    scene::{Client, Result},
};
use bytemuck::{Pod, Zeroable};
use mcbe_core::textures::Texture;
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}
#[derive(Clone, Copy)]
struct Sprite {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}
impl Sprite {
    fn crop(self, x: u32, y: u32, w: u32, h: u32) -> Self {
        Self {
            x: self.x + x,
            y: self.y + y,
            w,
            h,
        }
    }
}
pub struct Atlas {
    custom_ui: std::collections::BTreeMap<String, Sprite>,
    inventory: std::collections::BTreeMap<&'static str, Sprite>,
    pub rgba: Vec<u8>,
    pub size: u32,
    x: u32,
    y: u32,
    row: u32,
    white: Sprite,
    font: Option<Sprite>,
    advances: Vec<u32>,
    slots: Vec<Sprite>,
    selected: Option<Sprite>,
    start: Option<Sprite>,
    end: Option<Sprite>,
    cross: Option<Sprite>,
    panel: Option<Sprite>,
    cell: Option<Sprite>,
    icons: Vec<Sprite>,
}
pub struct Context<'a> {
    pub chat: &'a crate::engine::ui::chat::Chat,
    pub entity_target: Option<&'a crate::engine::entities::interaction::TargetInfo>,
    pub controls: &'a crate::engine::input::runtime::Runtime,
    pub use_kind: crate::engine::input::hints::Use,
    pub ray_status: u8,
    pub menu: &'a crate::engine_settings::Menu,
    pub settings: &'a crate::engine_settings::Settings,
    pub fullscreen: bool,
    pub startup: bool,
    pub loading: bool,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub wire: bool,
    pub debug: bool,
    pub statistics: &'a str,
    pub notice: Option<&'a str>,
}
pub struct Composition {
    pub vertices: Vec<Vertex>,
    pub preview_at: Option<u32>,
}
struct Canvas {
    clip: Option<crate::engine::ui::framework::layout::Rect>,
    preview_at: Option<u32>,
    vertices: Vec<Vertex>,
    width: f32,
    height: f32,
    white: Sprite,
}
impl Canvas {
    fn quad(&mut self, s: Sprite, r: [f32; 4], color: [f32; 4]) {
        let [x, y, w, h] = r;
        if w <= 0. || h <= 0. || color[3] <= 0. {
            return;
        }
        let clipped = self
            .clip
            .map_or(crate::engine::ui::framework::layout::Rect(r), |clip| {
                clip.intersect(crate::engine::ui::framework::layout::Rect(r))
            })
            .0;
        if clipped[2] <= 0. || clipped[3] <= 0. {
            return;
        }
        let [cx, cy, cw, ch] = clipped;
        let pos = [[cx, cy], [cx + cw, cy], [cx + cw, cy + ch], [cx, cy + ch]];
        let u0 = s.x as f32 + (cx - x) / w * s.w as f32;
        let u1 = s.x as f32 + (cx + cw - x) / w * s.w as f32;
        let v0 = s.y as f32 + (cy - y) / h * s.h as f32;
        let v1 = s.y as f32 + (cy + ch - y) / h * s.h as f32;
        let uv = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
        for i in [0usize, 1, 2, 0, 2, 3] {
            self.vertices.push(Vertex {
                position: [
                    pos[i][0] / self.width * 2. - 1.,
                    1. - pos[i][1] / self.height * 2.,
                ],
                uv: uv[i].map(|v| v / 1024.),
                color,
            });
        }
    }
    fn rect(&mut self, r: [f32; 4], color: [f32; 4]) {
        self.quad(self.white, r, color);
    }
}
impl Atlas {
    fn add(&mut self, t: &Texture) -> Result<Sprite> {
        if self.x + t.width > self.size {
            self.x = 0;
            self.y += self.row;
            self.row = 0;
        }
        if self.y + t.height > self.size {
            return Err("GPU UI atlas capacity exceeded".into());
        }
        let s = Sprite {
            x: self.x,
            y: self.y,
            w: t.width,
            h: t.height,
        };
        for y in 0..t.height {
            let src = (y * t.width * 4) as usize;
            let dst = ((self.y + y) * self.size * 4 + self.x * 4) as usize;
            self.rgba[dst..dst + (t.width * 4) as usize]
                .copy_from_slice(&t.rgba[src..src + (t.width * 4) as usize]);
        }
        self.x += t.width + 1;
        self.row = self.row.max(t.height + 1);
        Ok(s)
    }
    pub fn build(client: &Client) -> Result<Self> {
        let mut a = Self {
            custom_ui: std::collections::BTreeMap::new(),
            inventory: std::collections::BTreeMap::new(),
            rgba: vec![0; 1024 * 1024 * 4],
            size: 1024,
            x: 0,
            y: 0,
            row: 0,
            white: Sprite {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            },
            font: None,
            advances: vec![6; 256],
            slots: Vec::new(),
            selected: None,
            start: None,
            end: None,
            cross: None,
            panel: None,
            cell: None,
            icons: Vec::new(),
        };
        a.white = a.add(&Texture {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        })?;
        if let Some(src) = &client.engine_assets
            && client.scene.engine.as_ref().unwrap().ui.theme.legacy_ui
        {
            for (key, texture) in &src.inventory {
                let texture = if *key == "skin" {
                    &client.appearance.skin
                } else {
                    texture
                };
                let sprite = a.add(texture)?;
                a.inventory.insert(*key, sprite);
            }
            a.font = Some(a.add(&src.font)?);
            for n in 0..256 {
                let mut last = 2;
                for x in 0..8 {
                    for y in 0..8 {
                        let i =
                            (((n / 16 * 8 + y) * src.font.width + n % 16 * 8 + x) * 4 + 3) as usize;
                        if src.font.rgba[i] > 0 {
                            last = last.max(x);
                        }
                    }
                }
                a.advances[n as usize] = last + 2;
            }
            for s in &src.hotbar {
                let sprite = a.add(s)?;
                a.slots.push(sprite);
            }
            a.selected = Some(a.add(&src.selected)?);
            a.start = Some(a.add(&src.start_cap)?);
            a.end = Some(a.add(&src.end_cap)?);
            a.cross = Some(a.add(&src.icons)?.crop(0, 0, 16, 16));
            a.panel = Some(a.add(&src.panel)?);
            a.cell = Some(a.add(&src.cell)?);
        }
        if !a.inventory.contains_key("skin") {
            let sprite = a.add(&client.appearance.skin)?;
            a.inventory.insert("skin", sprite);
        }
        if a.font.is_none() {
            a.font = Some(a.add(&crate::engine_font::texture())?);
        }
        for i in 0..client.scene.engine.as_ref().unwrap().registry.blocks.len() {
            let mut pixels = vec![0; 32 * 32];
            crate::engine_ui::icon(client, &mut pixels, 32, 32, i as u16, [0, 0, 32]);
            let rgba = pixels
                .iter()
                .flat_map(|p| {
                    [
                        (p >> 16) as u8,
                        (p >> 8) as u8,
                        *p as u8,
                        if *p == 0 { 0 } else { 255 },
                    ]
                })
                .collect();
            let s = a.add(&Texture {
                width: 32,
                height: 32,
                rgba,
            })?;
            a.icons.push(s);
        }
        for (key, texture) in &client.scene.engine.as_ref().unwrap().ui.textures {
            let sprite = a.add(texture)?;
            a.custom_ui.insert(key.clone(), sprite);
        }
        Ok(a)
    }
    fn text(&self, c: &mut Canvas, x: f32, y: f32, message: &str, color: [f32; 4]) {
        self.text_scaled(c, x, y, message, color, 1.);
    }
    fn text_scaled(
        &self,
        c: &mut Canvas,
        x: f32,
        y: f32,
        message: &str,
        color: [f32; 4],
        scale: f32,
    ) {
        let Some(font) = self.font else {
            return;
        };
        let mut at = x;
        for ch in message.chars() {
            let n = if ch.is_ascii() { ch as u32 } else { 63 };
            let sprite = font.crop(n % 16 * 8, n / 16 * 8, 8, 8);
            if color[0] > 0.7 {
                c.quad(
                    sprite,
                    [at + scale, y + scale, 8. * scale, 8. * scale],
                    [0., 0., 0., 0.6],
                );
            }
            c.quad(sprite, [at, y, 8. * scale, 8. * scale], color);
            at += self.advances[n as usize] as f32 * scale;
        }
    }
    fn label_width(&self, text: &str) -> f32 {
        text.chars()
            .map(|c| self.advances[if c.is_ascii() { c as usize } else { 63 }])
            .sum::<u32>() as f32
    }
    fn panel(&self, c: &mut Canvas, r: [f32; 4], tint: [f32; 4]) {
        let [x, y, w, h] = r;
        c.rect([x - 1., y - 1., w + 2., h + 2.], [0.1, 0.1, 0.1, 1.]);
        if let Some(s) = self.panel {
            let bx = 4u32.min(s.w / 2);
            let by = 4u32.min(s.h / 2);
            let widths = [bx, s.w - 2 * bx, bx];
            let heights = [by, s.h - 2 * by, by];
            let dw = [bx as f32, w - 2. * bx as f32, bx as f32];
            let dh = [by as f32, h - 2. * by as f32, by as f32];
            let mut yy = y;
            let mut sy = 0;
            for row in 0..3 {
                let mut xx = x;
                let mut sx = 0;
                for col in 0..3 {
                    c.quad(
                        s.crop(sx, sy, widths[col].max(1), heights[row].max(1)),
                        [xx, yy, dw[col], dh[row]],
                        tint,
                    );
                    xx += dw[col];
                    sx += widths[col];
                }
                yy += dh[row];
                sy += heights[row];
            }
        } else {
            c.rect(r, [0.77, 0.77, 0.77, 1.]);
        }
    }
    fn stack(
        &self,
        c: &mut Canvas,
        stack: crate::engine_inventory::Stack,
        r: [f32; 4],
        registry: &crate::engine_blocks::Registry,
    ) {
        if let Some(icon) = self.icons.get(usize::from(stack.id)) {
            c.quad(*icon, r, [1.; 4]);
        }
        if let Some(crate::engine::items::ItemData::Fluid { amount, .. }) = stack.data {
            c.rect(
                [r[0] + 1., r[1] + r[3] - 3., r[2] - 2., 2.],
                [0.1, 0.1, 0.1, 1.],
            );
            c.rect(
                [
                    r[0] + 1.,
                    r[1] + r[3] - 3.,
                    (r[2] - 2.) * amount as f32
                        / registry
                            .get(stack.id)
                            .container
                            .map_or(1024, |v| v.capacity) as f32,
                    2.,
                ],
                [0.1, 0.45, 0.9, 1.],
            );
        }
        if stack.count > 1 {
            let count = stack.count.to_string();
            self.text(
                c,
                r[0] + r[2] + 1. - self.label_width(&count),
                r[1] + r[3] - 7.,
                &count,
                [1.; 4],
            );
        }
    }
    pub fn preview(
        &self,
        client: &Client,
        palette: &Palette,
        viewport: [f32; 2],
    ) -> Option<crate::engine_preview::Mesh> {
        if !palette.open {
            return None;
        }
        let appearance = &client.appearance;
        let sprite = self.inventory.get("skin")?;
        let rectangle = palette.document(client, viewport).ok()?.preview()?.0;
        Some(appearance.model.mesh(
            rectangle,
            palette.pointer,
            viewport,
            [
                sprite.x as f32,
                sprite.y as f32,
                sprite.w as f32,
                sprite.h as f32,
            ],
            client.movement.crouched,
        ))
    }
    pub fn preview_skin(&self) -> Option<[u32; 4]> {
        self.inventory.get("skin").map(|s| [s.x, s.y, s.w, s.h])
    }
    #[cfg(test)]
    pub fn compose(&self, client: &Client, palette: &Palette, ctx: Context<'_>) -> Composition {
        self.compose_inner(client, palette, ctx, None, 0.)
    }
    pub fn compose_animated(
        &self,
        client: &Client,
        palette: &Palette,
        ctx: Context<'_>,
        motion: &mut crate::engine::ui::framework::motion::Motion,
        now: f64,
    ) -> Composition {
        motion.begin(now);
        let result = self.compose_inner(client, palette, ctx, Some(&mut *motion), now);
        motion.finish();
        result
    }
    fn compose_inner(
        &self,
        client: &Client,
        palette: &Palette,
        ctx: Context<'_>,
        mut motion: Option<&mut crate::engine::ui::framework::motion::Motion>,
        now: f64,
    ) -> Composition {
        let mut motion_value = |id: &str, target: f32, initial: f32, role: &'static str| {
            motion.as_deref_mut().map_or(target, |m| {
                m.value(id, target, initial, role, &client.animations)
            })
        };

        use crate::engine_inventory::Slot;
        use crate::engine_ui::Target;
        let width = ctx.width as f32 / ctx.scale;
        let height = ctx.height as f32 / ctx.scale;
        let viewport = [width, height];
        let mut c = Canvas {
            clip: None,
            preview_at: None,
            vertices: Vec::new(),
            width,
            height,
            white: self.white,
        };
        let pulse = if client.animations.enabled {
            client
                .animations
                .sample("loading", "value", now)
                .unwrap_or(1.)
        } else {
            1.
        };
        if ctx.startup {
            c.rect([0., 0., width, height], [0.035, 0.05, 0.065, 1.]);
            let panel_width = (width - 24.).clamp(100., 440.);
            let left = (width - panel_width) / 2.;
            let top = (height / 2. - 52.).max(12.);
            c.rect(
                [left - 8., top - 8., panel_width + 16., 112.],
                [0.09, 0.12, 0.15, 1.],
            );
            self.text(&mut c, left, top, "BLOCKWORK", [0.55, 1., 0.72, 1.]);
            let message = ctx.notice.unwrap_or("Opening world...");
            let mut line = String::new();
            let mut y = top + 20.;
            for ch in message.chars() {
                if self.label_width(&line) + 6. > panel_width || ch == '|' {
                    self.text(&mut c, left, y, &line, [1.; 4]);
                    c.rect([left, top + 96., panel_width, 2.], [0.38, 0.88, 0.8, pulse]);
                    line.clear();
                    y += 12.;
                }
                if y > top + 84. {
                    break;
                }
                if ch != '|' {
                    line.push(ch);
                }
            }
            self.text(&mut c, left, y, &line, [1.; 4]);
            return Composition {
                vertices: c.vertices,
                preview_at: None,
            };
        }
        if ctx.menu.open() {
            use crate::engine_settings::Page;
            let reveal = motion_value(
                &format!("menu:{:?}:reveal", ctx.menu.page),
                1.,
                0.,
                "ui_reveal",
            );
            c.rect([0., 0., width, height], [0., 0., 0., 0.65 * reveal]);
            let [left, top] = ctx.menu.origin_for(viewport);
            c.rect(
                [
                    left - 10.,
                    top - 10.,
                    360.,
                    84. + 23. * ctx.menu.count() as f32,
                ],
                [0.09, 0.10, 0.11, 1.],
            );
            let title = match ctx.menu.page {
                Page::Pause => "Game Menu",
                Page::Graphics => "Rendering",
                _ => "Settings",
            };
            self.text(&mut c, left + 8., top + 8., title, [1.; 4]);
            let streamed = client.scene.engine.as_ref().unwrap().stream.is_some();
            for (i, label) in ctx
                .menu
                .labels(ctx.settings, streamed, ctx.fullscreen)
                .iter()
                .enumerate()
            {
                let y = top + 34. + i as f32 * 23.;
                let active = ctx.menu.selected == i;
                c.rect([left, y, 340., 20.], {
                    let weight = motion_value(
                        &format!("menu:{:?}:{i}", ctx.menu.page),
                        f32::from(active),
                        0.,
                        "ui_focus",
                    );
                    let off = [0.27, 0.28, 0.29, 1.];
                    let on = [0.28, 0.55, 0.24, 1.];
                    std::array::from_fn(|a| off[a] + (on[a] - off[a]) * weight)
                });
                c.rect([left, y + 18., 340., 2.], [0.04, 0.05, 0.04, 1.]);
                self.text(&mut c, left + 20., y + 6., label, [1.; 4]);
                if ctx.menu.page == Page::Settings && i == 0 {
                    let target = (ctx.settings.render_distance - 2) as f32
                        / (crate::engine_stream::MAX_RADIUS - 2) as f32;
                    let value = motion_value("menu:distance", target, target, "ui_progress");
                    c.rect([left + 30., y + 15., 280., 2.], [0.07, 0.09, 0.07, 1.]);
                    c.rect([left + 30., y + 15., 280. * value, 2.], [0.7, 1., 0.65, 1.]);
                    c.rect([left + 28. + 280. * value, y + 14., 4., 4.], [1.; 4]);
                }
                if ctx.menu.page == Page::Settings && i < 6
                    || ctx.menu.page == Page::Graphics && i < 7
                {
                    self.text(&mut c, left + 6., y + 6., "-", [1.; 4]);
                    self.text(&mut c, left + 328., y + 6., "+", [1.; 4]);
                }
            }
            self.text(
                &mut c,
                left + 3.,
                top + 44. + ctx.menu.count() as f32 * 23.,
                if ctx.controls.router.device == crate::engine::input::gamepad::Device::Gamepad {
                    "D-pad / LS: navigate / change | A: select"
                } else {
                    "Arrows / Slider: change  Home/End: min/max"
                },
                [0.8, 0.8, 0.8, 1.],
            );
            self.text(
                &mut c,
                left + 3.,
                top + 57. + ctx.menu.count() as f32 * 23.,
                if ctx.menu.page == Page::Graphics {
                    match ctx.ray_status {
                        1 => "Ray queries unavailable - Deferred fallback",
                        2 => "Building nearby hardware ray geometry...",
                        3 => "Hardware ray queries active",
                        4 => "Ray budget exceeded - Deferred fallback",
                        _ => "Ray controls apply to Ray-traced lighting",
                    }
                } else {
                    if ctx.controls.router.device == crate::engine::input::gamepad::Device::Gamepad
                    {
                        "D-pad up: flight   LS click: sprint   A / B: rise / descend"
                    } else {
                        "F: fly  Ctrl: sprint  Space / Shift: up / down"
                    }
                },
                [0.65, 0.8, 0.65, 1.],
            );
            self.control_overlay(&mut c, client, palette, &ctx, viewport, &mut motion_value);
            return Composition {
                vertices: c.vertices,
                preview_at: None,
            };
        }
        let state = client.scene.engine.as_ref().unwrap();
        let player = &state.player;
        let bar_width = player.profile.hotbar_slots as f32 * 20. + 2.;
        let x = (width - bar_width) / 2.;
        let y = height - 22.;
        let selection = motion_value(
            "hotbar:selection",
            player.active as f32,
            player.active as f32,
            "ui_selection",
        );
        c.rect([x, y, bar_width, 22.], [0.25, 0.25, 0.25, 0.8]);
        if let Some(s) = self.start {
            c.quad(s, [x, y, 1., 22.], [1., 1., 1., 0.65]);
        }
        if let Some(s) = self.end {
            c.quad(s, [x + bar_width - 1., y, 1., 22.], [1., 1., 1., 0.65]);
        }
        for i in 0..player.profile.hotbar_slots {
            if !state.ui.theme.legacy_ui {
                let mut style = state.ui.theme.style("slot");
                let selected = state.ui.theme.style("hotbar_selected");
                let weight = motion_value(
                    &format!("hotbar:{i}"),
                    f32::from(i == player.active),
                    0.,
                    "ui_focus",
                );
                for a in 0..4 {
                    style.border[a] += (selected.border[a] - style.border[a]) * weight;
                    style.fill[a] += (selected.fill[a] - style.fill[a]) * weight;
                }
                let rect = [x + 1. + i as f32 * 20., y + 1., 19., 20.];
                c.rect(rect, style.border);
                c.rect([rect[0] + 1., rect[1] + 1., 17., 18.], style.fill);
            }
            if let Some(s) = self.slots.get(i) {
                c.quad(*s, [x + 1. + i as f32 * 20., y, 20., 22.], [1.; 4]);
            }
            if let Some(stack) = player.slots[i] {
                self.stack(
                    &mut c,
                    stack,
                    [x + 3. + i as f32 * 20., y + 3., 16., 16.],
                    &state.registry,
                );
            }
        }
        if let Some(stack) = player.offhand {
            let r = [x - 26., y, 22., 22.];
            if let Some(sprite) = self.cell {
                c.quad(sprite, r, [1.; 4]);
            } else {
                c.rect(r, [0.25, 0.25, 0.25, 0.8]);
            }
            self.stack(
                &mut c,
                stack,
                [r[0] + 3., y + 3., 16., 16.],
                &state.registry,
            );
        }
        if let Some(s) = self.selected {
            c.quad(s, [x - 1. + selection * 20., y - 1., 24., 24.], [1.; 4]);
        }
        if !ctx.chat.open
            && self.cross.is_none()
            && !palette.open
            && matches!(
                client.view.mode,
                crate::engine::player::camera::Mode::FirstPerson
                    | crate::engine::player::camera::Mode::ThirdBack
            )
        {
            c.rect([width / 2. - 3., height / 2., 2., 1.], [0.9, 1., 0.96, 0.9]);
            c.rect([width / 2. + 1., height / 2., 2., 1.], [0.9, 1., 0.96, 0.9]);
            c.rect([width / 2., height / 2. - 3., 1., 2.], [0.9, 1., 0.96, 0.9]);
            c.rect([width / 2., height / 2. + 1., 1., 2.], [0.9, 1., 0.96, 0.9]);
        }
        if !ctx.chat.open
            && matches!(
                client.view.mode,
                crate::engine::player::camera::Mode::FirstPerson
                    | crate::engine::player::camera::Mode::ThirdBack
            )
            && let Some(sprite) = self.cross
        {
            c.quad(
                sprite,
                [width / 2. - 8., height / 2. - 8., 16., 16.],
                [1.; 4],
            );
        }
        if client.view.is_free() && !palette.open && !ctx.chat.open {
            self.text(
                &mut c,
                6.,
                height - 14.,
                if ctx.controls.router.device == crate::engine::input::gamepad::Device::Gamepad {
                    "Free camera | LS move  RS look  A/B rise/dive  X return"
                } else {
                    "Free camera | WASD  Space/Shift  Ctrl fast | F5 return"
                },
                [1.; 4],
            );
        }
        if let Some(id) = player.selected() {
            let mut label = state.registry.get(id).label.clone();
            if let Some(crate::engine::items::ItemData::Fluid { amount, .. }) =
                player.slots[player.active].and_then(|s| s.data)
            {
                let cap = state.registry.get(id).container.unwrap().capacity;
                label = format!("{label}  {amount}/{cap}  | Right use | Shift+Right pour");
            }
            let label = &label;
            self.text(
                &mut c,
                width / 2. - self.label_width(label) / 2.,
                height - 36.,
                label,
                [1.; 4],
            );
        }
        if let Some(target) = ctx.entity_target {
            let label = format!(
                "{}{}{}",
                target.label,
                if target.maximum > 0 {
                    format!("  {} / {}", target.current, target.maximum)
                } else {
                    String::new()
                },
                if target.following { "  Following" } else { "" }
            );
            self.text(
                &mut c,
                width / 2. - self.label_width(&label) / 2.,
                height / 2. + 16.,
                &label,
                [1.; 4],
            );
        }
        let mode = if ctx.controls.router.device == crate::engine::input::gamepad::Device::Gamepad {
            match client.parkour.action {
                crate::engine::player::locomotion::parkour::Action::Climbing => "CLIMBING",
                crate::engine::player::locomotion::parkour::Action::WallRunning => "WALL RUNNING",
                crate::engine::player::locomotion::parkour::Action::Hanging => "HANGING",
                crate::engine::player::locomotion::parkour::Action::Mantling => "PULLING UP",
                crate::engine::player::locomotion::parkour::Action::WallJump => "WALL JUMP",
                _ => match client.movement.mode {
                    crate::engine_player::Mode::Noclip => "NOCLIP",
                    crate::engine_player::Mode::Flying => "FLYING",
                    _ if client.movement.swimming => "SWIMMING",
                    _ if client.movement.crouched => "SNEAKING",
                    _ => "",
                },
            }
        } else if !client.parkour.label().is_empty() {
            client.parkour.label()
        } else {
            match client.movement.mode {
                crate::engine_player::Mode::Noclip => "NOCLIP | N COLLISION",
                crate::engine_player::Mode::Flying => "FLYING | SPACE UP | SHIFT DOWN",
                crate::engine_player::Mode::Walking if client.movement.swimming => {
                    "SWIMMING | SPACE UP | SHIFT DIVE | CTRL FAST"
                }
                crate::engine_player::Mode::Walking if client.movement.crouched => "SNEAKING",
                _ => "",
            }
        };
        if !mode.is_empty() && !client.view.is_free() {
            self.text(
                &mut c,
                width - self.label_width(mode) - 6.,
                height - 49.,
                mode,
                [0.65, 1., 0.75, 1.],
            );
        }
        if ctx.debug || ctx.wire {
            self.text(&mut c, 6., 6., ctx.statistics, [1.; 4]);
            if ctx.wire {
                self.text(
                    &mut c,
                    6.,
                    17.,
                    "F4 WIREFRAME / F3 DETAILS / F11 FULLSCREEN",
                    [0.5, 1., 0.5, 1.],
                );
            }
        }
        if ctx.debug
            && let Some(stream) = &state.stream
        {
            let p = client.actor.position;
            let line = format!(
                "X {:.1} Y {:.1} Z {:.1} | columns {}/{} | edits {} | F10 explore",
                p.x,
                p.y,
                p.z,
                stream.residents.len(),
                stream.wanted.len(),
                stream.edits.len()
            );
            self.text(&mut c, 6., 28., &line, [1.; 4]);
            if let Some(t) = &stream.generator.terrain {
                let sample = t.sample(p.x.floor() as i32, p.z.floor() as i32);
                let label = &t.settings.biomes[sample.biome].label;
                let v = stream.volume.as_ref().unwrap();
                self.text(
                    &mut c,
                    6.,
                    39.,
                    &format!(
                        "Biome {label} | 3D chunks {}/{} | dense {:.1} MiB",
                        v.residents.len(),
                        v.wanted.len(),
                        state.blocks.allocated_bytes() as f64 / 1048576.
                    ),
                    [0.7, 1., 0.8, 1.],
                );
            }
        }
        if ctx.loading {
            self.text(
                &mut c,
                6.,
                height - 60.,
                "Preparing terrain...",
                [1., 1., 1., pulse],
            );
        }
        if let Some(notice) = ctx.notice {
            let id = notice
                .bytes()
                .fold(0u64, |a, b| a.wrapping_mul(31) + u64::from(b));
            let reveal = motion_value(&format!("notice:{id}"), 1., 0., "ui_reveal");
            self.text(&mut c, 6., height - 49., notice, [1., 1., 1., reveal]);
        }
        if palette.open {
            use crate::engine::ui::framework::widgets::{Binding, Kind};
            let reveal = motion_value(
                &format!("screen:{}:reveal", palette.screen_id()),
                1.,
                0.,
                "ui_reveal",
            );
            c.rect([0., 0., width, height], [0., 0., 0., 0.4 * reveal]);
            let model = palette.ui_model(client);
            let result = palette.crafting_result(client, false);
            let table_result = palette.crafting_result(client, true);
            match palette.document(client, viewport) {
                Err(error) => {
                    self.text(&mut c, 8., 40., &format!("UI: {error}"), [1., 0.4, 0.3, 1.])
                }
                Ok(document) => {
                    for (index, node) in document.widgets.iter().enumerate() {
                        let r = node.rect.0;
                        let scale = document.scale;
                        c.clip = Some(node.clip);
                        let role = if node.widget.style.is_empty() {
                            match node.widget.kind {
                                Kind::Button { .. } => "button",
                                _ => "",
                            }
                        } else {
                            node.widget.style.as_str()
                        };
                        let style = state.ui.theme.style(role);
                        match &node.widget.kind {
                            Kind::Panel => {
                                if let Some(sprite) = style
                                    .sprite
                                    .as_ref()
                                    .and_then(|key| self.custom_ui.get(key))
                                {
                                    c.quad(*sprite, r, style.fill);
                                } else if state.ui.theme.legacy_ui {
                                    self.panel(&mut c, r, style.fill);
                                } else {
                                    c.rect(r, style.border);
                                    c.rect(
                                        [
                                            r[0] + scale,
                                            r[1] + scale,
                                            r[2] - 2. * scale,
                                            r[3] - 2. * scale,
                                        ],
                                        style.fill,
                                    );
                                }
                            }
                            Kind::Label { text } => self.text_scaled(
                                &mut c,
                                r[0],
                                r[1],
                                &model.text(text),
                                style.text,
                                scale,
                            ),
                            Kind::Image { sprite } => {
                                if let Some(sprite) = self
                                    .custom_ui
                                    .get(sprite)
                                    .or_else(|| self.inventory.get(sprite.as_str()))
                                {
                                    c.quad(*sprite, r, style.fill);
                                }
                            }
                            Kind::Preview => {
                                c.rect(r, [0., 0., 0., 1.]);
                                if let Some(border) = self.inventory.get("preview_border") {
                                    c.quad(
                                        *border,
                                        [
                                            r[0] - scale,
                                            r[1] - scale,
                                            r[2] + 2. * scale,
                                            r[3] + 2. * scale,
                                        ],
                                        [1.; 4],
                                    );
                                }
                                c.preview_at = Some(c.vertices.len() as u32);
                            }
                            Kind::Search => {
                                c.rect(
                                    r,
                                    if palette.search {
                                        style.border
                                    } else {
                                        style.fill
                                    },
                                );
                                let search = if palette.filter.is_empty() {
                                    "Search...".to_string()
                                } else {
                                    palette
                                        .filter
                                        .chars()
                                        .rev()
                                        .take(20)
                                        .collect::<String>()
                                        .chars()
                                        .rev()
                                        .collect()
                                };
                                self.text_scaled(
                                    &mut c,
                                    r[0] + 3. * scale,
                                    r[1] + 3. * scale,
                                    &search,
                                    style.text,
                                    scale,
                                );
                            }
                            Kind::Catalog { columns, rows } => {
                                let visible = palette.visible(&state.registry);
                                let page = columns * rows;
                                let base = palette.cursor / page * page;
                                let cell_width = r[2] / *columns as f32;
                                let cell_height = r[3] / *rows as f32;
                                for i in 0..columns * rows {
                                    let cell = [
                                        r[0] + (i % columns) as f32 * cell_width,
                                        r[1] + (i / columns) as f32 * cell_height,
                                        cell_width,
                                        cell_height,
                                    ];
                                    if let Some(sprite) = style
                                        .sprite
                                        .as_ref()
                                        .and_then(|key| self.custom_ui.get(key))
                                        .or(self.cell.as_ref())
                                    {
                                        c.quad(*sprite, cell, [1.; 4]);
                                    } else {
                                        c.rect(cell, style.border);
                                        c.rect(
                                            [
                                                cell[0] + scale,
                                                cell[1] + scale,
                                                cell[2] - 2. * scale,
                                                cell[3] - 2. * scale,
                                            ],
                                            style.fill,
                                        );
                                    }
                                    if let Some(id) = visible.get(base + i) {
                                        if let Some(icon) = self.icons.get(*id) {
                                            c.quad(
                                                *icon,
                                                [
                                                    cell[0] + scale,
                                                    cell[1] + scale,
                                                    cell[2] - 2. * scale,
                                                    cell[3] - 2. * scale,
                                                ],
                                                [1.; 4],
                                            );
                                        }
                                        let weight = motion_value(
                                            &format!("{}:catalog:{}", palette.screen_id(), *id),
                                            f32::from(
                                                palette.hover == Some(Target::Catalog(*id as u16)),
                                            ),
                                            0.,
                                            "ui_focus",
                                        );
                                        c.rect(cell, [1., 1., 1., 0.3 * weight]);
                                    }
                                }
                            }
                            Kind::SlotGrid { .. } => {} // Authored grids expand before rendering/hit testing.
                            Kind::Slot { binding } => {
                                let (stack, target) = match binding {
                                    Binding::Container { bank, index } => (
                                        player
                                            .get(Slot::Container {
                                                bank: *bank,
                                                index: *index,
                                            })
                                            .ok()
                                            .flatten(),
                                        Target::Slot(Slot::Container {
                                            bank: *bank,
                                            index: *index,
                                        }),
                                    ),
                                    Binding::Offhand => {
                                        (player.offhand, Target::Slot(Slot::Offhand))
                                    }
                                    Binding::Player(i) => (
                                        player.get(Slot::Player(*i)).ok().flatten(),
                                        Target::Slot(Slot::Player(*i)),
                                    ),
                                    Binding::Craft(i) => (
                                        player.get(Slot::Craft(*i)).ok().flatten(),
                                        Target::Slot(Slot::Craft(*i)),
                                    ),
                                    Binding::Machine(i) => (
                                        palette
                                            .station
                                            .and_then(|p| state.machines.furnaces.get(&p))
                                            .and_then(|f| f.slots.get(*i).copied().flatten()),
                                        Target::Machine(*i),
                                    ),
                                    Binding::Table(i) => (
                                        player.get(Slot::Table(*i)).ok().flatten(),
                                        Target::Slot(Slot::Table(*i)),
                                    ),
                                    Binding::TableResult => (
                                        table_result.as_ref().map(|m| m.stack),
                                        Target::TableOutput,
                                    ),
                                    Binding::Result => {
                                        (result.as_ref().map(|m| m.stack), Target::Output)
                                    }
                                };
                                if let Some(sprite) = style
                                    .sprite
                                    .as_ref()
                                    .and_then(|key| self.custom_ui.get(key))
                                    .or(self.cell.as_ref())
                                {
                                    c.quad(*sprite, r, [1.; 4]);
                                } else {
                                    c.rect(r, style.border);
                                    c.rect(
                                        [
                                            r[0] + scale,
                                            r[1] + scale,
                                            r[2] - 2. * scale,
                                            r[3] - 2. * scale,
                                        ],
                                        style.fill,
                                    );
                                }
                                if let Some(stack) = stack {
                                    self.stack(
                                        &mut c,
                                        stack,
                                        [
                                            r[0] + scale,
                                            r[1] + scale,
                                            r[2] - 2. * scale,
                                            r[3] - 2. * scale,
                                        ],
                                        &state.registry,
                                    );
                                }
                                let weight = motion_value(
                                    &format!("{}:slot:{}", palette.screen_id(), node.widget.id),
                                    f32::from(palette.hover == Some(target)),
                                    0.,
                                    "ui_focus",
                                );
                                c.rect(r, [1., 1., 1., 0.25 * weight]);
                            }
                            Kind::Button { text, .. } => {
                                c.rect(r, style.border);
                                let inside = [
                                    r[0] + scale,
                                    r[1] + scale,
                                    r[2] - 2. * scale,
                                    r[3] - 2. * scale,
                                ];
                                if let Some(sprite) = style
                                    .sprite
                                    .as_ref()
                                    .and_then(|key| self.custom_ui.get(key))
                                {
                                    c.quad(*sprite, inside, style.fill);
                                } else {
                                    c.rect(inside, style.fill);
                                }
                                let weight = motion_value(
                                    &format!("{}:button:{}", palette.screen_id(), node.widget.id),
                                    f32::from(palette.hover == Some(Target::Button(index as u16))),
                                    0.,
                                    "ui_focus",
                                );
                                c.rect(inside, [1., 1., 1., 0.2 * weight]);
                                self.text_scaled(
                                    &mut c,
                                    r[0] + (r[2] - self.label_width(text) * scale) / 2.,
                                    r[1] + (r[3] - 8. * scale) / 2.,
                                    &model.text(text),
                                    style.text,
                                    scale,
                                );
                            }
                            Kind::Progress { value, maximum } => {
                                c.rect(r, style.border);
                                let fraction = (model.number(value) / maximum).clamp(0., 1.) as f32;
                                let fraction = motion_value(
                                    &format!("{}:progress:{}", palette.screen_id(), node.widget.id),
                                    fraction,
                                    0.,
                                    "ui_progress",
                                );
                                let fill = [
                                    r[0] + scale,
                                    r[1] + scale,
                                    (r[2] - 2. * scale).max(0.) * fraction,
                                    (r[3] - 2. * scale).max(0.),
                                ];
                                if let Some(sprite) = style
                                    .sprite
                                    .as_ref()
                                    .and_then(|key| self.custom_ui.get(key))
                                {
                                    c.quad(*sprite, fill, style.fill);
                                } else {
                                    c.rect(fill, style.fill);
                                }
                            }
                        }
                    }
                }
            }
            c.clip = None;
            let hovered = match palette.hover {
                Some(Target::Catalog(id)) => Some(id),
                Some(Target::Slot(slot)) => player.get(slot).ok().flatten().map(|s| s.id),
                Some(Target::Output) => result.map(|m| m.stack.id),
                Some(Target::TableOutput) => table_result.map(|m| m.stack.id),
                Some(Target::Machine(i)) => palette
                    .station
                    .and_then(|p| state.machines.furnaces.get(&p))
                    .and_then(|f| f.slots.get(i).copied().flatten())
                    .map(|s| s.id),
                _ => None,
            };
            if player.cursor.is_none()
                && let Some(id) = hovered
            {
                let label = &state.registry.get(id).label;
                let tw = self.label_width(label) + 8.;
                let tx = (palette.pointer[0] + 10.).min(width - tw - 4.).max(4.);
                let ty = (palette.pointer[1] - 16.).max(4.);
                c.rect([tx - 1., ty - 1., tw + 2., 14.], [0.2, 0.06, 0.35, 0.95]);
                c.rect([tx, ty, tw, 12.], [0.06, 0.015, 0.10, 0.95]);
                self.text(&mut c, tx + 4., ty + 2., label, [1.; 4]);
            }
            if let Some(stack) = player.cursor {
                self.stack(
                    &mut c,
                    stack,
                    [palette.pointer[0] - 8., palette.pointer[1] - 8., 16., 16.],
                    &state.registry,
                );
            }
        }
        if !ctx.chat.open {
            self.control_overlay(&mut c, client, palette, &ctx, viewport, &mut motion_value);
        }
        if !palette.open {
            self.chat_overlay(&mut c, ctx.chat, viewport, &client.animations);
        }

        Composition {
            vertices: c.vertices,
            preview_at: c.preview_at,
        }
    }
}

mod controls;

mod chat;

#[cfg(test)]
mod tests;
