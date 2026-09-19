//! Measured selection geometry and ray traversal. These boxes are not movement collision.
use crate::{
    Result,
    blocks::{self, States},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct RayBlock {
    pub position: [i32; 3],
    pub id: String,
    pub states: States,
}
pub struct Shape {
    boxes: Vec<[f64; 6]>,
    passable: bool,
}
pub struct Shapes {
    rows: HashMap<(String, States), Shape>,
}
impl Shapes {
    pub fn read(rows: &Value) -> Result<Self> {
        let mut result = Self {
            rows: HashMap::new(),
        };
        for row in rows.as_array().ok_or("Shape rows must be an array")? {
            let id = row["id"]
                .as_str()
                .ok_or("Missing shape identifier")?
                .to_owned();
            let mut states = blocks::states(&row["states"]).map_err(|e| e.to_string())?;
            states.sort_by(|a, b| a.0.cmp(&b.0));
            let passable = row["passable"]
                .as_bool()
                .ok_or("Missing passable boolean")?;
            let mut boxes = vec![];
            for raw in row["boxes"].as_array().ok_or("Missing shape boxes")? {
                let values = raw.as_array().ok_or("Box must be array")?;
                if values.len() != 6 {
                    return Err("Selection box requires six coordinates".into());
                }
                let mut bounds = [0.0; 6];
                for i in 0..6 {
                    bounds[i] = values[i].as_f64().ok_or("Invalid box coordinate")?;
                }
                if bounds.iter().any(|v| !v.is_finite())
                    || (0..3).any(|i| {
                        bounds[i] < 0.0 || bounds[i] >= bounds[i + 3] || bounds[i + 3] > 1.0
                    })
                {
                    return Err("Invalid local selection box".into());
                }
                boxes.push(bounds);
            }
            if result
                .rows
                .insert((id, states), Shape { boxes, passable })
                .is_some()
            {
                return Err("Duplicate shape state".into());
            }
        }
        if result.rows.is_empty() {
            return Err("Empty shape table".into());
        }
        Ok(result)
    }
    fn boxes(&self, block: &RayBlock, include_passable: bool) -> Result<&[[f64; 6]]> {
        let mut states = block.states.clone();
        states.sort_by(|a, b| a.0.cmp(&b.0));
        if states.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err("Duplicate ray block state".into());
        }
        let shape = self.rows.get(&(block.id.clone(), states)).ok_or_else(|| {
            format!(
                "Unmeasured selection shape: {} at {:?}",
                block.id, block.position
            )
        })?;
        Ok(if shape.passable && !include_passable {
            &[]
        } else {
            &shape.boxes
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Face {
    West,
    East,
    Down,
    Up,
    North,
    South,
}
impl Face {
    fn axis(self) -> usize {
        match self {
            Self::West | Self::East => 0,
            Self::Down | Self::Up => 1,
            Self::North | Self::South => 2,
        }
    }
    fn positive(self) -> bool {
        matches!(self, Self::East | Self::Up | Self::South)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Coordinates<T> {
    pub x: T,
    pub y: T,
    pub z: T,
}
impl<T> From<[T; 3]> for Coordinates<T> {
    fn from([x, y, z]: [T; 3]) -> Self {
        Self { x, y, z }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    pub id: String,
    pub position: Coordinates<i32>,
    pub face: Face,
    #[serde(rename = "faceLocation")]
    pub face_location: Coordinates<f32>,
}
#[derive(Clone, Copy)]
pub struct Options {
    pub max_distance: f64,
    pub include_passable: bool,
    pub include_liquids: bool,
}
fn f32_value(value: f64) -> Result<f32> {
    let value = value as f32;
    if value.is_finite() {
        Ok(value)
    } else {
        Err("Ray values must fit finite float32".into())
    }
}
fn interval(
    origin: [f64; 3],
    direction: [f64; 3],
    bounds: [f64; 6],
) -> Option<(f64, f64, Face, Face)> {
    let mut near = f64::NEG_INFINITY;
    let mut far = f64::INFINITY;
    let mut near_face = Face::West;
    let mut far_face = Face::East;
    let faces = [
        (Face::West, Face::East),
        (Face::Down, Face::Up),
        (Face::North, Face::South),
    ];
    for axis in 0..3 {
        if direction[axis] == 0.0 {
            if origin[axis] < bounds[axis] || origin[axis] > bounds[axis + 3] {
                return None;
            }
            continue;
        }
        let mut a = (bounds[axis] - origin[axis]) / direction[axis];
        let mut b = (bounds[axis + 3] - origin[axis]) / direction[axis];
        let (mut fa, mut fb) = faces[axis];
        if a > b {
            std::mem::swap(&mut a, &mut b);
            std::mem::swap(&mut fa, &mut fb);
        }
        if a > near {
            near = a;
            near_face = fa;
        }
        if b < far {
            far = b;
            far_face = fb;
        }
        if near > far {
            return None;
        }
    }
    if far < 0.0 {
        None
    } else {
        Some((near, far, near_face, far_face))
    }
}
pub fn cast(
    origin: [f64; 3],
    direction: [f64; 3],
    options: Options,
    shapes: &Shapes,
    mut lookup: impl FnMut([i32; 3]) -> Result<Option<RayBlock>>,
) -> Result<Option<Hit>> {
    if options.include_liquids {
        return Err("Liquid targeting is unmeasured".into());
    }
    let mut o = [0.0f32; 3];
    let mut d = [0.0f32; 3];
    for i in 0..3 {
        o[i] = f32_value(origin[i])?;
        d[i] = f32_value(direction[i])?;
    }
    let squares = d.map(|v| v * v);
    let length = ((squares[0] + squares[1]) + squares[2]).sqrt();
    if !length.is_finite() || length == 0.0 {
        return Err("Zero or overflowing ray direction".into());
    }
    d = d.map(|v| v / length);
    let distance = f32_value(options.max_distance)?;
    if !(0.0..=4096.0).contains(&distance) {
        return Err("Ray distance outside 0..4096 budget".into());
    }
    let distance = distance.trunc();
    if distance == 0.0 {
        return Ok(None);
    }
    let origin = o.map(f64::from);
    let direction = d.map(f64::from);
    let mut cell = [0i32; 3];
    let mut step = [0i32; 3];
    let mut crossing = [f64::INFINITY; 3];
    let mut stride = [f64::INFINITY; 3];
    for i in 0..3 {
        let initial = origin[i].floor();
        if initial < f64::from(i32::MIN) + 4097.0 || initial > f64::from(i32::MAX) - 4097.0 {
            return Err("Ray traversal exceeds integer coordinate bounds".into());
        }
        cell[i] = initial as i32;
        step[i] = if d[i] > 0.0 {
            1
        } else if d[i] < 0.0 {
            -1
        } else {
            0
        };
        if step[i] != 0 {
            crossing[i] = (f64::from(cell[i]) + if step[i] > 0 { 1.0 } else { 0.0 } - origin[i])
                / direction[i];
            stride[i] = (1.0 / direction[i]).abs();
        }
    }
    for _ in 0..=distance as usize {
        let broad = [
            f64::from(cell[0]),
            f64::from(cell[1]),
            f64::from(cell[2]),
            f64::from(cell[0]) + 1.0,
            f64::from(cell[1]) + 1.0,
            f64::from(cell[2]) + 1.0,
        ];
        if interval(origin, direction, broad).is_some_and(|v| v.0 < f64::from(distance))
            && let Some(block) = lookup(cell)?
        {
            if block.position != cell {
                return Err("Ray lookup returned a different cell".into());
            }
            let mut best: Option<(f64, Face, [f64; 6])> = None;
            for local in shapes.boxes(&block, options.include_passable)? {
                // Fractional planes are translated and rounded in world float32.
                let bounds =
                    std::array::from_fn(|i| f64::from(local[i] as f32 + cell[i % 3] as f32));
                if let Some((near, far, near_face, far_face)) = interval(origin, direction, bounds)
                {
                    let (t, face) = if near >= 0.0 {
                        (near, near_face)
                    } else {
                        (far, far_face)
                    };
                    if t >= 0.0 && t < f64::from(distance) && best.is_none_or(|old| t < old.0) {
                        best = Some((t, face, bounds));
                    }
                }
            }
            if let Some((_, _, bounds)) = best {
                let endpoint: [f32; 3] = std::array::from_fn(|i| o[i] + d[i] * distance);
                let local_origin: [f32; 3] =
                    std::array::from_fn(|i| (origin[i] - f64::from(cell[i])) as f32);
                let local_endpoint: [f32; 3] =
                    std::array::from_fn(|i| (f64::from(endpoint[i]) - f64::from(cell[i])) as f32);
                let delta: [f32; 3] = std::array::from_fn(|i| local_endpoint[i] - local_origin[i]);
                // The native segment endpoint is rounded in world coordinates before
                // face selection. At corner hits this can change the winning plane.
                let faces = [
                    (Face::West, Face::East),
                    (Face::Down, Face::Up),
                    (Face::North, Face::South),
                ];
                let mut near = f32::NEG_INFINITY;
                let mut far = f32::INFINITY;
                let mut near_face = Face::West;
                let mut far_face = Face::East;
                for i in 0..3 {
                    if delta[i] == 0.0 {
                        continue;
                    }
                    let mut a =
                        ((bounds[i] - f64::from(cell[i])) as f32 - local_origin[i]) / delta[i];
                    let mut b =
                        ((bounds[i + 3] - f64::from(cell[i])) as f32 - local_origin[i]) / delta[i];
                    let (mut fa, mut fb) = faces[i];
                    if a > b {
                        std::mem::swap(&mut a, &mut b);
                        std::mem::swap(&mut fa, &mut fb);
                    }
                    if a > near {
                        near = a;
                        near_face = fa;
                    }
                    if b < far {
                        far = b;
                        far_face = fb;
                    }
                }
                let face = if near >= 0.0 { near_face } else { far_face };
                let axis = face.axis();
                let plane = (bounds[axis + if face.positive() { 3 } else { 0 }]
                    - f64::from(cell[axis])) as f32;
                let parameter = (plane - local_origin[axis]) / delta[axis];
                let point: [f32; 3] = std::array::from_fn(|i| {
                    ((local_origin[i] + delta[i] * parameter) as f64 + f64::from(cell[i])) as f32
                });
                let relative: [f32; 3] = std::array::from_fn(|i| {
                    (((f64::from(point[i]) - f64::from(cell[i])) as f32) % 1.0).abs()
                });
                if relative.iter().any(|v| !v.is_finite()) {
                    return Err("Unmeasured degenerate hit arithmetic".into());
                }
                return Ok(Some(Hit {
                    id: block.id,
                    position: cell.into(),
                    face,
                    face_location: relative.into(),
                }));
            }
        }
        let mut axis = 0;
        for i in 1..3 {
            if crossing[i] < crossing[axis] {
                axis = i;
            }
        }
        cell[axis] += step[axis];
        crossing[axis] += stride[axis];
    }
    Ok(None)
}

pub struct Scene {
    pub blocks: HashMap<[i32; 3], RayBlock>,
}
impl Scene {
    pub fn new(blocks: Vec<RayBlock>) -> Result<Self> {
        let mut result = Self {
            blocks: HashMap::new(),
        };
        for block in blocks {
            if result.blocks.insert(block.position, block).is_some() {
                return Err("Duplicate scene position".into());
            }
        }
        Ok(result)
    }
    pub fn cast(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
        options: Options,
        shapes: &Shapes,
    ) -> Result<Option<Hit>> {
        cast(origin, direction, options, shapes, |position| {
            Ok(self.blocks.get(&position).cloned())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn fractional_world_plane_rounding_matches_native_chest_holdout() {
        let hit = scene([-88, 64, 56])
            .cast(
                [-87.15518994426633, 65.18172333368952, 58.71014028116316],
                [
                    -0.22471532941550265,
                    -0.2402943909517603,
                    -2.677562479905866,
                ],
                options(9.0),
                &shapes(vec![[0.025, 0.0, 0.025, 0.975, 0.95, 0.975]], false),
            )
            .unwrap()
            .unwrap();
        assert_eq!(hit.face, Face::Up);
        assert_eq!(
            hit.face_location,
            [0.62810516, 0.94999695, 0.12804413].into()
        );
    }
    fn shapes(boxes: Vec<[f64; 6]>, passable: bool) -> Shapes {
        Shapes::read(&json!([{"id":"test:block","states":{},"boxes":boxes,"passable":passable}]))
            .unwrap()
    }
    fn cube() -> Shapes {
        shapes(vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]], false)
    }
    fn scene(position: [i32; 3]) -> Scene {
        Scene::new(vec![RayBlock {
            position,
            id: "test:block".into(),
            states: vec![],
        }])
        .unwrap()
    }
    fn options(distance: f64) -> Options {
        Options {
            max_distance: distance,
            include_passable: false,
            include_liquids: false,
        }
    }
    #[test]
    fn fractional_distance_and_endpoint_exclusion() {
        let s = scene([0, 0, 0]);
        let shapes = cube();
        assert!(
            s.cast(
                [2.5, 0.25, 0.25],
                [-1.0, 0.0, 0.0],
                options(1.99999),
                &shapes
            )
            .unwrap()
            .is_none()
        );
        let hit = s
            .cast([2.5, 0.25, 0.25], [-1.0, 0.0, 0.0], options(2.0), &shapes)
            .unwrap()
            .unwrap();
        assert_eq!(hit.face, Face::East);
        assert_eq!(hit.face_location, [0.0, 0.25, 0.25].into());
        let half = super::tests::shapes(vec![[0.0, 0.0, 0.0, 0.5, 0.5, 1.0]], false);
        assert!(
            s.cast([2.5, 0.25, 0.25], [-1.0, 0.0, 0.0], options(2.00001), &half)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn cell_ownership_and_inside_exit() {
        let s = scene([0, 0, 0]);
        let shapes = cube();
        assert_eq!(
            s.cast([0.25; 3], [1.0, 0.0, 0.0], options(3.0), &shapes)
                .unwrap()
                .unwrap()
                .face,
            Face::East
        );
        assert!(
            s.cast([1.0, 0.25, 0.25], [1.0, 0.0, 0.0], options(3.0), &shapes)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            s.cast([0.0, 0.25, 0.25], [-1.0, 0.0, 0.0], options(3.0), &shapes)
                .unwrap()
                .unwrap()
                .face,
            Face::West
        );
        assert!(
            s.cast([0.25; 3], [1.0, 0.0, 0.0], options(0.0), &shapes)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn grid_transition_budget_is_separate_from_geometric_range() {
        let s = scene([0, -1, 0]);
        let shapes = cube();
        let o = [1.3046581265, -1.75, -0.072192445];
        let d = [-0.416044093, 0.866584429, 0.275569845];
        assert!(s.cast(o, d, options(1.5), &shapes).unwrap().is_none());
        assert_eq!(
            s.cast(o, d, options(3.0), &shapes).unwrap().unwrap().face,
            Face::Down
        );
    }
    #[test]
    fn passable_unknown_and_lazy_lookup() {
        let s = scene([0, 0, 0]);
        let carpet = shapes(vec![[0.0, 0.0, 0.0, 1.0, 0.0625, 1.0]], true);
        assert!(
            s.cast([0.5, 2.5, 0.5], [0.0, -1.0, 0.0], options(3.0), &carpet)
                .unwrap()
                .is_none()
        );
        let mut opt = options(3.0);
        opt.include_passable = true;
        assert_eq!(
            s.cast([0.5, 2.5, 0.5], [0.0, -1.0, 0.0], opt, &carpet)
                .unwrap()
                .unwrap()
                .face_location
                .y,
            0.0625
        );
        let result = cast(
            [-2.5, 0.25, 0.25],
            [1.0, 0.0, 0.0],
            options(20.0),
            &cube(),
            |pos| {
                if pos[0] > 0 {
                    return Err("Unrecorded column behind hit".into());
                }
                Ok(s.blocks.get(&pos).cloned())
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.position, [0, 0, 0].into());
        let unknown = Scene::new(vec![RayBlock {
            position: [0, 0, 0],
            id: "test:unknown".into(),
            states: vec![],
        }])
        .unwrap();
        assert!(
            unknown
                .cast([0.5, 2.5, 0.5], [0.0, -1.0, 0.0], opt, &carpet)
                .is_err()
        );
    }
    #[test]
    fn stair_octants_keep_internal_faces() {
        let mut boxes = vec![];
        for z in 0..2 {
            for y in 0..2 {
                for x in 0..2 {
                    if y == 1 || x == 1 {
                        boxes.push([
                            x as f64 * 0.5,
                            y as f64 * 0.5,
                            z as f64 * 0.5,
                            (x + 1) as f64 * 0.5,
                            (y + 1) as f64 * 0.5,
                            (z + 1) as f64 * 0.5,
                        ]);
                    }
                }
            }
        }
        let hit = scene([0, 0, 0])
            .cast(
                [0.5; 3],
                [-1.0, 0.0, 0.0],
                options(3.0),
                &shapes(boxes, false),
            )
            .unwrap()
            .unwrap();
        assert_eq!(hit.face, Face::West);
        assert_eq!(hit.face_location.x, 0.5);
    }
    #[test]
    fn local_intersection_and_world_rounding_regressions() {
        let s = scene([-4, 48, -14]);
        let shapes = cube();
        let hit = s
            .cast(
                [2.8142908160533615, 49.4972857156875, -16.539700018208123],
                [-5.977295318997227, -0.7326263641755304, 2.6754454795471965],
                options(24.0),
                &shapes,
            )
            .unwrap()
            .unwrap();
        assert_eq!(hit.face, Face::East);
        assert_eq!(
            hit.face_location,
            Coordinates {
                x: 0.0,
                y: 0.78463745,
                z: 0.06278515
            }
        );
        let hit = s
            .cast(
                [3.579234327987913, 46.88286920328677, -15.653477633091024],
                [-6.865293443819683, 1.3152550995138341, 1.8275533431750013],
                options(24.0),
                &shapes,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            hit.face_location,
            Coordinates {
                x: 4.7683716e-7,
                y: 0.14332199,
                z: 0.09792614
            }
        );
    }
    #[test]
    fn rounded_endpoint_selects_corner_face_in_world_coordinates() {
        let hit = scene([-126, 64, -14])
            .cast(
                [-127.75, 66.25, -15.75],
                [2.25, -1.75, 2.25],
                options(12.0),
                &cube(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(hit.face, Face::North);
        assert_eq!(
            hit.face_location,
            Coordinates {
                x: 0.0,
                y: 0.8888855,
                z: 0.0
            }
        );
    }
    #[test]
    fn invalid_inputs_and_shapes_fail_explicitly() {
        let s = scene([0, 0, 0]);
        let shapes = cube();
        for d in [[0.0; 3], [f64::NAN, 0.0, 0.0], [f64::MAX, 0.0, 0.0]] {
            assert!(s.cast([0.0; 3], d, options(3.0), &shapes).is_err());
        }
        assert!(
            s.cast(
                [i32::MAX as f64, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                options(3.0),
                &shapes
            )
            .is_err()
        );
        assert!(
            s.cast([0.0; 3], [1.0, 0.0, 0.0], options(4097.0), &shapes)
                .is_err()
        );
        let mut opt = options(1.0);
        opt.include_liquids = true;
        assert!(s.cast([0.0; 3], [1.0, 0.0, 0.0], opt, &shapes).is_err());
        assert!(
            Shapes::read(
                &json!([{"id":"bad","states":{},"boxes":[[0,0,0,2,1,1]],"passable":false}])
            )
            .is_err()
        );
        let row = json!({"id":"test","states":{},"boxes":[],"passable":false});
        assert!(Shapes::read(&json!([row, row])).is_err());
    }
}
