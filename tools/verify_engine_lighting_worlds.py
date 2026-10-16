"""Deferred renderer on real distance/terrain/fluid scenes and exact sync geometry."""
import argparse,hashlib,json,subprocess
from pathlib import Path
import numpy as np
from PIL import Image

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p):return json.loads(p.read_bytes())
def main():
    ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,required=True);a=ap.parse_args();root=Path(__file__).resolve().parents[1];out=a.out.resolve();binary=root/'target/release/mcbe-client.exe'
    if out.exists():raise ValueError('Fresh world lighting proof directory required')
    out.mkdir(parents=True);bh=sha(binary);files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'content').rglob('*')),Path(__file__)];sources={p.relative_to(root).as_posix():sha(p) for p in files if p.is_file()};checks={}
    cases=[('sync','--engine-lighting-smoke',['--engine-lighting-case','lit','--engine-sync-mesh']),('legacy-255','--engine-distance-smoke',[]),('ocean-255','--engine-terrain-smoke',['--engine-terrain-case','ocean','--engine-render-distance','255']),('high','--engine-terrain-smoke',['--engine-terrain-case','high']),('underwater','--engine-fluid-smoke',['--engine-fluid-case','underwater'])]
    for name,flag,extra in cases:
        target=out/name;run=subprocess.run([str(binary),flag,str(target),*extra,'--engine-home',str(root),'--engine-renderer','deferred','--engine-original'],cwd=root,capture_output=True,text=True,timeout=240);(out/(name+'.stdout.txt')).write_text(run.stdout,encoding='utf-8');(out/(name+'.stderr.txt')).write_text(run.stderr,encoding='utf-8')
        if run.returncode:raise ValueError(name+': '+run.stderr)
        g=read(target/'gpu.json');w=read(target/'window.json');s=g['statistics'];assert g['complete'] and s['meshes_ready'] and s['native_resolution'] and s['texture_uploads']==1 and s['mesh_peak_jobs']<=2 and w['maximized'] and not w['fullscreen'];assert g['renderer']['method']=='deferred' and s['graphics_bytes']>0
        width,height=g['renderer']['size'];normal=np.fromfile(target/'gbuffer-normal.bin',dtype='<f2').reshape(height,width,4).astype(np.float32);depth=np.fromfile(target/'gbuffer-depth.bin',dtype='<f4').reshape(height,width);albedo=np.fromfile(target/'gbuffer-albedo.bin',dtype=np.uint8).reshape(height,width,4);solid=depth<1;assert solid.sum()>1000 and np.isfinite(normal).all() and np.isfinite(depth).all() and np.all((depth>=0)&(depth<=1));assert np.all(albedo[:,:,3][solid]==255) and np.max(abs(np.linalg.norm(normal[:,:,:3][solid],axis=1)-1))<.005
        if name in ['legacy-255','ocean-255']:assert s['distant_ready'] and s['distant_vertices']>2000000 and g['settings']['render_distance']==255
        if name=='ocean-255':assert s['distant_water_vertices']>0
        if name=='high':assert abs(g['player_position']['y'])>1000
        if name=='underwater':assert g['fluids']['head_underwater'] and g['renderer']['frame']['medium'][3]==1
        checks[name]=dict(statistics=s,renderer=g['renderer'],solid_pixels=int(solid.sum()),image_sha256=sha(target/'gpu.png'));print(json.dumps(dict(case=name,solid_pixels=int(solid.sum()),distant_vertices=s['distant_vertices'])),flush=True)
    original=root/'.local/verification/deferred-final-02/lit';assert read(original/'gpu.json')['renderer']['method']=='deferred';image=np.array(Image.open(original/'gpu.png'));reference=np.array(Image.open(out/'sync/gpu.png'));assert np.array_equal(image[100:-100],reference[100:-100]);assert all(sha(original/n)==sha(out/'sync'/n) for n in ['gbuffer-albedo.bin','gbuffer-normal.bin','gbuffer-depth.bin','shadow-depth.bin'])
    assert sha(binary)==bh and all(sha(root/n)==h for n,h in sources.items());result=dict(complete=True,binary_sha256=bh,source_hashes=sources,checks=checks,exact_async_sync_pixels=True,exact_async_sync_gbuffer=True,scope='Actual deferred rendering on legacy/modern 255 range, high vertical construction and underwater fluid state. Primary G-buffer occupancy/normals/depth remain valid; synchronous and worker geometry produce identical final world pixels and G-buffer/shadow bytes.')
    (out/'summary.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8');print(json.dumps(dict(complete=True,cases=len(checks))),flush=True)
if __name__=='__main__':main()
