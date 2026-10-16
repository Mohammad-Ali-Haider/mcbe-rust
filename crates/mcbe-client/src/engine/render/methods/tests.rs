use super::*;
#[test]
fn shaders_parse_validate_and_publish_real_gbuffer_outputs() {
    for (name, code) in [
        ("world", include_str!("../shaders/world.wgsl").to_owned()),
        (
            "shadow",
            format!(
                "{}\n{}",
                include_str!("../shaders/lighting_frame.wgsl"),
                include_str!("../shaders/shadow.wgsl")
            ),
        ),
        (
            "lighting",
            format!(
                "{}\n{}",
                include_str!("../shaders/lighting_frame.wgsl"),
                include_str!("../shaders/deferred.wgsl")
            ),
        ),
    ] {
        let module = wgpu::naga::front::wgsl::parse_str(&code)
            .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(&code)));
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        if name == "world" {
            assert!(module.entry_points.iter().any(|e| e.name == "fs_gbuffer"));
        }
    }
}
#[test]
fn quality_and_frame_matrices_are_finite_bounded_and_invertible() {
    let p = glam::Mat4::perspective_rh(70f32.to_radians(), 16. / 9., 0.08, 500.)
        * glam::Mat4::look_to_rh(
            glam::Vec3::new(2., 10000., -3.),
            glam::Vec3::new(0.2, -0.3, 1.),
            glam::Vec3::Y,
        );
    for q in [Quality::Low, Quality::Balanced, Quality::High] {
        assert!(q.shadow_size() <= 4096);
        assert!(q.samples() <= 12);
        let settings = Rendering {
            quality: q,
            method: Method::Deferred,
            ..Default::default()
        };
        let f = deferred::Frame::new(
            p,
            [2., 10000., -3.],
            &settings,
            70,
            false,
            [5., 50., 0., 0.],
            [0.; 4],
        );
        assert_eq!(std::mem::size_of::<deferred::Frame>(), 224);
        assert!(f.inverse.iter().chain(&f.light).all(|v| v.is_finite()));
        let sun = glam::Vec3::new(f.sun[0], f.sun[1], f.sun[2]);
        assert!((sun.length() - 1.).abs() < 1e-5);
        assert!(q.radius() <= 96.);
    }
}
#[test]
fn old_preferences_load_and_graphics_controls_persist_with_strict_limits() {
    use crate::engine_settings::{Action, Menu, Page, Settings};
    use winit::keyboard::KeyCode;
    let s = Settings::default();
    let mut old = serde_json::to_value(&s).unwrap();
    old.as_object_mut().unwrap().remove("rendering");
    assert_eq!(serde_json::from_value::<Settings>(old).unwrap(), s);
    let mut s = s;
    let mut m = Menu {
        page: Page::Settings,
        selected: 7,
        ..Default::default()
    };
    m.key(&mut s, KeyCode::Enter);
    assert_eq!(m.page, Page::Graphics);
    assert_eq!(m.labels(&s, true, false).len(), m.count());
    assert_eq!(m.key(&mut s, KeyCode::Enter), Action::Changed);
    assert_eq!(s.rendering.method, Method::Deferred);
    m.selected = 3;
    for _ in 0..100 {
        m.key(&mut s, KeyCode::ArrowRight);
    }
    assert_eq!(s.rendering.exposure, 200);
    for _ in 0..100 {
        m.key(&mut s, KeyCode::ArrowLeft);
    }
    assert_eq!(s.rendering.exposure, 50);
    m.key(&mut s, KeyCode::Escape);
    assert_eq!(m.page, Page::Settings);
    assert_eq!(m.selected, 7);
    let mut invalid = serde_json::to_value(&s).unwrap();
    invalid["rendering"]["method"] = serde_json::json!("unknown_renderer");
    assert!(serde_json::from_value::<Settings>(invalid).is_err());
    s.rendering.exposure = 201;
    assert!(s.validate().is_err());
}
#[test]
fn authored_materials_validate_roundtrip_and_reject_old_schema_payloads() {
    use crate::engine::blocks::components::material::Material;
    let mut c = crate::engine::generate(95).unwrap();
    let mut r = c.scene.engine.as_ref().unwrap().registry.clone();
    let id = r.find("slate").unwrap();
    r.blocks[id as usize].material = Material {
        roughness: 0.25,
        metallic: 0.7,
        emission: 2.,
    };
    crate::engine::apply_registry(&mut c, r).unwrap();
    let doc = serde_json::to_value(crate::engine::document(&c).unwrap()).unwrap();
    assert_eq!(doc["version"], 10);
    let restored = crate::engine::decode(&serde_json::to_vec(&doc).unwrap(), None).unwrap();
    assert_eq!(
        restored
            .scene
            .engine
            .as_ref()
            .unwrap()
            .registry
            .get(id)
            .material,
        c.scene.engine.as_ref().unwrap().registry.get(id).material
    );
    let mut old = doc.clone();
    old["version"] = serde_json::json!(9);
    assert!(crate::engine::decode(&serde_json::to_vec(&old).unwrap(), None).is_err());
    for material in [
        Material {
            roughness: 0.,
            ..Default::default()
        },
        Material {
            metallic: 1.1,
            ..Default::default()
        },
        Material {
            emission: 8.1,
            ..Default::default()
        },
        Material {
            roughness: f32::NAN,
            ..Default::default()
        },
    ] {
        assert!(material.validate().is_err());
    }
    let plain = crate::engine::generate(96).unwrap();
    let mut old = serde_json::to_value(crate::engine::document(&plain).unwrap()).unwrap();
    old["version"] = serde_json::json!(9);
    assert!(crate::engine::decode(&serde_json::to_vec(&old).unwrap(), None).is_ok());
}

#[test]
fn hardware_ray_shaders_validate() {
    for (name, code) in [
        ("ray lighting", ray::shader()),
        (
            "ray localize",
            include_str!("../shaders/ray_localize.wgsl").to_owned(),
        ),
    ] {
        let module = wgpu::naga::front::wgsl::parse_str(&code)
            .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(&code)));
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));
    }
}

#[test]
fn ray_settings_cycle_both_ways_and_preserve_legacy_defaults() {
    use crate::engine_settings::{Action, Menu, Page, Settings};
    use winit::keyboard::KeyCode;
    assert_eq!(Method::Forward.cycle(-1), Method::RayTraced);
    assert_eq!(Method::RayTraced.cycle(1), Method::Forward);
    let mut settings = Settings::default();
    let mut menu = Menu {
        page: Page::Graphics,
        ..Default::default()
    };
    menu.key(&mut settings, KeyCode::ArrowLeft);
    assert_eq!(settings.rendering.method, Method::RayTraced);
    for i in [5, 6] {
        menu.selected = i;
        assert_eq!(menu.key(&mut settings, KeyCode::Enter), Action::Changed);
    }
    assert!(!settings.rendering.ray_reflections && !settings.rendering.ray_indirect);
    menu.selected = 7;
    menu.key(&mut settings, KeyCode::Enter);
    assert_eq!(menu.page, Page::Settings);
    let mut old = serde_json::to_value(Settings::default()).unwrap();
    for field in ["ray_reflections", "ray_indirect"] {
        old["rendering"].as_object_mut().unwrap().remove(field);
    }
    assert_eq!(
        serde_json::from_value::<Settings>(old).unwrap(),
        Settings::default()
    );
    assert_eq!(std::mem::size_of::<ray::scene::Parameters>(), 64);
    assert_eq!(std::mem::size_of::<crate::engine_mesh::Vertex>(), 40);
    for q in [Quality::Low, Quality::Balanced, Quality::High] {
        assert!(ray::scene::Scene::capacity_for(q) <= 2097024);
        assert!(q.ray_samples() <= 4);
    }
}
