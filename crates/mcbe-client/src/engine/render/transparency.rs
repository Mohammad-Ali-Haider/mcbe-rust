//! One cached vertex batch and a depth-sorted index batch for blended faces.
use crate::engine_mesh::Vertex;
use glam::Vec3;
use wgpu::util::DeviceExt;
#[derive(Default)]
pub struct Batch {
    pub vertex: Option<wgpu::Buffer>,
    pub index: Option<wgpu::Buffer>,
    pub count: u32,
    pub uploads: usize,
    pub sorts: usize,
    pub bytes: usize,
    stamp: usize,
    keys: Vec<[i32; 3]>,
    pose: Option<([f32; 3], [f32; 3])>,
    centers: Vec<Vec3>,
}
pub fn order(centers: &[Vec3], eye: Vec3, dir: Vec3) -> Vec<u32> {
    let mut faces: Vec<_> = centers
        .iter()
        .enumerate()
        .map(|(i, c)| (i, (*c - eye).dot(dir)))
        .collect();
    faces.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    faces
        .into_iter()
        .flat_map(|(i, _)| (0..6).map(move |v| i as u32 * 6 + v))
        .collect()
}
impl Batch {
    pub fn update<'a>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stamp: usize,
        keys: &[[i32; 3]],
        chunks: impl Iterator<Item = &'a [Vertex]>,
        view: ([f32; 3], [f32; 3]),
    ) {
        let (eye, dir) = view;
        let changed = self.stamp != stamp || self.keys != keys;
        if changed {
            let vertices: Vec<_> = chunks.flatten().copied().collect();
            self.centers = vertices
                .chunks_exact(6)
                .map(|q| q.iter().fold(Vec3::ZERO, |a, v| a + Vec3::from(v.position)) / 6.)
                .collect();
            self.count = vertices.len() as u32;
            self.bytes = vertices.len() * (std::mem::size_of::<Vertex>() + 4);
            self.vertex = (!vertices.is_empty()).then(|| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("cached blended faces"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
            });
            self.index = (!vertices.is_empty()).then(|| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("sorted blended face indices"),
                    size: vertices.len() as u64 * 4,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            });
            self.stamp = stamp;
            self.keys = keys.to_vec();
            self.pose = None;
            self.uploads += usize::from(!vertices.is_empty());
        }
        if self.count > 0 && self.pose != Some((eye, dir)) {
            let indices = order(&self.centers, Vec3::from(eye), Vec3::from(dir));
            queue.write_buffer(
                self.index.as_ref().unwrap(),
                0,
                bytemuck::cast_slice(&indices),
            );
            self.sorts += 1;
            self.pose = Some((eye, dir));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn faces_sort_back_to_front_and_turning_reverses_order() {
        let c = [Vec3::new(0., 0., 2.), Vec3::new(0., 0., 8.)];
        assert_eq!(order(&c, Vec3::ZERO, Vec3::Z)[0], 6);
        assert_eq!(order(&c, Vec3::new(0., 0., 10.), -Vec3::Z)[0], 0);
    }
}
