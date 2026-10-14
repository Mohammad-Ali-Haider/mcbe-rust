//! Independent visual-following, empty, cube, or custom collision/selection shapes.
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case", deny_unknown_fields)]
pub enum ShapeComponent {
    #[default]
    Model,
    None,
    Cube,
    Cuboids {
        boxes: Vec<[f32; 6]>,
    },
}
impl ShapeComponent {
    pub fn compile(&self) -> crate::scene::Result<Option<Vec<[f32; 6]>>> {
        let boxes = match self {
            Self::Model => return Ok(None),
            Self::None => vec![],
            Self::Cube => vec![crate::engine_geometry::UNIT],
            Self::Cuboids { boxes } => boxes.clone(),
        };
        crate::engine_geometry::validate_boxes(&boxes, true)?;
        Ok(Some(boxes))
    }
}
