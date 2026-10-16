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
            let cap = self.label_width(key) + 8.;
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
            let style = theme.styles.get(hint.role);
            let sprite = style
                .and_then(|s| s.sprite.as_ref())
                .and_then(|id| self.custom_ui.get(id));
            if device == Device::Gamepad
                && let Some(sprite) = sprite
            {
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
                c.rect([at, y, cap, 10.], fill);
                self.text(c, at + 4., y + 1., key, style.map_or([1.; 4], |s| s.text));
            }
            self.text(c, at + cap + 4., y + 1., hint.label, [0.95, 0.97, 1., 1.]);
            at += width + 5.;
        }
    }
    pub(super) fn control_overlay(
        &self,
        c: &mut Canvas,
        client: &Client,
        palette: &Palette,
        ctx: &Context<'_>,
        viewport: [f32; 2],
    ) {
        let controls = ctx.controls;
        let menu = ctx.menu;
        let use_kind = ctx.use_kind;
        let theme = &client.scene.engine.as_ref().unwrap().ui.theme;
        if controls.keyboard.open {
            c.clip = None;
            c.rect([0., 0., viewport[0], viewport[1]], [0., 0., 0., 0.48]);
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
                            c.rect(
                                r,
                                if node.rect.contains(controls.keyboard.pointer) {
                                    [0.25, 0.62, 0.32, 1.]
                                } else {
                                    style.fill
                                },
                            );
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
