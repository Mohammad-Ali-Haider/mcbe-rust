//! Continuous fluid surfaces shared by synchronous and worker snapshots.
use crate::engine::fluids::FULL;
use crate::{
    engine_geometry::{Library, Surface},
    engine_mesh::{self, ChunkMesh},
};
fn height(p: [i32; 3], x: i32, z: i32, amount: &impl Fn([i32; 3]) -> u16) -> f32 {
    let mut sum = 0;
    let mut count = 0;
    for dx in [-1, 0] {
        for dz in [-1, 0] {
            let q = [p[0] + x + dx, p[1], p[2] + z + dz];
            let a = amount(q);
            if a > 0 {
                sum += u32::from(a);
                count += 1;
            }
        }
    }
    if count == 0 {
        0.
    } else {
        sum as f32 / (FULL as f32 * count as f32)
    }
}
pub fn append(
    mesh: &mut ChunkMesh,
    key: [i32; 3],
    geometry: &Library,
    block: impl Fn([i32; 3]) -> Option<u16>,
    amount: impl Fn([i32; 3]) -> u16,
) {
    for x in 0..16 {
        for y in 0..16 {
            for z in 0..16 {
                let p = [key[0] * 16 + x, key[1] * 16 + y, key[2] * 16 + z];
                let a = amount(p);
                if a == 0 {
                    continue;
                }
                let id = block(p).unwrap();
                mesh.block_count += 1;
                for (face, d) in engine_mesh::DIRECTIONS.iter().enumerate() {
                    let q = std::array::from_fn(|i| p[i] + d[i]);
                    let other = amount(q);
                    if (face >= 2 && other > 0)
                        || (face == 0 && a == FULL && other > 0)
                        || (face == 1 && other == FULL)
                    {
                        mesh.occluded_faces += 1;
                        continue;
                    }
                    let surfaces = if face == 0 && a < FULL {
                        vec![Surface {
                            bounds: [0., 0., 0., 1., 1., 1.],
                        }]
                    } else {
                        geometry.visible(id, block(q), face).surfaces.clone()
                    };
                    for surface in surfaces {
                        let first = mesh.vertices.len();
                        engine_mesh::emit_surface(&mut mesh.vertices, p, face, id, surface);
                        for v in &mut mesh.vertices[first..] {
                            let lx = (v.position[0] - p[0] as f32).round() as i32;
                            let lz = (v.position[2] - p[2] as f32).round() as i32;
                            let h = height(p, lx, lz, &amount);
                            v.position[1] = v.position[1].min(p[1] as f32 + h);
                            v.layer |= geometry.flags(id);
                        }
                        mesh.exposed_faces += 1;
                    }
                }
            }
        }
    }
}
/// Back-face-culling remains enabled; reverse copies make only water two-sided.
pub fn back_faces(mesh: &mut ChunkMesh) {
    let reverse: Vec<_> = mesh
        .vertices
        .chunks_exact(3)
        .filter(|t| t[0].layer & 0x00400000 != 0)
        .flat_map(|t| [t[2], t[1], t[0]])
        .collect();
    mesh.vertices.extend(reverse);
}
#[cfg(test)]
mod tests {
    #[test]
    fn fractional_surfaces_match_worker_and_sync_and_share_edge_heights() {
        let mut c = crate::engine::generate(5).unwrap();
        let s = c.scene.engine.as_mut().unwrap();
        s.blocks.clear();
        let w = s.registry.find("water").unwrap();
        c.scene.set_fluid([15, 5, 0], w, 256).unwrap();
        c.scene.set_fluid([16, 5, 0], w, 768).unwrap();
        let s = c.scene.engine.as_ref().unwrap();
        let left = crate::engine_mesh::Snapshot::capture(s, [0, 0, 0]);
        let right = crate::engine_mesh::Snapshot::capture(s, [1, 0, 0]);
        let a = left.build(true);
        let b = right.build(true);
        assert!(
            a.vertices
                .iter()
                .filter(|v| v.position[0] == 16. && v.layer & 0x003fffff == u32::from(w) * 6)
                .all(|v| (v.position[1] - 5.5).abs() < 0.0001)
        );
        assert!(
            b.vertices
                .iter()
                .filter(|v| v.position[0] == 16. && v.layer & 0x003fffff == u32::from(w) * 6)
                .all(|v| (v.position[1] - 5.5).abs() < 0.0001)
        );
        assert!(a.vertices.iter().all(|v| v.position[1] <= 5.5));
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&a.vertices),
            bytemuck::cast_slice::<_, u8>(&left.build(false).vertices)
        );
    }
    #[test]
    fn same_material_amount_changes_keep_snapshots_immutable_and_dirty_diagonals() {
        let mut c = crate::engine::generate(8).unwrap();
        let s = c.scene.engine.as_mut().unwrap();
        s.blocks.clear();
        let w = s.registry.find("water").unwrap();
        let p = [15, 5, 15];
        c.scene.set_fluid(p, w, 256).unwrap();
        let old =
            crate::engine_mesh::Snapshot::capture(c.scene.engine.as_ref().unwrap(), [0, 0, 0]);
        let before = old.build(true).vertices;
        c.scene.engine.as_mut().unwrap().dirty_chunks.clear();
        c.scene.set_fluid(p, w, 768).unwrap();
        assert!(
            c.scene
                .engine
                .as_ref()
                .unwrap()
                .dirty_chunks
                .contains_key(&[1, 0, 1])
        );
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&before),
            bytemuck::cast_slice::<_, u8>(&old.build(true).vertices)
        );
        let new =
            crate::engine_mesh::Snapshot::capture(c.scene.engine.as_ref().unwrap(), [0, 0, 0])
                .build(true);
        assert_ne!(
            bytemuck::cast_slice::<_, u8>(&before),
            bytemuck::cast_slice::<_, u8>(&new.vertices)
        );
        assert!(
            new.vertices
                .iter()
                .any(|v| (v.position[1] - 5.75).abs() < 0.0001)
        );
    }
}
