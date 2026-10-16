struct Camera{matrix:mat4x4<f32>,eye:vec4<f32>,settings:vec4<f32>,focus:vec4<f32>,lod:vec4<f32>,lod_old:vec4<f32>,medium:vec4<f32>,lod_range:vec4<f32>};
@group(0) @binding(0) var terrain:texture_2d_array<f32>;
@group(0) @binding(1) var terrain_sampler:sampler;
@group(0) @binding(2) var<uniform> camera:Camera;
@group(0) @binding(3) var creature_skins:texture_2d_array<f32>;
@group(0) @binding(4) var<storage,read> detail_coverage:array<vec4<u32>>;
fn detail_at(k:vec3<i32>)->f32 {
 let u=bitcast<vec3<u32>>(k);let mask=arrayLength(&detail_coverage)-1u;let h=(u.x*73856093u)^(u.y*19349663u)^(u.z*83492791u);
 for(var probe=0u;probe<64u;probe++){let s=detail_coverage[(h+probe)&mask];if s.w==0xffffffffu{return -1.;}if all(s.xyz==u){return bitcast<f32>(s.w);}}return -1.;
}
fn detail_weight(world:vec3<f32>,normal:vec3<f32>)->f32 {
 let p=(world-normal*0.001)/16.;let k=vec3<i32>(floor(p));let own=detail_at(k);if own<=0. {return own;}
 var weight=own;let local=fract(p)*16.;
 for(var axis=0u;axis<3u;axis++){let low=local[axis]<8.;let d=select(16.-local[axis],local[axis],low);if d<4. {var step=vec3<i32>(0);step[axis]=select(1,-1,low);let neighbour=max(detail_at(k+step),0.);weight=min(weight,mix(neighbour,own,smoothstep(0.,4.,d)));}}
 return weight;
}
struct Out{@builtin(position) position:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) @interpolate(flat) layer:u32,@location(2) shade:f32,@location(3) bary:vec3<f32>,@location(4) world:vec3<f32>,@location(5) relative:vec3<f32>,@location(6) @interpolate(flat) tint:vec3<f32>,@location(7) @interpolate(flat) entity:u32,@location(8) @interpolate(flat) skin:u32};
@vertex fn vs(@location(0) position:vec3<f32>,@location(1) uv:vec2<f32>,@location(2) layer:u32,@location(3) shade:f32,@location(4) bary:vec3<f32>)->Out{
    var o:Out;o.position=camera.matrix*vec4(position,1.);o.uv=uv;o.layer=layer;o.shade=shade;o.bary=bary;o.world=position;o.relative=position-camera.eye.xyz;o.tint=vec3(1.);o.entity=0u;o.skin=0u;return o;
}
@vertex fn vs_previous(@location(0) position:vec3<f32>,@location(1) uv:vec2<f32>,@location(2) layer:u32,@location(3) shade:f32,@location(4) bary:vec3<f32>)->Out{
    var o:Out;o.position=camera.matrix*vec4(position,1.);o.uv=uv;o.layer=layer|0x04000000u;o.shade=shade;o.bary=bary;o.world=position;o.relative=position-camera.eye.xyz;o.tint=vec3(1.);o.entity=0u;o.skin=0u;return o;
}
fn noise(p:vec2<u32>,salt:u32)->f32 {
    var n=p.x*1973u+p.y*9277u+salt*26699u;n=(n^(n>>16u))*2246822519u;n=(n^(n>>13u))*3266489917u;
    return f32(n&65535u)/65536.;
}
fn surface(i:Out,shaded:bool)->vec4<f32>{
    let layer=i.layer&0x003fffffu;
    let custom=(i.layer&0x80000000u)!=0u;
    let dx=dpdx(i.uv);let dy=dpdy(i.uv);var texel:vec4<f32>;
    if i.skin>0u {texel=textureSampleGrad(creature_skins,terrain_sampler,i.uv,i32(i.skin-1u),dx,dy);}else{texel=textureSampleGrad(terrain,terrain_sampler,i.uv,i32(layer),dx,dy);}
    let thickness=max(max(fwidth(i.bary.x),fwidth(i.bary.y)),fwidth(i.bary.z));
    if camera.lod.w>0.5 && i.entity==0u {
        let pixel=vec2<u32>(i.position.xy);
        let in_current=all(abs(i.world.xz-camera.lod.xy)<=vec2(camera.lod_range.x));
        let in_previous=camera.lod_old.z>0.5 && all(abs(i.world.xz-camera.lod_old.xy)<=vec2(camera.lod_range.y));
        let old=in_previous && (!in_current || noise(pixel,31u)>=camera.lod.z);
        let is_previous=(i.layer&0x04000000u)!=0u;
        let fine=(i.layer&0x10000000u)!=0u;
        let field=(i.layer&0x08000000u)!=0u;
        let object=(i.layer&0x03000000u)!=0u;
        let far=fine || field || object;
        if far && old!=is_previous {discard;}
        let origin=select(camera.lod.xy,camera.lod_old.xy,old);
        let offset=abs(i.world.xz-origin);
        let d=max(offset.x,offset.y);
        let normals=array<vec3<f32>,6>(vec3(0.,1.,0.),vec3(0.,-1.,0.),vec3(0.,0.,-1.),vec3(0.,0.,1.),vec3(1.,0.,0.),vec3(-1.,0.,0.));
        let readiness=detail_weight(i.world,normals[layer%6u]);
        // Unknown fallback cells retain near edits if the bounded lookup is full.
        let a=1.-select(select(1.,0.,far),readiness,readiness>=0.);
        let b=smoothstep(208.,256.,d);
        let value=noise(pixel,7u);
        let glass=(i.layer&0x40000000u)!=0u;
        if !far && (!glass || (i.layer&0x00400000u)!=0u) && value<a {discard;}
        let layered=fine && field;
        if layered && value>=a {discard;}
        if fine && !layered && (value>=a || value<a*b) {discard;}
        if field && !layered && value>=a*b {discard;}
        let trunk=(i.layer&0x00800000u)!=0u;
        let tree_detail=select(1.,1.-smoothstep(384.,640.,d),trunk);
        if object && value>=a*(1.-smoothstep(1024.,1536.,d))*tree_detail {discard;}
    }

    let cutout=(i.layer&0x20000000u)!=0u;
    let blended=(i.layer&0x40000000u)!=0u;
    if cutout && texel.a<0.5 {discard;}
    if blended && texel.a<0.004 {discard;}
    var rgb=texel.rgb*i.tint*select(1.,i.shade,shaded);
    if camera.settings.x>0.5 {
        let edge=min(min(i.bary.x,i.bary.y),i.bary.z);
        if edge<thickness*0.8 {rgb=vec3(0.05,0.9,0.3);}
    }
    if camera.focus.w>0.5 && i.entity==0u {
        let normals=array<vec3<f32>,6>(vec3(0.,1.,0.),vec3(0.,-1.,0.),vec3(0.,0.,-1.),vec3(0.,0.,1.),vec3(1.,0.,0.),vec3(-1.,0.,0.));
        let cell=floor(i.world-normals[layer%6u]*0.001);
        let tile=fract(i.uv);
        if all(abs(cell-camera.focus.xyz)<vec3(0.1)) {
            if custom {rgb=mix(rgb,vec3(0.1,0.9,0.55),0.2);}
            else if min(tile.x,tile.y)<0.018 || max(tile.x,tile.y)>0.982 {rgb=vec3(0.12);}
        }
    }
    return vec4(rgb,select(1.,texel.a,blended));
}
fn fogged(i:Out,color:vec4<f32>)->vec4<f32>{var rgb=color.rgb;
    let fog_distance=select(distance(i.world,camera.eye.xyz),distance(i.world.xz,camera.eye.xz),camera.settings.w>0.5 && camera.medium.w<0.5);
    let fog=clamp((fog_distance-camera.settings.y)/(camera.settings.z-camera.settings.y),0.,1.);rgb=mix(rgb,camera.medium.xyz,fog);
    return vec4(rgb,color.a);
}

@fragment fn fs(i:Out)->@location(0) vec4<f32>{return fogged(i,surface(i,true));}
struct GBuffer { @location(0) albedo:vec4<f32>, @location(1) normal:vec4<f32> };
@fragment fn fs_gbuffer(i:Out)->GBuffer{let normal=normalize(cross(dpdy(i.relative),dpdx(i.relative)));let color=surface(i,false);var g:GBuffer;g.albedo=vec4(color.rgb,1.);g.normal=vec4(normal,f32((i.layer&0x003fffffu)/6u));return g;}
