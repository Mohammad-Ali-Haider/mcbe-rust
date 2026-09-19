//! Bounded cuboid unions with shared render, collision and selection geometry.
use crate::{
    engine_blocks::{BlockId, Registry},
    scene::Result,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    models: Vec<Model>,
    by_block: Vec<usize>,
    visibility: Vec<[Visible; 6]>,
}
const AXES: [usize; 6] = [1, 1, 2, 2, 0, 0];
const POSITIVE: [bool; 6] = [true, false, false, true, true, false];
pub const UNIT: [f32; 6] = [0., 0., 0., 1., 1., 1.];
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
    pub fn compile(registry: &Registry) -> Result<Self> {
        let mut models: Vec<Model> = Vec::new();
        let mut by_block = Vec::new();
        for block in &registry.blocks {
            let boxes = block
                .geometry
                .as_ref()
                .map_or_else(|| vec![UNIT], |g| g.boxes.clone());
            let model = if let Some(i) = models.iter().position(|m| m.boxes == boxes) {
                i
            } else {
                if models.len() >= 32 {
                    return Err("Version 1 supports at most 32 distinct cuboid models; textures may share models".into());
                }
                models.push(Model::compile(boxes)?);
                models.len() - 1
            };
            by_block.push(model);
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
        Ok(Self {
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
    pub fn visible(&self, id: BlockId, neighbor: Option<BlockId>, face: usize) -> &Visible {
        let shape = self.by_block[usize::from(id)];
        let neighbor = neighbor.map_or(self.models.len(), |id| self.by_block[usize::from(id)]);
        &self.visibility[shape * (self.models.len() + 1) + neighbor][face]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
