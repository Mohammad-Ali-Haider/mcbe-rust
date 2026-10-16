//! Code-built picker using the ordinary shared layout/hit/focus tree.
use crate::engine::ui::framework::{
    layout::{Align, Layout},
    screen::{Action, Document, Model, Screen},
    widgets::{Kind, Widget},
};
pub const PAGE: usize = 6;
pub struct Picker;
impl Screen for Picker {
    fn build(&self, model: &Model) -> Document {
        let mut root = Widget::new("dimensions_root", [0., 0., 326., 222.], Kind::Panel);
        root.layout = Layout {
            anchor: [Align::Center; 2],
            offset: [0.; 2],
            size: [326., 222.],
        };
        root.children.push(
            Widget::new(
                "dimensions_title",
                [14., 12., 298., 12.],
                Kind::Label {
                    text: "Dimensions".into(),
                },
            )
            .styled("heading"),
        );
        root.children.push(Widget::new(
            "dimension_current",
            [14., 29., 298., 12.],
            Kind::Label {
                text: "Current: {dimension_active_label}".into(),
            },
        ));
        for i in 0..PAGE {
            let id = model.texts.get(&format!("dimension_id_{i}"));
            if let Some(id) = id {
                let label = model
                    .texts
                    .get(&format!("dimension_label_{i}"))
                    .cloned()
                    .unwrap_or_else(|| id.clone());
                let rect = [14., 48. + i as f32 * 20., 298., 18.];
                let kind = if model.texts.get("dimension_active_id") == Some(id) {
                    Kind::Label {
                        text: format!("Here: {}", label.chars().take(32).collect::<String>()),
                    }
                } else {
                    Kind::Button {
                        text: format!(
                            "{}{}",
                            if model.number(&format!("dimension_visited_{i}")) > 0. {
                                "Return: "
                            } else {
                                "Visit: "
                            },
                            label.chars().take(32).collect::<String>()
                        ),
                        action: Action::TravelDimension {
                            dimension: id.clone(),
                        },
                    }
                };
                root.children
                    .push(Widget::new(format!("dimension_{i}"), rect, kind));
            }
        }
        let page = model.number("dimension_page").max(0.) as usize;
        let pages = (model.number("dimension_count") as usize)
            .div_ceil(PAGE)
            .max(1);
        root.children.push(Widget::new(
            "dimension_page_label",
            [14., 172., 298., 10.],
            Kind::Label {
                text: format!("Page {} / {}", page + 1, pages),
            },
        ));
        for (id, text, x, action) in [
            (
                "dimension_previous",
                "Previous",
                14.,
                Action::SetNumber {
                    key: "dimension_page".into(),
                    value: page.saturating_sub(1) as f64,
                },
            ),
            (
                "dimension_next",
                "Next",
                116.,
                Action::SetNumber {
                    key: "dimension_page".into(),
                    value: (page + 1).min(pages - 1) as f64,
                },
            ),
            ("dimension_close", "Close", 218., Action::Close),
        ] {
            root.children.push(Widget::new(
                id,
                [x, 190., 94., 18.],
                Kind::Button {
                    text: text.into(),
                    action,
                },
            ));
        }
        Document { version: 1, root }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{App, engine_blocks, engine_stream, engine_ui};
    use winit::keyboard::KeyCode;
    #[test]
    fn shared_picker_pages_and_emits_a_real_worker_travel() {
        let mut c = engine_stream::generate(713, engine_blocks::Registry::built_in()).unwrap();
        engine_stream::set_radius(&mut c, 2).unwrap();
        super::super::attach_legacy(&mut c, super::super::Definitions::built_in()).unwrap();
        let context = c
            .scene
            .engine
            .as_mut()
            .unwrap()
            .dimensions
            .as_mut()
            .unwrap();
        for i in 0..13 {
            let mut d = context.definitions.dimensions[2].clone();
            d.id = format!("extra_{i}");
            d.label = format!("Extra {i}");
            context.definitions.dimensions.push(d);
        }
        context.revision += 1;
        let mut app = App::new(c, std::path::PathBuf::from("work"), false, true).unwrap();
        app.engine_key(KeyCode::KeyU).unwrap();
        assert_eq!(app.palette.screen_id(), "dimensions");
        app.palette.key(&mut app.client, KeyCode::PageDown).unwrap();
        assert_eq!(app.palette.model.number("dimension_page"), 1.);
        let d = app
            .palette
            .document(&app.client, app.palette.viewport)
            .unwrap();
        assert!(d.widgets.iter().any(|w|matches!(&w.widget.kind,Kind::Button{action:Action::TravelDimension{dimension},..} if dimension=="extra_3")));
        app.palette.control_focus(&app.client).unwrap();
        app.palette
            .control_activate(
                &mut app.client,
                crate::engine_inventory::Button::Left,
                false,
            )
            .unwrap();
        app.ui_commands().unwrap();
        assert!(app.load_busy());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while app.load_busy() {
            app.poll_loads().unwrap();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(
            app.client
                .scene
                .engine
                .as_ref()
                .unwrap()
                .dimensions
                .as_ref()
                .unwrap()
                .active,
            "extra_3"
        );
        assert!(!app.palette.open);
        // The pause menu uses the same picker entry point.
        app.menu.page = crate::engine_settings::Page::Pause;
        app.menu.selected = 3;
        let action = app.menu.activate(&mut app.settings, 1);
        app.settings_action(action).unwrap();
        assert_eq!(app.palette.screen_id(), "dimensions");
        assert!(matches!(
            app.palette.hover,
            None | Some(engine_ui::Target::Button(_))
        ));
    }
}
