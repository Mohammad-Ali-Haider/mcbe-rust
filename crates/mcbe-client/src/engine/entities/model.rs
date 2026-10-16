//! Shared local cuboid poses for rendering, visible-model picking and conservative bounds.
use super::{components::Entity, definition::Part};
use glam::{Mat4, Vec3};
pub struct Pose {
    root: Mat4,
    walk: f32,
    gait: f32,
}
impl Pose {
    pub fn new(
        e: &Entity,
        position: [f32; 3],
        animations: &crate::engine::animation::Library,
    ) -> Self {
        let walk = e
            .physics
            .as_ref()
            .map_or(0., |p| {
                (p.velocity[0] * p.velocity[0] + p.velocity[2] * p.velocity[2]).sqrt() * 20.
                    / p.speed.max(0.01)
            })
            .min(1.);
        Self {
            root: Mat4::from_translation(Vec3::from(position))
                * Mat4::from_rotation_y(e.transform.yaw),
            walk,
            gait: animations.wave(
                "entity_walk",
                (e.visual.as_ref().map_or(0., |v| v.gait)
                    - super::components::distance(position, e.transform.position) * 7.)
                    / std::f32::consts::TAU,
            ),
        }
    }
    pub fn part(&self, part: &Part) -> Mat4 {
        let low = Vec3::new(part.bounds[0], part.bounds[1], part.bounds[2]);
        let high = Vec3::new(part.bounds[3], part.bounds[4], part.bounds[5]);
        let pivot = Vec3::from(part.pivot);
        self.root
            * Mat4::from_translation(pivot)
            * Mat4::from_rotation_x(self.gait * part.swing * self.walk * 0.6)
            * Mat4::from_translation(-pivot)
            * part.transform.map_or_else(
                || Mat4::from_translation(low) * Mat4::from_scale(high - low),
                |m| Mat4::from_cols_array_2d(&m),
            )
    }
}
pub fn radius(parts: &[Part]) -> f32 {
    parts
        .iter()
        .map(|p| {
            Vec3::new(
                p.bounds[0].abs().max(p.bounds[3].abs()),
                p.bounds[1].abs().max(p.bounds[4].abs()),
                p.bounds[2].abs().max(p.bounds[5].abs()),
            )
            .length()
                + 2. * Vec3::from(p.pivot).length()
        })
        .fold(0f32, f32::max)
}
