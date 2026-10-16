@group(0) @binding(0) var terrain:texture_2d_array<f32>;
@group(0) @binding(1) var terrain_sampler:sampler;
@group(0) @binding(3) var creature_skins:texture_2d_array<f32>;
@group(1) @binding(0) var<uniform> frame:Frame;
struct ShadowVertex { @builtin(position) position:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) @interpolate(flat) layer:u32 };
@vertex fn vs(@location(0) position:vec3<f32>,@location(1) uv:vec2<f32>,@location(2) layer:u32)->ShadowVertex{var o:ShadowVertex;o.position=frame.light*vec4(position,1.);o.uv=uv;o.layer=layer;return o;}
@fragment fn fs(i:ShadowVertex){let dx=dpdx(i.uv);let dy=dpdy(i.uv);var alpha:f32;if (i.layer&0x01000000u)!=0u {alpha=textureSampleGrad(creature_skins,terrain_sampler,i.uv,i32(i.layer&0x003fffffu),dx,dy).a;}else{alpha=textureSampleGrad(terrain,terrain_sampler,i.uv,i32(i.layer&0x003fffffu),dx,dy).a;}if (i.layer&0x20000000u)!=0u && alpha<0.5 {discard;}}
