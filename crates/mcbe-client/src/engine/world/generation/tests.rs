use super::*;
use crate::{
    engine::world::{
        storage, stream,
        terrain::{Settings, Terrain},
        volume,
    },
    engine_blocks::Registry as Blocks,
};
fn settings(kind: &str) -> Settings {
    let mut s = Settings::built_in();
    s.version = 2;
    s.generator = Some(Box::new(Definition {
        kind: kind.into(),
        options: serde_json::json!({}),
        climate: Climate::default(),
        materials: Materials::default(),
    }));
    s
}
#[test]
fn factories_are_deterministic_configurable_and_strict() {
    let r = Registry::built_in();
    for kind in ["surface", "cavern", "islands"] {
        let s = settings(kind);
        let d = s.generator.as_ref().unwrap();
        let g = r.compile(d).unwrap();
        for x in [-511, -192, -1, 0, 63, 96, 511] {
            for z in [-512, -96, 0, 96, 511] {
                let a = g.column(Noise(713), 64, x, z);
                let b = g.column(Noise(713), 64, x, z);
                assert_eq!(a.top, b.top);
                assert_eq!(a.bottom, b.bottom);
                assert_eq!(a.ceiling, b.ceiling);
                assert_eq!(a.present, b.present);
                if let Some(bottom) = a.bottom {
                    assert!(bottom < a.top);
                }
                if let Some(ceiling) = a.ceiling {
                    assert!(ceiling > a.top + 7 && a.roof_top.unwrap() > ceiling);
                }
            }
        }
        let mut bad = d.clone();
        bad.options = serde_json::json!({"typo":true});
        assert!(r.compile(&bad).is_err());
    }
    let mut d = settings("islands").generator.unwrap();
    d.options = serde_json::json!({"spacing":48,"radius":40});
    assert!(r.compile(&d).is_err());
    d.kind = "missing".into();
    d.options = serde_json::json!({});
    assert!(r.compile(&d).is_err());
    let mut d = settings("cavern").generator.unwrap();
    d.options = serde_json::json!({"clearance":12,"ceiling_height":20});
    assert!(r.compile(&d).is_err());
    d.options = serde_json::json!({"floor_height":-5});
    assert!(r.compile(&d).is_err());
}
#[test]
fn cavern_has_a_real_open_volume_and_islands_have_voids_and_undersides() {
    let r = Blocks::built_in();
    let t = Terrain::new(713, &r, settings("cavern")).unwrap();
    let s = t.sample(96, 96);
    let ceiling = s.column.ceiling.unwrap();
    assert!(t.block([96, s.height - 1, 96], s, None).is_some());
    for y in s.height..ceiling {
        assert!(
            t.block([96, y, 96], s, None).is_none()
                || Some(t.water) == t.block([96, y, 96], s, None)
        );
    }
    assert!(t.block([96, ceiling, 96], s, None).is_some());
    assert!(
        t.block([96, s.column.roof_top.unwrap(), 96], s, None)
            .is_none()
    );
    let t = Terrain::new(713, &r, settings("islands")).unwrap();
    assert!(!t.sample(0, 0).column.present);
    let spawn = t.spawn();
    let s = t.sample(spawn[0], spawn[2]);
    assert!(s.column.present);
    let bottom = s.column.bottom.unwrap();
    assert!(t.block([spawn[0], bottom - 1, spawn[2]], s, None).is_none());
    assert!(t.block([spawn[0], bottom, spawn[2]], s, None).is_some());
    assert!(
        t.block([spawn[0], s.height - 1, spawn[2]], s, None)
            .is_some()
    );
    assert!(t.block([0, -100, 0], t.sample(0, 0), None).is_none());
}
#[test]
fn authored_biomes_and_materials_do_not_require_legacy_world_names() {
    let r = Blocks::built_in();
    let mut s = settings("cavern");
    s.biomes.truncate(1);
    let b = &mut s.biomes[0];
    b.id = "mushroom_cave".into();
    b.surface = "moss".into();
    b.subsurface = "ivory".into();
    b.trees = 0;
    let d = s.generator.as_mut().unwrap();
    d.climate.fallback = b.id.clone();
    d.climate.ocean = None;
    d.climate.river = None;
    d.materials.stone = "oak_planks".into();
    let t = Terrain::new(714, &r, s.clone()).unwrap();
    let p = t.sample(10, 20);
    assert_eq!(t.settings.biomes[p.biome].id, "mushroom_cave");
    assert_eq!(
        t.block([10, p.height - 1, 20], p, None),
        Some(r.find("moss").unwrap())
    );
    assert_eq!(
        t.block([10, p.column.ceiling.unwrap(), 20], p, None),
        Some(r.find("oak_planks").unwrap())
    );
    s.generator.as_mut().unwrap().climate.fallback = "missing".into();
    assert!(Terrain::new(1, &r, s).is_err());
}
#[test]
fn voxel_chunks_match_column_queries_including_roofs_and_island_air() {
    let r = Blocks::built_in();
    for kind in ["surface", "cavern", "islands"] {
        let s = settings(kind);
        let g = stream::Generator::new(
            715,
            &r,
            stream::Config {
                version: 2,
                terrain: Some(s),
                radius: 2,
                exhibits: vec![],
            },
        )
        .unwrap();
        let t = g.terrain.as_ref().unwrap();
        let p = t.spawn();
        let windows = volume::Volume::wanted(
            &g,
            mcbe_core::motion::Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32),
        );
        let column = t.sample(p[0], p[2]).column;
        for y in [
            column.top - 1,
            column.ceiling.unwrap_or(column.top),
            column.roof_top.unwrap_or(column.top) - 1,
        ] {
            assert!(windows.wanted.contains(&storage::key([p[0], y, p[2]])));
        }
        for k in [
            storage::key(p),
            storage::key([p[0], column.top - 1, p[2]]),
            storage::key([p[0], column.ceiling.unwrap_or(column.top), p[2]]),
            [0, 0, 0],
        ] {
            let chunk = volume::generate(&g, k);
            for i in 0..storage::CELLS {
                assert_eq!(
                    (chunk.ids[i] != storage::AIR).then_some(chunk.ids[i]),
                    g.block(storage::position(k, i)),
                    "{kind} {k:?} {i}"
                );
            }
        }
    }
}
struct Flat;
impl Generator for Flat {
    fn column(&self, _: Noise, _: i32, _: i32, _: i32) -> Column {
        Column::surface(42)
    }
}
fn custom(_: &serde_json::Value) -> Result<Arc<dyn Generator>> {
    Ok(Arc::new(Flat))
}
#[test]
fn another_rust_generator_registers_without_adding_an_enum_case() {
    let mut r = Registry::built_in();
    r.register("custom_flat", custom).unwrap();
    assert!(r.register("custom_flat", custom).is_err());
    let mut s = settings("custom_flat");
    s.ravines = false;
    s.caves = false;
    let t = Terrain::with_generators(9, &Blocks::built_in(), s, &r).unwrap();
    assert_eq!(t.sample(-100, 900).height, 42);
}
