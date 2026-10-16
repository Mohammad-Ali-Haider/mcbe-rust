//! Immutable CPU texture/atlas preparation and bounded GPU copy batches.
use crate::{
    engine_gpu_ui::Atlas,
    scene::{Client, Result},
};
use std::ops::Range;
pub const BATCH_BYTES: usize = 256 * 1024;
pub const BATCH_WRITES: usize = 32;
pub enum Step {
    Terrain {
        layer: u32,
        mip: u32,
        width: u32,
        rgba: Vec<u8>,
    },
    Ui {
        row: u32,
        rows: u32,
    },
}
impl Step {
    pub fn bytes<'a>(&'a self, atlas: &'a Atlas) -> &'a [u8] {
        match self {
            Self::Terrain { rgba, .. } => rgba,
            Self::Ui { row, rows } => {
                &atlas.rgba
                    [(row * atlas.size * 4) as usize..((row + rows) * atlas.size * 4) as usize]
            }
        }
    }
}
pub struct Prepared {
    pub instance: u64,
    pub registry: u64,
    pub layers: u32,
    pub atlas: Atlas,
    pub steps: Vec<Step>,
}
impl Prepared {
    pub fn matches(&self, client: &Client) -> bool {
        client
            .scene
            .engine
            .as_ref()
            .is_some_and(|s| self.instance == s.instance && self.registry == s.registry_revision)
    }
    pub fn build(client: &Client) -> Result<Self> {
        let state = client
            .scene
            .engine
            .as_ref()
            .ok_or("Texture preparation requires an engine world")?;
        let layers = state.registry.blocks.len() as u32 * 6;
        let mut steps = Vec::with_capacity(layers as usize * 6 + 16);
        for layer in 0..layers {
            let id = (layer / 6) as u16;
            let face = (layer % 6) as usize;
            let mut rgba = Vec::with_capacity(32 * 32 * 4);
            for y in 0..32 {
                for x in 0..32 {
                    let u = (x as f64 + 0.5) / 32.;
                    let v = (y as f64 + 0.5) / 32.;
                    let rgb = crate::engine_materials::color(
                        client,
                        id,
                        face,
                        u,
                        if face <= 1 { v } else { 1. - v },
                    )
                    .unwrap_or_else(|| crate::engine::color(state.registry.get(id), face, u, v));
                    let block = state.registry.get(id);
                    let alpha = match block.render_layer {
                        crate::engine_blocks::RenderLayer::Opaque => 255,
                        _ => client
                            .owned_materials
                            .as_ref()
                            .and_then(|a| a.textures.get(&(id, face)))
                            .map(|t| t.as_ref())
                            .or_else(|| {
                                client
                                    .engine_assets
                                    .as_ref()
                                    .and_then(|a| a.materials.get(&(id, face)))
                                    .map(|m| &m.texture)
                            })
                            .map_or_else(
                                || {
                                    if block.render_layer
                                        == crate::engine_blocks::RenderLayer::Translucent
                                    {
                                        90
                                    } else if x < 2
                                        || y < 2
                                        || x >= 30
                                        || y >= 30
                                        || (x == y && x < 10)
                                    {
                                        255
                                    } else {
                                        0
                                    }
                                },
                                |m| m.sample_repeat(u, v)[3],
                            ),
                    };
                    rgba.extend([rgb[0] as u8, rgb[1] as u8, rgb[2] as u8, alpha]);
                }
            }
            let mut width = 32;
            for mip in 0..6 {
                let reduced = if width > 1 {
                    Some(reduce(&rgba, width))
                } else {
                    None
                };
                steps.push(Step::Terrain {
                    layer,
                    mip,
                    width,
                    rgba,
                });
                if let Some(next) = reduced {
                    rgba = next;
                    width /= 2;
                } else {
                    break;
                }
            }
        }
        let atlas = Atlas::build(client)?;
        let rows = (BATCH_BYTES / (atlas.size as usize * 4)) as u32;
        if rows == 0 {
            return Err("UI atlas row exceeds upload budget".into());
        }
        for row in (0..atlas.size).step_by(rows as usize) {
            steps.push(Step::Ui {
                row,
                rows: rows.min(atlas.size - row),
            });
        }
        Ok(Self {
            instance: state.instance,
            registry: state.registry_revision,
            layers,
            atlas,
            steps,
        })
    }
    pub fn batch(&self, cursor: usize) -> Range<usize> {
        let mut end = cursor;
        let mut bytes = 0;
        while end < self.steps.len() && end - cursor < BATCH_WRITES {
            let next = self.steps[end].bytes(&self.atlas).len();
            if bytes + next > BATCH_BYTES {
                break;
            }
            bytes += next;
            end += 1;
        }
        cursor..end
    }
}
fn reduce(rgba: &[u8], width: u32) -> Vec<u8> {
    let next = width / 2;
    let mut reduced = vec![0; (next * next * 4) as usize];
    for y in 0..next {
        for x in 0..next {
            for channel in 0..4 {
                let sum: u32 = [(0, 0), (1, 0), (0, 1), (1, 1)]
                    .iter()
                    .map(|(dx, dy)| {
                        u32::from(
                            rgba[(((y * 2 + dy) * width + x * 2 + dx) * 4 + channel) as usize],
                        )
                    })
                    .sum();
                reduced[((y * next + x) * 4 + channel) as usize] = (sum / 4) as u8;
            }
        }
    }
    reduced
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mip_and_atlas_batches_are_complete_bounded_and_owned() {
        let mut client = crate::engine::generate(360036).unwrap();
        let plan = Prepared::build(&client).unwrap();
        assert!(plan.matches(&client));
        assert_eq!(plan.layers, 41 * 6);
        assert_eq!(plan.steps.len(), 41 * 6 * 6 + 16);
        let mut cursor = 0;
        let mut bytes = 0;
        let mut frames = 0;
        while cursor < plan.steps.len() {
            let batch = plan.batch(cursor);
            assert!(!batch.is_empty());
            assert!(batch.len() <= BATCH_WRITES);
            let size: usize = plan.steps[batch.clone()]
                .iter()
                .map(|s| s.bytes(&plan.atlas).len())
                .sum();
            assert!(size <= BATCH_BYTES);
            bytes += size;
            cursor = batch.end;
            frames += 1;
        }
        assert_eq!(bytes, plan.layers as usize * 5460 + 1024 * 1024 * 4);
        assert_eq!(frames, 63); // 41 compiled states plus the UI atlas, within byte/write budgets.
        let Step::Terrain { rgba: base, .. } = &plan.steps[0] else {
            panic!()
        };
        let Step::Terrain { rgba: mip, .. } = &plan.steps[1] else {
            panic!()
        };
        for channel in 0..4 {
            let sum: u32 = [0, 1, 32, 33]
                .iter()
                .map(|p| u32::from(base[p * 4 + channel]))
                .sum();
            assert_eq!(mip[channel], (sum / 4) as u8);
        }
        let before = base.clone();
        client.scene.engine.as_mut().unwrap().registry_revision += 1;
        assert!(!plan.matches(&client));
        assert_eq!(plan.steps[0].bytes(&plan.atlas), before);
    }
}
