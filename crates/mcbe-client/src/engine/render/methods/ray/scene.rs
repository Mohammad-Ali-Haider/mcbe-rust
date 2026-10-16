//! Incremental hardware BLAS/TLAS over actual worker-produced nearby triangle meshes.
use super::arena::Arena;
use crate::{
    engine::render::methods::{Quality, Rendering},
    engine_mesh::Vertex,
    scene::Result,
};
use std::collections::{BTreeMap, BTreeSet};
use wgpu::util::DeviceExt;
pub const MAX_INSTANCES: u32 = 4096;
pub const BUILD_BUDGET: usize = 2;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stamp {
    Generation(u64),
    Digest([u8; 32]),
}
pub struct Source<'a> {
    pub key: [i32; 3],
    pub opaque: Option<&'a wgpu::Buffer>,
    pub opaque_count: u32,
    pub transparent: &'a [Vertex],
    pub generation: u64,
    pub hashes: Option<[[u8; 32]; 2]>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Part {
    key: [i32; 3],
    transparent: bool,
}
struct Chunk {
    blas: wgpu::Blas,
    size: wgpu::BlasTriangleGeometrySizeDescriptor,
    base: u32,
    capacity: u32,
    stamp: Stamp,
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, serde::Serialize)]
pub struct Parameters {
    pub anchor: [f32; 4],
    pub center: [f32; 4],
    pub options: [f32; 4],
    pub limits: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Localize {
    range: [u32; 4],
    origin: [f32; 4],
}
pub struct Scene {
    pub data: wgpu::Buffer,
    pub uniform: wgpu::Buffer,
    pub package: wgpu::TlasPackage,
    pub layout: wgpu::BindGroupLayout,
    pub bind: wgpu::BindGroup,
    compute: wgpu::ComputePipeline,
    arena: Arena,
    chunks: BTreeMap<Part, Chunk>,
    source_key: Option<(u64, u64)>,
    anchor: [i32; 3],
    pub ready: bool,
    pub pending: usize,
    pub builds: usize,
    pub tlas_builds: usize,
    pub compactions: usize,
    pub effective_range: f32,
    pub params: Parameters,
    pub built_vertices: usize,
    pub last_build_ms: f64,
}
fn max_vertices(q: Quality) -> u32 {
    match q {
        Quality::Low => 262080,
        Quality::Balanced => 1048512,
        Quality::High => 2097024,
    }
}
pub fn radius(q: Quality) -> f32 {
    match q {
        Quality::Low => 24.,
        Quality::Balanced => 48.,
        Quality::High => 96.,
    }
}
fn distance(key: [i32; 3], eye: [f32; 3]) -> f32 {
    (0..3)
        .map(|i| {
            let lo = (key[i] * 16) as f32;
            let d = (lo - eye[i]).max(eye[i] - lo - 16.).max(0.);
            d * d
        })
        .sum()
}
impl Scene {
    pub fn new(device: &wgpu::Device, quality: Quality) -> Result<Self> {
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let arena = Arena::new(max_vertices(quality));
        let data = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bounded hardware ray vertex arena"),
            size: u64::from(arena.capacity()) * std::mem::size_of::<Vertex>() as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::BLAS_INPUT
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("hardware ray scene parameters"),
            contents: bytemuck::bytes_of(&Parameters {
                anchor: [0.; 4],
                center: [0.; 4],
                options: [0.; 4],
                limits: [0.; 4],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let package = wgpu::TlasPackage::new(device.create_tlas(&wgpu::CreateTlasDescriptor {
            label: Some("nearby voxel ray TLAS"),
            max_instances: MAX_INSTANCES,
            flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
            update_mode: wgpu::AccelerationStructureUpdateMode::Build,
        }));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hardware ray scene"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::AccelerationStructure {
                        vertex_return: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("active hardware ray scene"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: package.as_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: data.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("localize ray geometry"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../shaders/ray_localize.wgsl").into(),
            ),
        });
        let compute = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("chunk-local ray geometry"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(e) = pollster::block_on(device.pop_error_scope()) {
            return Err(format!("Ray scene validation: {e}").into());
        }
        Ok(Self {
            data,
            uniform,
            package,
            layout,
            bind,
            compute,
            arena,
            chunks: BTreeMap::new(),
            source_key: None,
            anchor: [0; 3],
            ready: false,
            pending: 0,
            builds: 0,
            tlas_builds: 0,
            compactions: 0,
            effective_range: 0.,
            params: Parameters {
                anchor: [0.; 4],
                center: [0.; 4],
                options: [0.; 4],
                limits: [0.; 4],
            },
            built_vertices: 0,
            last_build_ms: 0.,
        })
    }
    pub fn capacity_for(quality: Quality) -> u32 {
        max_vertices(quality)
    }
    pub fn capacity(&self) -> u32 {
        self.arena.capacity()
    }
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sources: &[Source<'_>],
        eye: [f32; 3],
        settings: &Rendering,
        key: (u64, u64),
    ) -> Result<()> {
        let started = std::time::Instant::now();
        let mut dirty = self.tlas_builds == 0;
        let anchor = eye.map(|v| (v.floor() as i32).div_euclid(16) * 16);
        if self.source_key != Some(key) {
            self.chunks.clear();
            self.arena = Arena::new(self.arena.capacity());
            self.source_key = Some(key);
            dirty = true;
        }
        if self.anchor != anchor {
            self.anchor = anchor;
            dirty = true;
        }
        let mut range = radius(settings.quality);
        let mut selected = Vec::new();
        loop {
            selected.clear();
            let mut total = 0u64;
            for (index, source) in sources.iter().enumerate() {
                let d = distance(source.key, eye);
                if d > range * range {
                    continue;
                }
                for transparent in [false, true] {
                    let count = if transparent {
                        source.transparent.len() as u32
                    } else {
                        source.opaque_count
                    };
                    if count == 0 {
                        continue;
                    }
                    total += u64::from(Arena::reserve(count));
                    let stamp = source
                        .hashes
                        .map_or(Stamp::Generation(source.generation), |h| {
                            Stamp::Digest(h[usize::from(transparent)])
                        });
                    selected.push((
                        Part {
                            key: source.key,
                            transparent,
                        },
                        index,
                        count,
                        stamp,
                        d,
                    ));
                }
            }
            if total <= u64::from(self.arena.capacity()) && selected.len() <= MAX_INSTANCES as usize
            {
                break;
            }
            range -= 4.;
            if range < 8. {
                selected.clear();
                range = 0.;
                break;
            }
        }
        selected.sort_by(|a, b| a.4.total_cmp(&b.4).then(a.0.cmp(&b.0)));
        let desired = selected.iter().map(|v| v.0).collect::<BTreeSet<_>>();
        let removed = self
            .chunks
            .keys()
            .filter(|k| !desired.contains(k))
            .copied()
            .collect::<Vec<_>>();
        for part in removed {
            let c = self.chunks.remove(&part).unwrap();
            self.arena.release(c.base, c.capacity);
            dirty = true;
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("incremental hardware ray scene build"),
        });
        let mut built = Vec::new();
        for (part, index, count, stamp, _) in &selected {
            if built.len() == BUILD_BUDGET {
                break;
            }
            if self.chunks.get(part).is_some_and(|c| c.stamp == *stamp) {
                continue;
            }
            let capacity = Arena::reserve(*count);
            let prior = self.chunks.remove(part);
            let (base, blas, cap) = if let Some(old) = prior {
                if old.capacity >= capacity {
                    (old.base, old.blas, old.capacity)
                } else {
                    self.arena.release(old.base, old.capacity);
                    let Some(base) = self.arena.allocate(capacity) else {
                        self.chunks.clear();
                        self.arena = Arena::new(self.arena.capacity());
                        self.compactions += 1;
                        dirty = true;
                        break;
                    };
                    (base, create_blas(device, capacity), capacity)
                }
            } else {
                let Some(base) = self.arena.allocate(capacity) else {
                    self.chunks.clear();
                    self.arena = Arena::new(self.arena.capacity());
                    self.compactions += 1;
                    dirty = true;
                    break;
                };
                (base, create_blas(device, capacity), capacity)
            };
            let source = &sources[*index];
            let temporary;
            let buffer = if part.transparent {
                temporary = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("transparent ray mesh staging"),
                    contents: bytemuck::cast_slice(source.transparent),
                    usage: wgpu::BufferUsages::COPY_SRC,
                });
                &temporary
            } else {
                source.opaque.ok_or("Ray source has no opaque buffer")?
            };
            encoder.copy_buffer_to_buffer(
                buffer,
                0,
                &self.data,
                u64::from(base) * 40,
                u64::from(*count) * 40,
            );
            let params = Localize {
                range: [base, *count, 0, 0],
                origin: [
                    (part.key[0] * 16) as f32,
                    (part.key[1] * 16) as f32,
                    (part.key[2] * 16) as f32,
                    0.,
                ],
            };
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("local ray chunk range"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("local ray chunk vertices"),
                layout: &self.compute.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.data.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("localize current ray chunk"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.compute);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(count.div_ceil(64), 1, 1);
            }
            self.chunks.insert(
                *part,
                Chunk {
                    blas,
                    size: descriptor(*count),
                    base,
                    capacity: cap,
                    stamp: *stamp,
                },
            );
            built.push(*part);
            dirty = true;
        }
        self.pending = selected
            .iter()
            .filter(|(part, _, _, stamp, _)| {
                self.chunks.get(part).is_none_or(|c| c.stamp != *stamp)
            })
            .count();
        self.effective_range = range;
        // Reuse an older mesh while its replacement is budgeted, but never
        // query a scene with missing chunks after travel or an arena reset.
        self.ready = range >= 8.
            && selected
                .iter()
                .all(|(part, _, _, _, _)| self.chunks.contains_key(part));
        if dirty {
            for slot in self
                .package
                .get_mut_slice(0..MAX_INSTANCES as usize)
                .unwrap()
            {
                *slot = None;
            }
            for (index, (part, c)) in self.chunks.iter().enumerate() {
                let p =
                    std::array::from_fn::<_, 3, _>(|i| (part.key[i] * 16 - self.anchor[i]) as f32);
                self.package[index] = Some(wgpu::TlasInstance::new(
                    &c.blas,
                    [1., 0., 0., p[0], 0., 1., 0., p[1], 0., 0., 1., p[2]],
                    c.base,
                    if part.transparent { 2 } else { 1 },
                ));
            }
            let entries = built
                .iter()
                .filter_map(|p| self.chunks.get(p))
                .map(|c| wgpu::BlasBuildEntry {
                    blas: &c.blas,
                    geometry: wgpu::BlasGeometries::TriangleGeometries(vec![
                        wgpu::BlasTriangleGeometry {
                            size: &c.size,
                            vertex_buffer: &self.data,
                            first_vertex: c.base,
                            vertex_stride: 40,
                            index_buffer: None,
                            first_index: None,
                            transform_buffer: None,
                            transform_buffer_offset: None,
                        },
                    ]),
                })
                .collect::<Vec<_>>();
            encoder.build_acceleration_structures(entries.iter(), [&self.package]);
            queue.submit(Some(encoder.finish()));
            self.tlas_builds += 1;
            self.builds += entries.len();
        }
        self.built_vertices = self
            .chunks
            .values()
            .map(|c| c.size.vertex_count as usize)
            .sum();
        self.params = Parameters {
            anchor: [
                self.anchor[0] as f32,
                self.anchor[1] as f32,
                self.anchor[2] as f32,
                range,
            ],
            center: [
                eye[0] - self.anchor[0] as f32,
                eye[1] - self.anchor[1] as f32,
                eye[2] - self.anchor[2] as f32,
                range,
            ],
            options: [
                f32::from(settings.ray_reflections),
                f32::from(settings.ray_indirect),
                settings.quality.ray_samples() as f32,
                f32::from(self.ready),
            ],
            limits: [
                self.arena.capacity() as f32,
                self.chunks.len() as f32,
                self.pending as f32,
                0.,
            ],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&self.params));
        self.last_build_ms = started.elapsed().as_secs_f64() * 1000.;
        Ok(())
    }
    /// Verification-only readback: prove the ray vertex arena still represents
    /// current uploaded meshes, including metadata, UVs and chunk-local positions.
    pub fn capture_geometry(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sources: &[Source<'_>],
        path: &std::path::Path,
    ) -> Result<()> {
        use sha2::{Digest, Sha256};
        if !self.ready || self.pending != 0 {
            return Err("Ray geometry capture requires a settled scene".into());
        }
        let stride = std::mem::size_of::<Vertex>() as u64;
        let payload = self.built_vertices as u64 * stride;
        if payload == 0 {
            std::fs::write(
                path.join("ray-geometry.json"),
                b"{\"complete\":true,\"vertices\":0,\"parts\":[]}",
            )?;
            return Ok(());
        }
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("verification current and ray geometry"),
            size: payload * 2,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("verify localized ray geometry"),
        });
        let mut offset = 0u64;
        let mut rows = Vec::new();
        for (part, chunk) in &self.chunks {
            let source = sources
                .iter()
                .find(|s| s.key == part.key)
                .ok_or("Ray cache references an evicted chunk")?;
            let stamp = source
                .hashes
                .map_or(Stamp::Generation(source.generation), |h| {
                    Stamp::Digest(h[usize::from(part.transparent)])
                });
            let count = if part.transparent {
                source.transparent.len() as u32
            } else {
                source.opaque_count
            };
            if stamp != chunk.stamp || count != chunk.size.vertex_count {
                return Err("Ray cache is stale relative to the uploaded mesh".into());
            }
            let bytes = u64::from(count) * stride;
            encoder.copy_buffer_to_buffer(
                &self.data,
                u64::from(chunk.base) * stride,
                &buffer,
                offset,
                bytes,
            );
            if !part.transparent {
                encoder.copy_buffer_to_buffer(
                    source.opaque.ok_or("Opaque ray source missing")?,
                    0,
                    &buffer,
                    payload + offset,
                    bytes,
                );
            }
            rows.push((part, source, offset as usize, bytes as usize));
            offset += bytes;
        }
        queue.submit(Some(encoder.finish()));
        let (tx, rx) = std::sync::mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |v| {
            let _ = tx.send(v);
        });
        device.poll(wgpu::PollType::Wait)?;
        rx.recv()??;
        let mapped = buffer.slice(..).get_mapped_range();
        let mut parts = Vec::new();
        for (part, source, offset, bytes) in rows {
            let actual = &mapped[offset..offset + bytes];
            let original = if part.transparent {
                bytemuck::cast_slice(source.transparent)
            } else {
                &mapped[payload as usize + offset..payload as usize + offset + bytes]
            };
            for (a, b) in actual.chunks_exact(40).zip(original.chunks_exact(40)) {
                if a[12..] != b[12..] {
                    return Err("Ray UV/material data differs from current mesh".into());
                }
                for axis in 0..3 {
                    let i = axis * 4;
                    let actual = f32::from_le_bytes(a[i..i + 4].try_into().unwrap());
                    let expected = f32::from_le_bytes(b[i..i + 4].try_into().unwrap())
                        - (part.key[axis] * 16) as f32;
                    if actual.to_bits() != expected.to_bits() {
                        return Err("Ray position differs from current chunk-local mesh".into());
                    }
                }
            }
            parts.push(serde_json::json!({"key":part.key,"transparent":part.transparent,"vertices":bytes/40,"localized_sha256":format!("{:x}",Sha256::digest(actual))}));
        }
        drop(mapped);
        buffer.unmap();
        std::fs::write(
            path.join("ray-geometry.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"complete":true,"vertices":self.built_vertices,"parts":parts,"scope":"Exact GPU ray-arena positions, UVs, flags and material data match current uploaded opaque/transparent worker geometry; cache stamps and counts are current."}),
            )?,
        )?;
        Ok(())
    }
    pub fn observation(&self) -> serde_json::Value {
        serde_json::json!({"hardware":true,"ready":self.ready,"pending":self.pending,"instances":self.chunks.len(),"vertices":self.built_vertices,"reserved_vertex_bytes":u64::from(self.arena.capacity())*40,"builds":self.builds,"tlas_builds":self.tlas_builds,"build_budget":BUILD_BUDGET,"compactions":self.compactions,"last_build_ms":self.last_build_ms,"effective_range":self.effective_range,"parameters":self.params})
    }
}
fn descriptor(count: u32) -> wgpu::BlasTriangleGeometrySizeDescriptor {
    wgpu::BlasTriangleGeometrySizeDescriptor {
        vertex_format: wgpu::VertexFormat::Float32x3,
        vertex_count: count,
        index_format: None,
        index_count: None,
        flags: wgpu::AccelerationStructureGeometryFlags::empty(),
    }
}
fn create_blas(device: &wgpu::Device, count: u32) -> wgpu::Blas {
    device.create_blas(
        &wgpu::CreateBlasDescriptor {
            label: Some("current chunk ray BLAS"),
            flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
            update_mode: wgpu::AccelerationStructureUpdateMode::Build,
        },
        wgpu::BlasGeometrySizeDescriptors::Triangles {
            descriptors: vec![descriptor(count)],
        },
    )
}
