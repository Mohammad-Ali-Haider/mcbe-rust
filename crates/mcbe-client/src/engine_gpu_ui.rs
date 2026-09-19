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
    pub loading: bool,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub wire: bool,
    pub debug: bool,
    pub statistics: &'a str,
    pub notice: Option<&'a str>,
}
struct Canvas {
    vertices: Vec<Vertex>,
    width: f32,
    height: f32,
    white: Sprite,
}
impl Canvas {
    fn quad(&mut self, s: Sprite, r: [f32; 4], color: [f32; 4]) {
        let [x, y, w, h] = r;
        let pos = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
        let uv = [
            [s.x as f32, s.y as f32],
            [(s.x + s.w) as f32, s.y as f32],
            [(s.x + s.w) as f32, (s.y + s.h) as f32],
            [s.x as f32, (s.y + s.h) as f32],
        ];
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
        if let Some(src) = &client.engine_assets {
            for (key, texture) in &src.inventory {
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
        Ok(a)
    }
    fn text(&self, c: &mut Canvas, x: f32, y: f32, message: &str, color: [f32; 4]) {
        let Some(font) = self.font else {
            return;
        };
        let mut at = x;
        for ch in message.chars() {
            let n = if ch.is_ascii() { ch as u32 } else { 63 };
            let sprite = font.crop(n % 16 * 8, n / 16 * 8, 8, 8);
            if color[0] > 0.7 {
                c.quad(sprite, [at + 1., y + 1., 8., 8.], [0., 0., 0., 0.6]);
            }
            c.quad(sprite, [at, y, 8., 8.], color);
            at += self.advances[n as usize] as f32;
        }
    }
    fn label_width(&self, text: &str) -> f32 {
        text.chars()
            .map(|c| self.advances[if c.is_ascii() { c as usize } else { 63 }])
            .sum::<u32>() as f32
    }
    fn panel(&self, c: &mut Canvas, r: [f32; 4]) {
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
                        [1.; 4],
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
    fn stack(&self, c: &mut Canvas, stack: crate::engine_inventory::Stack, r: [f32; 4]) {
        if let Some(icon) = self.icons.get(usize::from(stack.id)) {
            c.quad(*icon, r, [1.; 4]);
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
    fn slot(
        &self,
        c: &mut Canvas,
        client: &Client,
        palette: &Palette,
        slot: crate::engine_inventory::Slot,
        viewport: [f32; 2],
    ) {
        let r = Palette::slot_rect(slot, viewport);
        c.rect(
            r,
            if palette.hover == Some(crate::engine_ui::Target::Slot(slot)) {
                [1.; 4]
            } else {
                [0.45, 0.45, 0.45, 1.]
            },
        );
        if let Some(cell) = self.cell {
            c.quad(cell, r, [1.; 4]);
        }
        if let Ok(Some(stack)) = client.scene.engine.as_ref().unwrap().player.get(slot) {
            self.stack(c, stack, [r[0] + 1., r[1] + 1., 16., 16.]);
        }
        if palette.hover == Some(crate::engine_ui::Target::Slot(slot)) {
            c.rect(r, [1., 1., 1., 0.25]);
        }
    }
    fn paper_doll(&self, c: &mut Canvas, x: f32, y: f32) {
        c.rect([x + 26., y + 8., 52., 70.], [0., 0., 0., 1.]);
        if let Some(border) = self.inventory.get("preview_border") {
            c.quad(*border, [x + 25., y + 7., 54., 72.], [1.; 4]);
        }
        if let Some(skin) = self.inventory.get("skin") {
            let x = x + 36.;
            let y = y + 11.;
            let modern = skin.h >= 64;
            for (src, dst) in [
                ([8, 8, 8, 8], [x + 8., y, 16., 16.]),
                ([20, 20, 8, 12], [x + 8., y + 16., 16., 24.]),
                ([44, 20, 4, 12], [x, y + 16., 8., 24.]),
                (
                    if modern {
                        [36, 52, 4, 12]
                    } else {
                        [44, 20, 4, 12]
                    },
                    [x + 24., y + 16., 8., 24.],
                ),
                ([4, 20, 4, 12], [x + 8., y + 40., 8., 24.]),
                (
                    if modern {
                        [20, 52, 4, 12]
                    } else {
                        [4, 20, 4, 12]
                    },
                    [x + 16., y + 40., 8., 24.],
                ),
            ] {
                c.quad(skin.crop(src[0], src[1], src[2], src[3]), dst, [1.; 4]);
            }
            c.quad(skin.crop(40, 8, 8, 8), [x + 8., y, 16., 16.], [1.; 4]);
        }
    }
    pub fn compose(&self, client: &Client, palette: &Palette, ctx: Context<'_>) -> Vec<Vertex> {
        use crate::engine_inventory::Slot;
        use crate::engine_ui::Target;
        let width = ctx.width as f32 / ctx.scale;
        let height = ctx.height as f32 / ctx.scale;
        let viewport = [width, height];
        let mut c = Canvas {
            vertices: Vec::new(),
            width,
            height,
            white: self.white,
        };
        let state = client.scene.engine.as_ref().unwrap();
        let player = &state.player;
        let x = (width - 182.) / 2.;
        let y = height - 22.;
        c.rect([x, y, 182., 22.], [0.25, 0.25, 0.25, 0.8]);
        if let Some(s) = self.start {
            c.quad(s, [x, y, 1., 22.], [1., 1., 1., 0.65]);
        }
        if let Some(s) = self.end {
            c.quad(s, [x + 181., y, 1., 22.], [1., 1., 1., 0.65]);
        }
        for i in 0..9 {
            if let Some(s) = self.slots.get(i) {
                c.quad(*s, [x + 1. + i as f32 * 20., y, 20., 22.], [1.; 4]);
            }
            if let Some(stack) = player.slots[i] {
                self.stack(&mut c, stack, [x + 3. + i as f32 * 20., y + 3., 16., 16.]);
            }
        }
        if let Some(s) = self.selected {
            c.quad(
                s,
                [x - 1. + player.active as f32 * 20., y - 1., 24., 24.],
                [1.; 4],
            );
        }
        if let Some(s) = self.cross {
            c.quad(s, [width / 2. - 8., height / 2. - 8., 16., 16.], [1.; 4]);
        }
        if let Some(id) = player.selected() {
            let label = &state.registry.get(id).label;
            self.text(
                &mut c,
                (width - self.label_width(label)) / 2.,
                height - 36.,
                label,
                [1.; 4],
            );
        }
        let mode = match client.movement.mode {
            crate::engine_player::Mode::Noclip => "NOCLIP | N COLLISION",
            crate::engine_player::Mode::Flying => "FLYING | SPACE UP | SHIFT DOWN",
            crate::engine_player::Mode::Walking if client.movement.crouched => "SNEAKING",
            _ => "",
        };
        if !mode.is_empty() {
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
        }
        if ctx.loading {
            self.text(&mut c, 6., height - 60., "Preparing terrain...", [1.; 4]);
        }
        if let Some(notice) = ctx.notice {
            self.text(&mut c, 6., height - 49., notice, [1.; 4]);
        }
        if palette.open {
            c.rect([0., 0., width, height], [0., 0., 0., 0.4]);
            let [left, top] = Palette::origin(viewport);
            let right = left + 150.;
            self.panel(&mut c, [left, top, 146., 166.]);
            self.panel(&mut c, [right, top, 176., 166.]);
            if let Some(fold) = self.inventory.get("fold") {
                c.quad(*fold, [left + 142., top + 3., 12., 160.], [1.; 4]);
            }
            c.rect(
                [left + 6., top + 7., 134., 16.],
                if palette.search {
                    [1.; 4]
                } else {
                    [0.35, 0.35, 0.35, 1.]
                },
            );
            c.rect([left + 7., top + 8., 132., 14.], [0.15, 0.15, 0.15, 1.]);
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
            self.text(&mut c, left + 10., top + 11., &search, [0.9, 0.9, 0.9, 1.]);
            let visible = palette.visible(&state.registry);
            let base = palette.cursor / crate::engine_ui::PAGE * crate::engine_ui::PAGE;
            for i in 0..crate::engine_ui::PAGE {
                let xx = left + 7. + (i % crate::engine_ui::COLS) as f32 * 18.;
                let yy = top + 28. + (i / crate::engine_ui::COLS) as f32 * 18.;
                let r = [xx, yy, 18., 18.];
                if let Some(cell) = self.cell {
                    c.quad(cell, r, [1.; 4]);
                } else {
                    c.rect(r, [0.45, 0.45, 0.45, 1.]);
                }
                if let Some(id) = visible.get(base + i) {
                    if let Some(icon) = self.icons.get(*id) {
                        c.quad(*icon, [xx + 1., yy + 1., 16., 16.], [1.; 4]);
                    }
                    if palette.hover == Some(Target::Catalog(*id as u16)) {
                        c.rect(r, [1., 1., 1., 0.3]);
                    }
                }
            }
            self.text(
                &mut c,
                left + 7.,
                top + 143.,
                &format!(
                    "{} blocks / page {}",
                    visible.len(),
                    palette.cursor / crate::engine_ui::PAGE + 1
                ),
                [0.25, 0.25, 0.25, 1.],
            );
            self.text(
                &mut c,
                left + 7.,
                top + 154.,
                "Shift-click to collect",
                [0.25, 0.25, 0.25, 1.],
            );
            self.paper_doll(&mut c, right, top);
            for (i, key) in ["helmet", "chestplate", "leggings", "boots"]
                .iter()
                .enumerate()
            {
                let r = [right + 7., top + 7. + i as f32 * 18., 18., 18.];
                if let Some(cell) = self.cell {
                    c.quad(cell, r, [1.; 4]);
                }
                if let Some(s) = self.inventory.get(key) {
                    c.quad(*s, [r[0] + 1., r[1] + 1., 16., 16.], [1.; 4]);
                }
            }
            let offhand = [right + 79., top + 61., 18., 18.];
            if let Some(cell) = self.cell {
                c.quad(cell, offhand, [1.; 4]);
            }
            if let Some(s) = self.inventory.get("shield") {
                c.quad(*s, [offhand[0] + 1., offhand[1] + 1., 16., 16.], [1.; 4]);
            }
            self.text(
                &mut c,
                right + 96.,
                top + 7.,
                "Crafting",
                [0.25, 0.25, 0.25, 1.],
            );
            for i in 0..4 {
                self.slot(&mut c, client, palette, Slot::Craft(i), viewport);
            }
            if let Some(arrow) = self.inventory.get("arrow") {
                c.quad(*arrow, [right + 134., top + 29., 16., 13.], [1.; 4]);
            }
            let output = [right + 152., top + 26., 18., 18.];
            if let Some(cell) = self.cell {
                c.quad(cell, output, [1.; 4]);
            }
            let result = state.recipes.find(&state.registry, player);
            if let Some(m) = &result {
                self.stack(&mut c, m.stack, [output[0] + 1., output[1] + 1., 16., 16.]);
            }
            if palette.hover == Some(Target::Output) {
                c.rect(output, [1., 1., 1., 0.25]);
            }
            for i in 0..36 {
                self.slot(&mut c, client, palette, Slot::Player(i), viewport);
            }
            let hovered = match palette.hover {
                Some(Target::Catalog(id)) => Some(id),
                Some(Target::Slot(slot)) => player.get(slot).ok().flatten().map(|s| s.id),
                Some(Target::Output) => result.map(|m| m.stack.id),
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
                );
            }
        }
        c.vertices
    }
}
