// A single mirror ray represents only the narrow specular lobe. Rough
// materials keep diffuse/direct lighting without a sharp reflected image.
fn mirror_response(roughness:f32)->f32 {
 let gloss=clamp(1.-roughness,0.,1.);
 return gloss*gloss*gloss*gloss;
}
// Hybrid primary visibility with real hardware queries against nearby worker geometry.
struct RayScene {anchor:vec4<f32>,center:vec4<f32>,options:vec4<f32>,limits:vec4<f32>};
@group(1) @binding(0) var ray_scene:acceleration_structure;
@group(1) @binding(1) var<storage,read> ray_vertices:array<u32>;
@group(1) @binding(2) var<uniform> scene:RayScene;
@group(2) @binding(0) var ray_terrain:texture_2d_array<f32>;
@group(2) @binding(1) var ray_sampler:sampler;
struct Hit {found:bool,t:f32,point:vec3<f32>,normal:vec3<f32>,color:vec4<f32>,material:vec4<f32>};
fn vertex_position(index:u32)->vec3<f32>{let o=index*10u;return vec3(bitcast<f32>(ray_vertices[o]),bitcast<f32>(ray_vertices[o+1u]),bitcast<f32>(ray_vertices[o+2u]));}
fn vertex_uv(index:u32)->vec2<f32>{let o=index*10u;return vec2(bitcast<f32>(ray_vertices[o+3u]),bitcast<f32>(ray_vertices[o+4u]));}
fn hit_color(base:u32,b:vec2<f32>)->vec4<f32>{let uv=vertex_uv(base)*(1.-b.x-b.y)+vertex_uv(base+1u)*b.x+vertex_uv(base+2u)*b.y;let layer=ray_vertices[base*10u+5u]&0x003fffffu;return textureSampleLevel(ray_terrain,ray_sampler,uv,i32(layer),0.);}
fn trace(origin:vec3<f32>,direction:vec3<f32>,requested:f32,mask:u32)->Hit{
 var hit:Hit;hit.found=false;hit.t=requested;
 let offset=origin-scene.center.xyz;let b=dot(offset,direction);let disc=b*b-dot(offset,offset)+scene.center.w*scene.center.w;
 if disc<=0.{return hit;}
 let limit=min(requested,-b+sqrt(disc)-0.01);if limit<=0.02{return hit;}
 var query:ray_query;rayQueryInitialize(&query,ray_scene,RayDesc(0u,mask,0.02,limit,origin,direction));
 while rayQueryProceed(&query){
  let c=rayQueryGetCandidateIntersection(&query);
  if c.kind==RAY_QUERY_INTERSECTION_TRIANGLE {
   let base=c.instance_custom_data+c.primitive_index*3u;
   let flags=ray_vertices[base*10u+5u];let color=hit_color(base,c.barycentrics);
   if ((flags&0x20000000u)==0u || color.a>=0.5) && ((flags&0x40000000u)==0u || color.a>=0.004){rayQueryConfirmIntersection(&query);}
  }
 }
 let c=rayQueryGetCommittedIntersection(&query);
 if c.kind!=RAY_QUERY_INTERSECTION_NONE {
  let base=c.instance_custom_data+c.primitive_index*3u;let flags=ray_vertices[base*10u+5u];
  let a=vertex_position(base);let b=vertex_position(base+1u);let d=vertex_position(base+2u);
  var normal=normalize(cross(b-a,d-a));if dot(normal,direction)>0.{normal=-normal;}
  hit.found=true;hit.t=c.t;hit.point=origin+direction*c.t;hit.normal=normal;hit.color=hit_color(base,c.barycentrics);
  if (flags&0x40000000u)==0u {hit.color.a=1.;}
  hit.material=materials.rows[min((flags&0x003fffffu)/6u,255u)];
 }
 return hit;
}
fn sun_visibility(p:vec3<f32>,n:vec3<f32>)->f32{
 if frame.parameters.z<0.5{return 1.;}
 let h=trace(p+n*0.035,frame.sun.xyz,scene.center.w*2.,1u);return select(1.,0.,h.found);
}
fn sky_radiance(direction:vec3<f32>)->vec3<f32>{return mix(vec3(0.11,0.13,0.16),vec3(0.40,0.49,0.62),direction.y*0.5+0.5);}
fn radiance(hit:Hit,direction:vec3<f32>)->vec3<f32>{
 let direct=brdf(hit.color.rgb,hit.normal,-direction,frame.sun.xyz,hit.material.x,hit.material.y)*vec3(1.,0.94,0.84)*max(dot(hit.normal,frame.sun.xyz),0.)*3.2*sun_visibility(hit.point,hit.normal);
 return sky_radiance(hit.normal)*hit.color.rgb*(1.-hit.material.y*0.65)+direct+hit.color.rgb*hit.material.z;
}
fn reflected_color(origin:vec3<f32>,direction:vec3<f32>)->vec3<f32>{
 var p=origin;var color=vec3(0.);var throughput=1.;
 for(var step=0u;step<4u;step++){
  let hit=trace(p,direction,scene.center.w*2.,3u);
  if !hit.found{return color+throughput*sky_radiance(direction);}
  color+=throughput*hit.color.a*radiance(hit,direction);throughput*=1.-hit.color.a;
  if throughput<0.01{return color;}
  p=hit.point+direction*0.04;
 }
 return color+throughput*sky_radiance(direction);
}
fn ray_noise(p:vec2<u32>)->f32{var n=p.x*1973u+p.y*9277u+8917u;n=(n^(n>>16u))*2246822519u;n=(n^(n>>13u))*3266489917u;return f32(n&65535u)/65536.;}
@fragment fn fs_ray(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32>{
 let pixel=vec2<i32>(position.xy);let z=textureLoad(depth,pixel,0);
 if z>=1. || scene.options.w<0.5 || frame.parameters.w>0.5{return shade_deferred(position);}
 let p_world=world(pixel,z);let p=p_world-scene.anchor.xyz;let weight=1.-smoothstep(scene.center.w*0.65,scene.center.w*0.9,distance(p,scene.center.xyz));if weight<=0.{return shade_deferred(position);}
 var fallback=vec4(0.);if weight<1.{fallback=shade_deferred(position);}
 let base=textureLoad(albedo,pixel,0).rgb;let data=textureLoad(normals,pixel,0);let n=normalize(data.xyz);let material=materials.rows[min(u32(round(data.w)),255u)];let view=normalize(frame.eye.xyz-p_world);
 // Dynamic actor parts cast conservative raster sun shadows beside hardware world queries.
 var shadow=sun_visibility(p,n);if frame.fog.w>0.5{shadow=min(shadow,visibility(p_world,n));}
 let direct=brdf(base,n,view,frame.sun.xyz,material.x,material.y)*vec3(1.,0.94,0.84)*max(dot(n,frame.sun.xyz),0.)*3.2*shadow;
 var ao=1.;var indirect=vec3(0.);let samples=u32(scene.options.z);
 if frame.parameters.y>0.5 || scene.options.y>0.5 {
  let tangent=normalize(cross(select(vec3(0.,1.,0.),vec3(1.,0.,0.),abs(n.y)>0.9),n));let bitangent=cross(n,tangent);let rotation=ray_noise(vec2<u32>(pixel))*6.2831853;var occlusion=0.;
  for(var i=0u;i<samples;i++){
   let r=sqrt((f32(i)+0.5)/f32(samples));let angle=rotation+f32(i)*2.39996323;let direction=normalize(tangent*(r*cos(angle))+bitangent*(r*sin(angle))+n*sqrt(1.-r*r));
   let hit=trace(p+n*0.035,direction,select(1.8,8.,scene.options.y>0.5),1u);
   if hit.found {
    occlusion+=max(0.,1.-hit.t/1.8);
    if scene.options.y>0.5{indirect+=radiance(hit,direction)*0.35;}
   }
  }
  if frame.parameters.y>0.5{ao=clamp(1.-occlusion/f32(samples)*0.8,0.2,1.);}
  indirect/=f32(samples);
 }
 let ambient=sky_radiance(n)*base*(1.-material.y*0.65)*ao;
 var reflection=vec3(0.);
 let mirror=mirror_response(material.x);
 if scene.options.x>0.5 && mirror>0.002 {
  let fresnel=mix(vec3(0.04),base,material.y)+(vec3(1.)-mix(vec3(0.04),base,material.y))*pow(1.-max(dot(n,view),0.),5.);
  reflection=reflected_color(p+n*0.035,reflect(-view,n))*fresnel*mirror;
 }
 let hdr=ambient+direct+indirect*base*(1.-material.y)+reflection+base*material.z;let rgb=vec3(1.)-exp(-hdr*frame.parameters.x);
 let sky=frame.medium.xyz;let d=select(distance(p_world,frame.eye.xyz),distance(p_world.xz,frame.eye.xz),frame.fog.z>0.5 && frame.medium.w<0.5);let fog=clamp((d-frame.fog.x)/(frame.fog.y-frame.fog.x),0.,1.);
 return mix(fallback,vec4(mix(rgb,sky,fog),1.),weight);
}
