@group(0) @binding(0) var atlas:texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler:sampler;
@group(1) @binding(0) var<uniform> matrix:mat4x4<f32>;
struct Out{@builtin(position) position:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) color:vec4<f32>};
@vertex fn vs(@location(0) position:vec3<f32>,@location(1) uv:vec2<f32>,@location(2) color:vec4<f32>)->Out{var o:Out;o.position=matrix*vec4(position,1.);o.uv=uv;o.color=color;return o;}
@fragment fn fs(i:Out)->@location(0) vec4<f32>{let pixel=textureSample(atlas,atlas_sampler,i.uv);if pixel.a<0.5 {discard;}return pixel*i.color;}
