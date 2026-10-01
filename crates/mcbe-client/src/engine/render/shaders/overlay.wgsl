@group(0) @binding(0) var atlas:texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler:sampler;
struct Out{@builtin(position) position:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) color:vec4<f32>};
@vertex fn vs(@location(0) position:vec2<f32>,@location(1) uv:vec2<f32>,@location(2) color:vec4<f32>)->Out{var o:Out;o.position=vec4(position,0.,1.);o.uv=uv;o.color=color;return o;}
@fragment fn fs(i:Out)->@location(0) vec4<f32>{return textureSample(atlas,atlas_sampler,i.uv)*i.color;}
