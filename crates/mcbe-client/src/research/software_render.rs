use crate::scene::Client;
use flate2::{Compression, write::ZlibEncoder};
use mcbe_core::streaming::Bounds;
use mcbe_core::{
    container_geometry::{ContainerBlock, ContainerKind},
    cubes::Cube,
};
use std::{io::Write, path::Path};
pub const WIDTH: usize = 480;
pub const HEIGHT: usize = 270;
#[derive(Clone, Copy)]
enum Kind {
    Engine(crate::engine_blocks::BlockId),
    Cube(Cube),
    Container(ContainerBlock),
}
#[derive(Clone, Copy, serde::Serialize)]
pub struct CacheStats {
    pub rebuilds: usize,
    pub solid_voxels: usize,
    pub surface_voxels: usize,
    pub culled_voxels: usize,
}
pub struct EngineCache {
    revision: u64,
    dense: Vec<Option<Kind>>,
    ready: Vec<bool>,
    pub stats: CacheStats,
}
impl EngineCache {
    fn build(client: &Client, rebuilds: usize) -> Self {
        let state = client.scene.engine.as_ref().unwrap();
        let bounds = client.scene.bounds;
        let mut dense = vec![None; bounds.volume().unwrap()];
        let mut surface = 0;
        for (p, id) in &state.blocks {
            let hidden = client.engine_cache_enabled
                && state.geometry.full(*id)
                && [
                    [-1, 0, 0],
                    [1, 0, 0],
                    [0, -1, 0],
                    [0, 1, 0],
                    [0, 0, -1],
                    [0, 0, 1],
                ]
                .iter()
                .all(|d| {
                    state
                        .blocks
                        .get(&[p[0] + d[0], p[1] + d[1], p[2] + d[2]])
                        .is_some_and(|id| state.geometry.full(*id))
                });
            if !hidden {
                dense[index(bounds, *p)] = Some(Kind::Engine(*id));
                surface += 1;
            }
        }
        Self {
            revision: state.revision,
            dense,
            ready: vec![
                true;
                ((bounds.max[0] - bounds.min[0]) * (bounds.max[2] - bounds.min[2])) as usize
            ],
            stats: CacheStats {
                rebuilds,
                solid_voxels: state.blocks.len(),
                surface_voxels: surface,
                culled_voxels: state.blocks.len() - surface,
            },
        }
    }
}
fn index(bounds: Bounds, p: [i32; 3]) -> usize {
    bounds.index(p).expect("Checked renderer coordinate")
}
pub fn render(client: &Client, width: usize, height: usize) -> Vec<u32> {
    let bounds = client.scene.bounds;
    let volume = bounds.volume().expect("Validated scene bounds");
    let mut cache = client.engine_render_cache.borrow_mut();
    let mut owned_dense = Vec::new();
    let mut owned_ready = Vec::new();
    let mut tints = Vec::new();
    let mut container_shapes = Vec::new();
    if let Some(state) = &client.scene.engine {
        if !client.engine_cache_enabled
            || cache.as_ref().is_none_or(|c| c.revision != state.revision)
        {
            let rebuilds = cache.as_ref().map_or(1, |c| c.stats.rebuilds + 1);
            *cache = Some(EngineCache::build(client, rebuilds));
        }
    } else {
        let zsize = (bounds.max[2] - bounds.min[2]) as usize;
        owned_ready = vec![false; (bounds.max[0] - bounds.min[0]) as usize * zsize];
        for x in bounds.min[0]..bounds.max[0] {
            for z in bounds.min[2]..bounds.max[2] {
                owned_ready[(x - bounds.min[0]) as usize * zsize + (z - bounds.min[2]) as usize] =
                    client.scene.inside([x, bounds.min[1], z]);
            }
        }
        owned_dense = vec![None; volume];
        tints = vec![None; volume];
        for p in &client.scene.cells {
            if client.scene.inside(*p) {
                owned_dense[index(bounds, *p)] = client.scene.kind_at(*p).map(Kind::Cube);
                tints[index(bounds, *p)] = client
                    .scene
                    .biome_codes
                    .get(p)
                    .and_then(|c| client.grass_tints.get(c))
                    .map(|t| t.rgb);
            }
        }
        container_shapes = ContainerBlock::all()
            .into_iter()
            .map(|k| {
                client
                    .scene
                    .geometry
                    .collision(&k.block([0, 0, 0]))
                    .expect("Measured geometry")
                    .to_vec()
            })
            .collect();
        for (p, k) in &client.scene.containers {
            if client.scene.inside(*p) {
                owned_dense[index(bounds, *p)] = Some(Kind::Container(*k));
            }
        }
    }
    let (dense, ready) = if client.scene.engine.is_some() {
        let c = cache.as_ref().unwrap();
        (&c.dense, &c.ready)
    } else {
        (&owned_dense, &owned_ready)
    };
    let mut pixels = vec![0; width * height];
    let origin = client.eye();
    let forward = client.direction();
    let (sy, cy) = (client.yaw as f64).sin_cos();
    let right = [-cy, 0.0, sy];
    let up = [
        -sy * forward[1],
        (client.pitch as f64).cos(),
        -cy * forward[1],
    ];
    let aspect = width as f64 / height as f64;
    let scale = (70.0_f64.to_radians() * 0.5).tan();
    let target = client
        .target()
        .ok()
        .flatten()
        .map(|h| [h.position.x, h.position.y, h.position.z]);
    for y in 0..height {
        for x in 0..width {
            let u = (2.0 * (x as f64 + 0.5) / width as f64 - 1.0) * scale * aspect;
            let v = (1.0 - 2.0 * (y as f64 + 0.5) / height as f64) * scale;
            let mut d = std::array::from_fn::<_, 3, _>(|a| forward[a] + right[a] * u + up[a] * v);
            let length = (d.iter().map(|n| n * n).sum::<f64>()).sqrt();
            d = d.map(|n| n / length);
            let sky = if client.scene.engine.is_some() {
                let horizon = (1.0 - d[1].max(0.0)).powi(3);
                std::array::from_fn(|a| {
                    [102., 158., 198.][a] * (1. - horizon) + [203., 218., 210.][a] * horizon
                })
            } else if d[1] > 0.0 {
                [24.0, 40.0, 61.0]
            } else {
                [15.0, 24.0, 34.0]
            };
            let mut color = sky;
            if let Some((p, t, axis, kind, face)) = trace(
                dense,
                ready,
                bounds,
                origin,
                d,
                &container_shapes,
                client.scene.engine.as_ref().map(|s| s.geometry.as_ref()),
            ) {
                let hit = std::array::from_fn::<_, 3, _>(|a| origin[a] + d[a] * t);
                let face_axes = match axis {
                    0 => [2, 1],
                    1 => [0, 2],
                    _ => [0, 1],
                };
                let a = hit[face_axes[0]].rem_euclid(1.0);
                let b = hit[face_axes[1]].rem_euclid(1.0);
                let edge = !(0.018..=0.982).contains(&a) || !(0.018..=0.982).contains(&b);
                let grain = ((hit[0] * 19.0).floor() as i32
                    ^ (hit[1] * 23.0).floor() as i32
                    ^ (hit[2] * 17.0).floor() as i32)
                    & 7;
                let shade = match axis {
                    0 => 0.78,
                    1 => 1.0,
                    _ => 0.62,
                };
                let base = if edge {
                    64.0
                } else {
                    140.0 + grain as f64 * 2.2
                };
                let cube = if let Kind::Cube(cube) = kind {
                    Some(cube)
                } else {
                    None
                };
                let material = cube
                    .and_then(|cube| client.cube_materials.get(&(cube, face)))
                    .or_else(|| {
                        if cube == Some(Cube::Stone) {
                            client.stone_material.as_ref()
                        } else {
                            None
                        }
                    });
                let rgb = if target == Some(p) && edge {
                    [80.0, 219.0, 190.0]
                } else if let Kind::Engine(id) = kind {
                    let state = client.scene.engine.as_ref().unwrap();
                    crate::engine_materials::color(client, id, face, a, b)
                        .unwrap_or_else(|| crate::engine::color(state.registry.get(id), face, a, b))
                        .map(|v| v * shade)
                } else if let Some(material) = material {
                    let texel = material
                        .texture
                        .sample_repeat(a, if axis == 1 { b } else { 1.0 - b });
                    let rgb = if cube == Some(Cube::Grass)
                        && face != 1
                        && let Some(tint) = tints[index(bounds, p)]
                    {
                        mcbe_core::grass::texel(texel, tint, face != 0)
                    } else {
                        [texel[0], texel[1], texel[2]]
                    };
                    rgb.map(|v| f64::from(v) * shade)
                } else if let Kind::Container(kind) = kind {
                    let color = match kind.kind() {
                        ContainerKind::Barrel => [164., 112., 60.],
                        ContainerKind::Dispenser => [130., 135., 143.],
                        ContainerKind::Hopper => [76., 84., 94.],
                        ContainerKind::Chest => [183., 122., 47.],
                    };
                    color.map(|v| v * shade)
                } else {
                    [base * shade, base * shade * 1.02, base * shade * 1.07]
                };
                let fog = (t / 80.0).clamp(0.0, 0.84);
                color = std::array::from_fn(|a| rgb[a] * (1.0 - fog) + sky[a] * fog);
            }
            pixels[y * width + x] =
                (color[0] as u32) << 16 | (color[1] as u32) << 8 | color[2] as u32;
        }
    }
    for i in 0..9 {
        if client.engine_assets.is_none() && !(3..6).contains(&i) {
            pixels[(height / 2) * width + width / 2 + i - 4] = 0xd8fff2;
            pixels[(height / 2 + i - 4) * width + width / 2] = 0xd8fff2;
        }
    }
    if client.scene.engine.is_some() {
        crate::engine_ui::hud(client, &mut pixels, width, height);
    }
    pixels
}
fn trace(
    dense: &[Option<Kind>],
    ready: &[bool],
    bounds: Bounds,
    origin: [f64; 3],
    d: [f64; 3],
    shapes: &[Vec<[f32; 6]>],
    geometry: Option<&crate::engine_geometry::Library>,
) -> Option<([i32; 3], f64, usize, Kind, usize)> {
    let mut p = origin.map(|n| n.floor() as i32);
    let delta = d.map(|n| {
        if n == 0.0 {
            f64::INFINITY
        } else {
            (1.0 / n).abs()
        }
    });
    let step = d.map(|n| if n < 0.0 { -1 } else { 1 });
    let mut next = std::array::from_fn::<_, 3, _>(|a| {
        if d[a] == 0.0 {
            f64::INFINITY
        } else {
            ((p[a] + i32::from(step[a] > 0)) as f64 - origin[a]) / d[a]
        }
    });
    let mut t = 0.0;
    let mut axis = 1;
    for _ in 0..160 {
        if !bounds.contains(p) {
            return None;
        }
        if !ready[(p[0] - bounds.min[0]) as usize * (bounds.max[2] - bounds.min[2]) as usize
            + (p[2] - bounds.min[2]) as usize]
        {
            return None;
        }
        if let Some(kind) = dense[index(bounds, p)] {
            match kind {
                Kind::Engine(id) if geometry.is_some_and(|g| !g.full(id)) => {
                    if let Some((distance, hit_axis, positive)) = geometry
                        .unwrap()
                        .boxes(id)
                        .iter()
                        .filter_map(|b| box_hit(p, *b, origin, d))
                        .filter(|h| h.0 >= t - 1e-7 && h.0 <= 64.)
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                    {
                        return Some((p, distance, hit_axis, kind, face_index(hit_axis, positive)));
                    }
                }
                Kind::Cube(_) | Kind::Engine(_) => {
                    return Some((p, t, axis, kind, face_index(axis, d[axis] < 0.)));
                }
                Kind::Container(container) => {
                    if let Some((distance, hit_axis, positive)) = shapes[container.index()]
                        .iter()
                        .filter_map(|b| box_hit(p, *b, origin, d))
                        .filter(|h| h.0 >= t - 1e-7 && h.0 <= 64.)
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                    {
                        return Some((p, distance, hit_axis, kind, face_index(hit_axis, positive)));
                    }
                }
            }
        }
        axis = if next[0] < next[1] && next[0] < next[2] {
            0
        } else if next[1] < next[2] {
            1
        } else {
            2
        };
        t = next[axis];
        if t > 64.0 {
            return None;
        }
        next[axis] += delta[axis];
        p[axis] += step[axis];
    }
    None
}
pub(crate) fn face_index(axis: usize, positive: bool) -> usize {
    match axis {
        0 => {
            if positive {
                4
            } else {
                5
            }
        }
        1 => {
            if positive {
                0
            } else {
                1
            }
        }
        _ => {
            if positive {
                3
            } else {
                2
            }
        }
    }
}
pub(crate) fn box_hit(
    p: [i32; 3],
    b: [f32; 6],
    origin: [f64; 3],
    d: [f64; 3],
) -> Option<(f64, usize, bool)> {
    let (mut near, mut far) = (f64::NEG_INFINITY, f64::INFINITY);
    let (mut entry, mut exit) = ((0, false), (0, true));
    for a in 0..3 {
        let min = f64::from(p[a] as f32 + b[a]);
        let max = f64::from(p[a] as f32 + b[a + 3]);
        if d[a] == 0. {
            if origin[a] < min || origin[a] > max {
                return None;
            }
            continue;
        }
        let (mut lo, mut hi) = ((min - origin[a]) / d[a], (max - origin[a]) / d[a]);
        let (mut left, mut right) = ((a, false), (a, true));
        if lo > hi {
            std::mem::swap(&mut lo, &mut hi);
            std::mem::swap(&mut left, &mut right);
        }
        if lo > near {
            near = lo;
            entry = left;
        }
        if hi < far {
            far = hi;
            exit = right;
        }
        if near > far {
            return None;
        }
    }
    if far < 0. {
        None
    } else if near >= 0. {
        Some((near, entry.0, entry.1))
    } else {
        Some((far, exit.0, exit.1))
    }
}
pub fn png(path: &Path, pixels: &[u32], width: usize, height: usize) -> std::io::Result<()> {
    fn crc(bytes: &[u8]) -> u32 {
        let mut c = !0u32;
        for b in bytes {
            c ^= *b as u32;
            for _ in 0..8 {
                c = (c >> 1) ^ if c & 1 != 0 { 0xedb88320 } else { 0 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, name: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(name);
        out.extend_from_slice(data);
        let check = crc(&out[start..]);
        out.extend_from_slice(&check.to_be_bytes());
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&(width as u32).to_be_bytes());
    header.extend_from_slice(&(height as u32).to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    for row in pixels.chunks(width) {
        z.write_all(&[0])?;
        for p in row {
            z.write_all(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8])?;
        }
    }
    chunk(&mut out, b"IDAT", &z.finish()?);
    chunk(&mut out, b"IEND", &[]);
    std::fs::write(path, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hopper_draw_boxes_leave_the_center_cavity_open() {
        let geometry = mcbe_core::container_geometry::Geometry::measured().unwrap();
        let block = ContainerBlock::HOPPER.block([0, 0, 0]);
        let hit = geometry
            .collision(&block)
            .unwrap()
            .iter()
            .filter_map(|b| box_hit([0, 0, 0], *b, [0.5, 2., 0.5], [0., -1., 0.]))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap();
        assert_eq!(hit, (1.3125, 1, true));
        assert_eq!(face_index(hit.1, hit.2), 0);
    }
}
