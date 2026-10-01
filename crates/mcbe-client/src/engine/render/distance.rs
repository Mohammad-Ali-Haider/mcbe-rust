//! Stitched, progressively simplified terrain with matching voxel/tree overlap.
use crate::{
    engine_mesh::{self, Vertex},
    engine_stream::{self, Generator},
    scene::{Client, Result},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use wgpu::util::DeviceExt;
pub const MAX_VERTICES: usize = 3_145_728;
pub const FINE: u32 = 0x10000000;
pub const FIELD: u32 = 0x08000000;
pub const TREE: u32 = 0x02000000;
pub const OBJECT: u32 = 0x01000000;
const TRUNK: u32 = 0x00800000;
type Edits = Arc<HashMap<[i32; 3], Option<u16>>>;
type EditCache = (u64, u64, u64, Edits);
const CORE: i32 = 56;
const FINE_END: i32 = 256;
const FIELD_CORE: i32 = 128;
const TREES_END: i32 = 1536;
const FADE: Duration = Duration::from_millis(180);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    instance: u64,
    registry: u64,
    edits: u64,
    center: [i32; 2],
    radius: i32,
}
struct Request {
    epoch: u64,
    key: Key,
    generator: Arc<Generator>,
    geometry: Arc<crate::engine_geometry::Library>,
    edits: Edits,
    top: u16,
    side: u16,
    stone: u16,
    log: u16,
    leaf: u16,
}
struct Finished {
    epoch: u64,
    key: Key,
    mesh: std::result::Result<Option<Mesh>, String>,
}
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub leaves: usize,
    pub area: u64,
    pub trees: usize,
    pub objects: usize,
    pub stitched_edges: usize,
    pub fine_vertices: usize,
}
#[derive(Clone, Copy)]
struct Cell {
    x: i32,
    z: i32,
    size: i32,
}
struct Builder<'a> {
    request: &'a Request,
    token: &'a AtomicU64,
    view: [i32; 4],
    cells: Vec<Cell>,
    visited: usize,
}
impl Builder<'_> {
    fn node(&mut self, x: i32, z: i32, size: i32) -> bool {
        self.visited += 1;
        if self.visited.is_multiple_of(64)
            && self.token.load(Ordering::Acquire) != self.request.epoch
        {
            return false;
        }
        let [xmin, zmin, xmax, zmax] = self.view;
        let [cx, cz] = center(self.request.key);
        if x >= xmax || z >= zmax || x + size <= xmin || z + size <= zmin {
            return true;
        }
        if x >= cx - FIELD_CORE
            && z >= cz - FIELD_CORE
            && x + size <= cx + FIELD_CORE
            && z + size <= cz + FIELD_CORE
        {
            return true;
        }
        let cuts_view = x < xmin || z < zmin || x + size > xmax || z + size > zmax;
        let cuts_core = x < cx + FIELD_CORE
            && z < cz + FIELD_CORE
            && x + size > cx - FIELD_CORE
            && z + size > cz - FIELD_CORE;
        let distance = (x + size / 2 - cx).abs().max((z + size / 2 - cz).abs());
        let step = (distance / 32).clamp(2, 64);
        if size > 2 && (size > step || cuts_view || cuts_core) {
            let h = size / 2;
            for (dx, dz) in [(0, 0), (h, 0), (0, h), (h, h)] {
                if !self.node(x + dx, z + dz, h) {
                    return false;
                }
            }
        } else {
            self.cells.push(Cell { x, z, size });
        }
        true
    }
}
fn center(key: Key) -> [i32; 2] {
    key.center.map(|v| v * 16 + 8)
}
fn distance(key: Key, x: i32, z: i32) -> i32 {
    let [cx, cz] = center(key);
    (x - cx).abs().max((z - cz).abs())
}
fn bilinear(g: &Generator, x: f32, z: f32, step: i32) -> f32 {
    let x0 = (x.floor() as i32).div_euclid(step) * step;
    let z0 = (z.floor() as i32).div_euclid(step) * step;
    let u = (x - x0 as f32) / step as f32;
    let v = (z - z0 as f32) / step as f32;
    let h = |a, b| g.ground_height(a, b) as f32;
    (h(x0, z0) * (1. - u) + h(x0 + step, z0) * u) * (1. - v)
        + (h(x0, z0 + step) * (1. - u) + h(x0 + step, z0 + step) * u) * v
}
fn field_height(r: &Request, x: f32, z: f32) -> f32 {
    let [cx, cz] = center(r.key);
    let d = (x - cx as f32).abs().max((z - cz as f32).abs());
    let lod = (d / 32.).clamp(2., 64.).log2();
    let step = 1 << lod.floor() as i32;
    let t = lod.fract();
    let t = t * t * (3. - 2. * t);
    bilinear(&r.generator, x, z, step) * (1. - t)
        + bilinear(&r.generator, x, z, (step * 2).min(64)) * t
}
fn triangle(out: &mut Vec<Vertex>, points: [[f32; 3]; 3], id: u16, flags: u32) {
    for (i, p) in points.into_iter().enumerate() {
        let mut bary = [0.; 3];
        bary[i] = 1.;
        out.push(Vertex {
            position: p,
            uv: [p[2], 1. - p[0]],
            layer: (u32::from(id) * 6) | flags,
            shade: 1.,
            barycentric: bary,
        });
    }
}
fn field(r: &Request, cells: &[Cell], mesh: &mut Mesh) -> Result<()> {
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
    for c in cells {
        let mut boundary = Vec::new();
        for z in vertical[&c.x].range(c.z..c.z + c.size) {
            boundary.push((c.x, *z));
        }
        for x in horizontal[&(c.z + c.size)].range(c.x..c.x + c.size) {
            boundary.push((*x, c.z + c.size));
        }
        for z in vertical[&(c.x + c.size)]
            .range(c.z + 1..=c.z + c.size)
            .rev()
        {
            boundary.push((c.x + c.size, *z));
        }
        for x in horizontal[&c.z].range(c.x + 1..=c.x + c.size).rev() {
            boundary.push((*x, c.z));
        }
        mesh.stitched_edges += boundary.len().saturating_sub(4);
        let point = |x: f32, z: f32| [x, field_height(r, x, z), z];
        let mid = point(
            c.x as f32 + c.size as f32 / 2.,
            c.z as f32 + c.size as f32 / 2.,
        );
        if boundary.len() == 4 {
            let corners: Vec<_> = boundary
                .iter()
                .map(|(x, z)| point(*x as f32, *z as f32))
                .collect();
            for indices in [[0, 1, 2], [0, 2, 3]] {
                triangle(
                    &mut mesh.vertices,
                    indices.map(|i| corners[i]),
                    r.top,
                    FIELD,
                );
            }
        } else {
            for i in 0..boundary.len() {
                let (ax, az) = boundary[i];
                let (bx, bz) = boundary[(i + 1) % boundary.len()];
                triangle(
                    &mut mesh.vertices,
                    [
                        mid,
                        point(ax as f32, az as f32),
                        point(bx as f32, bz as f32),
                    ],
                    r.top,
                    FIELD,
                );
            }
        }
        mesh.leaves += 1;
        mesh.area += c.size as u64 * c.size as u64;
        if mesh.vertices.len() > MAX_VERTICES {
            return Err("Distant field budget exceeded".into());
        }
    }
    Ok(())
}
fn observed(r: &Request, p: [i32; 3]) -> Option<u16> {
    r.edits
        .get(&p)
        .copied()
        .unwrap_or_else(|| r.generator.block(p))
}
fn ground(r: &Request, x: i32, z: i32) -> (i32, u16) {
    let mut h = r.generator.ground_height(x, z);
    while h > 1 && r.edits.get(&[x, h - 1, z]) == Some(&None) {
        h -= 1;
    }
    (h, observed(r, [x, h - 1, z]).unwrap_or(r.top))
}
fn surface(
    out: &mut Vec<Vertex>,
    p: [i32; 3],
    face: usize,
    id: u16,
    mut bounds: [f32; 6],
    flags: u32,
) {
    let axis = match face {
        0 | 1 => 1,
        2 | 3 => 2,
        _ => 0,
    };
    let plane = if [0, 3, 4].contains(&face) {
        bounds[axis + 3]
    } else {
        bounds[axis]
    };
    bounds[axis] = plane;
    bounds[axis + 3] = plane;
    engine_mesh::lod_surface(out, p, face, id, bounds, flags);
}
fn fine(r: &Request, view: [i32; 4], token: &AtomicU64, mesh: &mut Mesh) -> bool {
    let [cx, cz] = center(r.key);
    let xmin = view[0].max(cx - FINE_END);
    let zmin = view[1].max(cz - FINE_END);
    let xmax = view[2].min(cx + FINE_END);
    let zmax = view[3].min(cz + FINE_END);
    let width = (xmax - xmin + 2) as usize;
    let depth = (zmax - zmin + 2) as usize;
    let mut heights = Vec::with_capacity(width * depth);
    for z in zmin - 1..=zmax {
        for x in xmin - 1..=xmax {
            heights.push(ground(r, x, z));
        }
    }
    let at = |x: i32, z: i32| heights[((z - zmin + 1) as usize) * width + (x - xmin + 1) as usize];
    for z in zmin..zmax {
        if token.load(Ordering::Acquire) != r.epoch {
            return false;
        }
        let mut x = xmin;
        while x < xmax {
            if distance(r.key, x, z) < CORE {
                x += 1;
                continue;
            }
            let (h, id) = at(x, z);
            let mut end = x + 1;
            // A morphed top must share every integer edge vertex with its
            // risers. Merging these faces would introduce curved T-junctions.
            while end < xmax
                && distance(r.key, x, z) < 127
                && distance(r.key, end, z) < 127
                && distance(r.key, end, z) >= CORE
                && at(end, z) == (h, id)
            {
                end += 1;
            }
            surface(
                &mut mesh.vertices,
                [x, h - 1, z],
                0,
                id,
                [0., 0., 0., (end - x) as f32, 1., 1.],
                FINE,
            );
            x = end;
        }
        for x in xmin..xmax {
            if distance(r.key, x, z) < CORE {
                continue;
            }
            let (h, top) = at(x, z);
            for (face, dx, dz) in [(2, 0, -1), (3, 0, 1), (4, 1, 0), (5, -1, 0)] {
                let (other, _) = at(x + dx, z + dz);
                if other >= h {
                    continue;
                }
                let mut y = other;
                while y < h {
                    let (id, end) = if y == h - 1 {
                        (top, h)
                    } else if y >= h - 3 {
                        (r.side, h - 1)
                    } else {
                        (r.stone, h - 3)
                    };
                    surface(
                        &mut mesh.vertices,
                        [x, y, z],
                        face,
                        id,
                        [0., 0., 0., 1., (end - y) as f32, 1.],
                        FINE,
                    );
                    y = end;
                }
            }
        }
    }
    // Collapse voxel risers gradually onto the same height function used by
    // the stitched field before the final coverage handoff. This avoids the
    // sky pinholes that dithering two differently shaped surfaces can expose.
    for v in &mut mesh.vertices {
        let [x, _, z] = v.position;
        let d = (x - cx as f32).abs().max((z - cz as f32).abs());
        let t = ((d - 128.) / 128.).clamp(0., 1.);
        let t = t * t * (3. - 2. * t);
        if t > 0. {
            v.position[1] += (field_height(r, x, z) - v.position[1]) * t;
        }
    }
    mesh.fine_vertices = mesh.vertices.len();
    true
}
fn cuboid(out: &mut Vec<Vertex>, p: [i32; 3], size: [f32; 3], id: u16, flags: u32) {
    for face in 0..6 {
        surface(
            out,
            p,
            face,
            id,
            [0., 0., 0., size[0], size[1], size[2]],
            flags,
        );
    }
}
fn objects(r: &Request, mesh: &mut Mesh) {
    let mut items: BTreeMap<_, _> = r.generator.structures().collect();
    for (p, id) in r.edits.iter() {
        match id {
            Some(id) => {
                if p[1] >= r.generator.ground_height(p[0], p[2]) {
                    items.insert(*p, *id);
                }
            }
            None => {
                items.remove(p);
            }
        }
    }
    let mut items: Vec<_> = items
        .into_iter()
        .filter(|(p, _)| {
            distance(r.key, p[0], p[2]) >= CORE && distance(r.key, p[0], p[2]) < TREES_END
        })
        .collect();
    items.sort_by_key(|(p, _)| (distance(r.key, p[0], p[2]), *p));
    for (p, id) in items {
        if r.geometry.layer(id) == crate::engine_blocks::RenderLayer::Translucent {
            continue;
        }
        let mut vertices = Vec::new();
        for (face, d) in engine_mesh::DIRECTIONS.iter().enumerate() {
            let neighbor = observed(r, std::array::from_fn(|a| p[a] + d[a]));
            for s in &r.geometry.visible(id, neighbor, face).surfaces {
                engine_mesh::lod_surface(
                    &mut vertices,
                    p,
                    face,
                    id,
                    s.bounds,
                    OBJECT | r.geometry.flags(id),
                );
            }
        }
        if mesh.vertices.len() + vertices.len() > MAX_VERTICES / 2 {
            break;
        }
        mesh.vertices.extend(vertices);
        mesh.objects += 1;
    }
}
fn trees(r: &Request, view: [i32; 4], token: &AtomicU64, mesh: &mut Mesh) -> bool {
    // Compress the generator's exact 21-column canopy union, preserving its outline.
    let mut registry = crate::engine_blocks::Registry::built_in();
    registry.blocks.truncate(1);
    registry.blocks[0].placement = None;
    registry.blocks[0].geometry = Some(crate::engine_geometry::Geometry {
        boxes: vec![
            [0.2, 0., 0., 0.8, 1., 1.],
            [0., 0., 0.2, 0.2, 1., 0.8],
            [0.8, 0., 0.2, 1., 1., 0.8],
        ],
    });
    let canopy = crate::engine_geometry::Library::compile(&registry).expect("Canopy model");
    let [cx, cz] = center(r.key);
    let radius = (r.key.radius + 1).min(TREES_END / 16 + 1);
    let mut candidates = Vec::new();
    for x in r.key.center[0] - radius..=r.key.center[0] + radius {
        for z in r.key.center[1] - radius..=r.key.center[1] + radius {
            if let Some(p) = r.generator.tree([x, z]) {
                let dist = (p[0] - cx).abs().max((p[2] - cz).abs());
                if (CORE - 4..TREES_END).contains(&dist)
                    && p[0] >= view[0] - 3
                    && p[0] < view[2] + 3
                    && p[2] >= view[1] - 3
                    && p[2] < view[3] + 3
                    && engine_stream::BOUNDS.contains([p[0], 0, p[2]])
                {
                    candidates.push((dist, p));
                }
            }
        }
    }
    candidates.sort();
    for (i, (dist, [x, y, z])) in candidates.into_iter().enumerate() {
        if i.is_multiple_of(64) && token.load(Ordering::Acquire) != r.epoch {
            return false;
        }
        let mut vertices = Vec::new();
        if dist < 640 {
            cuboid(&mut vertices, [x, y, z], [1., 2., 1.], r.log, TREE | TRUNK);
            let t = ((dist as f32 - 384.) / 256.).clamp(0., 1.);
            let t = t * t * (3. - 2. * t);
            for face in 0..6 {
                for s in &canopy.visible(0, None, face).surfaces {
                    let mut bounds = s.bounds;
                    for a in 0..3 {
                        let scale = if a == 1 { 3. } else { 5. };
                        bounds[a] *= scale;
                        bounds[a + 3] *= scale;
                        if a != 1 {
                            for j in [a, a + 3] {
                                if (bounds[j] - 1.).abs() < 0.001 {
                                    bounds[j] = 1. - t;
                                } else if (bounds[j] - 4.).abs() < 0.001 {
                                    bounds[j] = 4. + t;
                                }
                            }
                        }
                    }
                    engine_mesh::lod_surface(
                        &mut vertices,
                        [x - 2, y + 2, z - 2],
                        face,
                        r.leaf,
                        bounds,
                        TREE,
                    );
                }
            }
        } else {
            cuboid(
                &mut vertices,
                [x - 2, y + 2, z - 2],
                [5., 3., 5.],
                r.leaf,
                TREE,
            );
        }
        if mesh.vertices.len() + vertices.len() > MAX_VERTICES {
            break;
        }
        mesh.vertices.extend(vertices);
        mesh.trees += 1;
    }
    true
}
fn build(r: &Request, token: &AtomicU64) -> Result<Option<Mesh>> {
    let [x, z] = r.key.center;
    let radius = r.key.radius;
    let view = [
        ((x - radius) * 16).max(-8192),
        ((z - radius) * 16).max(-8192),
        ((x + radius + 1) * 16).min(8192),
        ((z + radius + 1) * 16).min(8192),
    ];
    let mut mesh = Mesh {
        vertices: Vec::new(),
        leaves: 0,
        area: 0,
        trees: 0,
        objects: 0,
        stitched_edges: 0,
        fine_vertices: 0,
    };
    if !fine(r, view, token, &mut mesh) {
        return Ok(None);
    }
    let mut b = Builder {
        request: r,
        token,
        view,
        cells: Vec::new(),
        visited: 0,
    };
    if !b.node(-8192, -8192, 16384) {
        return Ok(None);
    }
    field(r, &b.cells, &mut mesh)?;
    objects(r, &mut mesh);
    if !trees(r, view, token, &mut mesh) {
        return Ok(None);
    }
    if mesh.vertices.len() > MAX_VERTICES {
        return Err("Distant terrain budget exceeded".into());
    }
    if token.load(Ordering::Acquire) == r.epoch {
        Ok(Some(mesh))
    } else {
        Ok(None)
    }
}
struct Worker {
    sender: mpsc::SyncSender<Option<Request>>,
    receiver: mpsc::Receiver<Finished>,
    token: Arc<AtomicU64>,
    handle: Option<thread::JoinHandle<()>>,
}
impl Worker {
    fn new() -> Result<Self> {
        let (sender, jobs) = mpsc::sync_channel::<Option<Request>>(1);
        let (done, receiver) = mpsc::sync_channel(1);
        let token = Arc::new(AtomicU64::new(0));
        let active = token.clone();
        let handle = thread::Builder::new()
            .name("blockwork-distant-terrain".into())
            .spawn(move || {
                while let Ok(Some(r)) = jobs.recv() {
                    let result = build(&r, &active).map_err(|e| e.to_string());
                    if done
                        .send(Finished {
                            epoch: r.epoch,
                            key: r.key,
                            mesh: result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self {
            sender,
            receiver,
            token,
            handle: Some(handle),
        })
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.token.store(u64::MAX, Ordering::Release);
        let _ = self.sender.send(None);
        // Make room if the worker is publishing its one completed result.
        let _ = self.receiver.try_recv();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
#[derive(Default)]
pub struct Distant {
    worker: Option<Worker>,
    wanted: Option<Key>,
    adopted: Option<Key>,
    epoch: u64,
    busy: bool,
    edits_cache: Option<EditCache>,
    fade_started: Option<Instant>,
    previous_key: Option<Key>,
    pub buffer: Option<wgpu::Buffer>,
    pub previous_buffer: Option<wgpu::Buffer>,
    pub previous_vertices: u32,
    pub vertices: u32,
    pub jobs: usize,
    pub discarded: usize,
    pub leaves: usize,
    pub area: u64,
    pub trees: usize,
    pub objects: usize,
    pub stitched_edges: usize,
    pub fine_vertices: usize,
    pub retained_updates: usize,
    pub crossfades: usize,
}
impl Distant {
    fn clear(&mut self) {
        self.buffer = None;
        self.previous_buffer = None;
        self.vertices = 0;
        self.previous_vertices = 0;
        self.leaves = 0;
        self.area = 0;
        self.trees = 0;
        self.objects = 0;
        self.stitched_edges = 0;
        self.fine_vertices = 0;
        self.adopted = None;
        self.previous_key = None;
        self.fade_started = None;
    }
    fn fraction(&self) -> f32 {
        self.fade_started.map_or(1., |t| {
            (t.elapsed().as_secs_f32() / FADE.as_secs_f32()).min(1.)
        })
    }
    pub fn blend(&self) -> ([f32; 4], [f32; 4]) {
        let Some(key) = self.adopted else {
            return ([0.; 4], [0.; 4]);
        };
        let [x, z] = center(key);
        let old = center(self.previous_key.unwrap_or(key));
        let t = self.fraction();
        let previous = self.previous_buffer.is_some();
        (
            [x as f32, z as f32, if previous { t } else { 1. }, 1.],
            [
                old[0] as f32,
                old[1] as f32,
                if previous { 1. } else { 0. },
                if previous { 1. } else { t },
            ],
        )
    }
    pub fn update(&mut self, client: &Client, device: &wgpu::Device) -> Result<bool> {
        if self.fraction() >= 1. {
            self.previous_buffer = None;
            self.previous_vertices = 0;
            self.previous_key = None;
        }
        let state = client.scene.engine.as_ref().unwrap();
        let key = state
            .stream
            .as_ref()
            .filter(|s| s.generator.config.radius > engine_stream::DETAIL_RADIUS)
            .map(|s| Key {
                instance: state.instance,
                registry: state.registry_revision,
                edits: state.edit_revision,
                center: s.center,
                radius: s.generator.config.radius,
            });
        if self.wanted != key {
            self.wanted = key;
            self.epoch += 1;
            if let Some(w) = &self.worker {
                w.token.store(self.epoch, Ordering::Release);
            }
            let retain = match (self.adopted, key) {
                (Some(old), Some(new)) => {
                    old.instance == new.instance
                        && old.registry == new.registry
                        && (0..2).all(|a| (old.center[a] - new.center[a]).abs() <= 1)
                }
                _ => false,
            };
            if retain {
                self.retained_updates += 1;
            } else {
                self.clear();
            }
        }
        if let Some(worker) = &self.worker {
            match worker.receiver.try_recv() {
                Ok(done) => {
                    self.busy = false;
                    if done.epoch == self.epoch && Some(done.key) == self.wanted {
                        if let Some(mesh) =
                            done.mesh.map_err(|e| format!("Distant terrain: {e}"))?
                        {
                            self.previous_buffer = self.buffer.take();
                            self.previous_vertices = self.vertices;
                            self.previous_key = self.adopted;
                            self.crossfades += usize::from(self.previous_buffer.is_some());
                            self.vertices = mesh.vertices.len() as u32;
                            self.leaves = mesh.leaves;
                            self.area = mesh.area;
                            self.trees = mesh.trees;
                            self.objects = mesh.objects;
                            self.stitched_edges = mesh.stitched_edges;
                            self.fine_vertices = mesh.fine_vertices;
                            self.buffer = (!mesh.vertices.is_empty()).then(|| {
                                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                    label: Some("stitched terrain and vegetation LOD"),
                                    contents: bytemuck::cast_slice(&mesh.vertices),
                                    usage: wgpu::BufferUsages::VERTEX,
                                })
                            });
                            self.adopted = self.wanted;
                            self.fade_started = Some(Instant::now());
                        }
                    } else {
                        self.discarded += 1;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("Distant terrain worker stopped".into());
                }
            }
        }
        if let Some(key) = key
            && !self.busy
            && self.adopted != self.wanted
        {
            if self.worker.is_none() {
                self.worker = Some(Worker::new()?);
            }
            let identity = (state.instance, state.registry_revision, state.edit_revision);
            if self
                .edits_cache
                .as_ref()
                .is_none_or(|(a, b, c, _)| (*a, *b, *c) != identity)
            {
                self.edits_cache = Some((
                    identity.0,
                    identity.1,
                    identity.2,
                    Arc::new(
                        state
                            .stream
                            .as_ref()
                            .unwrap()
                            .edits
                            .iter()
                            .map(|(p, id)| (*p, *id))
                            .collect(),
                    ),
                ));
            }
            let worker = self.worker.as_ref().unwrap();
            worker.token.store(self.epoch, Ordering::Release);
            worker.sender.send(Some(Request {
                epoch: self.epoch,
                key,
                generator: state.stream.as_ref().unwrap().generator.clone(),
                geometry: state.geometry.clone(),
                edits: self.edits_cache.as_ref().unwrap().3.clone(),
                top: state.registry.find("meadow")?,
                side: state.registry.find("earth")?,
                stone: state.registry.find("slate")?,
                log: state
                    .registry
                    .find("oak_log")
                    .or_else(|_| state.registry.find("clay"))?,
                leaf: state.registry.find("moss")?,
            }))?;
            self.busy = true;
            self.jobs += 1;
        }
        Ok(key.is_none() || self.adopted == key && self.fraction() >= 1.)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request(center: [i32; 2]) -> Request {
        let r = crate::engine_blocks::Registry::built_in();
        let mut config = engine_stream::Config::new(&r);
        config.radius = 255;
        Request {
            epoch: 1,
            key: Key {
                instance: 1,
                registry: 0,
                edits: 0,
                center,
                radius: 255,
            },
            generator: Arc::new(Generator::new(360036, &r, config).unwrap()),
            geometry: Arc::new(crate::engine_geometry::Library::compile(&r).unwrap()),
            edits: Arc::new(HashMap::new()),
            top: r.find("meadow").unwrap(),
            side: r.find("earth").unwrap(),
            stone: r.find("slate").unwrap(),
            log: r.find("oak_log").unwrap(),
            leaf: r.find("moss").unwrap(),
        }
    }
    #[test]
    fn maximum_range_has_stitched_heights_fine_overlap_and_distant_trees() {
        for center in [[0, 0], [-1, -1], [511, 511], [-512, -512]] {
            let r = request(center);
            let m = build(&r, &AtomicU64::new(1)).unwrap().unwrap();
            assert!(m.vertices.len() <= MAX_VERTICES);
            assert!(m.trees > 100 && m.fine_vertices > 0 && m.stitched_edges > 0);
            let [cx, cz] = super::center(r.key);
            let side =
                |c: i32, extent: i32| ((c + extent).min(8192) - (c - extent).max(-8192)) as u64;
            let view_x = ((center[0] + 256) * 16).min(8192) - ((center[0] - 255) * 16).max(-8192);
            let view_z = ((center[1] + 256) * 16).min(8192) - ((center[1] - 255) * 16).max(-8192);
            assert_eq!(
                m.area,
                view_x as u64 * view_z as u64 - side(cx, FIELD_CORE) * side(cz, FIELD_CORE)
            );
            let mut heights = HashMap::new();
            for v in m.vertices.iter().filter(|v| v.layer & FINE != 0) {
                let [x, y, z] = v.position;
                let d = (x - cx as f32).abs().max((z - cz as f32).abs());
                if d >= 256. {
                    assert!((y - field_height(&r, x, z)).abs() < 0.00001);
                } else if d <= 128. {
                    assert_eq!(y.fract(), 0.);
                }
            }
            for v in m.vertices.iter().filter(|v| v.layer & FIELD != 0) {
                assert!(
                    v.position[0] >= -8192.
                        && v.position[0] <= 8192.
                        && v.position[2] >= -8192.
                        && v.position[2] <= 8192.
                );
                let key = (v.position[0].to_bits(), v.position[2].to_bits());
                if let Some(h) = heights.insert(key, v.position[1].to_bits()) {
                    assert_eq!(h, v.position[1].to_bits());
                }
            }
            for tri in m
                .vertices
                .chunks_exact(3)
                .filter(|t| t[0].layer & FIELD != 0)
            {
                let a = glam::Vec3::from(tri[1].position) - glam::Vec3::from(tri[0].position);
                let b = glam::Vec3::from(tri[2].position) - glam::Vec3::from(tri[0].position);
                assert!(a.cross(b).y > 0.);
            }
            for tri in m
                .vertices
                .chunks_exact(3)
                .filter(|t| t[0].layer & FINE != 0 && (t[0].layer & 0x007fffff) % 6 == 0)
            {
                if tri.iter().any(|v| {
                    (v.position[0] - cx as f32)
                        .abs()
                        .max((v.position[2] - cz as f32).abs())
                        > 128.
                }) {
                    let xmin = tri.iter().map(|v| v.position[0]).fold(f32::MAX, f32::min);
                    let xmax = tri.iter().map(|v| v.position[0]).fold(f32::MIN, f32::max);
                    assert!(xmax - xmin <= 1., "Morphed top skips riser edge vertices");
                }
            }
        }
    }
    #[test]
    fn superseded_builds_do_not_publish() {
        assert!(
            build(&request([0, 0]), &AtomicU64::new(2))
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn fine_ground_samples_match_generator_and_edits() {
        let mut r = request([0, 0]);
        for x in 80..128 {
            for z in 80..128 {
                let (h, id) = ground(&r, x, z);
                assert_eq!(observed(&r, [x, h - 1, z]), Some(id));
                assert_eq!(r.generator.ground_height(x, z), h);
            }
        }
        let h = r.generator.ground_height(100, 100);
        let mut edits = HashMap::new();
        edits.insert([100, h - 1, 100], None);
        r.edits = Arc::new(edits);
        assert_eq!(ground(&r, 100, 100).0, h - 1);
        assert_eq!(ground(&r, 100, 100).1, r.side);
    }
}
