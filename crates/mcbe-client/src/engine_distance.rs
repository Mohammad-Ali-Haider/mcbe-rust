//! Bounded adaptive distant terrain. Nearby blocks remain the authoritative
//! geometry/collision/edit data; far meshes approximate the generator's ground.
use crate::{
    engine_mesh::{self, Vertex},
    engine_stream::{self, Generator},
    scene::{Client, Result},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
};
use wgpu::util::DeviceExt;
pub const MAX_VERTICES: usize = 262_140;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    instance: u64,
    registry: u64,
    center: [i32; 2],
    radius: i32,
}
struct Request {
    epoch: u64,
    key: Key,
    generator: Arc<Generator>,
    top: u16,
    side: u16,
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
}
struct Builder<'a> {
    request: &'a Request,
    token: &'a AtomicU64,
    view: [i32; 4],
    detail: [i32; 4],
    mesh: Mesh,
    visited: usize,
}
impl Builder<'_> {
    fn node(&mut self, x: i32, z: i32, size: i32) -> Result<bool> {
        self.visited += 1;
        if self.visited.is_multiple_of(64)
            && self.token.load(Ordering::Acquire) != self.request.epoch
        {
            return Ok(false);
        }
        let [xmin, zmin, xmax, zmax] = self.view;
        if x >= xmax || z >= zmax || x + size <= xmin || z + size <= zmin {
            return Ok(true);
        }
        let [dx0, dz0, dx1, dz1] = self.detail;
        if x >= dx0 && z >= dz0 && x + size <= dx1 && z + size <= dz1 {
            return Ok(true);
        }
        let cuts_view = x < xmin || z < zmin || x + size > xmax || z + size > zmax;
        let cuts_detail = x < dx1 && z < dz1 && x + size > dx0 && z + size > dz0;
        let cx = self.request.key.center[0] * 16 + 8;
        let cz = self.request.key.center[1] * 16 + 8;
        let distance = (x + size / 2 - cx).abs().max((z + size / 2 - cz).abs());
        let step = (distance / 8).clamp(16, 256);
        if size > 16 && (size > step || cuts_view || cuts_detail) {
            let half = size / 2;
            for (ox, oz) in [(0, 0), (half, 0), (0, half), (half, half)] {
                if !self.node(x + ox, z + oz, half)? {
                    return Ok(false);
                }
            }
        } else {
            if self.mesh.vertices.len() + 30 > MAX_VERTICES {
                return Err("Distant terrain mesh budget exceeded".into());
            }
            let height = self.request.generator.height(x + size / 2, z + size / 2);
            engine_mesh::distant_cell(
                &mut self.mesh.vertices,
                [x, 0, z],
                size,
                height,
                self.request.top,
                self.request.side,
            );
            self.mesh.leaves += 1;
            self.mesh.area += size as u64 * size as u64;
        }
        Ok(true)
    }
}
fn build(r: &Request, token: &AtomicU64) -> Result<Option<Mesh>> {
    let [x, z] = r.key.center;
    let radius = r.key.radius;
    let d = engine_stream::DETAIL_RADIUS;
    let mut b = Builder {
        request: r,
        token,
        view: [
            (x - radius) * 16,
            (z - radius) * 16,
            (x + radius + 1) * 16,
            (z + radius + 1) * 16,
        ],
        detail: [
            (x - d) * 16,
            (z - d) * 16,
            (x + d + 1) * 16,
            (z + d + 1) * 16,
        ],
        mesh: Mesh {
            vertices: Vec::new(),
            leaves: 0,
            area: 0,
        },
        visited: 0,
    };
    if b.node(-8192, -8192, 16384)? && token.load(Ordering::Acquire) == r.epoch {
        Ok(Some(b.mesh))
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
    pub buffer: Option<wgpu::Buffer>,
    pub vertices: u32,
    pub jobs: usize,
    pub discarded: usize,
    pub leaves: usize,
    pub area: u64,
}
impl Distant {
    pub fn update(&mut self, client: &Client, device: &wgpu::Device) -> Result<bool> {
        let state = client.scene.engine.as_ref().unwrap();
        let key = state
            .stream
            .as_ref()
            .filter(|s| s.generator.config.radius > engine_stream::DETAIL_RADIUS)
            .map(|s| Key {
                instance: state.instance,
                registry: state.registry_revision,
                center: s.center,
                radius: s.generator.config.radius,
            });
        if self.wanted != key {
            self.wanted = key;
            self.epoch += 1;
            if let Some(worker) = &self.worker {
                worker.token.store(self.epoch, Ordering::Release);
            }
            // Old meshes have a different detailed-region hole; discard them
            // immediately instead of drawing them over newly resident blocks.
            self.buffer = None;
            self.vertices = 0;
            self.leaves = 0;
            self.area = 0;
            self.adopted = None;
        }
        if let Some(worker) = &self.worker {
            match worker.receiver.try_recv() {
                Ok(done) => {
                    self.busy = false;
                    if done.epoch == self.epoch && Some(done.key) == self.wanted {
                        if let Some(mesh) =
                            done.mesh.map_err(|e| format!("Distant terrain: {e}"))?
                        {
                            self.vertices = mesh.vertices.len() as u32;
                            self.leaves = mesh.leaves;
                            self.area = mesh.area;
                            self.buffer = (!mesh.vertices.is_empty()).then(|| {
                                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                    label: Some("cached adaptive distant terrain"),
                                    contents: bytemuck::cast_slice(&mesh.vertices),
                                    usage: wgpu::BufferUsages::VERTEX,
                                })
                            });
                            self.adopted = self.wanted;
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
            let worker = self.worker.as_ref().unwrap();
            worker.token.store(self.epoch, Ordering::Release);
            worker.sender.send(Some(Request {
                epoch: self.epoch,
                key,
                generator: state.stream.as_ref().unwrap().generator.clone(),
                top: state.registry.find("meadow")?,
                side: state.registry.find("earth")?,
            }))?;
            self.busy = true;
            self.jobs += 1;
        }
        Ok(key.is_none() || self.adopted == key)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maximum_range_covers_requested_ground_with_a_bounded_mesh_and_no_near_overlap() {
        let registry = crate::engine_blocks::Registry::built_in();
        let mut config = engine_stream::Config::new(&registry);
        config.radius = 255;
        let generator = Arc::new(Generator::new(360036, &registry, config).unwrap());
        for center in [[0, 0], [-1, -1], [511, 511], [-512, -512]] {
            let r = Request {
                epoch: 1,
                key: Key {
                    instance: 1,
                    registry: 0,
                    center,
                    radius: 255,
                },
                generator: generator.clone(),
                top: registry.find("meadow").unwrap(),
                side: registry.find("earth").unwrap(),
            };
            let mesh = build(&r, &AtomicU64::new(1)).unwrap().unwrap();
            assert!(mesh.vertices.len() <= MAX_VERTICES);
            let side = |c: i32, r: i32| ((c + r + 1).min(512) - (c - r).max(-512)) as u64;
            assert_eq!(
                mesh.area,
                (side(center[0], 255) * side(center[1], 255)
                    - side(center[0], 8) * side(center[1], 8))
                    * 256
            );
            for cell in mesh.vertices.chunks_exact(30) {
                let minx = cell.iter().map(|v| v.position[0] as i32).min().unwrap();
                let maxx = cell.iter().map(|v| v.position[0] as i32).max().unwrap();
                let minz = cell.iter().map(|v| v.position[2] as i32).min().unwrap();
                let maxz = cell.iter().map(|v| v.position[2] as i32).max().unwrap();
                assert!(
                    maxx <= (center[0] - 8) * 16
                        || minx >= (center[0] + 9) * 16
                        || maxz <= (center[1] - 8) * 16
                        || minz >= (center[1] + 9) * 16
                );
                assert!(minx >= -8192 && maxx <= 8192 && minz >= -8192 && maxz <= 8192);
            }
        }
    }
    #[test]
    fn superseded_builds_do_not_publish() {
        let r = crate::engine_blocks::Registry::built_in();
        let request = Request {
            epoch: 1,
            key: Key {
                instance: 1,
                registry: 0,
                center: [0, 0],
                radius: 255,
            },
            generator: Arc::new(Generator::new(1, &r, engine_stream::Config::new(&r)).unwrap()),
            top: 0,
            side: 1,
        };
        assert!(build(&request, &AtomicU64::new(2)).unwrap().is_none());
    }
}
