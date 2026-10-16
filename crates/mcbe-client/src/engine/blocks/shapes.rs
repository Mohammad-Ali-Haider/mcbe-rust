//! Bounded cuboid unions with shared render, collision and selection geometry.
use crate::{
    engine_blocks::{BlockId, Registry},
    scene::Result,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    pub boxes: Vec<[f32; 6]>,
}
#[derive(Clone, Copy, Debug)]
pub struct Surface {
    pub bounds: [f32; 6],
}
#[derive(Clone)]
pub struct Visible {
    pub surfaces: Vec<Surface>,
    pub hidden: usize,
    pub whole_cube_face: bool,
}
pub struct Model {
    pub boxes: Vec<[f32; 6]>,
    pub full: bool,
    faces: [Vec<Surface>; 6],
}
pub struct Library {
    connected: Vec<Option<[usize; 16]>>,
    channels: Vec<Option<u16>>,
    sources: Vec<bool>,
    collision: Vec<Vec<[f32; 6]>>,
    fluids: Vec<bool>,
    layers: Vec<crate::engine_blocks::RenderLayer>,
    groups: Vec<Option<String>>,
    models: Vec<Model>,
    by_block: Vec<usize>,
    visibility: Vec<[Visible; 6]>,
}
const AXES: [usize; 6] = [1, 1, 2, 2, 0, 0];
const POSITIVE: [bool; 6] = [true, false, false, true, true, false];
pub const UNIT: [f32; 6] = [0., 0., 0., 1., 1., 1.];
pub fn validate_boxes(boxes: &[[f32; 6]], allow_empty: bool) -> Result<()> {
    if (!allow_empty && boxes.is_empty())
        || boxes.len() > 16
        || boxes.iter().any(|b| {
            b.iter().any(|v| !v.is_finite())
                || (0..3).any(|a| b[a] < 0. || b[a + 3] > 1. || b[a] >= b[a + 3])
        })
    {
        return Err("Geometry requires at most 16 positive finite unit-block cuboids".into());
    }
    Ok(())
}
fn axes(face: usize) -> [usize; 2] {
    let a = AXES[face];
    match a {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    }
}
fn subtract(surface: Surface, occluder: [f32; 6], face: usize) -> Vec<Surface> {
    let [u, v] = axes(face);
    let b = surface.bounds;
    let lo_u = b[u].max(occluder[u]);
    let hi_u = b[u + 3].min(occluder[u + 3]);
    let lo_v = b[v].max(occluder[v]);
    let hi_v = b[v + 3].min(occluder[v + 3]);
    if lo_u >= hi_u || lo_v >= hi_v {
        return vec![surface];
    }
    let mut result = Vec::new();
    for (ul, uh, vl, vh) in [
        (b[u], lo_u, b[v], b[v + 3]),
        (hi_u, b[u + 3], b[v], b[v + 3]),
        (lo_u, hi_u, b[v], lo_v),
        (lo_u, hi_u, hi_v, b[v + 3]),
    ] {
        if ul < uh && vl < vh {
            let mut bounds = b;
            bounds[u] = ul;
            bounds[u + 3] = uh;
            bounds[v] = vl;
            bounds[v + 3] = vh;
            result.push(Surface { bounds });
        }
    }
    result
}
impl Model {
    fn compile(boxes: Vec<[f32; 6]>) -> Result<Self> {
        if boxes.is_empty()
            || boxes.len() > 16
            || boxes.iter().any(|b| {
                b.iter().any(|v| !v.is_finite())
                    || (0..3).any(|a| b[a] < 0. || b[a + 3] > 1. || b[a] >= b[a + 3])
            })
        {
            return Err(
                "Geometry requires 1..16 finite, positive cuboids inside the unit block".into(),
            );
        }
        let mut faces: [Vec<Surface>; 6] = std::array::from_fn(|_| Vec::new());
        for (i, b) in boxes.iter().enumerate() {
            for face in 0..6 {
                let axis = AXES[face];
                let positive = POSITIVE[face];
                let plane = b[axis + if positive { 3 } else { 0 }];
                let mut bounds = *b;
                bounds[axis] = plane;
                bounds[axis + 3] = plane;
                let mut pieces = vec![Surface { bounds }];
                for (j, other) in boxes.iter().enumerate() {
                    if i == j {
                        continue;
                    }
                    let covers = if positive {
                        other[axis] <= plane
                            && (other[axis + 3] > plane || other[axis + 3] == plane && j < i)
                    } else {
                        other[axis + 3] >= plane
                            && (other[axis] < plane || other[axis] == plane && j < i)
                    };
                    if covers {
                        pieces = pieces
                            .into_iter()
                            .flat_map(|p| subtract(p, *other, face))
                            .collect();
                    }
                    if pieces.len() > 256 {
                        return Err("Geometry surface budget exceeded".into());
                    }
                }
                faces[face].extend(pieces);
            }
        }
        if faces.iter().map(Vec::len).sum::<usize>() > 256 {
            return Err("Geometry exceeds 256 surface rectangles".into());
        }
        let full = (0..6).all(|face| {
            let a = AXES[face];
            let plane = if POSITIVE[face] { 1. } else { 0. };
            let [u, v] = axes(face);
            faces[face].iter().all(|s| s.bounds[a] == plane)
                && (faces[face]
                    .iter()
                    .map(|s| (s.bounds[u + 3] - s.bounds[u]) * (s.bounds[v + 3] - s.bounds[v]))
                    .sum::<f32>()
                    - 1.)
                    .abs()
                    < 0.000001
        });
        if full {
            faces = std::array::from_fn(|face| {
                let mut bounds = UNIT;
                let a = AXES[face];
                let plane = if POSITIVE[face] { 1. } else { 0. };
                bounds[a] = plane;
                bounds[a + 3] = plane;
                vec![Surface { bounds }]
            });
        }
        Ok(Self { boxes, full, faces })
    }
}
impl Library {
    fn intern(models: &mut Vec<Model>, boxes: Vec<[f32; 6]>) -> Result<usize> {
        if let Some(index) = models.iter().position(|m| m.boxes == boxes) {
            return Ok(index);
        }
        if models.len() >= 128 {
            return Err("At most 128 distinct cuboid models may be compiled".into());
        }
        models.push(Model::compile(boxes)?);
        Ok(models.len() - 1)
    }
    pub fn compile(registry: &Registry) -> Result<Self> {
        let mut models: Vec<Model> = Vec::new();
        let mut by_block = Vec::new();
        let mut connected = Vec::new();
        for block in &registry.blocks {
            let boxes = block
                .geometry
                .as_ref()
                .map_or_else(|| vec![UNIT], |g| g.boxes.clone());
            by_block.push(Self::intern(&mut models, boxes)?);
            connected.push(
                block
                    .connections
                    .as_ref()
                    .map(|c| {
                        c.models
                            .iter()
                            .map(|m| Self::intern(&mut models, m.boxes.clone()))
                            .collect::<Result<Vec<_>>>()
                            .map(|v| v.try_into().unwrap())
                    })
                    .transpose()?,
            );
        }
        let mut visibility = Vec::new();
        let mut total = 0;
        for model in &models {
            for neighbor in 0..=models.len() {
                let row = std::array::from_fn(|face| {
                    let mut surfaces = Vec::new();
                    let mut hidden = 0;
                    let axis = AXES[face];
                    let plane = if POSITIVE[face] { 1. } else { 0. };
                    for original in &model.faces[face] {
                        let mut pieces = vec![*original];
                        if original.bounds[axis] == plane && neighbor < models.len() {
                            for b in &models[neighbor].boxes {
                                let touches = if POSITIVE[face] {
                                    b[axis] == 0.
                                } else {
                                    b[axis + 3] == 1.
                                };
                                if touches {
                                    pieces = pieces
                                        .into_iter()
                                        .flat_map(|p| subtract(p, *b, face))
                                        .collect();
                                }
                            }
                        }
                        hidden += usize::from(pieces.is_empty());
                        surfaces.extend(pieces);
                    }
                    let whole_cube_face = model.full
                        && surfaces.len() == 1
                        && surfaces[0].bounds == model.faces[face][0].bounds;
                    total += surfaces.len();
                    Visible {
                        surfaces,
                        hidden,
                        whole_cube_face,
                    }
                });
                visibility.push(row);
                if total > 131_072 {
                    return Err("Combined model visibility cache exceeds 131072 rectangles".into());
                }
            }
        }
        let mut channel_ids = std::collections::BTreeMap::new();
        for connection in registry
            .blocks
            .iter()
            .filter_map(|b| b.connections.as_ref())
        {
            let next = channel_ids.len() as u16;
            channel_ids
                .entry(connection.channel.as_str())
                .or_insert(next);
        }
        Ok(Self {
            connected,
            channels: registry
                .blocks
                .iter()
                .map(|b| {
                    b.connections
                        .as_ref()
                        .map(|c| channel_ids[c.channel.as_str()])
                })
                .collect(),
            sources: registry
                .blocks
                .iter()
                .map(|b| b.signal.as_ref().is_some_and(|s| s.source > 0))
                .collect(),
            collision: registry
                .blocks
                .iter()
                .map(|b| {
                    b.collision.clone().unwrap_or_else(|| {
                        b.geometry
                            .as_ref()
                            .map_or_else(|| vec![UNIT], |g| g.boxes.clone())
                    })
                })
                .collect(),
            fluids: registry.blocks.iter().map(|b| b.fluid.is_some()).collect(),
            layers: registry.blocks.iter().map(|b| b.render_layer).collect(),
            groups: registry
                .blocks
                .iter()
                .map(|b| b.cull_group.clone())
                .collect(),
            models,
            by_block,
            visibility,
        })
    }
    pub fn boxes(&self, id: BlockId) -> &[[f32; 6]] {
        &self.models[self.by_block[usize::from(id)]].boxes
    }
    pub fn full(&self, id: BlockId) -> bool {
        self.models[self.by_block[usize::from(id)]].full
    }
    pub fn collision(&self, id: BlockId) -> &[[f32; 6]] {
        &self.collision[usize::from(id)]
    }
    pub fn layer(&self, id: BlockId) -> crate::engine_blocks::RenderLayer {
        self.layers[usize::from(id)]
    }
    pub fn flags(&self, id: BlockId) -> u32 {
        (if self.fluids[id as usize] {
            0x00400000
        } else {
            0
        }) | match self.layer(id) {
            crate::engine_blocks::RenderLayer::Opaque => 0,
            crate::engine_blocks::RenderLayer::Cutout => 0x20000000,
            crate::engine_blocks::RenderLayer::Translucent => 0x40000000,
        }
    }
    pub fn connection_mask(
        &self,
        id: BlockId,
        p: [i32; 3],
        lookup: &impl Fn([i32; 3]) -> Option<BlockId>,
    ) -> usize {
        let Some(channel) = &self.channels[id as usize] else {
            return 0;
        };
        [[0, 0, -1], [1, 0, 0], [0, 0, 1], [-1, 0, 0]]
            .iter()
            .enumerate()
            .fold(0, |mask, (bit, d)| {
                let connected = lookup(std::array::from_fn(|a| p[a] + d[a])).is_some_and(|other| {
                    self.channels[other as usize].as_ref() == Some(channel)
                        || self.sources[other as usize]
                });
                mask | (usize::from(connected) << bit)
            })
    }
    pub fn visible(&self, id: BlockId, neighbor: Option<BlockId>, face: usize) -> &Visible {
        self.visible_connected(id, neighbor, face, 0)
    }
    pub fn visible_connected(
        &self,
        id: BlockId,
        neighbor: Option<BlockId>,
        face: usize,
        mask: usize,
    ) -> &Visible {
        let shape = self.connected[id as usize]
            .as_ref()
            .map_or(self.by_block[id as usize], |states| states[mask & 15]);
        let neighbor = neighbor.filter(|n| {
            self.layer(*n) == crate::engine_blocks::RenderLayer::Opaque
                || (self.layer(id) != crate::engine_blocks::RenderLayer::Opaque
                    && self.groups[usize::from(id)]
                        .as_ref()
                        .is_some_and(|g| self.groups[usize::from(*n)].as_ref() == Some(g)))
        });
        let neighbor = neighbor.map_or(self.models.len(), |id| self.by_block[usize::from(id)]);
        &self.visibility[shape * (self.models.len() + 1) + neighbor][face]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_dust_connections_cover_all_masks_and_disconnect_across_chunk_boundaries() {
        let r = Registry::built_in();
        let geometry = Library::compile(&r).unwrap();
        let dust = r.find("redstone_dust").unwrap();
        let source = r.find("power_source").unwrap();
        let p = [15, 20, 0];
        let dirs = [[0, 0, -1], [1, 0, 0], [0, 0, 1], [-1, 0, 0]];
        for mask in 0..16 {
            let mut blocks = crate::engine::world::storage::Store::from([(p, dust)]);
            for (bit, d) in dirs.iter().enumerate() {
                if mask & (1 << bit) != 0 {
                    blocks.insert(
                        std::array::from_fn(|a| p[a] + d[a]),
                        if bit == 0 { source } else { dust },
                    );
                }
            }
            assert_eq!(
                geometry.connection_mask(dust, p, &|q| blocks.get(&q).copied()),
                mask
            );
            let faces = geometry.visible_connected(dust, None, 0, mask);
            assert!(!faces.surfaces.is_empty());
        }
        let mut c = crate::engine::generate(37).unwrap();
        let q = [16, 20, 0];
        c.scene.change_engine(p, Some(dust)).unwrap();
        c.scene.change_engine(q, Some(dust)).unwrap();
        let s = c.scene.engine.as_ref().unwrap();
        let joined =
            crate::engine_mesh::Snapshot::capture(s, crate::engine_mesh::chunk(p)).build(true);
        let max_x = |mesh: &crate::engine_mesh::ChunkMesh| {
            mesh.vertices
                .iter()
                .filter(|v| ((v.layer & 0x003fffff) / 6) == u32::from(dust))
                .map(|v| v.position[0])
                .fold(f32::NEG_INFINITY, f32::max)
        };
        assert_eq!(max_x(&joined), 16.);
        c.scene.change_engine(q, None).unwrap();
        let s = c.scene.engine.as_ref().unwrap();
        assert!(s.dirty_chunks.contains_key(&crate::engine_mesh::chunk(p)));
        let separate =
            crate::engine_mesh::Snapshot::capture(s, crate::engine_mesh::chunk(p)).build(true);
        assert_eq!(max_x(&separate), 15.625);
    }
    #[test]
    fn transparent_neighbors_do_not_hide_opaque_blocks_and_collision_is_independent() {
        let mut r = Registry::built_in();
        let glass = r.find("glass").unwrap();
        let stone = r.find("slate").unwrap();
        r.blocks[usize::from(glass)].collision = Some(vec![]);
        let g = Library::compile(&r).unwrap();
        assert!(g.visible(stone, Some(glass), 4).whole_cube_face);
        assert!(g.visible(glass, Some(glass), 4).surfaces.is_empty());
        assert!(g.visible(glass, Some(stone), 4).surfaces.is_empty());
        assert_eq!(g.boxes(glass), &[UNIT]);
        assert!(g.collision(glass).is_empty());
        assert_eq!(g.collision(stone), &[UNIT]);
        r.blocks[usize::from(glass)].collision = Some(vec![[0., 0., 0., 1., 1.5, 1.]]);
        assert!(r.validate().is_err());
    }
    #[test]
    fn unions_remove_internal_overlapping_and_duplicate_faces() {
        let m = Model::compile(vec![[0., 0., 0., 1., 0.5, 1.], [0., 0.5, 0., 1., 1., 1.]]).unwrap();
        assert!(m.full);
        assert_eq!(m.faces.iter().map(Vec::len).sum::<usize>(), 6);
        let m = Model::compile(vec![UNIT, UNIT]).unwrap();
        assert!(m.full);
        assert_eq!(m.faces.iter().map(Vec::len).sum::<usize>(), 6);
        let stair =
            Model::compile(vec![[0., 0., 0., 1., 0.5, 1.], [0., 0.5, 0.5, 1., 1., 1.]]).unwrap();
        assert!(!stair.full);
        assert!(
            !stair.faces[0]
                .iter()
                .any(|s| s.bounds[1] == 0.5 && s.bounds[2] >= 0.5)
        );
    }
    #[test]
    fn partial_neighbor_preserves_exposed_parts_and_rejects_invalid_models() {
        let mut r = Registry::built_in();
        r.blocks[0].geometry = Some(Geometry {
            boxes: vec![[0., 0., 0., 1., 0.5, 1.]],
        });
        let library = Library::compile(&r).unwrap();
        let visible = library.visible(1, Some(0), 4);
        assert!(!visible.whole_cube_face);
        assert_eq!(visible.surfaces.len(), 1);
        assert_eq!(visible.surfaces[0].bounds[1], 0.5);
        assert!(library.visible(1, Some(0), 0).surfaces.is_empty());
        assert!(Model::compile(vec![]).is_err());
        assert!(Model::compile(vec![[0., 0., 0., 1.1, 1., 1.]]).is_err());
        assert!(Model::compile(vec![[0., 0., 0., 0., 1., 1.]]).is_err());
    }
}
