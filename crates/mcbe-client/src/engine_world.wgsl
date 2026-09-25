struct Camera{matrix:mat4x4<f32>,eye:vec4<f32>,settings:vec4<f32>,focus:vec4<f32>};
@group(0) @binding(0) var terrain:texture_2d_array<f32>;
@group(0) @binding(1) var terrain_sampler:sampler;
@group(0) @binding(2) var<uniform> camera:Camera;
struct Out{@builtin(position) position:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) @interpolate(flat) layer:u32,@location(2) shade:f32,@location(3) bary:vec3<f32>,@location(4) world:vec3<f32>};
@vertex fn vs(@location(0) position:vec3<f32>,@location(1) uv:vec2<f32>,@location(2) layer:u32,@location(3) shade:f32,@location(4) bary:vec3<f32>)->Out{
    var o:Out;o.position=camera.matrix*vec4(position,1.);o.uv=uv;o.layer=layer;o.shade=shade;o.bary=bary;o.world=position;return o;
}
@fragment fn fs(i:Out)->@location(0) vec4<f32>{
    let layer=i.layer&0x1fffffffu;
    let custom=(i.layer&0x80000000u)!=0u;
    let texel=textureSample(terrain,terrain_sampler,i.uv,i32(layer));
    let cutout=(i.layer&0x20000000u)!=0u;
    let blended=(i.layer&0x40000000u)!=0u;
    if cutout && texel.a<0.5 {discard;}
    if blended && texel.a<0.004 {discard;}
    var rgb=texel.rgb*i.shade;
    if camera.settings.x>0.5 {
        let edge=min(min(i.bary.x,i.bary.y),i.bary.z);let thickness=max(max(fwidth(i.bary.x),fwidth(i.bary.y)),fwidth(i.bary.z));
        if edge<thickness*0.8 {rgb=vec3(0.05,0.9,0.3);}
    }
    if camera.focus.w>0.5 {
        let normals=array<vec3<f32>,6>(vec3(0.,1.,0.),vec3(0.,-1.,0.),vec3(0.,0.,-1.),vec3(0.,0.,1.),vec3(1.,0.,0.),vec3(-1.,0.,0.));
        let cell=floor(i.world-normals[layer%6u]*0.001);
        let tile=fract(i.uv);
        if all(abs(cell-camera.focus.xyz)<vec3(0.1)) {
            if custom {rgb=mix(rgb,vec3(0.1,0.9,0.55),0.2);}
            else if min(tile.x,tile.y)<0.018 || max(tile.x,tile.y)>0.982 {rgb=vec3(0.12);}
        }
    }
    let fog_distance=select(distance(i.world,camera.eye.xyz),distance(i.world.xz,camera.eye.xz),camera.settings.w>0.5);
    let fog=clamp((fog_distance-camera.settings.y)/(camera.settings.z-camera.settings.y),0.,1.);rgb=mix(rgb,vec3(0.62,0.77,0.89),fog);
    return vec4(rgb,select(1.,texel.a,blended));
}
