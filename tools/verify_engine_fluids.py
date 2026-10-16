"""Native conservative transport, swimming, live LOD and independent water/fog pixels."""
import argparse, hashlib, json, math, subprocess
from pathlib import Path
import numpy as np
from PIL import Image

def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p): return json.loads(p.read_bytes())
def linear(v): return np.where(v<=.04045,v/12.92,((v+.055)/1.055)**2.4)
def srgb(v): return np.where(v<=.0031308,v*12.92,1.055*v**(1/2.4)-.055)
def rays(g):
    w=g['statistics']['width'];h=g['statistics']['height'];yy,xx=np.mgrid[100:h-100:7,0:w:7]
    d=np.array(g['camera']['direction']);d=d/np.linalg.norm(d);right=np.cross(d,np.array([0.,1.,0.]));right/=np.linalg.norm(right);up=np.cross(right,d)
    tan=math.tan(math.radians(g['settings']['fov'])/2);nx=((xx+.5)/w*2-1)*w/h*tan;ny=(1-(yy+.5)/h*2)*tan
    return yy,xx,np.array(g['camera']['eye']),d+nx[:,:,None]*right+ny[:,:,None]*up

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,required=True);a=ap.parse_args();root=Path(__file__).resolve().parents[1];out=a.out.resolve();binary=root/'target/release/mcbe-client.exe'
    if out.exists():raise ValueError('Fresh fluid proof directory required')
    out.mkdir(parents=True);bh=sha(binary);files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'content').rglob('*')),Path(__file__)]
    sources={p.relative_to(root).as_posix():sha(p) for p in files if p.is_file()};checks={};images={};reports={};gpu={}
    def launch(name,case=None,extra=()):
        target=out/name;command=[str(binary),'--engine-fluid-smoke' if case else '--engine-render-smoke',str(target),'--engine-home',str(root)]
        if case:command+=['--engine-fluid-case',case]
        run=subprocess.run([*command,*map(str,extra)],cwd=root,capture_output=True,text=True,timeout=180)
        (out/(name+'.stdout.txt')).write_text(run.stdout,encoding='utf-8');(out/(name+'.stderr.txt')).write_text(run.stderr,encoding='utf-8')
        if run.returncode:raise ValueError(name+': '+run.stderr)
        g=read(target/'gpu.json');w=read(target/'window.json');s=g['statistics'];assert g['complete'] and s['meshes_ready'] and s['native_resolution'] and w['maximized'] and not w['fullscreen']
        if 'dry' not in name:assert s['two_sided_quads']>0
        assert s['mesh_peak_jobs']<=2 and s['mesh_jobs_in_flight']==0 and s['texture_uploads']==1
        images[name]=np.array(Image.open(target/'gpu.png').convert('RGB'));gpu[name]=g;checks[name]=dict(statistics=s,timing=g['timing'],image_sha256=sha(target/'gpu.png'))
        if case:
            r=read(target/'fluids.json');reports[name]=r;assert r['complete'] and r['eviction_reload'] and r['world_updates_with_ui_open'] and r['pending_after_reload']==0 and r['transport']['counters']['processed']>0
            if case!='underwater':assert r['transport']['counters']['transfers']>0
            assert r['transport']['counters']['peak_queue']<=65536 and r['transport']['counters']['last_processed']<=256
            save=read(Path(r['saved_world']));assert save['version']==9 and all(0<e['amount']<1024 for e in save['fluids'])
            checks[name]['report']=r
        print(json.dumps(dict(case=name,complete=True)),flush=True);return g
    for case in ['flow','underwater','pressure','lod']:launch(case,case)
    assert reports['flow']['mass']==10240 and reports['flow']['partial_cells']==16 and reports['pressure']['mass']==7168 and reports['underwater']['mass']==81920
    assert gpu['underwater']['fluids']['head_underwater'] and gpu['underwater']['fluids']['swimming'] and gpu['underwater']['fluids']['immersion']>.9
    live=read(out/'lod/fluid-live.json');assert live['complete'] and live['published_during_edits'] and live['mutations']>10 and live['mass']==10240
    assert gpu['lod']['statistics']['distant_ready'] and gpu['lod']['statistics']['distant_vertices']>0 and gpu['lod']['statistics']['distant_discarded']==0
    launch('flow-sync','flow',('--engine-sync-mesh',));h,w,_=images['flow'].shape
    assert np.array_equal(images['flow'][100:h-100],images['flow-sync'][100:h-100])
    for case in ['flow','underwater']:
        document=read(Path(reports[case]['saved_world']));document['edits']=[e for e in document['edits'] if e['material']!='water'];document['fluids']=[]
        saved=out/(case+'-dry.blockworld');saved.write_text(json.dumps(document),encoding='utf-8');launch(case+'-dry',None,('--engine-load',saved))
    # Flat settled pool: a ray-selected interior surface blends exactly once.
    yy,xx,eye,d=rays(gpu['flow']);distance=(10001.625-eye[1])/d[:,:,1];points=eye+distance[:,:,None]*d
    mask=(distance>0)&(points[:,:,0]>.25)&(points[:,:,0]<3.75)&(points[:,:,2]>8.25)&(points[:,:,2]<11.75)
    wet=images['flow'][yy,xx];dry=images['flow-dry'][yy,xx];mask&=np.max(np.abs(wet.astype(int)-dry.astype(int)),axis=2)>5
    water=linear(np.array([45.,115.,195.])/255);alpha=90/255;expected=np.rint(srgb(water*alpha+linear(dry/255)*(1-alpha))*255)
    error=np.max(np.abs(wet.astype(int)-expected),axis=2);count=int(mask.sum());bad=int(((error>3)&mask).sum());assert count>100 and bad==0,(count,bad,float(error[mask].max()))
    # Opaque far wall seen through water: independent 3D distance fog.
    yy,xx,eye,d=rays(gpu['underwater']);distance=(12-eye[2])/d[:,:,2];points=eye+distance[:,:,None]*d
    mask=(distance>0)&(points[:,:,0]>.3)&(points[:,:,0]<3.7)&(points[:,:,1]>10001.5)&(points[:,:,1]<10005.5)
    wet=images['underwater'][yy,xx];dry=images['underwater-dry'][yy,xx];fog=np.clip(distance*np.linalg.norm(d,axis=2)/18,0,1)[:,:,None]
    expected=np.rint(srgb(linear(dry/255)*(1-fog)+np.array([.04,.21,.34])*fog)*255);error=np.max(np.abs(wet.astype(int)-expected),axis=2)
    fog_count=int(mask.sum());fog_bad=int(((error>3)&mask).sum());assert fog_count>500 and fog_bad==0,(fog_count,fog_bad,float(error[mask].max()))
    # Tilt a copy of the same saved underwater scene upward to expose sky beyond the surface.
    document=read(Path(reports['underwater']['saved_world']));document['pitch']=.9
    saved=out/'surface-wet.blockworld';saved.write_text(json.dumps(document),encoding='utf-8');launch('surface-wet',None,('--engine-load',saved))
    document['edits']=[e for e in document['edits'] if e['material']!='water'];document['fluids']=[]
    saved=out/'surface-dry.blockworld';saved.write_text(json.dumps(document),encoding='utf-8');launch('surface-dry',None,('--engine-load',saved))
    yy,xx,eye,d=rays(gpu['surface-wet']);wet=images['surface-wet'][yy,xx];dry=images['surface-dry'][yy,xx]
    # Surface underside must be visible; retain sky-background rays only.
    distance=(10006-eye[1])/d[:,:,1];points=eye+distance[:,:,None]*d;sky=np.rint(srgb(np.array([.62,.77,.89]))*255)
    mask=(distance>0)&(points[:,:,0]>.3)&(points[:,:,0]<3.7)&(points[:,:,2]>8.3)&(points[:,:,2]<11.7)&(np.max(abs(dry.astype(float)-sky),axis=2)<=2)
    fog=np.clip(distance*np.linalg.norm(d,axis=2)/18,0,1)[:,:,None];surface=water*(1-fog)+np.array([.04,.21,.34])*fog
    expected=np.rint(srgb(surface*alpha+linear(dry/255)*(1-alpha))*255);error=np.max(abs(wet.astype(int)-expected),axis=2);underside=int(mask.sum());underside_bad=int(((error>3)&mask).sum());assert underside>100 and underside_bad==0,(underside,underside_bad)
    assert sha(binary)==bh and all(sha(root/n)==h for n,h in sources.items())
    result=dict(complete=True,binary_sha256=bh,source_hashes=sources,checks=checks,live_lod=live,pixels=dict(surface_samples=count,surface_bad=bad,fog_samples=fog_count,fog_bad=fog_bad,underside_samples=underside,underside_bad=underside_bad),scope='Actual conservative transport and covered pressure passage, rise/dive swimming, high vertical eviction/Ctrl+S/F9, fractional volume metadata, immutable async/sync exact world pixels, independent ray/sRGB water transparency, 3D underwater fog and surface undersides. Continuous edits must not starve first LOD publication. Timing covers these scenes only. Bucket items and large-scale CFD are outside this increment.')
    (out/'summary.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8');print(json.dumps(dict(complete=True,pixels=result['pixels'])),flush=True)
if __name__=='__main__':main()
