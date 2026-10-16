//! Desktop inventory controller and local Bedrock sprite helpers.
use crate::{
    engine_blocks::{BlockId, Registry},
    engine_inventory::{Button, Slot},
    scene::{Client, Result},
};
use mcbe_core::textures::Texture;
use winit::keyboard::KeyCode;
pub const COLS: usize = 7;
pub const ROWS: usize = 6;
pub const PAGE: usize = COLS * ROWS;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Catalog(BlockId),
    Slot(Slot),
    Output,
    TableOutput,
    Search,
    Button(u16),
    Machine(usize),
}
pub struct Pointer {
    pub position: [f32; 2],
    pub viewport: [f32; 2],
    pub button: Button,
    pub shift: bool,
}
#[derive(Default)]
pub struct Palette {
    pub screen: String,
    pub station: Option<[i32; 3]>,
    pub model: super::framework::screen::Model,
    pub open: bool,
    pub cursor: usize,
    pub hover: Option<Target>,
    pub pointer: [f32; 2],
    pub viewport: [f32; 2],
    pub filter: String,
    pub search: bool,
    pub revision: u64,
}
impl Palette {
    pub fn recover(client: &Client) -> Self {
        let Some(s) = &client.scene.engine else {
            return Self::default();
        };
        let table = s.player.table.iter().any(Option::is_some);
        Self {
            open: table
                || s.player.cursor.is_some()
                || s.player.crafting.iter().any(Option::is_some),
            screen: if table { "crafting_table" } else { "inventory" }.into(),
            viewport: [480., 270.],
            pointer: [240., 135.],
            ..Default::default()
        }
    }
    pub fn open_target(&mut self, client: &mut Client) -> Result<bool> {
        let Some(hit) = client.target()? else {
            return Ok(false);
        };
        let p = [hit.position.x, hit.position.y, hit.position.z];
        let Some(state) = &client.scene.engine else {
            return Ok(false);
        };
        let Some(interaction) = state
            .blocks
            .get(&p)
            .and_then(|id| state.registry.get(*id).interaction)
        else {
            return Ok(false);
        };
        self.open_screen(client, interaction.screen())?;
        self.station = Some(p);
        if interaction == crate::engine::blocks::components::Interaction::Furnace {
            client.scene.engine.as_mut().unwrap().machines.ensure(p)?;
        }
        Ok(true)
    }
    pub fn crafting_result(
        &self,
        client: &Client,
        table: bool,
    ) -> Option<crate::engine_crafting::Match> {
        let s = client.scene.engine.as_ref()?;
        if table {
            s.recipes.find_grid(&s.registry, &s.player.table, 3)
        } else {
            s.recipes.find(&s.registry, &s.player)
        }
    }
    pub fn observation(&self, client: &Client) -> serde_json::Value {
        let document = self.document(client, self.viewport);
        match document {
            Ok(document) => {
                serde_json::json!({"screen":self.screen_id(),"counter":self.model.number("counter"),"model":self.ui_model(client).numbers,"viewport":self.viewport,"layout_scale":document.scale,"widgets":document.widgets.iter().map(|w|serde_json::json!({"id":w.widget.id,"rect":w.rect.0,"clip":w.clip.0,"widget":w.widget})).collect::<Vec<_>>()})
            }
            Err(error) => serde_json::json!({"error":error.to_string()}),
        }
    }
    pub fn screen_id(&self) -> &str {
        if self.screen.is_empty() {
            "inventory"
        } else {
            &self.screen
        }
    }
    pub fn catalog_dimensions(&self, client: &Client) -> [usize; 2] {
        client
            .scene
            .engine
            .as_ref()
            .unwrap()
            .ui
            .resolve(self.screen_id(), &self.model, self.viewport)
            .ok()
            .and_then(|d| d.catalog().map(|(_, columns, rows)| [columns, rows]))
            .unwrap_or([COLS, ROWS])
    }
    pub fn page_size(&self, client: &Client) -> usize {
        let [c, r] = self.catalog_dimensions(client);
        c * r
    }
    pub fn ui_model(&self, client: &Client) -> super::framework::screen::Model {
        let mut model = self.model.clone();
        model
            .texts
            .entry("ui_controls".into())
            .or_insert_with(|| "Tab / arrows navigate, Enter selects".into());
        let state = client.scene.engine.as_ref().unwrap();
        if let Some(f) = self.station.and_then(|p| state.machines.furnaces.get(&p)) {
            model
                .numbers
                .insert("cook_progress".into(), f.cook_fraction(&state.smelting));
            model
                .numbers
                .insert("burn_progress".into(), f.burn_fraction());
        }
        model.numbers.insert(
            "catalog_count".into(),
            self.visible(&client.scene.engine.as_ref().unwrap().registry)
                .len() as f64,
        );
        model.numbers.insert(
            "page".into(),
            (self.cursor / self.page_size(client) + 1) as f64,
        );
        model
    }
    pub fn document(
        &self,
        client: &Client,
        viewport: [f32; 2],
    ) -> Result<super::framework::layout::ResolvedScreen> {
        client.scene.engine.as_ref().unwrap().ui.resolve(
            self.screen_id(),
            &self.ui_model(client),
            viewport,
        )
    }
    pub fn open_screen(&mut self, client: &Client, id: &str) -> Result<()> {
        if !client.scene.engine.as_ref().unwrap().ui.contains(id) {
            return Err("UI screen not registered".into());
        }
        self.station = None;
        self.screen = id.into();
        self.open = true;
        self.search = false;
        self.hover = None;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    pub fn visible(&self, r: &Registry) -> Vec<usize> {
        let term = self.filter.to_ascii_lowercase();
        r.blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| {
                !b.hidden
                    && (term.is_empty()
                        || b.id.contains(&term)
                        || b.label.to_ascii_lowercase().contains(&term))
            })
            .map(|(i, _)| i)
            .collect()
    }
    pub fn toggle(&mut self, client: &mut Client) -> Result<()> {
        if self.open {
            return self.close(client);
        }
        self.station = None;
        self.screen = "inventory".into();
        self.open = true;
        self.cursor = 0;
        self.hover = None;
        self.search = false;
        self.viewport = [480., 270.];
        self.pointer = [240., 135.];
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    fn close(&mut self, client: &mut Client) -> Result<()> {
        if !client
            .scene
            .engine
            .as_mut()
            .unwrap()
            .player
            .close_inventory()
        {
            self.revision = self.revision.wrapping_add(1);
            return Err("Make space for the held stack and crafting inputs before closing".into());
        }
        self.open = false;
        self.station = None;
        self.search = false;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub fn text(&mut self, text: &str) {
        if self.search {
            for c in text.chars().filter(|c| c.is_ascii_graphic() || *c == ' ') {
                if self.filter.len() < 32 {
                    self.filter.push(c);
                }
            }
            self.cursor = 0;
            self.hover = None;
            self.revision = self.revision.wrapping_add(1);
        }
    }
    pub fn key(&mut self, client: &mut Client, key: KeyCode) -> Result<()> {
        if key == KeyCode::Escape || (!self.search && matches!(key, KeyCode::KeyI | KeyCode::KeyE))
        {
            return self.close(client);
        }
        if self.screen_id() != "inventory" {
            let document = self.document(client, self.viewport)?;
            let direction = match key {
                KeyCode::ArrowLeft => Some([-1., 0.]),
                KeyCode::ArrowRight => Some([1., 0.]),
                KeyCode::ArrowUp => Some([0., -1.]),
                KeyCode::ArrowDown => Some([0., 1.]),
                _ => None,
            };
            let points = document.focus_targets();
            let destination = if let Some(direction) = direction {
                document
                    .neighbor(self.pointer, direction)
                    .or_else(|| points.first().copied())
            } else if key == KeyCode::Tab {
                let index = points
                    .iter()
                    .position(|p| *p == self.pointer)
                    .map_or(0, |i| (i + 1) % points.len().max(1));
                points.get(index).copied()
            } else {
                None
            };
            if let Some(position) = destination {
                self.move_pointer(client, position, self.viewport);
            }
            if matches!(key, KeyCode::Enter | KeyCode::Space) {
                self.click(
                    client,
                    Pointer {
                        position: self.pointer,
                        viewport: self.viewport,
                        button: Button::Left,
                        shift: false,
                    },
                )?;
            }
            self.revision = self.revision.wrapping_add(1);
            return Ok(());
        }
        if key == KeyCode::Tab {
            self.search = !self.search;
            self.revision = self.revision.wrapping_add(1);
            return Ok(());
        }
        if self.search {
            if key == KeyCode::Backspace {
                self.filter.pop();
                self.cursor = 0;
                self.hover = None;
            }
            if key == KeyCode::Enter {
                self.search = false;
            }
            self.revision = self.revision.wrapping_add(1);
            return Ok(());
        }
        let visible = self.visible(&client.scene.engine.as_ref().unwrap().registry);
        let count = visible.len();
        let columns = self.catalog_dimensions(client)[0];
        let page = self.page_size(client);
        let digit = match key {
            KeyCode::Digit1 => Some(0),
            KeyCode::Digit2 => Some(1),
            KeyCode::Digit3 => Some(2),
            KeyCode::Digit4 => Some(3),
            KeyCode::Digit5 => Some(4),
            KeyCode::Digit6 => Some(5),
            KeyCode::Digit7 => Some(6),
            KeyCode::Digit8 => Some(7),
            KeyCode::Digit9 => Some(8),
            _ => None,
        };
        if let Some(i) = digit {
            let state = client.scene.engine.as_mut().unwrap();
            let r = &state.registry;
            let p = &mut state.player;
            match self.hover {
                Some(Target::Catalog(id)) => p.assign_item(r.catalog_stack(id), i)?,
                Some(Target::Slot(slot)) => p.swap_hotbar(slot, i)?,
                _ => p.select(i)?,
            }
            self.revision = self.revision.wrapping_add(1);
            return Ok(());
        }
        if key == KeyCode::Delete {
            client
                .scene
                .engine
                .as_mut()
                .unwrap()
                .player
                .discard_cursor();
            return Ok(());
        }
        if count == 0 {
            return Ok(());
        }
        self.cursor = self.cursor.min(count - 1);
        match key {
            KeyCode::ArrowLeft => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::ArrowRight => self.cursor = (self.cursor + 1).min(count - 1),
            KeyCode::ArrowUp => self.cursor = self.cursor.saturating_sub(columns),
            KeyCode::ArrowDown => self.cursor = (self.cursor + columns).min(count - 1),
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(page),
            KeyCode::PageDown => self.cursor = (self.cursor + page).min(count - 1),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = count - 1,
            KeyCode::Enter => {
                let id = visible[self.cursor] as BlockId;
                self.close(client)?;
                let state = client.scene.engine.as_mut().unwrap();
                let r = &state.registry;
                let p = &mut state.player;
                p.assign_item(r.catalog_stack(id), p.active)?;
                return Ok(());
            }
            _ => {}
        }
        self.hover = Some(Target::Catalog(visible[self.cursor] as BlockId));
        if let Some((grid, columns, rows)) = self.document(client, self.viewport)?.catalog() {
            let r = grid.rect.0;
            let cell = self.cursor % page;
            self.pointer = [
                r[0] + (cell % columns) as f32 * r[2] / columns as f32 + r[2] / columns as f32 / 2.,
                r[1] + (cell / columns) as f32 * r[3] / rows as f32 + r[3] / rows as f32 / 2.,
            ];
        }
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub fn scroll(&mut self, client: &Client, delta: i32) {
        let count = self
            .visible(&client.scene.engine.as_ref().unwrap().registry)
            .len();
        if count > 0 {
            self.cursor = (self.cursor as i32 + delta * self.catalog_dimensions(client)[0] as i32)
                .clamp(0, count as i32 - 1) as usize;
            self.revision = self.revision.wrapping_add(1);
        }
    }
    pub fn origin(viewport: [f32; 2]) -> [f32; 2] {
        [(viewport[0] - 326.) / 2., (viewport[1] - 166.) / 2.]
    }
    pub fn slot_rect(slot: Slot, viewport: [f32; 2]) -> [f32; 4] {
        let [x, y] = Self::origin(viewport);
        let right = x + 150.;
        match slot {
            Slot::Player(i) if i < 9 => [right + 7. + i as f32 * 18., y + 143., 18., 18.],
            Slot::Player(i) => [
                right + 7. + ((i - 9) % 9) as f32 * 18.,
                y + 86. + ((i - 9) / 9) as f32 * 18.,
                18.,
                18.,
            ],
            Slot::Table(i) => [
                right + 30. + (i % 3) as f32 * 18.,
                y + 18. + (i / 3) as f32 * 18.,
                18.,
                18.,
            ],
            Slot::Craft(i) => [
                right + 96. + (i % 2) as f32 * 18.,
                y + 18. + (i / 2) as f32 * 18.,
                18.,
                18.,
            ],
        }
    }
    pub fn hit(&self, client: &Client, position: [f32; 2], viewport: [f32; 2]) -> Option<Target> {
        use super::framework::{layout::Hit, widgets::Binding};
        let document = self.document(client, viewport).ok()?;
        match document.hit(position)? {
            Hit::Search => Some(Target::Search),
            Hit::Slot(Binding::Player(i)) => Some(Target::Slot(Slot::Player(i))),
            Hit::Slot(Binding::Craft(i)) => Some(Target::Slot(Slot::Craft(i))),
            Hit::Slot(Binding::Machine(i)) => Some(Target::Machine(i)),
            Hit::Slot(Binding::Table(i)) => Some(Target::Slot(Slot::Table(i))),
            Hit::Slot(Binding::Result) => Some(Target::Output),
            Hit::Slot(Binding::TableResult) => Some(Target::TableOutput),
            Hit::Catalog(index) => self
                .visible(&client.scene.engine.as_ref().unwrap().registry)
                .get(self.cursor / self.page_size(client) * self.page_size(client) + index)
                .map(|i| Target::Catalog(*i as BlockId)),
            Hit::Button(index) => Some(Target::Button(index as u16)),
        }
    }
    pub fn move_pointer(&mut self, client: &Client, position: [f32; 2], viewport: [f32; 2]) {
        let target = self.hit(client, position, viewport);
        if self.hover != target || self.pointer != position {
            self.revision = self.revision.wrapping_add(1);
        }
        self.pointer = position;
        self.viewport = viewport;
        self.hover = target;
    }
    pub fn click(&mut self, client: &mut Client, pointer: Pointer) -> Result<()> {
        self.move_pointer(client, pointer.position, pointer.viewport);
        self.search = self.hover == Some(Target::Search);
        if let Some(Target::Button(index)) = self.hover {
            let document = self.document(client, pointer.viewport)?;
            if let super::framework::widgets::Kind::Button { action, .. } =
                &document.widgets[index as usize].widget.kind
            {
                let screen_id = self.screen_id().to_owned();
                let effect = client.scene.engine.as_ref().unwrap().ui.action(
                    &screen_id,
                    action,
                    &mut self.model,
                )?;
                match effect {
                    super::framework::screen::Effect::None => {}
                    super::framework::screen::Effect::Close => self.close(client)?,
                    super::framework::screen::Effect::Open(id) => self.open_screen(client, &id)?,
                }
            }
            self.revision = self.revision.wrapping_add(1);
            return Ok(());
        }
        if let Some(p) = self.station {
            if let Some(Target::Machine(i)) = self.hover {
                super::stations::click(client, p, i, pointer.button, pointer.shift)?;
                self.revision = self.revision.wrapping_add(1);
                return Ok(());
            }
            if pointer.shift
                && self.screen_id() == "furnace"
                && let Some(Target::Slot(slot)) = self.hover
            {
                super::stations::quick_from_player(client, p, slot)?;
                self.revision = self.revision.wrapping_add(1);
                return Ok(());
            }
        }
        let table = self.hover == Some(Target::TableOutput);
        let state = client.scene.engine.as_mut().unwrap();
        match self.hover {
            Some(Target::Catalog(id)) => {
                if pointer.shift {
                    state.player.add_item(state.registry.catalog_stack(id));
                } else {
                    state.player.catalog_item(state.registry.catalog_stack(id));
                }
            }
            Some(Target::Slot(slot)) => {
                state.player.click(slot, pointer.button, pointer.shift)?;
            }
            Some(Target::Output | Target::TableOutput) => {
                state
                    .recipes
                    .take_grid(&state.registry, &mut state.player, pointer.shift, table);
            }
            _ => {}
        }
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
}

pub fn blit(
    p: &mut [u32],
    w: usize,
    h: usize,
    t: &Texture,
    src: [u32; 4],
    dst: [i32; 4],
    opacity: f64,
) {
    for dy in 0..dst[3] {
        for dx in 0..dst[2] {
            let x = dst[0] + dx;
            let y = dst[1] + dy;
            if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                continue;
            }
            let sx = src[0] + dx as u32 * src[2] / dst[2] as u32;
            let sy = src[1] + dy as u32 * src[3] / dst[3] as u32;
            if sx >= t.width || sy >= t.height {
                continue;
            }
            let i = ((sy * t.width + sx) * 4) as usize;
            let a = f64::from(t.rgba[i + 3]) / 255. * opacity;
            let old = p[y as usize * w + x as usize];
            let rgb = std::array::from_fn::<_, 3, _>(|c| {
                let prev = ((old >> (16 - c * 8)) & 255) as f64;
                (f64::from(t.rgba[i + c]) * a + prev * (1. - a)) as u32
            });
            p[y as usize * w + x as usize] = (rgb[0] << 16) | (rgb[1] << 8) | rgb[2];
        }
    }
}
fn rect(p: &mut [u32], w: usize, h: usize, bounds: [i32; 4], c: u32) {
    let [x, y, sx, sy] = bounds;
    for yy in y.max(0)..(y + sy).min(h as i32) {
        for xx in x.max(0)..(x + sx).min(w as i32) {
            p[yy as usize * w + xx as usize] = c;
        }
    }
}
pub fn text(client: &Client, p: &mut [u32], w: usize, h: usize, x: i32, y: i32, message: &str) {
    let Some(assets) = &client.engine_assets else {
        return;
    };
    let font = &assets.font;
    let mut at = x;
    for c in message.chars() {
        let n = if c.is_ascii() { c as u32 } else { 63 };
        let sx = n % 16 * 8;
        let sy = n / 16 * 8;
        blit(p, w, h, font, [sx, sy, 8, 8], [at + 1, y + 1, 8, 8], 0.4);
        blit(p, w, h, font, [sx, sy, 8, 8], [at, y, 8, 8], 1.);
        let mut last = 2;
        for gx in 0..8 {
            for gy in 0..8 {
                let i = (((sy + gy) * font.width + sx + gx) * 4 + 3) as usize;
                if i < font.rgba.len() && font.rgba[i] > 0 {
                    last = last.max(gx);
                }
            }
        }
        at += last as i32 + 2;
    }
}
pub fn icon(client: &Client, p: &mut [u32], w: usize, h: usize, id: BlockId, bounds: [i32; 3]) {
    let [x, y, size] = bounds;
    let state = client.scene.engine.as_ref().unwrap();
    if !state.geometry.full(id) {
        let direction = [-0.577350269, -0.577350269, 0.577350269];
        let right = [
            std::f64::consts::FRAC_1_SQRT_2,
            0.,
            std::f64::consts::FRAC_1_SQRT_2,
        ];
        let up = [-0.408248290, 0.816496581, 0.408248290];
        for yy in 0..size {
            for xx in 0..size {
                let u = ((xx as f64 + 0.5) / size as f64 - 0.5) * 1.7;
                let v = (0.5 - (yy as f64 + 0.5) / size as f64) * 1.7;
                let origin =
                    std::array::from_fn(|a| 0.5 - direction[a] * 3. + right[a] * u + up[a] * v);
                if let Some((t, axis, positive)) = state
                    .geometry
                    .boxes(id)
                    .iter()
                    .filter_map(|b| crate::render::box_hit([0, 0, 0], *b, origin, direction))
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                {
                    let hit: [f64; 3] = std::array::from_fn(|a| origin[a] + direction[a] * t);
                    let face = crate::render::face_index(axis, positive);
                    let (tu, tv) = match face {
                        0 => (hit[2], 1. - hit[0]),
                        1 => (1. - hit[2], 1. - hit[0]),
                        2 => (1. - hit[0], 1. - hit[1]),
                        3 => (hit[0], 1. - hit[1]),
                        4 => (1. - hit[2], 1. - hit[1]),
                        _ => (hit[2], 1. - hit[1]),
                    };
                    let shade = match face {
                        0 => 1.,
                        1 => 0.5,
                        2 | 3 => 0.7,
                        _ => 0.82,
                    };
                    let rgb = crate::engine_materials::color(client, id, face, tu, tv)
                        .unwrap_or_else(|| {
                            crate::engine::color(state.registry.get(id), face, tu, tv)
                        })
                        .map(|c| (c * shade) as u32);
                    let dx = x + xx;
                    let dy = y + yy;
                    if dx >= 0 && dy >= 0 && dx < w as i32 && dy < h as i32 {
                        p[dy as usize * w + dx as usize] = (rgb[0] << 16) | (rgb[1] << 8) | rgb[2];
                    }
                }
            }
        }
        return;
    }
    for (origin, a, b, face, shade) in [
        ([8., 1.], [7., 4.], [-7., 4.], 0, 1.),
        ([1., 5.], [7., 4.], [0., 7.], 2, 0.72),
        ([8., 9.], [7., -4.], [0., 7.], 4, 0.86),
    ] {
        for yy in 0..size {
            for xx in 0..size {
                let px = (xx as f64 + 0.5) * 16. / size as f64 - origin[0];
                let py = (yy as f64 + 0.5) * 16. / size as f64 - origin[1];
                let det = a[0] * b[1] - a[1] * b[0];
                let u = (px * b[1] - py * b[0]) / det;
                let v = (a[0] * py - a[1] * px) / det;
                if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                    continue;
                }
                let rgb = crate::engine_materials::color(client, id, face, u, 1. - v)
                    .unwrap_or_else(|| crate::engine::color(state.registry.get(id), face, u, v))
                    .map(|v| (v * shade) as u32);
                let dx = x + xx;
                let dy = y + yy;
                if dx >= 0 && dy >= 0 && dx < w as i32 && dy < h as i32 {
                    p[dy as usize * w + dx as usize] = (rgb[0] << 16) | (rgb[1] << 8) | rgb[2];
                }
            }
        }
    }
}
pub fn hud(client: &Client, p: &mut [u32], w: usize, h: usize) {
    let state = client.scene.engine.as_ref().unwrap();

    let x = (w as i32 - 182) / 2;
    let y = h as i32 - 22;
    if let Some(a) = &client.engine_assets {
        blit(
            p,
            w,
            h,
            &a.start_cap,
            [0, 0, a.start_cap.width, a.start_cap.height],
            [x, y, 1, 22],
            0.65,
        );
        for i in 0..9 {
            let t = &a.hotbar[i];
            blit(
                p,
                w,
                h,
                t,
                [0, 0, t.width, t.height],
                [x + 1 + i as i32 * 20, y, 20, 22],
                1.,
            );
        }
        blit(
            p,
            w,
            h,
            &a.end_cap,
            [0, 0, a.end_cap.width, a.end_cap.height],
            [x + 181, y, 1, 22],
            0.65,
        );
    } else {
        rect(p, w, h, [x, y, 182, 22], 0x555555);
    }
    for i in 0..9 {
        if let Some(stack) = state.player.slots[i] {
            icon(
                client,
                p,
                w,
                h,
                stack.id,
                [x + 3 + i as i32 * 20, y + 3, 16],
            );
            if stack.count > 1 {
                text(
                    client,
                    p,
                    w,
                    h,
                    x + 13 + i as i32 * 20,
                    y + 13,
                    &stack.count.to_string(),
                );
            }
        }
    }
    let selected = state.player.active;
    if let Some(a) = &client.engine_assets {
        let t = &a.selected;
        blit(
            p,
            w,
            h,
            t,
            [0, 0, t.width, t.height],
            [x - 1 + selected as i32 * 20, y - 1, 24, 24],
            1.,
        );
        blit(
            p,
            w,
            h,
            &a.icons,
            [0, 0, 16, 16],
            [w as i32 / 2 - 8, h as i32 / 2 - 8, 16, 16],
            1.,
        );
    }
    let label = state
        .player
        .selected()
        .map_or("", |id| state.registry.get(id).label.as_str());
    text(
        client,
        p,
        w,
        h,
        w as i32 / 2 - label.len() as i32 * 3,
        h as i32 - 36,
        label,
    );
}
pub fn draw(client: &Client, palette: &Palette, p: &mut [u32], w: usize, h: usize) {
    if !palette.open {
        return;
    }
    for px in p.iter_mut() {
        *px = (*px & 0xfefefe) >> 1;
    }
    let viewport = [w as f32, h as f32];
    let [left, top] = Palette::origin(viewport);
    rect(p, w, h, [left as i32, top as i32, 326, 166], 0xc6c6c6);
    let state = client.scene.engine.as_ref().unwrap();
    let visible = palette.visible(&state.registry);
    let base = palette.cursor / PAGE * PAGE;
    text(
        client,
        p,
        w,
        h,
        left as i32 + 7,
        top as i32 + 8,
        "Creative / Search",
    );
    for i in 0..PAGE {
        let x = left as i32 + 7 + (i % COLS) as i32 * 18;
        let y = top as i32 + 28 + (i / COLS) as i32 * 18;
        rect(p, w, h, [x, y, 18, 18], 0x777777);
        if let Some(id) = visible.get(base + i) {
            icon(client, p, w, h, *id as BlockId, [x + 1, y + 1, 16]);
        }
    }
    for slot in (0..36).map(Slot::Player).chain((0..4).map(Slot::Craft)) {
        let r = Palette::slot_rect(slot, viewport);
        rect(p, w, h, r.map(|v| v as i32), 0x777777);
        if let Ok(Some(stack)) = state.player.get(slot) {
            icon(
                client,
                p,
                w,
                h,
                stack.id,
                [r[0] as i32 + 1, r[1] as i32 + 1, 16],
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_table_inputs_recover_into_a_visible_grid_and_close_safely() {
        let mut c = crate::engine::generate(37).unwrap();
        c.scene.engine.as_mut().unwrap().player.table[8] = Some(crate::engine_inventory::Stack {
            data: None,
            id: 1,
            count: 3,
        });
        let mut palette = Palette::recover(&c);
        assert!(palette.open);
        assert_eq!(palette.screen_id(), "crafting_table");
        assert!(palette.station.is_none());
        assert!(
            palette
                .document(&c, palette.viewport)
                .unwrap()
                .binding(super::super::framework::widgets::Binding::Table(8))
                .is_some()
        );
        let count = c.scene.engine.as_ref().unwrap().player.total();
        palette.key(&mut c, KeyCode::Escape).unwrap();
        assert!(!palette.open);
        assert_eq!(c.scene.engine.as_ref().unwrap().player.total(), count);
    }
    #[test]
    fn selecting_new_definition_uses_owned_id() {
        let mut client = crate::engine::generate(37).unwrap();
        let mut palette = Palette::default();
        palette.toggle(&mut client).unwrap();
        palette.key(&mut client, KeyCode::End).unwrap();
        palette.key(&mut client, KeyCode::Enter).unwrap();
        assert!(!palette.open);
        assert_eq!(
            client.scene.engine.as_ref().unwrap().player.selected(),
            Some(
                client
                    .scene
                    .engine
                    .as_ref()
                    .unwrap()
                    .registry
                    .find("bucket")
                    .unwrap()
            )
        );
    }
    #[test]
    fn pointer_motion_outside_slots_invalidates_the_live_preview() {
        let client = crate::engine::generate(47).unwrap();
        let mut palette = Palette::default();
        palette.move_pointer(&client, [-40., -40.], [480., 270.]);
        let revision = palette.revision;
        assert!(palette.hover.is_none());
        palette.move_pointer(&client, [-80., -40.], [480., 270.]);
        assert!(palette.hover.is_none());
        assert!(palette.revision > revision);
        let revision = palette.revision;
        palette.move_pointer(&client, [-80., -40.], [480., 270.]);
        assert_eq!(palette.revision, revision);
    }
}
