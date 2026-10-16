use super::*;
use crate::engine::{
    blocks::shapes,
    world::{generation, terrain::Settings},
};
fn request(kind: &str, radius: i32) -> Request {
    request_seed(kind, radius, 713)
}
fn request_seed(kind: &str, radius: i32, seed: u64) -> Request {
    request_options(kind, radius, seed, serde_json::json!({}))
}
fn request_options(kind: &str, radius: i32, seed: u64, options: serde_json::Value) -> Request {
    let registry = crate::engine_blocks::Registry::built_in();
    let mut terrain = Settings::built_in();
    terrain.version = 2;
    terrain.generator = Some(Box::new(generation::Definition {
        kind: kind.into(),
        options,
        climate: generation::Climate::default(),
        materials: generation::Materials::default(),
    }));
    let generator = Generator::new(
        seed,
        &registry,
        engine_stream::Config {
            version: 2,
            terrain: Some(terrain),
            radius,
            exhibits: vec![],
        },
    )
    .unwrap();
    Request {
        solid_surface: false,
        epoch: 1,
        key: Key {
            instance: 1,
            registry: 1,
            edits: 0,
            center: [0, 0],
            radius,
        },
        generator: Arc::new(generator),
        geometry: Arc::new(shapes::Library::compile(&registry).unwrap()),
        edits: Arc::new(HashMap::new()),
        side: registry.find("earth").unwrap(),
        stone: registry.find("slate").unwrap(),
        log: registry.find("oak_log").unwrap(),
        leaf: registry.find("oak_leaves").unwrap(),
    }
}
#[test]
fn layered_lod_draws_roof_undersides_and_island_voids_within_budget() {
    for kind in ["cavern", "islands"] {
        let r = request(kind, 16);
        let token = AtomicU64::new(1);
        let mesh = super::super::build(&r, &token).unwrap().unwrap();
        assert!(!mesh.vertices.is_empty());
        assert!(mesh.vertices.len() + mesh.water.len() <= MAX_VERTICES);
        assert!(mesh.vertices.iter().any(|v| (v.layer & 0x3fffff) % 6 == 1));
        assert!(
            mesh.vertices
                .iter()
                .all(|v| v.position.iter().all(|n| n.is_finite()))
        );
        assert!(mesh.stitched_edges > 0);
        if kind == "islands" {
            assert!(mesh.vertices.iter().all(|v| v.position[1] > 20.));
            // There is no artificial ground spanning the origin's void.
            for tri in mesh.vertices.chunks_exact(3) {
                let x = tri.iter().map(|v| v.position[0]).sum::<f32>() / 3.;
                let z = tri.iter().map(|v| v.position[2]).sum::<f32>() / 3.;
                if (tri[0].layer & 0x3fffff) % 6 == 0 && x.abs() < 25. && z.abs() < 25. {
                    panic!("LOD filled an island void");
                }
            }
        }
    }
}
#[test]
fn cancelled_layered_build_never_publishes_a_partial_mesh() {
    let r = request("islands", 16);
    assert!(
        super::super::build(&r, &AtomicU64::new(2))
            .unwrap()
            .is_none()
    );
}
#[test]
fn clipping_and_interval_subtraction_preserve_closed_rims() {
    let p = |x: f32, z: f32, c: f32| Point {
        position: [x, 10., z],
        other: 0.,
        coverage: c,
    };
    let polygon = clip(&[p(0., 0., 1.), p(0., 1., 1.), p(1., 1., -1.)]);
    assert_eq!(polygon.len(), 4);
    assert_eq!(polygon.iter().filter(|v| v.coverage == 0.).count(), 2);
    assert_eq!(
        subtract(vec![(0, 10)], &[(2, 4), (7, 15)]),
        vec![(0, 2), (4, 7)]
    );
}

#[test]
#[ignore = "Explicit release-mode maximum-range budget proof"]
fn maximum_range_layered_budget() {
    for (kind, seed, options) in [
        ("cavern", 713, serde_json::json!({})),
        ("islands", 713, serde_json::json!({})),
        ("islands", 714, serde_json::json!({})),
        ("islands", 0xa5672348, serde_json::json!({})),
        (
            "islands",
            713,
            serde_json::json!({"radius":12,"spacing":512}),
        ),
    ] {
        let small = options.get("radius").is_some();
        let r = request_options(kind, 255, seed, options);
        let start = Instant::now();
        let mesh = super::super::build(&r, &AtomicU64::new(1))
            .unwrap()
            .unwrap();
        println!(
            "{kind} seed {seed}: {} solid + {} water vertices, {} ms",
            mesh.vertices.len(),
            mesh.water.len(),
            start.elapsed().as_millis()
        );
        assert!(mesh.vertices.len() + mesh.water.len() <= MAX_VERTICES);
        if kind == "islands" {
            check_closed_far_shell(&r, &mesh);
            if small {
                for (x, z) in [(1424., 192.), (1424.25, 192.25)] {
                    for face in [0, 1] {
                        assert!(
                            mesh.vertices.chunks_exact(3).any(|t| {
                                if (t[0].layer & 0x003fffff) % 6 != face {
                                    return false;
                                }
                                let area = (t[1].position[0] - t[0].position[0])
                                    * (t[2].position[2] - t[0].position[2])
                                    - (t[1].position[2] - t[0].position[2])
                                        * (t[2].position[0] - t[0].position[0]);
                                if area.abs() < 0.0001 {
                                    return false;
                                }
                                let side = |a: &Vertex, b: &Vertex| {
                                    (b.position[0] - a.position[0]) * (z - a.position[2])
                                        - (b.position[2] - a.position[2]) * (x - a.position[0])
                                };
                                let v =
                                    [side(&t[0], &t[1]), side(&t[1], &t[2]), side(&t[2], &t[0])];
                                v.iter().all(|s| *s >= -0.001) || v.iter().all(|s| *s <= 0.001)
                            }),
                            "small island missing mesh face {face} at {x},{z}"
                        );
                    }
                }
            }
        }
    }
}
fn check_closed_far_shell(r: &Request, mesh: &Mesh) {
    let mut edges = HashMap::<([i32; 3], [i32; 3]), usize>::new();
    let point = |v: &Vertex| v.position.map(|n| (n * 64.).round() as i32);
    for tri in mesh.vertices.chunks_exact(3) {
        if tri[0].layer & (FINE | FIELD) != (FINE | FIELD) {
            continue;
        }
        let p = [point(&tri[0]), point(&tri[1]), point(&tri[2])];
        if p[0] == p[1] || p[1] == p[2] || p[0] == p[2] {
            continue;
        }
        for (a, b) in [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])] {
            let pair = if a < b { (a, b) } else { (b, a) };
            *edges.entry(pair).or_default() += 1;
        }
    }
    let [cx, cz] = center(r.key);
    let view = [
        ((r.key.center[0] - r.key.radius) * 16).max(-8192),
        ((r.key.center[1] - r.key.radius) * 16).max(-8192),
        ((r.key.center[0] + r.key.radius + 1) * 16).min(8192),
        ((r.key.center[1] + r.key.radius + 1) * 16).min(8192),
    ];
    let open: Vec<_> = edges
        .iter()
        .filter(|((a, b), count)| {
            **count != 2
                && [a, b].iter().all(|p| {
                    let x = p[0] as f32 / 64.;
                    let z = p[2] as f32 / 64.;
                    (x - cx as f32).abs().max((z - cz as f32).abs()) > 300.
                        && x > view[0] as f32 + 1.
                        && x < view[2] as f32 - 1.
                        && z > view[1] as f32 + 1.
                        && z < view[3] as f32 - 1.
                })
        })
        .collect();
    assert!(
        open.is_empty(),
        "{} unmatched far island shell edges; sample {:?}",
        open.len(),
        open.iter().take(4).collect::<Vec<_>>()
    );
}

#[test]
fn small_far_islands_keep_coverage_and_thickness_as_the_camera_approaches() {
    let r = request_options(
        "islands",
        255,
        713,
        serde_json::json!({"radius":12,"spacing":512}),
    );
    let terrain = r.generator.terrain.as_ref().unwrap();
    let (x, z) = (1024..1536)
        .step_by(4)
        .flat_map(|x| (0..512).step_by(4).map(move |z| (x, z)))
        .find(|&(x, z)| terrain.sample(x, z).column.coverage > 0.9)
        .expect("small island interior");
    let base = terrain.sample(x, z);
    let thickness = base.height - base.column.bottom.unwrap();
    for offset in [0, 96, 128, 192, 256, 512, 1536] {
        let mut sampler = Sampler {
            terrain,
            cache: HashMap::new(),
            key: Key {
                center: [(x - offset).div_euclid(16), z.div_euclid(16)],
                ..r.key
            },
        };
        let c = Cell { x, z, size: 1 };
        let top = sampler.point(Layer::Floor, c, x as f32, z as f32);
        let bottom = sampler.point(Layer::Bottom, c, x as f32, z as f32);
        assert!(
            top.coverage > 0.,
            "island ({x},{z}) vanished at offset {offset}: {}",
            top.coverage
        );
        assert!(
            (top.position[1] - base.height as f32).abs() <= 2.01,
            "island height moved at offset {offset}"
        );
        assert!(
            (top.position[1] - bottom.position[1] - thickness as f32).abs() < 0.001,
            "island thickness collapsed at offset {offset}"
        );
    }
}
