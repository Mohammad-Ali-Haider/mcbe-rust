//! Bottom-corner input hints and the shared-widget controller keyboard overlay.
use super::*;
use crate::engine::{
    input::{gamepad::Device, hints::Hint},
    ui::framework::{screen::Model, widgets::Kind},
};
impl Atlas {
    fn hint_row(
        &self,
        c: &mut Canvas,
        hints: &[Hint],
        device: Device,
        origin: [f32; 2],
        maximum: f32,
        theme: &crate::engine::ui::framework::theme::Theme,
    ) {
        let [x, y] = origin;
        let mut at = x;
        let mut row = 0;
        for hint in hints {
            let key = hint.key(device);
            let cap = if device == Device::KeyboardMouse {
                match key {
                    "WASD" | "Arrows" => 19.,
                    "Mouse" | "LMB" | "RMB" | "Wheel" => 12.,
                    "Alt+RMB" => self.label_width("Alt+") + 14.,
                    _ => self.label_width(key) + 8.,
                }
            } else {
                self.label_width(key) + 8.
            };
            let width = cap + self.label_width(hint.label) + 11.;
            if at > x && at + width > x + maximum {
                at = x;
                row += 1;
            }
            if row >= 2 || width > maximum {
                break;
            }
            let y = y + row as f32 * 15.;
            c.rect([at - 2., y - 2., width, 13.], [0.025, 0.035, 0.05, 0.72]);
            let style = theme.styles.get(hint.style_role(device));
            let sprite = style
                .and_then(|s| s.sprite.as_ref())
                .and_then(|id| self.custom_ui.get(id));
            if let Some(sprite) = sprite {
                c.quad(*sprite, [at, y, cap, 10.], style.unwrap().fill);
            } else {
                let fill = style.map_or(
                    if device == Device::Gamepad {
                        match hint.pad {
                            "A" => [0.18, 0.47, 0.23, 1.],
                            "B" => [0.60, 0.16, 0.15, 1.],
                            "X" => [0.15, 0.34, 0.65, 1.],
                            "Y" => [0.65, 0.51, 0.11, 1.],
                            _ => [0.20, 0.24, 0.29, 1.],
                        }
                    } else {
                        [0.20, 0.24, 0.29, 1.]
                    },
                    |s| s.fill,
                );
                if device == Device::KeyboardMouse {
                    self.keyboard_glyph(
                        c,
                        [at, y, cap, 10.],
                        key,
                        fill,
                        style.map_or([1.; 4], |s| s.text),
                    );
                } else {
                    self.keycap(c, [at, y, cap, 10.], fill);
                    self.text(c, at + 4., y + 1., key, style.map_or([1.; 4], |s| s.text));
                }
            }
            self.text(c, at + cap + 4., y + 1., hint.label, [0.95, 0.97, 1., 1.]);
            at += width + 5.;
        }
    }
    fn keycap(&self, c: &mut Canvas, r: [f32; 4], fill: [f32; 4]) {
        let [x, y, w, h] = r;
        c.rect([x + 1., y + 1., w - 2., h], [0.02, 0.03, 0.05, 1.]);
        c.rect([x, y + 1., w, h - 2.], fill);
        c.rect([x + 1., y, w - 2., h - 1.], fill);
        c.rect([x + 1., y, w - 2., 1.], [0.65, 0.73, 0.8, 1.]);
        c.rect([x + 1., y + h - 2., w - 2., 1.], [0.09, 0.12, 0.17, 1.]);
    }
    fn keyboard_glyph(
        &self,
        c: &mut Canvas,
        r: [f32; 4],
        key: &str,
        fill: [f32; 4],
        text: [f32; 4],
    ) {
        let [x, y, w, h] = r;
        if matches!(key, "Mouse" | "LMB" | "RMB" | "Wheel" | "Alt+RMB") {
            let x = if key == "Alt+RMB" {
                self.text(c, x, y + 1., "Alt+", text);
                x + self.label_width("Alt+") + 2.
            } else {
                x
            };
            c.rect([x + 2., y, 6., 10.], [0.65, 0.73, 0.8, 1.]);
            c.rect([x + 1., y + 1., 8., 8.], [0.65, 0.73, 0.8, 1.]);
            c.rect([x + 2., y + 1., 6., 8.], fill);
            c.rect([x + 4., y + 1., 1., 4.], [0.08, 0.11, 0.15, 1.]);
            if key == "LMB" {
                c.rect([x + 2., y + 1., 2., 3.], [0.5, 0.95, 0.72, 1.]);
            }
            if key == "RMB" || key == "Alt+RMB" {
                c.rect([x + 5., y + 1., 3., 3.], [0.5, 0.95, 0.72, 1.]);
            }
            c.rect(
                [x + 4., y + 2., 1., 2.],
                if key == "Wheel" {
                    [0.5, 0.95, 0.72, 1.]
                } else {
                    [0.9, 0.94, 1., 1.]
                },
            );
            return;
        }
        if key == "WASD" || key == "Arrows" {
            let patterns = if key == "WASD" {
                [[5, 5, 7], [2, 5, 7], [3, 2, 6], [6, 5, 6]]
            } else {
                [[2, 7, 2], [2, 6, 2], [2, 7, 2], [2, 3, 2]]
            };
            for (i, (dx, dy)) in [(6., 0.), (0., 5.), (6., 5.), (12., 5.)]
                .into_iter()
                .enumerate()
            {
                self.keycap(c, [x + dx, y + dy, 5., 5.], fill);
                for (row, bits) in patterns[i].iter().enumerate() {
                    for column in 0..3 {
                        if bits & (1 << (2 - column)) != 0 {
                            c.rect(
                                [
                                    x + dx + 1. + column as f32,
                                    y + dy + 1. + row as f32,
                                    1.,
                                    1.,
                                ],
                                text,
                            );
                        }
                    }
                }
            }
            return;
        }
        self.keycap(c, [x, y, w, h], fill);
        self.text(c, x + 4., y + 1., key, text);
    }
    pub(super) fn control_overlay(
        &self,
        c: &mut Canvas,
        client: &Client,
        palette: &Palette,
        ctx: &Context<'_>,
        viewport: [f32; 2],
        motion: &mut impl FnMut(&str, f32, f32, &'static str) -> f32,
    ) {
        let controls = ctx.controls;
        let menu = ctx.menu;
        let use_kind = ctx.use_kind;
        let theme = &client.scene.engine.as_ref().unwrap().ui.theme;
        if controls.keyboard.open {
            c.clip = None;
            let reveal = motion("keyboard:reveal", 1., 0., "ui_reveal");
            c.rect(
                [0., 0., viewport[0], viewport[1]],
                [0., 0., 0., 0.48 * reveal],
            );
            let mut model = Model::default();
            model
                .texts
                .insert("search_term".into(), palette.filter.clone());
            if let Ok(document) = client.scene.engine.as_ref().unwrap().ui.resolve(
                "controller_keyboard",
                &model,
                viewport,
            ) {
                for node in &document.widgets {
                    c.clip = Some(node.clip);
                    let r = node.rect.0;
                    let scale = document.scale;
                    let style = theme.style(if matches!(node.widget.kind, Kind::Button { .. }) {
                        "button"
                    } else {
                        node.widget.style.as_str()
                    });
                    match &node.widget.kind {
                        Kind::Panel => self.panel(c, r, [0.94, 0.95, 0.97, 1.]),
                        Kind::Label { text } => {
                            self.text_scaled(c, r[0], r[1], &model.text(text), style.text, scale)
                        }
                        Kind::Button { text, .. } => {
                            c.rect(r, {
                                let weight = motion(
                                    &format!("keyboard:{}", node.widget.id),
                                    f32::from(node.rect.contains(controls.keyboard.pointer)),
                                    0.,
                                    "ui_focus",
                                );
                                let active = [0.25, 0.62, 0.32, 1.];
                                std::array::from_fn(|a| {
                                    style.fill[a] + (active[a] - style.fill[a]) * weight
                                })
                            });
                            let width = self.label_width(text) * scale;
                            self.text_scaled(
                                c,
                                r[0] + (r[2] - width) * 0.5,
                                r[1] + (r[3] - 8. * scale) * 0.5,
                                text,
                                style.text,
                                scale,
                            );
                        }
                        Kind::Image { sprite } => {
                            if let Some(sprite) = self.custom_ui.get(sprite) {
                                c.quad(*sprite, r, style.fill);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        c.clip = None;
        let (left, right) = crate::engine::input::hints::rows(
            client,
            palette,
            menu,
            controls.keyboard.open,
            use_kind,
        );
        let width = ((viewport[0] - 190.) * 0.5 - 12.).clamp(80., 300.);
        let y = viewport[1] - 31.;
        self.hint_row(c, &left, controls.router.device, [6., y], width, theme);
        self.hint_row(
            c,
            &right,
            controls.router.device,
            [viewport[0] - width - 6., y],
            width,
            theme,
        );
    }
}
