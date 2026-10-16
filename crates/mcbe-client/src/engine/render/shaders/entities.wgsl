@vertex fn vs_entity(@location(0) position:vec3<f32>,@location(1) uv:vec2<f32>,@location(2) layer:u32,@location(3) shade:f32,@location(4) bary:vec3<f32>,@location(5) m0:vec4<f32>,@location(6) m1:vec4<f32>,@location(7) m2:vec4<f32>,@location(8) m3:vec4<f32>,@location(9) metadata:vec4<u32>)->Out {
 let model=mat4x4(m0,m1,m2,m3);let world=(model*vec4(position,1.)).xyz;var o:Out;o.position=camera.matrix*vec4(world,1.);o.world=world;o.relative=world-camera.eye.xyz;o.uv=uv;o.layer=(metadata.x+layer)|0x80000000u;o.bary=bary;o.entity=1u;
 let normals=array<vec3<f32>,6>(vec3(0.,1.,0.),vec3(0.,-1.,0.),vec3(0.,0.,-1.),vec3(0.,0.,1.),vec3(1.,0.,0.),vec3(-1.,0.,0.));let normal=normalize((model*vec4(normals[layer%6u],0.)).xyz);o.shade=0.5+0.5*max(dot(normal,normalize(vec3(-0.4,0.8,-0.25))),0.);
 o.tint=select(select(vec3(1.),vec3(0.8,1.,0.85),(metadata.y&2u)!=0u),vec3(1.,0.35,0.35),(metadata.y&1u)!=0u);return o;
}
