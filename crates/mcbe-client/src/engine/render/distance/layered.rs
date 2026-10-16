//! Multi-surface LOD: stitched floor, underside and ceiling fields with clipped voids.
use super::*;
use crate::engine::world::terrain::{Sample, Terrain};
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layer {
    Floor,
    Bottom,
    Ceiling,
    Roof,
    Water,
}
impl Layer {
    fn raw(self, s: Sample) -> Option<f32> {
        match self {
            Self::Floor => Some(s.height as f32),
            Self::Bottom => s.column.bottom.map(|v| v as f32),
            Self::Ceiling => s.column.ceiling.map(|v| v as f32),
            Self::Roof => s.column.roof_top.map(|v| v as f32),
            Self::Water => s.water.map(|v| v as f32),
        }
    }
    fn face(self) -> usize {
        if matches!(self, Self::Bottom | Self::Ceiling) {
            1
        } else {
            0
        }
    }
    fn opposite(self) -> Self {
        match self {
            Self::Floor => Self::Bottom,
            Self::Ceiling => Self::Roof,
            _ => Self::Floor,
        }
    }
}
struct Sampler<'a> {
    terrain: &'a Terrain,
    cache: HashMap<[i32; 2], Sample>,
    key: Key,
}
impl Sampler<'_> {
    fn sample(&mut self, x: i32, z: i32) -> Sample {
        *self
            .cache
            .entry([x, z])
            .or_insert_with(|| self.terrain.sample(x, z))
    }
    fn value(&mut self, layer: Option<Layer>, x: i32, z: i32) -> f32 {
        let s = self.sample(x, z);
        match layer {
            Some(v) => v.raw(s).unwrap_or(s.height as f32),
            None => s.column.coverage.clamp(-1., 1.) as f32,
        }
    }
    fn bilinear(&mut self, layer: Option<Layer>, x: f32, z: f32, step: i32) -> f32 {
        let x0 = (x.floor() as i32).div_euclid(step) * step;
        let z0 = (z.floor() as i32).div_euclid(step) * step;
        let u = (x - x0 as f32) / step as f32;
        let v = (z - z0 as f32) / step as f32;
        let a = self.value(layer, x0, z0);
        let b = self.value(layer, x0 + step, z0);
        let c = self.value(layer, x0, z0 + step);
        let d = self.value(layer, x0 + step, z0 + step);
        (a * (1. - u) + b * u) * (1. - v) + (c * (1. - u) + d * u) * v
    }
    fn field(&mut self, layer: Option<Layer>, x: f32, z: f32) -> f32 {
        let [cx, cz] = center(self.key);
        let d = (x - cx as f32).abs().max((z - cz as f32).abs());
        let lod = (d / 32.).clamp(1., 64.).log2();
        let step = 1 << lod.floor() as i32;
        let t = smooth(lod.fract());
        let a = self.bilinear(layer, x, z, step);
        let b = self.bilinear(layer, x, z, (step * 2).min(64));
        a * (1. - t) + b * t
    }
    fn point(&mut self, layer: Layer, c: Cell, x: f32, z: f32) -> Point {
        let base = self.sample(c.x, c.z);
        let [cx, cz] = center(self.key);
        let d = (x - cx as f32).abs().max((z - cz as f32).abs());
        let near = smooth(((d - 112.) / 16.).clamp(0., 1.));
        let far = smooth(((d - 128.) / 128.).clamp(0., 1.));
        let height = |s: &mut Self, l: Layer| {
            let a = l.raw(base).unwrap_or(base.height as f32);
            let b = s.bilinear(Some(l), x, z, 1);
            let f = s.field(Some(l), x, z);
            (a * (1. - near) + b * near) * (1. - far) + f * far
        };
        let y = height(self, layer);
        let other = height(self, layer.opposite());
        let a = if base.column.present { 1. } else { -1. };
        let b = self.bilinear(None, x, z, 1);
        let f = self.field(None, x, z);
        let mut coverage = (a * (1. - near) + b * near) * (1. - far) + f * far;
        if layer == Layer::Water {
            let a = if base.water.is_some() { 1. } else { -1. };
            // Water shores follow the same ground field, rather than covering dry land.
            coverage = coverage.min(
                (y - self.field(Some(Layer::Floor), x, z))
                    .clamp(-1., 1.)
                    .max(a * (1. - near) - near),
            );
        }
        Point {
            position: [x, y, z],
            other,
            coverage,
        }
    }
}
fn smooth(t: f32) -> f32 {
    t * t * (3. - 2. * t)
}
#[derive(Clone, Copy)]
struct Point {
    position: [f32; 3],
    other: f32,
    coverage: f32,
}
fn crossing(mut a: Point, mut b: Point) -> Point {
    // Shared edges use an identical arithmetic order, including from the other cell.
    if a.position[0] > b.position[0]
        || (a.position[0] == b.position[0] && a.position[2] > b.position[2])
    {
        std::mem::swap(&mut a, &mut b);
    }
    let t = (a.coverage / (a.coverage - b.coverage)).clamp(0., 1.);
    Point {
        position: std::array::from_fn(|i| a.position[i] + (b.position[i] - a.position[i]) * t),
        other: a.other + (b.other - a.other) * t,
        coverage: 0.,
    }
}
fn clip(input: &[Point]) -> Vec<Point> {
    let mut out = Vec::with_capacity(input.len() + 2);
    for i in 0..input.len() {
        let a = input[i];
        let b = input[(i + 1) % input.len()];
        if a.coverage >= 0. {
            out.push(a);
        }
        if (a.coverage >= 0.) != (b.coverage >= 0.) {
            out.push(crossing(a, b));
        }
    }
    let same = |a: &Point, b: &Point| {
        (0..3).all(|i| (a.position[i] - b.position[i]).abs() < 1e-6)
            && (a.other - b.other).abs() < 1e-6
    };
    out.dedup_by(|a, b| same(a, b));
    if out.len() > 1 && same(&out[0], out.last().unwrap()) {
        out.pop();
    }
    out
}
fn tri(out: &mut Vec<Vertex>, p: [[f32; 3]; 3], id: u16, face: usize, flags: u32) {
    let shade = match face {
        0 => 1.,
        1 => 0.5,
        2 | 3 => 0.8,
        _ => 0.6,
    };
    for (i, position) in p.into_iter().enumerate() {
        let mut bary = [0.; 3];
        bary[i] = 1.;
        let uv = match face {
            0 | 1 => [position[2], 1. - position[0]],
            2 | 3 => [position[0], 1. - position[1]],
            _ => [position[2], 1. - position[1]],
        };
        out.push(Vertex {
            position,
            uv,
            layer: (u32::from(id) * 6 + face as u32) | flags,
            shade,
            barycentric: bary,
        });
    }
}
fn wall(out: &mut Vec<Vertex>, a: Point, b: Point, id: u16, flags: u32) {
    let low_a = [a.position[0], a.other, a.position[2]];
    let low_b = [b.position[0], b.other, b.position[2]];
    if (a.position[1] - a.other).abs() + (b.position[1] - b.other).abs() < 0.001 {
        return;
    }
    let dx = b.position[0] - a.position[0];
    let dz = b.position[2] - a.position[2];
    let face = if dx.abs() > dz.abs() {
        if dx > 0. { 3 } else { 2 }
    } else if dz > 0. {
        5
    } else {
        4
    };
    tri(out, [a.position, low_a, low_b], id, face, flags);
    tri(out, [a.position, low_b, b.position], id, face, flags);
}
fn polygon(out: &mut Vec<Vertex>, input: &[Point], layer: Layer, id: u16, flags: u32, close: bool) {
    let p = clip(input);
    if p.len() < 3 {
        return;
    }
    for i in 1..p.len() - 1 {
        let v = if layer.face() == 0 {
            [p[0].position, p[i].position, p[i + 1].position]
        } else {
            [p[0].position, p[i + 1].position, p[i].position]
        };
        tri(out, v, id, layer.face(), flags);
    }
    if close {
        for i in 0..p.len() {
            let a = p[i];
            let b = p[(i + 1) % p.len()];
            if a.coverage.abs() < 1e-6 && b.coverage.abs() < 1e-6 {
                wall(out, a, b, id, flags);
            }
        }
    }
}
fn intervals(s: Sample) -> Vec<(i32, i32)> {
    if !s.column.present {
        return vec![];
    }
    let mut result = vec![(s.column.bottom.unwrap_or(i32::MIN + 32), s.height)];
    if let (Some(a), Some(b)) = (s.column.ceiling, s.column.roof_top) {
        result.push((a, b));
    }
    result
}
fn subtract(mut ranges: Vec<(i32, i32)>, occluders: &[(i32, i32)]) -> Vec<(i32, i32)> {
    for &(a, b) in occluders {
        let mut next = Vec::new();
        for (lo, hi) in ranges {
            if b <= lo || a >= hi {
                next.push((lo, hi));
            } else {
                if lo < a {
                    next.push((lo, a));
                }
                if hi > b {
                    next.push((b, hi));
                }
            }
        }
        ranges = next;
    }
    ranges
}
fn material(t: &Terrain, s: Sample, l: Layer, x: i32, z: i32) -> u16 {
    if l == Layer::Water {
        return t.water;
    }
    let y = match l {
        Layer::Floor => s.height - 1,
        Layer::Bottom => s.column.bottom.unwrap_or(s.height),
        Layer::Ceiling => s.column.ceiling.unwrap_or(s.height),
        Layer::Roof => s.column.roof_top.unwrap_or(s.height) - 1,
        Layer::Water => unreachable!(),
    };
    t.block([x, y, z], s, None).unwrap_or(t.solid_surface(s).1)
}
fn sides(r: &Request, sampler: &mut Sampler<'_>, c: Cell, mesh: &mut Mesh) {
    if c.size != 1 {
        return;
    }
    let s = sampler.sample(c.x, c.z);
    if !s.column.present {
        return;
    }
    let flags = FINE | FIELD;
    for (dx, dz, a, b) in [
        (
            -1,
            0,
            [c.x as f32, c.z as f32],
            [c.x as f32, c.z as f32 + 1.],
        ),
        (
            0,
            1,
            [c.x as f32, c.z as f32 + 1.],
            [c.x as f32 + 1., c.z as f32 + 1.],
        ),
        (
            1,
            0,
            [c.x as f32 + 1., c.z as f32 + 1.],
            [c.x as f32 + 1., c.z as f32],
        ),
        (
            0,
            -1,
            [c.x as f32 + 1., c.z as f32],
            [c.x as f32, c.z as f32],
        ),
    ] {
        let n = sampler.sample(c.x + dx, c.z + dz);
        let visible = subtract(intervals(s), &intervals(n));
        for (lo, hi) in visible {
            let lo = lo.max(s.height - 128);
            if hi <= lo {
                continue;
            }
            let classify = |y: i32| {
                if Some(y) == s.column.bottom || Some(y) == n.column.bottom {
                    Layer::Bottom
                } else if y == s.height || y == n.height {
                    Layer::Floor
                } else if Some(y) == s.column.roof_top || Some(y) == n.column.roof_top {
                    Layer::Roof
                } else {
                    Layer::Ceiling
                }
            };
            let top_layer = classify(hi);
            let low_layer = classify(lo);
            let mut pa = sampler.point(top_layer, c, a[0], a[1]);
            let mut pb = sampler.point(top_layer, c, b[0], b[1]);
            let mut la = sampler.point(low_layer, c, a[0], a[1]);
            let mut lb = sampler.point(low_layer, c, b[0], b[1]);
            // Exact voxel endpoints before the common continuous morph window.
            for (p, raw, layer) in [
                (&mut pa, hi, top_layer),
                (&mut pb, hi, top_layer),
                (&mut la, lo, low_layer),
                (&mut lb, lo, low_layer),
            ] {
                let [cx, cz] = center(r.key);
                let d = (p.position[0] - cx as f32)
                    .abs()
                    .max((p.position[2] - cz as f32).abs());
                let t = smooth(((d - 112.) / 16.).clamp(0., 1.));
                p.position[1] += (raw as f32 - layer.raw(s).unwrap_or(s.height as f32)) * (1. - t);
            }
            pa.other = la.position[1];
            pb.other = lb.position[1];
            let id = sampler
                .terrain
                .block([c.x, hi - 1, c.z], s, None)
                .unwrap_or(r.stone);
            let segment = if pa.coverage >= 0. && pb.coverage >= 0. {
                Some((pa, pb))
            } else if pa.coverage < 0. && pb.coverage < 0. {
                None
            } else if pa.coverage >= 0. {
                Some((pa, crossing(pa, pb)))
            } else {
                Some((crossing(pa, pb), pb))
            };
            if let Some((a, b)) = segment {
                wall(&mut mesh.vertices, a, b, id, flags | r.geometry.flags(id));
            }
        }
    }
}
pub(super) fn build(
    r: &Request,
    token: &AtomicU64,
    cells: &[Cell],
    mesh: &mut Mesh,
) -> Result<bool> {
    let terrain = r.generator.terrain.as_ref().unwrap();
    let mut sampler = Sampler {
        terrain,
        cache: HashMap::new(),
        key: r.key,
    };
    let mut vertical: BTreeMap<i32, BTreeSet<i32>> = BTreeMap::new();
    let mut horizontal: BTreeMap<i32, BTreeSet<i32>> = BTreeMap::new();
    for c in cells {
        for x in [c.x, c.x + c.size] {
            for z in [c.z, c.z + c.size] {
                vertical.entry(x).or_default().insert(z);
                horizontal.entry(z).or_default().insert(x);
            }
        }
    }
    for (i, c) in cells.iter().copied().enumerate() {
        if i.is_multiple_of(64) && token.load(Ordering::Acquire) != r.epoch {
            return Ok(false);
        }
        let mut boundary = Vec::new();
        for z in vertical[&c.x].range(c.z..c.z + c.size) {
            boundary.push([c.x as f32, *z as f32]);
        }
        for x in horizontal[&(c.z + c.size)].range(c.x..c.x + c.size) {
            boundary.push([*x as f32, (c.z + c.size) as f32]);
        }
        for z in vertical[&(c.x + c.size)]
            .range(c.z + 1..=c.z + c.size)
            .rev()
        {
            boundary.push([(c.x + c.size) as f32, *z as f32]);
        }
        for x in horizontal[&c.z].range(c.x + 1..=c.x + c.size).rev() {
            boundary.push([*x as f32, c.z as f32]);
        }
        mesh.stitched_edges += boundary.len().saturating_sub(4);
        let s = sampler.sample(c.x, c.z);
        for layer in [
            Layer::Floor,
            Layer::Bottom,
            Layer::Ceiling,
            Layer::Roof,
            Layer::Water,
        ] {
            if layer.raw(s).is_none() {
                continue;
            }
            let id = material(terrain, s, layer, c.x, c.z);
            let flags = FINE | FIELD | r.geometry.flags(id);
            let points: Vec<_> = boundary
                .iter()
                .map(|[x, z]| sampler.point(layer, c, *x, *z))
                .collect();
            let mid = sampler.point(
                layer,
                c,
                c.x as f32 + c.size as f32 / 2.,
                c.z as f32 + c.size as f32 / 2.,
            );
            let close =
                (layer == Layer::Floor && s.column.bottom.is_some()) || layer == Layer::Ceiling;
            let out = if layer == Layer::Water {
                &mut mesh.water
            } else {
                &mut mesh.vertices
            };
            if points.len() == 4 && points.iter().all(|p| p.coverage >= 0.) {
                polygon(
                    out,
                    &[points[0], points[1], points[2]],
                    layer,
                    id,
                    flags,
                    close,
                );
                polygon(
                    out,
                    &[points[0], points[2], points[3]],
                    layer,
                    id,
                    flags,
                    close,
                );
            } else {
                for j in 0..points.len() {
                    polygon(
                        out,
                        &[mid, points[j], points[(j + 1) % points.len()]],
                        layer,
                        id,
                        flags,
                        close,
                    );
                }
            }
        }
        sides(r, &mut sampler, c, mesh);
        mesh.leaves += 1;
        mesh.area += c.size as u64 * c.size as u64;
        if c.size == 1 {
            mesh.fine_vertices = mesh.vertices.len();
        }
        if mesh.vertices.len() + mesh.water.len() > MAX_VERTICES {
            return Err("Layered terrain LOD budget exceeded".into());
        }
    }
    Ok(true)
}
#[cfg(test)]
mod tests;
