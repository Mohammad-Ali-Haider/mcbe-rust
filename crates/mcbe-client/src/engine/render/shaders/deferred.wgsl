@group(0) @binding(0) var albedo:texture_2d<f32>;
@group(0) @binding(1) var normals:texture_2d<f32>;
@group(0) @binding(2) var depth:texture_depth_2d;
@group(0) @binding(3) var shadow_depth:texture_depth_2d;
@group(0) @binding(4) var<uniform> frame:Frame;
struct Materials {rows:array<vec4<f32>,256>};
@group(0) @binding(5) var<uniform> materials:Materials;
@vertex fn vs(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32>{let p=array<vec2<f32>,3>(vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.));return vec4(p[index],0.,1.);}
fn world(pixel:vec2<i32>,z:f32)->vec3<f32>{let size=vec2<f32>(textureDimensions(depth));let uv=(vec2<f32>(pixel)+vec2(0.5))/size;let p=frame.inverse*vec4(uv.x*2.-1.,1.-uv.y*2.,z,1.);return p.xyz/p.w;}
fn visibility(p:vec3<f32>,n:vec3<f32>)->f32{
 if frame.parameters.z<0.5{return 1.;}
 let clip=frame.light*vec4(p+n*0.025,1.);let q=clip.xyz/clip.w;let uv=vec2(q.x*0.5+0.5,0.5-q.y*0.5);
 if q.z<0. || q.z>1. || any(uv<vec2(0.)) || any(uv>=vec2(1.)){return 1.;}
 let size=vec2<i32>(textureDimensions(shadow_depth));let base=vec2<i32>(uv*vec2<f32>(size));var sum=0.;var count=0.;let radius=select(0,1,frame.sampling.z>4.5);
 for(var y=-radius;y<=radius;y++){for(var x=-radius;x<=radius;x++){let coord=clamp(base+vec2(x,y),vec2(0),size-vec2(1));let stored=textureLoad(shadow_depth,coord,0);sum+=select(0.,1.,q.z-0.0005<=stored);count+=1.;}}
 let fade=smoothstep(frame.sampling.x*0.65,frame.sampling.x*0.88,distance(p.xz,frame.eye.xz));return mix(sum/max(count,1.),1.,fade);
}
fn ambient_occlusion(pixel:vec2<i32>,p:vec3<f32>,n:vec3<f32>)->f32{
 if frame.parameters.y<0.5{return 1.;}
 let samples=array<vec2<f32>,12>(vec2(0.35,0.11),vec2(-0.12,0.43),vec2(-0.52,-0.16),vec2(0.19,-0.61),vec2(0.67,0.39),vec2(-0.38,0.73),vec2(-0.79,-0.43),vec2(0.51,-0.78),vec2(0.91,-0.12),vec2(-0.89,0.31),vec2(0.11,0.97),vec2(-0.16,-0.98));
 let size=vec2<i32>(textureDimensions(depth));let radius=min(64.,1.8*f32(size.y)/(max(distance(p,frame.eye.xyz),0.1)*2.*frame.sampling.w));var occlusion=0.;
 for(var i=0u;i<u32(frame.sampling.z);i++){let coord=pixel+vec2<i32>(round(samples[i]*radius));if any(coord<vec2(0)) || any(coord>=size){continue;}let z=textureLoad(depth,coord,0);if z>=1.{continue;}let delta=world(coord,z)-p;let length=length(delta);if length>0.03 && length<1.8{occlusion+=max(0.,dot(n,delta/length)-0.15)*(1.-length/1.8);}}
 return clamp(1.-occlusion*2.2/frame.sampling.z,0.2,1.);
}
fn brdf(base:vec3<f32>,n:vec3<f32>,v:vec3<f32>,l:vec3<f32>,roughness:f32,metallic:f32)->vec3<f32>{
 let h=normalize(v+l);let nv=max(dot(n,v),0.001);let nl=max(dot(n,l),0.001);let nh=max(dot(n,h),0.);let vh=max(dot(v,h),0.);let alpha=roughness*roughness;let a2=alpha*alpha;let denom=nh*nh*(a2-1.)+1.;let distribution=a2/(3.14159265*denom*denom);let k=(roughness+1.)*(roughness+1.)/8.;let geometry=(nv/(nv*(1.-k)+k))*(nl/(nl*(1.-k)+k));let f0=mix(vec3(0.04),base,metallic);let fresnel=f0+(vec3(1.)-f0)*pow(1.-vh,5.);let diffuse=(vec3(1.)-fresnel)*(1.-metallic)*base/3.14159265;return diffuse+distribution*geometry*fresnel/(4.*nv*nl);
}
@fragment fn fs(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32>{
 let pixel=vec2<i32>(position.xy);let z=textureLoad(depth,pixel,0);let sky=frame.medium.xyz;if z>=1.{return vec4(sky,1.);}
 let base=textureLoad(albedo,pixel,0).rgb;let data=textureLoad(normals,pixel,0);let n=normalize(data.xyz);let p=world(pixel,z);let material=materials.rows[min(u32(round(data.w)),255u)];let v=normalize(frame.eye.xyz-p);let sun=frame.sun.xyz;let light=max(dot(n,sun),0.);let ao=ambient_occlusion(pixel,p,n);let shadow=visibility(p,n);
 let ambient=mix(vec3(0.11,0.13,0.16),vec3(0.40,0.49,0.62),n.y*0.5+0.5)*base*(1.-material.y*0.65)*ao;
 let direct=brdf(base,n,v,sun,material.x,material.y)*vec3(1.,0.94,0.84)*light*3.2*shadow;let hdr=ambient+direct+base*material.z;var rgb=vec3(1.)-exp(-hdr*frame.parameters.x);if frame.parameters.w>0.5{rgb=base;}
 let d=select(distance(p,frame.eye.xyz),distance(p.xz,frame.eye.xz),frame.fog.z>0.5 && frame.medium.w<0.5);let fog=clamp((d-frame.fog.x)/(frame.fog.y-frame.fog.x),0.,1.);return vec4(mix(rgb,sky,fog),1.);
}
