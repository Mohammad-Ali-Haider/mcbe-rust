@group(0) @binding(0) var<storage,read_write> vertices:array<u32>;
struct Chunk {range:vec4<u32>,origin:vec4<f32>};
@group(0) @binding(1) var<uniform> chunk:Chunk;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id:vec3<u32>){if id.x>=chunk.range.y{return;}let offset=(chunk.range.x+id.x)*10u;for(var i=0u;i<3u;i++){vertices[offset+i]=bitcast<u32>(bitcast<f32>(vertices[offset+i])-chunk.origin[i]);}}
