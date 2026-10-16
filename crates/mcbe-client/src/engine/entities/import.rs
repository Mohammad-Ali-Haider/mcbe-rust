//! Bounded legacy Bedrock box-net adapter. Imported bytes remain local resources.
use super::{assets::ModelResource, definition::Part};
use crate::scene::Result;
use glam::{EulerRot, Mat4, Vec3};
use serde_json::Value;
fn vector<const N: usize>(value: &Value) -> Result<[f32; N]> {
    let a = value.as_array().ok_or("Model vector required")?;
    if a.len() != N {
        return Err("Model vector length mismatch".into());
    }
    let mut out = [0.; N];
    for (i, n) in a.iter().enumerate() {
        out[i] = n.as_f64().ok_or("Numeric model vector required")? as f32;
        if !out[i].is_finite() {
            return Err("Nonfinite model vector".into());
        }
    }
    Ok(out)
}
pub fn model(data: &Value, source: &ModelResource) -> Result<Vec<Part>> {
    let geometry = data
        .get(&source.geometry)
        .ok_or("Requested entity geometry missing")?;
    let dimensions = [
        geometry["texturewidth"]
            .as_f64()
            .ok_or("Model texture width missing")? as f32,
        geometry["textureheight"]
            .as_f64()
            .ok_or("Model texture height missing")? as f32,
    ];
    if dimensions
        .iter()
        .any(|v| !v.is_finite() || *v < 1. || *v > 256.)
    {
        return Err("Model skin size invalid".into());
    }
    let bones = geometry["bones"].as_array().ok_or("Model bones missing")?;
    if bones.len() > 64 {
        return Err("Model bone limit exceeded".into());
    }
    let facing = Mat4::from_rotation_y(std::f32::consts::PI);
    let mut parts = Vec::new();
    for bone in bones {
        if bone["neverRender"].as_bool() == Some(true) {
            continue;
        }
        let name = bone["name"].as_str().ok_or("Model bone name missing")?;
        let pivot = bone
            .get("pivot")
            .map(vector::<3>)
            .transpose()?
            .unwrap_or([0.; 3]);
        // Source rotation angles use the opposite sign to our right-handed matrices.
        // Neutral bind orientation applies to this bone's cubes; child bind positions
        // are already expressed in model coordinates.
        let rotation = source
            .poses
            .get(name)
            .copied()
            .or(bone
                .get("bind_pose_rotation")
                .or(bone.get("rotation"))
                .map(vector::<3>)
                .transpose()?)
            .unwrap_or([0.; 3])
            .map(|v| -v.to_radians());
        let pivot = Vec3::from(pivot) / 16.;
        let rotate = Mat4::from_translation(pivot)
            * Mat4::from_euler(EulerRot::ZYX, rotation[2], rotation[1], rotation[0])
            * Mat4::from_translation(-pivot);
        let Some(cubes) = bone["cubes"].as_array() else {
            continue;
        };
        for cube in cubes {
            if parts.len() >= 32 {
                return Err("Entity model exceeds 32 cuboids".into());
            }
            if cube.get("rotation").is_some() || cube.get("pivot").is_some() {
                return Err("Cube-local rotations require an authored engine transform".into());
            }
            let origin = vector::<3>(&cube["origin"])?;
            let size = vector::<3>(&cube["size"])?;
            let uv = vector::<2>(&cube["uv"])?;
            if size.iter().any(|s| *s <= 0.) {
                return Err("Nonpositive model cuboid size".into());
            }
            let inflate = cube
                .get("inflate")
                .or(bone.get("inflate"))
                .and_then(Value::as_f64)
                .unwrap_or(0.) as f32;
            if !inflate.is_finite() || inflate.abs() > 2. {
                return Err("Model inflate invalid".into());
            }
            let low = (Vec3::from(origin) - Vec3::splat(inflate)) / 16.;
            let scale = (Vec3::from(size) + Vec3::splat(2. * inflate)) / 16.;
            let transform = facing * rotate * Mat4::from_translation(low) * Mat4::from_scale(scale);
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            for i in 0..8 {
                let q = transform.transform_point3(Vec3::new(
                    (i & 1) as f32,
                    ((i >> 1) & 1) as f32,
                    ((i >> 2) & 1) as f32,
                ));
                min = min.min(q);
                max = max.max(q);
            }
            let [w, h, d] = size;
            let [u, v] = uv;
            let mut rects = [
                [u + d, v, w, d],
                [u + d + w, v, w, d],
                [u + d, v + d, w, h],
                [u + d + w + d, v + d, w, h],
                [u + d + w, v + d, d, h],
                [u, v + d, d, h],
            ];
            let mirror = cube
                .get("mirror")
                .or(bone.get("mirror"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if mirror {
                rects.swap(4, 5);
                for r in &mut rects {
                    r[0] += r[2];
                    r[2] = -r[2];
                }
            }
            for r in &mut rects {
                r[0] /= dimensions[0];
                r[2] /= dimensions[0];
                r[1] /= dimensions[1];
                r[3] /= dimensions[1];
            }
            let pivot = facing.transform_point3(pivot).to_array();
            parts.push(Part {
                bounds: [min.x, min.y, min.z, max.x, max.y, max.z],
                material: source.material.clone(),
                swing: source.swings.get(name).copied().unwrap_or(0.),
                pivot,
                uv: Some(rects),
                transform: Some(transform.to_cols_array_2d()),
            });
        }
    }
    if parts.is_empty() {
        return Err("Resource model contains no visible cuboids".into());
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn neutral_bind_pose_rotates_body_without_rotating_child_bind_coordinates() {
        let source = ModelResource {
            pack: "vanilla".into(),
            path: "model.json".into(),
            geometry: "geometry.test".into(),
            material: "ivory".into(),
            swings: Default::default(),
            poses: Default::default(),
        };
        let data = serde_json::json!({"geometry.test":{"texturewidth":64,"textureheight":32,"bones":[{"name":"body","pivot":[0,16,0],"bind_pose_rotation":[90,0,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[0,0]}]},{"name":"head","parent":"body","mirror":true,"cubes":[{"origin":[-4,20,-12],"size":[8,8,8],"uv":[0,0]}]}]}});
        let parts = model(&data, &source).unwrap();
        assert_eq!(parts.len(), 2);
        assert!((parts[0].bounds[4] - parts[0].bounds[1] - 0.25).abs() < 0.0001);
        assert!((parts[1].bounds[1] - 1.25).abs() < 0.0001);
        assert!(parts[1].bounds[2] > 0.2);
        assert!(parts[1].uv.unwrap()[2][2] < 0.);
        for p in parts {
            super::super::assets::validate_part(&p).unwrap();
        }
        let mut bad = data.clone();
        bad["geometry.test"]["bones"][0]["cubes"][0]["size"] = serde_json::json!([0, 1, 1]);
        assert!(model(&bad, &source).is_err());
    }
}
