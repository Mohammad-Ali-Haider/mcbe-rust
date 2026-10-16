//! Controller navigation uses the same resolved widget geometry and click transactions.
use super::inventory::{Palette, Pointer, Target};
use crate::{
    engine_inventory::Button,
    scene::{Client, Result},
};
impl Palette {
    pub fn control_points(&self, client: &Client) -> Result<Vec<[f32; 2]>> {
        use super::framework::layout::Hit;
        let document = self.document(client, self.viewport)?;
        let size = self.page_size(client);
        let start = self.cursor / size * size;
        let count = self
            .visible(&client.scene.engine.as_ref().unwrap().registry)
            .len();
        Ok(document
            .focus_targets()
            .into_iter()
            .filter(|p| match document.hit(*p) {
                Some(Hit::Catalog(i)) => start + i < count,
                Some(_) => true,
                None => false,
            })
            .collect())
    }
    pub fn control_focus(&mut self, client: &Client) -> Result<()> {
        let points = self.control_points(client)?;
        let selected = points
            .iter()
            .find(|p| {
                matches!(
                    self.hit(client, **p, self.viewport),
                    Some(Target::Catalog(_))
                )
            })
            .or_else(|| points.first())
            .copied();
        if let Some(p) = selected {
            self.control_move(client, p);
        }
        Ok(())
    }
    fn control_move(&mut self, client: &Client, p: [f32; 2]) {
        self.move_pointer(client, p, self.viewport);
        if let Some(Target::Catalog(id)) = self.hover
            && let Some(index) = self
                .visible(&client.scene.engine.as_ref().unwrap().registry)
                .iter()
                .position(|v| *v == usize::from(id))
        {
            self.cursor = index;
        }
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn control_cycle(&mut self, client: &Client, delta: i32) -> Result<()> {
        let points = self.control_points(client)?;
        if points.is_empty() {
            return Ok(());
        }
        let index = points
            .iter()
            .position(|p| self.hit(client, *p, self.viewport) == self.hover);
        let next = index.map_or(0, |i| {
            (i as i32 + delta).rem_euclid(points.len() as i32) as usize
        });
        self.search = false;
        self.control_move(client, points[next]);
        Ok(())
    }
    pub fn control_resize(&mut self, client: &Client, viewport: [f32; 2]) -> Result<()> {
        let target = self.hover;
        self.viewport = viewport;
        if let Some(p) = self
            .control_points(client)?
            .into_iter()
            .find(|p| self.hit(client, *p, viewport) == target)
        {
            self.control_move(client, p);
        } else {
            self.control_focus(client)?;
        }
        Ok(())
    }
    pub fn control_navigate(&mut self, client: &Client, direction: [f32; 2]) -> Result<()> {
        let points = self.control_points(client)?;
        let old = self.pointer;
        let point = points
            .iter()
            .filter(|p| (p[0] - old[0]) * direction[0] + (p[1] - old[1]) * direction[1] > 0.01)
            .min_by(|a, b| {
                let score = |p: &[f32; 2]| {
                    let x = p[0] - old[0];
                    let y = p[1] - old[1];
                    x * x + y * y + 4. * (x * direction[1] - y * direction[0]).powi(2)
                };
                score(a).total_cmp(&score(b))
            })
            .or_else(|| {
                if self.hover.is_none() {
                    points.first()
                } else {
                    None
                }
            })
            .copied();
        if let Some(p) = point {
            self.search = false;
            self.control_move(client, p);
        }
        Ok(())
    }
    pub fn control_activate(
        &mut self,
        client: &mut Client,
        button: Button,
        quick: bool,
    ) -> Result<()> {
        self.click(
            client,
            Pointer {
                position: self.pointer,
                viewport: self.viewport,
                button,
                shift: quick,
            },
        )
    }
    pub fn control_page(&mut self, client: &Client, delta: i32) -> Result<()> {
        let document = self.document(client, self.viewport)?;
        let Some((grid, columns, rows)) = document.catalog() else {
            return Ok(());
        };
        let count = self
            .visible(&client.scene.engine.as_ref().unwrap().registry)
            .len();
        if count == 0 {
            return Ok(());
        }
        let size = columns * rows;
        let page = (self.cursor / size) as i32;
        let page = (page + delta).clamp(0, ((count - 1) / size) as i32) as usize;
        self.cursor = (page * size + self.cursor % size).min(count - 1);
        let cell = self.cursor % size;
        let r = grid.rect.0;
        let p = [
            r[0] + (cell % columns) as f32 * r[2] / columns as f32 + r[2] / columns as f32 * 0.5,
            r[1] + (cell / columns) as f32 * r[3] / rows as f32 + r[3] / rows as f32 * 0.5,
        ];
        self.control_move(client, p);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ui::framework::widgets::Binding;
    #[test]
    fn gamepad_focus_reaches_catalog_and_player_crafting_slots_on_shared_geometry() {
        let mut c = crate::engine::generate(44).unwrap();
        let mut p = Palette::default();
        p.toggle(&mut c).unwrap();
        p.control_focus(&c).unwrap();
        assert!(matches!(p.hover, Some(Target::Catalog(_))));
        let points = p.control_points(&c).unwrap();
        let d = p.document(&c, p.viewport).unwrap();
        for slot in [
            Binding::Player(0),
            Binding::Player(35),
            Binding::Craft(0),
            Binding::Result,
        ] {
            let r = d.binding(slot).unwrap();
            assert!(points.iter().any(|v| r.contains(*v)));
        }
        p.control_activate(&mut c, Button::Left, false).unwrap();
        assert!(c.scene.engine.as_ref().unwrap().player.cursor.is_some());
        let r = p
            .document(&c, p.viewport)
            .unwrap()
            .binding(Binding::Player(9))
            .unwrap()
            .0;
        p.control_move(&c, [r[0] + 9., r[1] + 9.]);
        p.control_activate(&mut c, Button::Left, false).unwrap();
        assert!(c.scene.engine.as_ref().unwrap().player.slots[9].is_some());
    }
    #[test]
    fn code_built_screen_buttons_share_controller_activation() {
        let mut c = crate::engine::generate(44).unwrap();
        let mut p = Palette {
            viewport: [480., 270.],
            ..Default::default()
        };
        p.open_screen(&c, "ui_lab").unwrap();
        p.control_focus(&c).unwrap();
        let points = p.control_points(&c).unwrap();
        assert!(!points.is_empty());
        for point in points {
            p.control_move(&c, point);
            if let Some(Target::Button(_)) = p.hover {
                let before = p.model.number("counter");
                p.control_activate(&mut c, Button::Left, false).unwrap();
                if p.model.number("counter") != before {
                    return;
                }
            }
        }
        panic!("Code-built button was not reachable");
    }
}
