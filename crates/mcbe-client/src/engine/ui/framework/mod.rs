//! Shared UI documents, layout, themes and code-defined screen controllers.
mod grids;
pub(crate) mod layout;
pub(crate) mod screen;
pub(crate) mod theme;
pub(crate) mod widgets;
use crate::scene::Result;
use layout::ResolvedScreen;
use screen::{Document, Laboratory, Model, Screen};
use std::{collections::BTreeMap, path::Path, sync::Arc};
pub struct Library {
    screens: BTreeMap<String, Arc<dyn Screen>>,
    pub theme: theme::Theme,
    pub textures: BTreeMap<String, mcbe_core::textures::Texture>,
}
impl Library {
    pub fn built_in() -> Self {
        let document: Document = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../content/base/ui/inventory.json"
        )))
        .expect("Built-in inventory layout");
        let theme = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../content/base/ui/theme.json"
        )))
        .expect("Built-in UI theme");
        let mut result = Self {
            screens: BTreeMap::new(),
            theme,
            textures: BTreeMap::new(),
        };
        result
            .register("inventory", Arc::new(document))
            .expect("Inventory screen");
        let furnace: Document = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../content/base/ui/furnace.json"
        )))
        .expect("Furnace UI");
        result
            .register("furnace", Arc::new(furnace))
            .expect("Furnace screen");
        let table: Document = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../content/base/ui/crafting_table.json"
        )))
        .expect("Table UI");
        result
            .register("crafting_table", Arc::new(table))
            .expect("Table screen");
        result
            .register(
                "dimensions",
                Arc::new(crate::engine::world::dimensions::screen::Picker),
            )
            .expect("Dimension picker");
        result
            .register("ui_lab", Arc::new(Laboratory))
            .expect("Code-built UI laboratory");
        result
            .register(
                "controller_keyboard",
                Arc::new(super::controller_keyboard::KeyboardScreen),
            )
            .expect("Controller keyboard screen");
        result
    }
    pub fn register(&mut self, id: &str, screen: Arc<dyn Screen>) -> Result<()> {
        if !crate::engine::blocks::state::identifier(id) || self.screens.len() >= 64 {
            return Err("UI screen ID/count invalid".into());
        }
        screen.build(&Model::default()).validate()?;
        self.screens.insert(id.into(), screen);
        Ok(())
    }
    pub fn read_home(home: &Path) -> Result<Self> {
        let mut result = Self::built_in();
        let directory = home.join("content/base/ui");
        if directory.is_dir() {
            let mut paths: Vec<_> = std::fs::read_dir(&directory)?
                .map(|e| e.map(|e| e.path()))
                .collect::<std::io::Result<_>>()?;
            paths.sort();
            if paths.len() > 64 {
                return Err("UI directory exceeds 64 files".into());
            }
            for path in paths
                .iter()
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
            {
                if std::fs::metadata(path)?.len() > 256 * 1024 {
                    return Err("UI JSON exceeds256 KiB".into());
                }
                let bytes = std::fs::read(path)?;
                if path.file_stem().is_some_and(|p| p == "theme") {
                    result.theme = serde_json::from_slice(&bytes)?;
                } else {
                    let document: Document = serde_json::from_slice(&bytes)?;
                    let id = path
                        .file_stem()
                        .and_then(|p| p.to_str())
                        .ok_or("UI file ID invalid")?;
                    result.register(id, Arc::new(document))?;
                }
            }
        }
        result.theme.validate()?;
        for (id, path) in &result.theme.textures {
            if !crate::engine_materials::valid_path(path) {
                return Err("UI texture path must be a relative PNG".into());
            }
            let file = home.join(path);
            if std::fs::metadata(&file)?.len() > 1024 * 1024 {
                return Err("UI PNG exceeds1 MiB".into());
            }
            let texture = mcbe_core::textures::Texture::decode(&std::fs::read(file)?, "png")?;
            if texture.width > 256 || texture.height > 256 {
                return Err("UI sprites must be at most256x256".into());
            }
            result.textures.insert(id.clone(), texture);
        }
        Ok(result)
    }
    pub fn resolve(&self, id: &str, model: &Model, viewport: [f32; 2]) -> Result<ResolvedScreen> {
        let mut model = model.clone();
        if !model.numbers.contains_key("hotbar_slots") {
            crate::engine_inventory::profile::Profile::default().write_model(&mut model);
        }
        self.screens
            .get(id)
            .ok_or("UI screen not registered")?
            .build(&model)
            .expanded(&model)?
            .resolve(viewport)
    }
    pub fn action(
        &self,
        id: &str,
        action: &screen::Action,
        model: &mut Model,
    ) -> Result<screen::Effect> {
        self.screens
            .get(id)
            .ok_or("UI screen not registered")?
            .action(action, model)
    }
    pub fn validate_inventory(
        &self,
        p: &crate::engine::player::inventory::profile::Profile,
    ) -> Result<()> {
        let mut model = Model::default();
        p.write_model(&mut model);
        fn check(
            w: &widgets::Widget,
            m: &Model,
            p: &crate::engine::player::inventory::profile::Profile,
        ) -> Result<()> {
            if w.visible
                .as_ref()
                .is_some_and(|v| v.read(m).is_ok_and(|n| n == 0))
            {
                return Ok(());
            }
            match &w.kind {
                widgets::Kind::Slot { binding } if !binding.valid_for(p) => {
                    return Err(format!("UI slot {} is outside its inventory profile", w.id).into());
                }
                widgets::Kind::SlotGrid {
                    binding,
                    count,
                    offset,
                    ..
                } => {
                    let n = count.read(m)?;
                    if n > 0 && !binding.advanced(offset.read(m)? + n - 1)?.valid_for(p) {
                        return Err(format!("UI slot grid {} exceeds its container", w.id).into());
                    }
                }
                _ => {}
            }
            for c in &w.children {
                check(c, m, p)?;
            }
            Ok(())
        }
        for screen in self.screens.values() {
            let doc = screen.build(&model);
            check(&doc.root, &model, p)?;
            doc.expanded(&model)?;
        }
        Ok(())
    }
    pub fn contains(&self, id: &str) -> bool {
        self.screens.contains_key(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::{
        layout::{Hit, Rect},
        screen::{Action, Effect, Screen},
        widgets::{Binding, Kind, Widget},
    };
    #[test]
    fn station_layouts_fit_and_hit_all_shared_bindings() {
        let library = Library::built_in();
        for viewport in [[480., 270.], [200., 130.]] {
            for name in ["crafting_table", "furnace"] {
                let document = library.resolve(name, &Model::default(), viewport).unwrap();
                let extra: Vec<_> = if name == "furnace" {
                    (0..3).map(Binding::Machine).collect()
                } else {
                    (0..9)
                        .map(Binding::Table)
                        .chain([Binding::TableResult])
                        .collect()
                };
                for b in (0..36).map(Binding::Player).chain(extra) {
                    let r = document.binding(b).unwrap().0;
                    assert!(
                        r[0] >= 0.
                            && r[1] >= 0.
                            && r[0] + r[2] <= viewport[0]
                            && r[1] + r[3] <= viewport[1]
                    );
                    assert!(
                        matches!(document.hit([r[0]+r[2]/2.,r[1]+r[3]/2.]),Some(Hit::Slot(actual)) if actual==b)
                    );
                }
            }
        }
    }
    #[test]
    fn layout_and_hit_testing_share_slots_and_fit_small_viewports() {
        let library = Library::built_in();
        for viewport in [[480., 270.], [900., 400.], [200., 130.]] {
            let document = library
                .resolve("inventory", &Model::default(), viewport)
                .unwrap();
            for i in 0..36 {
                let r = document.binding(Binding::Player(i)).unwrap().0;
                assert!(
                    matches!(document.hit([r[0]+r[2]/2.,r[1]+r[3]/2.]),Some(Hit::Slot(Binding::Player(j))) if j==i)
                );
                assert!(
                    r[0] >= 0.
                        && r[1] >= 0.
                        && r[0] + r[2] <= viewport[0]
                        && r[1] + r[3] <= viewport[1]
                );
            }
            assert_eq!(document.catalog().map(|(_, c, r)| (c, r)), Some((6, 7)));
        }
        assert_eq!(
            Rect([0., 0., 10., 10.]).intersect(Rect([8., 8., 10., 10.])),
            Rect([8., 8., 2., 2.])
        );
    }
    #[test]
    fn rust_screen_actions_update_widgets_and_keyboard_focus() {
        let library = Library::built_in();
        let mut model = Model::default();
        assert!(matches!(
            library
                .action(
                    "ui_lab",
                    &Action::Increment {
                        key: "counter".into(),
                        amount: 3.
                    },
                    &mut model
                )
                .unwrap(),
            Effect::None
        ));
        let document = library.resolve("ui_lab", &model, [480., 270.]).unwrap();
        assert!(
            document
                .widgets
                .iter()
                .any(|w| matches!(&w.widget.kind,Kind::Label{text} if text=="Button presses: 3"))
        );
        let targets = document.focus_targets();
        assert_eq!(targets.len(), 4);
        assert_eq!(document.neighbor(targets[0], [1., 0.]), Some(targets[1]));
        assert!(matches!(
            library
                .action("ui_lab", &Action::Close, &mut model)
                .unwrap(),
            Effect::Close
        ));
        assert!(
            library
                .action(
                    "ui_lab",
                    &Action::Increment {
                        key: "counter".into(),
                        amount: f64::NAN
                    },
                    &mut model
                )
                .is_err()
        );
        assert_eq!(model.number("counter"), 3.);
    }
    #[test]
    fn authoring_rejects_duplicate_ids_nonfinite_layout_and_clips_child_hits() {
        let mut document = Laboratory.build(&Model::default());
        document
            .root
            .children
            .push(document.root.children[0].clone());
        assert!(document.validate().is_err());
        document.root.children.pop();
        document.root.layout.size[0] = f32::NAN;
        assert!(document.validate().is_err());
        let document = Document {
            version: 1,
            root: Widget::new("root", [0., 0., 100., 100.], Kind::Panel).child(Widget::new(
                "clipped",
                [90., 90., 30., 30.],
                Kind::Slot {
                    binding: Binding::Player(0),
                },
            )),
        };
        let resolved = document.resolve([200., 200.]).unwrap();
        assert!(matches!(resolved.hit([95., 95.]), Some(Hit::Slot(_))));
        assert!(resolved.hit([105., 105.]).is_none());
        let mut value = serde_json::to_value(&document).unwrap();
        value["root"]["children"][0]["invented_field"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Document>(value).is_err());
    }
    #[test]
    fn inventory_reload_rejects_item_loss_and_publishes_valid_profile_with_ui() {
        let home = std::env::temp_dir().join(format!(
            "mc-rust-storage-reload-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(home.join("assets")).unwrap();
        std::fs::create_dir_all(home.join("content/base")).unwrap();
        let mut client = crate::engine::generate(37).unwrap();
        let mut r = client.scene.engine.as_ref().unwrap().registry.clone();
        for b in &mut r.blocks {
            b.texture_files = None;
        }
        let definitions = home.join("assets/blocks.json");
        std::fs::write(&definitions, serde_json::to_vec(&r).unwrap()).unwrap();
        client.scene.engine.as_mut().unwrap().player.slots[35] =
            Some(crate::engine_inventory::Stack {
                id: 0,
                count: 9,
                data: None,
            });
        let before = client.scene.engine.as_ref().unwrap().player.clone();
        let ui = client.scene.engine.as_ref().unwrap().ui.clone();
        let revision = client.scene.engine.as_ref().unwrap().registry_revision;
        let mut p = crate::engine_inventory::profile::Profile {
            player_slots: 20,
            ..Default::default()
        };
        let path = home.join("content/base/inventory.json");
        std::fs::write(&path, serde_json::to_vec(&p).unwrap()).unwrap();
        assert!(crate::engine::reload_definitions(&mut client, &definitions).is_err());
        let state = client.scene.engine.as_ref().unwrap();
        assert_eq!(state.registry_revision, revision);
        assert!(Arc::ptr_eq(&ui, &state.ui));
        assert_eq!(state.player.changed_slots(&before), 0);
        p.player_slots = 64;
        p.hotbar_slots = 12;
        std::fs::write(&path, serde_json::to_vec(&p).unwrap()).unwrap();
        crate::engine::reload_definitions(&mut client, &definitions).unwrap();
        let state = client.scene.engine.as_ref().unwrap();
        assert_eq!(state.player.slots.len(), 64);
        assert_eq!(state.player.profile.hotbar_slots, 12);
        assert_eq!(state.player.slots[35], before.slots[35]);
        state.ui.validate_inventory(&state.player.profile).unwrap();
        std::fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn failed_ui_reload_keeps_world_registry_and_screen_library() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "mc-rust-ui-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(home.join("assets")).unwrap();
        std::fs::create_dir_all(home.join("content/base/ui")).unwrap();
        let mut client = crate::engine::generate(360036).unwrap();
        let mut registry = client.scene.engine.as_ref().unwrap().registry.clone();
        for b in &mut registry.blocks {
            b.texture_files = None;
        }
        std::fs::write(
            home.join("assets/blocks.json"),
            serde_json::to_vec(&registry).unwrap(),
        )
        .unwrap();
        std::fs::write(
            home.join("content/base/ui/theme.json"),
            br#"{"version":2,"styles":{}}"#,
        )
        .unwrap();
        let before = client.scene.engine.as_ref().unwrap().ui.clone();
        let revision = client.scene.engine.as_ref().unwrap().registry_revision;
        assert!(
            crate::engine::reload_definitions(&mut client, &home.join("assets/blocks.json"))
                .unwrap_err()
                .to_string()
                .contains("UI theme")
        );
        let state = client.scene.engine.as_ref().unwrap();
        assert_eq!(state.registry_revision, revision);
        assert!(Arc::ptr_eq(&before, &state.ui));
        std::fs::remove_dir_all(home).unwrap();
    }
}
