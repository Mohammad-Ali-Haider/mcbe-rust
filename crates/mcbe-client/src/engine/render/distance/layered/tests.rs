use super::*;
use crate::engine::{
    blocks::shapes,
    world::{generation, terrain::Settings},
};
fn request(kind: &str, radius: i32) -> Request {
    request_seed(kind, radius, 713)
}
fn request_seed(kind: &str, radius: i32, seed: u64) -> Request {
    let registry = crate::engine_blocks::Registry::built_in();
    let mut terrain = Settings::built_in();
    terrain.version = 2;
    terrain.generator = Some(Box::new(generation::Definition {
        kind: kind.into(),
        options: serde_json::json!({}),
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
    for (kind, seed) in [
        ("cavern", 713),
        ("islands", 713),
        ("islands", 714),
        ("islands", 0xa5672348),
    ] {
        let r = request_seed(kind, 255, seed);
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
