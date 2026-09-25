//! Greedy exposed-face chunk meshes and conservative camera-frustum rejection.
use crate::engine::WorldState;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub layer: u32,
    pub shade: f32,
    pub barycentric: [f32; 3],
}
pub struct ChunkMesh {
    pub vertices: Vec<Vertex>,
    /// Unit block faces, distinct from the smaller number of merged rectangles.
    pub exposed_faces: usize,
    pub block_count: usize,
    pub occluded_faces: usize,
}
pub const DIRECTIONS: [[i32; 3]; 6] = [
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [1, 0, 0],
    [-1, 0, 0],
];
const QUADS: [[[f32; 3]; 4]; 6] = [
    [[0., 1., 0.], [0., 1., 1.], [1., 1., 1.], [1., 1., 0.]],
    [[0., 0., 1.], [0., 0., 0.], [1., 0., 0.], [1., 0., 1.]],
    [[1., 0., 0.], [0., 0., 0.], [0., 1., 0.], [1., 1., 0.]],
    [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
    [[1., 0., 1.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1.]],
    [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
];
pub fn chunk(p: [i32; 3]) -> [i32; 3] {
    p.map(|v| v.div_euclid(16))
}
fn emit(out: &mut Vec<Vertex>, p: [i32; 3], face: usize, id: u16, size: [usize; 2]) {
    let width = size[0] as f32;
    let height = size[1] as f32;
    let origin = QUADS[face][0];
    let u: [f32; 3] = std::array::from_fn(|a| QUADS[face][1][a] - origin[a]);
    let v: [f32; 3] = std::array::from_fn(|a| QUADS[face][3][a] - origin[a]);
    let corners = [[0., 0.], [width, 0.], [width, height], [0., height]];
    for (n, index) in [0usize, 1, 2, 0, 2, 3].iter().enumerate() {
        let [s, t] = corners[*index];
        let mut barycentric = [0.; 3];
        barycentric[n % 3] = 1.;
        out.push(Vertex {
            position: std::array::from_fn(|a| p[a] as f32 + origin[a] + u[a] * s + v[a] * t),
            // Repeat once per block, retaining the reference face orientation.
            uv: [s, 1. - t],
            layer: u32::from(id) * 6 + face as u32,
            shade: match face {
                0 => 1.,
                1 => 0.5,
                2 | 3 => 0.7,
                _ => 0.82,
            },
            barycentric,
        });
    }
}
fn emit_surface(
    out: &mut Vec<Vertex>,
    p: [i32; 3],
    face: usize,
    id: u16,
    surface: crate::engine_geometry::Surface,
) {
    let b = surface.bounds;
    let origin = QUADS[face][0];
    let u: [f32; 3] = std::array::from_fn(|a| QUADS[face][1][a] - origin[a]);
    let v: [f32; 3] = std::array::from_fn(|a| QUADS[face][3][a] - origin[a]);
    for (n, index) in [0usize, 1, 2, 0, 2, 3].into_iter().enumerate() {
        let local: [f32; 3] = std::array::from_fn(|a| {
            if QUADS[face][index][a] == 0. {
                b[a]
            } else {
                b[a + 3]
            }
        });
        let project = |axis: [f32; 3]| {
            (0..3)
                .map(|a| (local[a] - origin[a]) * axis[a])
                .sum::<f32>()
        };
        let mut barycentric = [0.; 3];
        barycentric[n % 3] = 1.;
        out.push(Vertex {
            position: std::array::from_fn(|a| p[a] as f32 + local[a]),
            uv: [project(u), 1. - project(v)],
            layer: (u32::from(id) * 6 + face as u32) | 0x80000000,
            shade: match face {
                0 => 1.,
                1 => 0.5,
                2 | 3 => 0.7,
                _ => 0.82,
            },
            barycentric,
        });
    }
}
pub fn build_naive(state: &WorldState, key: [i32; 3]) -> ChunkMesh {
    naive(key, |p| state.blocks.get(&p).copied(), &state.geometry)
}
fn naive(
    key: [i32; 3],
    lookup: impl Fn([i32; 3]) -> Option<u16>,
    geometry: &crate::engine_geometry::Library,
) -> ChunkMesh {
    let mut out = Vec::new();
    let mut block_count = 0;
    let mut occluded_faces = 0;
    for x in key[0] * 16..key[0] * 16 + 16 {
        for y in key[1] * 16..key[1] * 16 + 16 {
            for z in key[2] * 16..key[2] * 16 + 16 {
                let p = [x, y, z];
                let Some(id) = lookup(p) else {
                    continue;
                };
                block_count += 1;
                for (face, d) in DIRECTIONS.iter().enumerate() {
                    let visible =
                        geometry.visible(id, lookup([x + d[0], y + d[1], z + d[2]]), face);
                    occluded_faces += visible.hidden;
                    if visible.whole_cube_face {
                        emit(&mut out, p, face, id, [1, 1]);
                    } else {
                        for surface in &visible.surfaces {
                            emit_surface(&mut out, p, face, id, *surface);
                        }
                    }
                }
            }
        }
    }
    for v in &mut out {
        v.layer |= geometry.flags(((v.layer & 0x1fffffff) / 6) as u16);
    }
    ChunkMesh {
        exposed_faces: out.len() / 6,
        block_count,
        occluded_faces,
        vertices: out,
    }
}
pub fn build(state: &WorldState, key: [i32; 3]) -> ChunkMesh {
    greedy(key, |p| state.blocks.get(&p).copied(), &state.geometry)
}
fn greedy(
    key: [i32; 3],
    lookup: impl Fn([i32; 3]) -> Option<u16>,
    geometry: &crate::engine_geometry::Library,
) -> ChunkMesh {
    let mut mesh = ChunkMesh {
        vertices: Vec::new(),
        exposed_faces: 0,
        block_count: 0,
        occluded_faces: 0,
    };
    // Each direction has a local U axis matching its texture coordinates.
    // Negative axes enumerate their cells in reverse without mirroring textures.
    for (face, normal) in DIRECTIONS.iter().enumerate() {
        let axis = normal.iter().position(|v| *v != 0).unwrap();
        let u: [i32; 3] = std::array::from_fn(|a| (QUADS[face][1][a] - QUADS[face][0][a]) as i32);
        let v: [i32; 3] = std::array::from_fn(|a| (QUADS[face][3][a] - QUADS[face][0][a]) as i32);
        let u_axis = u.iter().position(|n| *n != 0).unwrap();
        let v_axis = v.iter().position(|n| *n != 0).unwrap();
        for slice in 0..16 {
            let cell = |s: usize, t: usize| {
                let mut p = key.map(|n| n * 16);
                p[axis] += slice;
                p[u_axis] += if u[u_axis] < 0 {
                    15 - s as i32
                } else {
                    s as i32
                };
                p[v_axis] += t as i32;
                p
            };
            let mut mask = [None; 256];
            for t in 0..16 {
                for s in 0..16 {
                    let p = cell(s, t);
                    if let Some(id) = lookup(p) {
                        if face == 0 {
                            mesh.block_count += 1;
                        }
                        let visible = geometry.visible(
                            id,
                            lookup(std::array::from_fn(|a| p[a] + normal[a])),
                            face,
                        );
                        mesh.occluded_faces += visible.hidden;
                        if visible.whole_cube_face
                            && geometry.layer(id) != crate::engine_blocks::RenderLayer::Translucent
                        {
                            mask[t * 16 + s] = Some(id);
                        } else {
                            for surface in &visible.surfaces {
                                emit_surface(&mut mesh.vertices, p, face, id, *surface);
                            }
                            mesh.exposed_faces += visible.surfaces.len();
                        }
                    }
                }
            }
            for t in 0..16 {
                let mut s = 0;
                while s < 16 {
                    let Some(id) = mask[t * 16 + s] else {
                        s += 1;
                        continue;
                    };
                    let mut width = 1;
                    while s + width < 16 && mask[t * 16 + s + width] == Some(id) {
                        width += 1;
                    }
                    let mut height = 1;
                    while t + height < 16
                        && (s..s + width).all(|i| mask[(t + height) * 16 + i] == Some(id))
                    {
                        height += 1;
                    }
                    emit(&mut mesh.vertices, cell(s, t), face, id, [width, height]);
                    mesh.exposed_faces += width * height;
                    for row in t..t + height {
                        mask[row * 16 + s..row * 16 + s + width].fill(None);
                    }
                    s += width;
                }
            }
        }
    }
    for v in &mut mesh.vertices {
        v.layer |= geometry.flags(((v.layer & 0x1fffffff) / 6) as u16);
    }
    mesh
}
/// Owned 18-cubed halo: workers never borrow or clone the live world.
pub struct Snapshot {
    geometry: std::sync::Arc<crate::engine_geometry::Library>,
    key: [i32; 3],
    cells: Box<[u16; 5832]>,
    solid: usize,
}
impl Snapshot {
    pub fn capture(state: &WorldState, key: [i32; 3]) -> Self {
        let mut cells = Box::new([u16::MAX; 5832]);
        let mut solid = 0;
        for x in 0..18 {
            for y in 0..18 {
                for z in 0..18 {
                    let p = [
                        key[0] * 16 + x as i32 - 1,
                        key[1] * 16 + y as i32 - 1,
                        key[2] * 16 + z as i32 - 1,
                    ];
                    if let Some(id) = state.blocks.get(&p) {
                        cells[(x * 18 + y) * 18 + z] = *id;
                        if (1..17).contains(&x) && (1..17).contains(&y) && (1..17).contains(&z) {
                            solid += 1;
                        }
                    }
                }
            }
        }
        Self {
            key,
            cells,
            solid,
            geometry: state.geometry.clone(),
        }
    }
    fn block(&self, p: [i32; 3]) -> Option<u16> {
        let local: [i32; 3] = std::array::from_fn(|a| p[a] - self.key[a] * 16 + 1);
        if local.iter().any(|n| !(0..18).contains(n)) {
            return None;
        }
        let id = self.cells[((local[0] * 18 + local[1]) * 18 + local[2]) as usize];
        (id != u16::MAX).then_some(id)
    }
    pub fn build(&self, merge: bool) -> ChunkMesh {
        if self.solid == 0 {
            return ChunkMesh {
                vertices: Vec::new(),
                exposed_faces: 0,
                block_count: 0,
                occluded_faces: 0,
            };
        }
        if merge {
            greedy(self.key, |p| self.block(p), &self.geometry)
        } else {
            naive(self.key, |p| self.block(p), &self.geometry)
        }
    }
}
pub fn visible(key: [i32; 3], matrix: Mat4) -> bool {
    let min = key.map(|v| v as f32 * 16.);
    let points: [glam::Vec4; 8] = std::array::from_fn(|i| {
        matrix
            * Vec3::new(
                min[0] + (i & 1) as f32 * 16.,
                min[1] + ((i >> 1) & 1) as f32 * 16.,
                min[2] + ((i >> 2) & 1) as f32 * 16.,
            )
            .extend(1.)
    });
    ![(0, -1.), (0, 1.), (1, -1.), (1, 1.), (2, -1.), (2, 1.)]
        .iter()
        .any(|(axis, side)| {
            points.iter().all(|p| {
                if *axis == 2 && *side < 0. {
                    p.z < 0.
                } else {
                    p[*axis] * side > p.w
                }
            })
        })
}
pub fn visible_range(key: [i32; 3], eye: [f32; 3], distance: f32) -> bool {
    range(key, eye, distance, false)
}
pub fn visible_range_flat(key: [i32; 3], eye: [f32; 3], distance: f32) -> bool {
    range(key, eye, distance, true)
}
fn range(key: [i32; 3], eye: [f32; 3], distance: f32, flat: bool) -> bool {
    let minimum = key.map(|v| v as f32 * 16.);
    let squared: f32 = (0..3)
        .filter(|a| !flat || *a != 1)
        .map(|a| {
            let closest = eye[a].clamp(minimum[a], minimum[a] + 16.);
            (eye[a] - closest).powi(2)
        })
        .sum();
    squared <= distance * distance
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flight_range_keeps_ground_visible_from_above() {
        assert!(!visible_range([0, 0, 0], [0.5, 62., 0.5], 40.));
        assert!(visible_range_flat([0, 0, 0], [0.5, 62., 0.5], 40.));
        assert!(!visible_range_flat([4, 0, 0], [0.5, 62., 0.5], 40.));
    }
    #[test]
    fn partial_models_emit_only_union_boundaries_and_match_dense_worker_meshes() {
        let mut client = crate::engine::generate(44).unwrap();
        let state = client.scene.engine.as_mut().unwrap();
        state.blocks.clear();
        let slab = state.registry.find("oak_slab").unwrap();
        let stair = state.registry.find("oak_stairs_north").unwrap();
        let window = state.registry.find("oak_window").unwrap();
        for (p, id) in [
            ([0, 0, 0], slab),
            ([1, 0, 0], 1),
            ([0, 1, 0], stair),
            ([-1, 0, 0], window),
        ] {
            state.blocks.insert(p, id);
        }
        let occupied = |point: [f32; 3]| {
            let cell = point.map(|v| v.floor() as i32);
            state.blocks.get(&cell).is_some_and(|id| {
                state.geometry.boxes(*id).iter().any(|b| {
                    (0..3).all(|a| {
                        point[a] >= cell[a] as f32 + b[a] && point[a] < cell[a] as f32 + b[a + 3]
                    })
                })
            })
        };
        for key in [[0, 0, 0], [-1, 0, 0]] {
            let dense = Snapshot::capture(state, key);
            for greedy in [false, true] {
                let mesh = if greedy {
                    build(state, key)
                } else {
                    build_naive(state, key)
                };
                let other = dense.build(greedy);
                assert_eq!(
                    bytemuck::cast_slice::<_, u8>(&mesh.vertices),
                    bytemuck::cast_slice::<_, u8>(&other.vertices)
                );
                assert_eq!(mesh.occluded_faces, other.occluded_faces);
                for q in mesh.vertices.chunks_exact(6) {
                    let face = ((q[0].layer & 0x7fffffff) % 6) as usize;
                    let normal = DIRECTIONS[face];
                    let center: [f32; 3] =
                        std::array::from_fn(|a| (q[0].position[a] + q[2].position[a]) * 0.5);
                    let inside = std::array::from_fn(|a| center[a] - normal[a] as f32 * 0.0001);
                    let outside = std::array::from_fn(|a| center[a] + normal[a] as f32 * 0.0001);
                    assert!(
                        occupied(inside),
                        "Surface has no solid interior at {center:?}"
                    );
                    assert!(!occupied(outside), "Hidden surface emitted at {center:?}");
                    let n = (Vec3::from(q[1].position) - Vec3::from(q[0].position))
                        .cross(Vec3::from(q[2].position) - Vec3::from(q[0].position));
                    assert!(n.dot(Vec3::from(normal.map(|v| v as f32))) > 0.);
                }
            }
        }
    }
    #[test]
    fn dense_halos_match_live_meshes_and_remain_immutable_after_edits() {
        let mut client = crate::engine::generate(360036).unwrap();
        let state = client.scene.engine.as_mut().unwrap();
        for key in [[0, 0, 0], [-1, 0, -1], [1, 0, -2], [-3, 0, 1]] {
            let snapshot = Snapshot::capture(state, key);
            for merge in [false, true] {
                let a = snapshot.build(merge);
                let b = if merge {
                    build(state, key)
                } else {
                    build_naive(state, key)
                };
                assert_eq!(a.block_count, b.block_count);
                assert_eq!(a.exposed_faces, b.exposed_faces);
                assert_eq!(
                    bytemuck::cast_slice::<_, u8>(&a.vertices),
                    bytemuck::cast_slice::<_, u8>(&b.vertices)
                );
            }
        }
        let frozen = Snapshot::capture(state, [0, 0, 0]);
        let before = frozen.build(true);
        state.blocks.insert([2, 40, 2], 3);
        state.blocks.insert([2, 4, 2], 3);
        let after = frozen.build(true);
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&before.vertices),
            bytemuck::cast_slice::<_, u8>(&after.vertices)
        );
        assert_ne!(build(state, [0, 0, 0]).vertices.len(), after.vertices.len());
    }
    fn compare_surface(state: &WorldState, key: [i32; 3]) {
        use std::collections::BTreeSet;
        let reference: BTreeSet<_> = state
            .blocks
            .iter()
            .filter(|(p, _)| chunk(**p) == key)
            .flat_map(|(p, id)| {
                DIRECTIONS.iter().enumerate().filter_map(move |(face, d)| {
                    let neighbor = std::array::from_fn(|a| p[a] + d[a]);
                    (!state.blocks.contains_key(&neighbor))
                        .then_some((*p, u32::from(*id) * 6 + face as u32))
                })
            })
            .collect();
        let mesh = build(state, key);
        assert_eq!(mesh.exposed_faces, reference.len());
        let mut actual = BTreeSet::new();
        for quad in mesh.vertices.chunks_exact(6) {
            let face = (quad[0].layer % 6) as usize;
            let start: [i32; 3] =
                std::array::from_fn(|a| (quad[0].position[a] - QUADS[face][0][a]) as i32);
            let width = quad[1].uv[0] as usize;
            let height = (1. - quad[5].uv[1]) as usize;
            assert!((1..=16).contains(&width) && (1..=16).contains(&height));
            for t in 0..height {
                for s in 0..width {
                    let p = std::array::from_fn(|a| {
                        start[a]
                            + s as i32 * (QUADS[face][1][a] - QUADS[face][0][a]) as i32
                            + t as i32 * (QUADS[face][3][a] - QUADS[face][0][a]) as i32
                    });
                    assert_eq!(chunk(p), key);
                    assert!(
                        actual.insert((p, quad[0].layer)),
                        "Overlapping face at {p:?}"
                    );
                    assert!(
                        reference.contains(&(p, quad[0].layer)),
                        "Unexpected material/face at {p:?}"
                    );
                }
            }
            for tri in quad.chunks_exact(3) {
                let n = (Vec3::from(tri[1].position) - Vec3::from(tri[0].position))
                    .cross(Vec3::from(tri[2].position) - Vec3::from(tri[0].position));
                assert!(n.dot(Vec3::from(DIRECTIONS[face].map(|v| v as f32))) > 0.);
            }
        }
        assert_eq!(actual.len(), reference.len());
    }
    #[test]
    fn greedy_surface_matches_generated_world_and_negative_chunk_boundaries() {
        let mut client = crate::engine::generate(39).unwrap();
        let state = client.scene.engine.as_mut().unwrap();
        let keys: std::collections::BTreeSet<_> = state.blocks.keys().copied().map(chunk).collect();
        for key in keys {
            compare_surface(state, key);
        }
        state.blocks.clear();
        for x in -18..18 {
            for y in -2..4 {
                for z in -3..20 {
                    if (x * x + y * y + z * z) % 7 != 0 {
                        state.blocks.insert([x, y, z], (x + z).rem_euclid(3) as u16);
                    }
                }
            }
        }
        let keys: std::collections::BTreeSet<_> = state.blocks.keys().copied().map(chunk).collect();
        for key in keys {
            compare_surface(state, key);
        }
    }
    #[test]
    fn solid_chunk_becomes_six_rectangles_and_respects_neighbor_coverage() {
        let mut client = crate::engine::generate(39).unwrap();
        let state = client.scene.engine.as_mut().unwrap();
        state.blocks.clear();
        for x in 0..16 {
            for y in 0..16 {
                for z in 0..16 {
                    state.blocks.insert([x, y, z], 0);
                }
            }
        }
        let m = build(state, [0, 0, 0]);
        assert_eq!(m.exposed_faces, 1536);
        assert_eq!(m.vertices.len(), 36);
        compare_surface(state, [0, 0, 0]);
        for y in 0..16 {
            for z in 0..16 {
                state.blocks.insert([16, y, z], 1);
            }
        }
        let m = build(state, [0, 0, 0]);
        assert_eq!(m.exposed_faces, 1280);
        assert_eq!(m.vertices.len(), 30);
        compare_surface(state, [0, 0, 0]);
        compare_surface(state, [1, 0, 0]);
    }
    #[test]
    fn internal_faces_and_winding_are_correct() {
        let mut client = crate::engine::generate(37).unwrap();
        let state = client.scene.engine.as_mut().unwrap();
        state.blocks.clear();
        state.blocks.insert([0, 0, 0], 0);
        state.blocks.insert([1, 0, 0], 0);
        let mesh = build(state, [0, 0, 0]);
        assert_eq!(mesh.exposed_faces, 10);
        assert_eq!(mesh.vertices.len(), 6 * 6);
        for tri in mesh.vertices.chunks_exact(3) {
            let a = Vec3::from(tri[0].position);
            let b = Vec3::from(tri[1].position);
            let c = Vec3::from(tri[2].position);
            let n = (b - a).cross(c - a);
            assert!(
                n.dot(Vec3::from(
                    DIRECTIONS[(tri[0].layer % 6) as usize].map(|v| v as f32)
                )) > 0.
            );
        }
        let vp = Mat4::perspective_rh(70f32.to_radians(), 1., 0.05, 100.)
            * Mat4::look_to_rh(Vec3::new(0., 8., -8.), Vec3::Z, Vec3::Y);
        assert!(visible([0, 0, 0], vp));
        assert!(!visible([0, 0, -8], vp));
    }
}
