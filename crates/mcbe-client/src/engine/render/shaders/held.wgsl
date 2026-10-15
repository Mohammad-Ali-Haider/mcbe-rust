struct Camera{matrix:mat4x4<f32>,eye:vec4<f32>,settings:vec4<f32>,focus:vec4<f32>,lod:vec4<f32>,lod_old:vec4<f32>};
@group(0) @binding(0) var terrain:texture_2d_array<f32>;
@group(0) @binding(1) var terrain_sampler:sampler;
@group(0) @binding(2) var<uniform> camera:Camera;
@group(1) @binding(0) var<uniform> matrix:mat4x4<f32>;
struct Out{@builtin(position) position:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) @interpolate(flat) layer:u32,@location(2) shade:f32,@location(3) bary:vec3<f32>};
@vertex fn vs(@location(0) position:vec3<f32>,@location(1) uv:vec2<f32>,@location(2) layer:u32,@location(3) shade:f32,@location(4) bary:vec3<f32>)->Out{var o:Out;o.position=matrix*vec4(position,1.);o.uv=uv;o.layer=layer;o.shade=shade;o.bary=bary;return o;}
@fragment fn fs(i:Out)->@location(0) vec4<f32>{let pixel=textureSample(terrain,terrain_sampler,i.uv,i32(i.layer&0x003fffffu));if pixel.a<0.004 || ((i.layer&0x20000000u)!=0u && pixel.a<0.5) {discard;}var rgb=pixel.rgb*i.shade;if camera.settings.x>0.5 {let edge=min(min(i.bary.x,i.bary.y),i.bary.z);if edge<max(max(fwidth(i.bary.x),fwidth(i.bary.y)),fwidth(i.bary.z))*0.8 {rgb=vec3(0.05,0.9,0.3);}}return vec4(rgb,pixel.a);}
