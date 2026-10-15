//! Local player cuboids projected into the inventory's depth-tested viewport.
use crate::scene::Result;
use glam::{Mat4, Vec3};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Clone, Serialize)]
pub struct Cube {
    pub bone: String,
    pub origin: [f32; 3],
    pub size: [f32; 3],
    pub uv: [f32; 2],
    pub pivot: [f32; 3],
    pub inflate: f32,
    pub mirror: bool,
}
#[derive(Clone, Serialize)]
pub struct Model {
    pub cubes: Vec<Cube>,
}
#[repr(C)]
#[derive(Clone, Copy, Serialize, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub rectangle: [f32; 4],
    pub head_angles: [f32; 2],
}
fn vector<const N: usize>(v: &Value) -> Result<[f32; N]> {
    let a = v
        .as_array()
        .filter(|a| a.len() == N)
        .ok_or("Player model vector dimension changed")?;
    let mut result = [0.; N];
    for (i, v) in a.iter().enumerate() {
        let f = v.as_f64().ok_or("Player model numeric vector required")? as f32;
        if !f.is_finite() {
            return Err("Player model nonfinite vector".into());
        }
        result[i] = f;
    }
    Ok(result)
}
impl Model {
    pub fn standard(skin: [u32; 2], slim: bool) -> Self {
        let width = if slim { 3. } else { 4. };
        let mut cubes = Vec::new();
        let mut add = |name: &str,
                       origin: [f32; 3],
                       size: [f32; 3],
                       uv: [f32; 2],
                       pivot: [f32; 3],
                       inflate: f32,
                       mirror: bool| {
            cubes.push(Cube {
                bone: name.into(),
                origin,
                size,
                uv,
                pivot,
                inflate,
                mirror,
            })
        };
        add(
            "head",
            [-4., 24., -4.],
            [8., 8., 8.],
            [0., 0.],
            [0., 24., 0.],
            0.,
            false,
        );
        add(
            "body",
            [-4., 12., -2.],
            [8., 12., 4.],
            [16., 16.],
            [0., 24., 0.],
            0.,
            false,
        );
        add(
            "rightArm",
            [-4. - width, 12., -2.],
            [width, 12., 4.],
            [40., 16.],
            [-5., 22., 0.],
            0.,
            false,
        );
        add(
            "leftArm",
            [4., 12., -2.],
            [width, 12., 4.],
            if skin[1] == 32 {
                [40., 16.]
            } else {
                [32., 48.]
            },
            [5., 22., 0.],
            0.,
            skin[1] == 32,
        );
        add(
            "rightLeg",
            [-4., 0., -2.],
            [4., 12., 4.],
            [0., 16.],
            [-2., 12., 0.],
            0.,
            false,
        );
        add(
            "leftLeg",
            [0., 0., -2.],
            [4., 12., 4.],
            if skin[1] == 32 { [0., 16.] } else { [16., 48.] },
            [2., 12., 0.],
            0.,
            skin[1] == 32,
        );
        add(
            "hat",
            [-4., 24., -4.],
            [8., 8., 8.],
            [32., 0.],
            [0., 24., 0.],
            0.5,
            false,
        );
        if skin[1] == 64 {
            add(
                "jacket",
                [-4., 12., -2.],
                [8., 12., 4.],
                [16., 32.],
                [0., 24., 0.],
                0.25,
                false,
            );
            add(
                "rightSleeve",
                [-4. - width, 12., -2.],
                [width, 12., 4.],
                [40., 32.],
                [-5., 22., 0.],
                0.25,
                false,
            );
            add(
                "leftSleeve",
                [4., 12., -2.],
                [width, 12., 4.],
                [48., 48.],
                [5., 22., 0.],
                0.25,
                false,
            );
            add(
                "rightPants",
                [-4., 0., -2.],
                [4., 12., 4.],
                [0., 32.],
                [-2., 12., 0.],
                0.25,
                false,
            );
            add(
                "leftPants",
                [0., 0., -2.],
                [4., 12., 4.],
                [0., 48.],
                [2., 12., 0.],
                0.25,
                false,
            );
        }
        Self { cubes }
    }
    pub fn parse(data: &Value, skin: [u32; 2]) -> Result<Self> {
        if skin != [64, 64] && skin != [64, 32] {
            return Err("Player preview requires a 64x64 or legacy 64x32 skin".into());
        }
        let base = data["geometry.humanoid"]["bones"]
            .as_array()
            .ok_or("Local humanoid geometry missing")?;
        let custom = data["geometry.humanoid.custom:geometry.humanoid"]["bones"]
            .as_array()
            .ok_or("Local custom humanoid geometry missing")?;
        let mut bones: BTreeMap<String, Value> = BTreeMap::new();
        for (bone, is_custom) in base
            .iter()
            .map(|b| (b, false))
            .chain(custom.iter().map(|b| (b, true)))
        {
            let name = bone["name"].as_str().ok_or("Player bone name missing")?;
            let current = bones.entry(name.to_owned()).or_insert_with(|| json!({}));
            if is_custom && bone.get("cubes").is_some() {
                *current = json!({});
            }
            for (k, v) in bone.as_object().ok_or("Player bone object required")? {
                current[k] = v.clone();
            }
        }
        let mut cubes = Vec::new();
        for (name, bone) in bones {
            if bone["neverRender"].as_bool() == Some(true) {
                continue;
            }
            let Some(boxes) = bone["cubes"].as_array() else {
                continue;
            };
            if skin[1] == 32
                && matches!(
                    name.as_str(),
                    "jacket" | "leftSleeve" | "rightSleeve" | "leftPants" | "rightPants"
                )
            {
                continue;
            }
            let pivot = vector::<3>(&bone["pivot"])?;
            for b in boxes {
                if b.get("rotation").is_some() || bone.get("rotation").is_some() {
                    return Err(
                        "Rotated local player geometry requires an updated preview parser".into(),
                    );
                }
                let origin = vector::<3>(&b["origin"])?;
                let size = vector::<3>(&b["size"])?;
                let mut uv = vector::<2>(&b["uv"])?;
                let mut mirror = b
                    .get("mirror")
                    .or_else(|| bone.get("mirror"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if skin[1] == 32 && name == "leftArm" {
                    uv = [40., 16.];
                    mirror = true;
                }
                if skin[1] == 32 && name == "leftLeg" {
                    uv = [0., 16.];
                    mirror = true;
                }
                let inflate = b["inflate"].as_f64().unwrap_or(0.) as f32;
                if size
                    .iter()
                    .any(|v| *v <= 0. || *v > 32. || *v + 2. * inflate <= 0.)
                    || origin.iter().chain(pivot.iter()).any(|v| v.abs() > 64.)
                    || !inflate.is_finite()
                    || inflate.abs() > 2.
                    || uv[0] < 0.
                    || uv[1] < 0.
                    || uv[0] + 2. * (size[0] + size[2]) > skin[0] as f32
                    || uv[1] + size[2] + size[1] > skin[1] as f32
                {
                    return Err("Local player cube exceeds bounded model/skin dimensions".into());
                }
                cubes.push(Cube {
                    bone: name.clone(),
                    origin,
                    size,
                    uv,
                    pivot,
                    inflate,
                    mirror,
                });
            }
        }
        if cubes.is_empty() || cubes.len() > 32 || !cubes.iter().any(|c| c.bone == "head") {
            return Err("Player preview requires 1..32 cubes including a head".into());
        }
        Ok(Self { cubes })
    }
    pub fn mesh(
        &self,
        rectangle: [f32; 4],
        pointer: [f32; 2],
        viewport: [f32; 2],
        skin: [f32; 4],
        crouched: bool,
    ) -> Mesh {
        let [x, y, w, h] = rectangle;
        let angles = [
            ((pointer[0] - (x + w / 2.)) / 100.)
                .atan()
                .clamp(-0.55, 0.55),
            ((pointer[1] - (y + h * 0.25)) / 110.)
                .atan()
                .clamp(-0.4, 0.4),
        ];
        let body = Mat4::from_rotation_y(0.25);
        let view = Mat4::look_at_rh(Vec3::new(0., 18., -60.), Vec3::new(0., 16., 0.), Vec3::Y);
        let project = Mat4::orthographic_rh(-13., 13., -17.5, 17.5, 1., 100.) * view;
        let mut vertices = Vec::with_capacity(self.cubes.len() * 36);
        for cube in &self.cubes {
            let p = Vec3::from(cube.pivot);
            let head = matches!(cube.bone.as_str(), "head" | "hat");
            let mut transform = body;
            if crouched {
                transform *= Mat4::from_translation(Vec3::new(0., -1.5, 0.));
            }
            if head {
                transform = transform
                    * Mat4::from_translation(p)
                    * Mat4::from_rotation_y(angles[0])
                    * Mat4::from_rotation_x(-angles[1])
                    * Mat4::from_translation(-p);
            }
            let first = vertices.len();
            cube.emit(&mut vertices, transform, skin);
            for v in &mut vertices[first..] {
                let point = project.transform_point3(Vec3::from(v.position));
                let sx = x + (point.x + 1.) * 0.5 * w;
                let sy = y + (1. - point.y) * 0.5 * h;
                v.position = [
                    sx / viewport[0] * 2. - 1.,
                    1. - sy / viewport[1] * 2.,
                    point.z,
                ];
            }
        }
        Mesh {
            vertices,
            rectangle,
            head_angles: angles,
        }
    }
}
impl Cube {
    pub fn emit(&self, vertices: &mut Vec<Vertex>, transform: Mat4, skin: [f32; 4]) {
        let lo = Vec3::from(self.origin) - Vec3::splat(self.inflate);
        let hi = Vec3::from(self.origin) + Vec3::from(self.size) + Vec3::splat(self.inflate);
        let [a, b, c] = lo.to_array();
        let [d, e, f] = hi.to_array();
        let [width, height, depth] = self.size;
        let [u, v] = self.uv;
        let quads = [
            (
                [[d, e, c], [a, e, c], [a, b, c], [d, b, c]],
                [u + depth, v + depth, width, height],
                0.94,
            ),
            (
                [[a, e, f], [d, e, f], [d, b, f], [a, b, f]],
                [u + depth + width + depth, v + depth, width, height],
                0.78,
            ),
            (
                [[d, e, f], [d, e, c], [d, b, c], [d, b, f]],
                [u + depth + width, v + depth, depth, height],
                0.82,
            ),
            (
                [[a, e, c], [a, e, f], [a, b, f], [a, b, c]],
                [u, v + depth, depth, height],
                0.86,
            ),
            (
                [[d, e, f], [a, e, f], [a, e, c], [d, e, c]],
                [u + depth, v, width, depth],
                1.,
            ),
            (
                [[d, b, c], [a, b, c], [a, b, f], [d, b, f]],
                [u + depth + width, v, width, depth],
                0.7,
            ),
        ];
        for (face, (positions, mut r, shade)) in quads.into_iter().enumerate() {
            if self.mirror && face == 2 {
                r = quads[3].1;
            } else if self.mirror && face == 3 {
                r = quads[2].1;
            }
            let [uu, vv, uw, vh] = r;
            let coords = if self.mirror {
                [[uu + uw, vv], [uu, vv], [uu, vv + vh], [uu + uw, vv + vh]]
            } else {
                [[uu, vv], [uu + uw, vv], [uu + uw, vv + vh], [uu, vv + vh]]
            };
            for index in [0, 2, 1, 0, 3, 2] {
                let point = transform.transform_point3(Vec3::from(positions[index]));
                vertices.push(Vertex {
                    position: point.to_array(),
                    uv: [
                        (skin[0] + coords[index][0]) / 1024.,
                        (skin[1] + coords[index][1]) / 1024.,
                    ],
                    color: [shade, shade, shade, 1.],
                });
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        json!({"geometry.humanoid":{"bones":[{"name":"head","pivot":[0,24,0],"cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]}]},"geometry.humanoid.custom:geometry.humanoid":{"bones":[]}})
    }
    #[test]
    fn skin_model_validation_projection_and_pointer_response_are_bounded() {
        let mut data = fixture();
        let model = Model::parse(&data, [64, 64]).unwrap();
        let left = model.mesh(
            [0., 0., 52., 70.],
            [-100., 0.],
            [480., 270.],
            [0., 0., 64., 64.],
            false,
        );
        let right = model.mesh(
            [0., 0., 52., 70.],
            [200., 100.],
            [480., 270.],
            [0., 0., 64., 64.],
            false,
        );
        assert_eq!(left.vertices.len(), 36);
        let neutral = model.mesh(
            [0., 0., 52., 70.],
            [26., 17.5],
            [480., 270.],
            [0., 0., 64., 64.],
            false,
        );
        let area = |at: usize| {
            let a = neutral.vertices[at].position;
            let b = neutral.vertices[at + 1].position;
            let c = neutral.vertices[at + 2].position;
            (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
        };
        assert!(area(0) > 0.);
        assert!(area(6) < 0.);
        assert!(left.head_angles[0] < 0. && right.head_angles[0] > 0.);
        assert!(
            left.vertices
                .iter()
                .all(|v| v.position.iter().all(|x| x.is_finite())
                    && v.position[2] > 0.
                    && v.position[2] < 1.
                    && v.uv.iter().all(|x| *x >= 0. && *x <= 64. / 1024.))
        );
        assert_ne!(
            bytemuck::cast_slice::<Vertex, u8>(&left.vertices),
            bytemuck::cast_slice::<Vertex, u8>(&right.vertices)
        );
        let view = Mat4::look_at_rh(Vec3::new(0., 18., -60.), Vec3::new(0., 16., 0.), Vec3::Y);
        let facing = |angles: [f32; 2]| {
            view.transform_vector3(
                (Mat4::from_rotation_y(0.25 + angles[0]) * Mat4::from_rotation_x(-angles[1]))
                    .transform_vector3(-Vec3::Z),
            )
        };
        assert!(facing(left.head_angles).x < 0.);
        assert!(facing(right.head_angles).x > 0.);
        assert!(facing(right.head_angles).y < 0.);
        data["geometry.humanoid"]["bones"][0]["cubes"][0]["uv"] = json!([63, 63]);
        assert!(Model::parse(&data, [64, 64]).is_err());
        assert!(Model::parse(&fixture(), [128, 128]).is_err());
    }
}
