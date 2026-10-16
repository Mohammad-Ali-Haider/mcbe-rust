//! Hardware ray-query lighting, sharing the deferred primary surface pass.
pub(crate) mod arena;
pub(crate) mod scene;

pub fn shader() -> String {
    let lighting = include_str!("../../shaders/deferred.wgsl").replace(
        "@fragment fn fs(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32>",
        "fn shade_deferred(position:vec4<f32>)->vec4<f32>",
    );
    format!(
        "{}\n{}\n{}",
        include_str!("../../shaders/lighting_frame.wgsl"),
        lighting,
        include_str!("../../shaders/ray_lighting.wgsl")
    )
}
