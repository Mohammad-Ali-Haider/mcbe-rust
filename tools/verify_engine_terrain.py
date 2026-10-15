"""Native modern biomes, caves, water LOD and sparse high/deep save restoration."""
import argparse, hashlib, json, math, subprocess
from pathlib import Path
from PIL import Image

def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p): return json.loads(p.read_bytes())
def main():
    ap=argparse.ArgumentParser(); ap.add_argument('--out',type=Path,required=True); a=ap.parse_args()
    root=Path(__file__).resolve().parents[1]; out=a.out.resolve(); binary=root/'target/release/mcbe-client.exe'
    if out.exists(): raise ValueError('Fresh terrain proof directory required')
    out.mkdir(parents=True); bh=sha(binary)
    files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'content').rglob('*')),Path(__file__)]
    hashes={p.relative_to(root).as_posix():sha(p) for p in files if p.is_file()}; checks={}
    for case in ['plains','forest','desert','mountains','snow','ocean','river','ravine','cave','high','deep','ocean-sync','ocean-255']:
        native=case.split('-')[0]; target=out/case
        distance='255' if case=='ocean-255' else '16' if case in ('ocean','ocean-sync') else '3'
        run=subprocess.run([str(binary),'--engine-terrain-smoke',str(target),'--engine-terrain-case',native,'--engine-home',str(root),'--engine-render-distance',distance,*(['--engine-sync-mesh'] if case=='ocean-sync' else [])],cwd=root,capture_output=True,text=True,timeout=180)
        (out/(case+'.stdout.txt')).write_text(run.stdout,encoding='utf-8');(out/(case+'.stderr.txt')).write_text(run.stderr,encoding='utf-8')
        if run.returncode: raise ValueError(case+': '+run.stderr)
        g=read(target/'gpu.json'); report=read(target/'terrain.json'); w=read(target/'window.json'); s=g['statistics']; stream=g['streaming']
        assert g['complete'] and report['complete'] and w['maximized'] and not w['fullscreen'] and s['native_resolution'] and s['meshes_ready']
        assert stream['generator_version']==2 and stream['resident_chunks']==stream['wanted_chunks'] and stream['queued_jobs']==0
        assert s['mesh_peak_jobs']<=2 and s['mesh_jobs_in_flight']==0 and report['legacy_cell_copies']==0 and report['dynamic_height']
        assert report['storage_bytes']<32*1024*1024 and stream['resident_columns']<=(2*min(int(distance),8)+1)**2
        assert not report['flow_physics']
        if native in ('plains','forest','desert','mountains','snow','ocean','river'): assert report['biome']==native
        if native in ('ocean','river'): assert report['sample']['water'] is not None and s['blended_vertices']>0
        if native=='ravine': assert report['sample']['ravine']
        if native in ('high','deep'):
            marker=report['marker']; assert marker[1]==(10000 if native=='high' else -10000)
            save=read(Path(report['saved_world']));assert save['version']==7 and save['generator']['version']==2 and save['generator']['terrain']['version']==1
            assert any(e['position']==marker and e['material']=='slate' for e in save['edits'])
        if int(distance)>8:
            assert s['distant_ready'] and s['distant_water_vertices']>0 and 0<s['distant_vertices']<=3145728 and s['distant_bytes']<=3145728*40
        im=Image.open(target/'gpu.png').convert('RGB'); width,height=im.size; region=im.crop((0,height//3,width,height*9//10)); colors=region.getcolors(width*height) or []; assert len(colors)>2 and max(max(c) for _,c in colors)-min(min(c) for _,c in colors)>40
        if native in ('high','deep'):
            gray=sum(n for n,c in colors if max(c)-min(c)<=3 and 50<sum(c)/3<200);assert gray>width*height//20
        if case=='ocean-sync':
            async_image=Image.open(out/'ocean/gpu.png').convert('RGB'); assert im.crop((0,100,width,height-100)).tobytes()==async_image.crop((0,100,width,height-100)).tobytes()
        checks[case]=dict(report=report,statistics=s,streaming=stream,timing=g['timing'],image_sha256=sha(target/'gpu.png'))
        print(json.dumps(dict(case=case,chunks=stream['resident_chunks'],bytes=report['storage_bytes'],water_vertices=s['distant_water_vertices'])),flush=True)
    # A separate overhead camera makes the narrow canyon visible as a whole.
    ravine=read(out/'ravine/gpu.json')['player_position']; sample=checks['ravine']['report']['sample']
    document=read(Path(checks['high']['report']['saved_world']));document['edits']=[]
    document['player']=dict(x=ravine['x']-15,y=sample['height']+45,z=ravine['z'])
    document['yaw']=math.pi/2;document['pitch']=-math.atan2(45,15)
    saved=out/'ravine-overlook.blockworld';saved.write_text(json.dumps(document),encoding='utf-8')
    target=out/'ravine-overlook'
    run=subprocess.run([str(binary),'--engine-render-smoke',str(target),'--engine-home',str(root),'--engine-load',str(saved)],cwd=root,capture_output=True,text=True,timeout=180)
    (out/'ravine-overlook.stderr.txt').write_text(run.stderr,encoding='utf-8')
    if run.returncode: raise ValueError(run.stderr)
    g=read(target/'gpu.json');assert g['complete'] and g['statistics']['meshes_ready'] and g['streaming']['generator_version']==2
    assert abs(g['player_position']['y']-document['player']['y'])<.01
    checks['ravine-overlook']=dict(image_sha256=sha(target/'gpu.png'),statistics=g['statistics'],streaming=g['streaming'],saved_sha256=sha(saved))
    print(json.dumps(dict(case='ravine-overlook',complete=True)),flush=True)
    assert sha(binary)==bh and all(sha(root/n)==h for n,h in hashes.items())
    result=dict(complete=True,binary_sha256=bh,source_hashes=hashes,checks=checks,scope='Native modern biome/cave/ravine scenes, exact async/sync ocean world pixels (excluding debug HUD), maximized native window, bounded real 3D residency/dense storage/two meshing jobs, 16/255 chunk transparent water and solid seabed LOD, construction at positive/negative 10000 with Ctrl+S, eviction and F9 sparse restore. Backend tests cover density, climate, async vertical crossing and background travel/cancel. Static water; flow, buckets and swimming remain pending. Timing describes captured scenes only.')
    (out/'summary.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8');print(json.dumps(dict(complete=True,binary_sha256=bh)),flush=True)
if __name__=='__main__': main()
