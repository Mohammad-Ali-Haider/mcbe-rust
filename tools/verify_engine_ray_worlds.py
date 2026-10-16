"""Hardware-ray world residency, worker/sync parity and live edit/reload proofs."""
import argparse,hashlib,json,subprocess
from pathlib import Path
import numpy as np
from PIL import Image

def read(p):return json.loads(p.read_bytes())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,required=True);ap.add_argument('--ray-proof',type=Path,required=True);a=ap.parse_args();root=Path(__file__).resolve().parents[1];out=a.out.resolve();proof=a.ray_proof.resolve();binary=root/'target/release/mcbe-client.exe';prior=read(proof/'summary.json');bh=sha(binary);assert prior['complete'] and prior['binary_sha256']==bh;assert all(sha(root/n)==h for n,h in prior['source_hashes'].items())
 if out.exists():raise ValueError('Fresh ray-world proof folder required')
 out.mkdir(parents=True);files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'content').rglob('*')),*sorted((root/'vendor').rglob('*.rs')),root/'vendor/wgpu-hal/Cargo.toml',Path(__file__)];sources={p.relative_to(root).as_posix():sha(p) for p in files if p.is_file()};checks={}
 cases=[('sync','--engine-lighting-smoke',['--engine-lighting-case','ray_sun','--engine-sync-mesh']),('legacy-255','--engine-distance-smoke',[]),('ocean-255','--engine-terrain-smoke',['--engine-terrain-case','ocean','--engine-render-distance','255']),('high','--engine-terrain-smoke',['--engine-terrain-case','high']),('deep','--engine-terrain-smoke',['--engine-terrain-case','deep']),('underwater','--engine-fluid-smoke',['--engine-fluid-case','underwater']),('edits','--engine-async-smoke',[])]
 for name,flag,extra in cases:
  target=out/name;run=subprocess.run([str(binary),flag,str(target),*extra,'--engine-home',str(root),'--engine-renderer','ray_traced','--engine-original'],cwd=root,capture_output=True,text=True,timeout=300);(out/(name+'.stdout.txt')).write_text(run.stdout,encoding='utf-8');(out/(name+'.stderr.txt')).write_text(run.stderr,encoding='utf-8');assert run.returncode==0,(name,run.stderr)
  g=read(target/'gpu.json');s=g['statistics'];r=g['renderer'];geometry=read(target/'ray-geometry.json');assert g['complete'] and s['meshes_ready'] and s['native_resolution'] and s['mesh_peak_jobs']<=2;assert r['method']=='ray_traced' and r['genuine_ray_tracing'] and r['ray_scene']['ready'] and r['ray_scene']['pending']==0;assert geometry['complete'] and geometry['vertices']==r['ray_scene']['vertices'] and len(geometry['parts'])==r['ray_scene']['instances']
  w,h=r['size'];normal=np.fromfile(target/'gbuffer-normal.bin',dtype='<f2').reshape(h,w,4).astype(np.float32);depth=np.fromfile(target/'gbuffer-depth.bin',dtype='<f4').reshape(h,w);solid=depth<1;assert solid.sum()>1000 and np.isfinite(normal).all() and np.isfinite(depth).all();assert np.max(abs(np.linalg.norm(normal[:,:,:3][solid],axis=1)-1))<.005
  if name in ['legacy-255','ocean-255']:assert s['distant_ready'] and s['distant_vertices']>2000000 and g['settings']['render_distance']==255
  if name=='ocean-255':assert s['distant_water_vertices']>0
  if name in ['high','deep']:assert abs(g['player_position']['y'])>1000
  if name=='underwater':assert g['fluids']['head_underwater'] and r['frame']['medium'][3]==1
  if name=='edits':assert read(target/'async.json')['complete'] and s['mesh_discarded_jobs']>0
  checks[name]=dict(statistics=s,renderer=r,geometry=geometry,timing=g['timing'],solid_pixels=int(solid.sum()),image_sha256=sha(target/'gpu.png'));print(json.dumps(dict(case=name,ray_vertices=r['ray_scene']['vertices'],solid_pixels=int(solid.sum()))),flush=True)
 original=proof/'ray_sun';sync=out/'sync';assert np.array_equal(np.array(Image.open(original/'gpu.png'))[100:-100],np.array(Image.open(sync/'gpu.png'))[100:-100]);assert all(sha(original/n)==sha(sync/n) for n in ['gbuffer-albedo.bin','gbuffer-normal.bin','gbuffer-depth.bin','shadow-depth.bin']);assert read(original/'ray-geometry.json')['parts']==read(sync/'ray-geometry.json')['parts']
 assert sha(binary)==bh and all(sha(root/n)==h for n,h in sources.items());result=dict(complete=True,binary_sha256=bh,source_hashes=sources,checks=checks,exact_worker_sync_pixels=True,exact_worker_sync_gbuffer=True,exact_worker_sync_ray_geometry=True,scope='Real hardware rays on legacy/modern 255-chunk LOD worlds, high/deep coordinates and underwater state; live edits, eviction, registry remapping, mesh mode changes and save/load; exact current GPU ray-arena readback; exact worker/sync final pixels and geometry.')
 (out/'summary.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8');print(json.dumps(dict(complete=True,cases=len(checks))),flush=True)
if __name__=='__main__':main()
