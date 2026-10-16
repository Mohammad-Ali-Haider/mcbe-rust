"""Native dual-hand geometry, linear-light bucket pixels, cameras and save roundtrips."""
import argparse, hashlib, json, subprocess
from pathlib import Path
import numpy as np
from PIL import Image
from verify_engine_buckets import pixels

def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p): return json.loads(p.read_bytes())
def main():
    ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,required=True);a=ap.parse_args()
    root=Path(__file__).resolve().parents[1];out=a.out.resolve();binary=root/'target/release/mcbe-client.exe'
    if out.exists(): raise ValueError('Fresh offhand proof directory required')
    out.mkdir(parents=True)
    files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'content').rglob('*')),Path(__file__),root/'tools/verify_engine_buckets.py']
    hashes={p.relative_to(root).as_posix():sha(p) for p in files if p.is_file()};bh=sha(binary)
    checks={};images={};gp={}
    cases=[('empty','forward'),('offhand','forward'),('offhand-back','forward'),('offhand-front','forward'),('offhand-only','forward'),('offhand-bucket','forward'),('offhand-bucket','deferred'),('offhand-bucket','ray_traced')]
    for case,renderer in cases:
        name=case+'-'+renderer;target=out/name
        r=subprocess.run([str(binary),'--engine-player-smoke',str(target),'--engine-player-case',case,'--engine-home',str(root),'--engine-original','--engine-renderer',renderer],cwd=root,capture_output=True,text=True,timeout=180)
        (out/(name+'.stdout.txt')).write_text(r.stdout,encoding='utf-8');(out/(name+'.stderr.txt')).write_text(r.stderr,encoding='utf-8')
        assert r.returncode==0,(name,r.stderr)
        g=read(target/'gpu.json');w=read(target/'window.json');p=read(target/'player.json');ch=g['character'];assert g['complete'] and p['complete'] and w['maximized'] and not w['fullscreen'] and g['statistics']['native_resolution']
        image=np.array(Image.open(target/'gpu.png').convert('RGB'));images[name]=image;gp[name]=g
        if case=='empty': assert ch['hand_vertices']==72 and ch['offhand_item_vertices']==0
        else:
            assert ch['offhand'] in ['blue_glass','bucket'] and ch['offhand_item_vertices']>0
            assert ch['item_vertices']==ch['main_item_vertices']+ch['offhand_item_vertices']
            assert ch['mesh_uploads']<=2,(name,ch['mesh_uploads'])
            saves=list((target/'saves').glob('*.blockworld'));assert len(saves)==1,(name,saves)
            d=read(saves[0]);assert d['version']==12 and d['inventory']['offhand']['block']==ch['offhand']
            if case in ['offhand-back','offhand-front']: assert ch['body_vertices']==432 and ch['hand_vertices']==0
            else: assert ch['body_vertices']==0 and ch['hand_vertices']==144
            if case=='offhand-only': assert ch['main_item_vertices']==0
            if case=='offhand-bucket':
                assert ch['offhand_data']['amount']==731 and d['inventory']['offhand']['data']['amount']==731
                blocks=d['registry']['blocks'];water=next(i for i,b in enumerate(blocks) if b['id']=='water');bucket=next(i for i,b in enumerate(blocks) if b['id']=='bucket')
                checks[name]={'bucket_pixels':pixels(g,image,water,bucket)}
                if renderer=='ray_traced': assert g['renderer']['genuine_ray_tracing']
        checks.setdefault(name,{}).update(image_sha256=sha(target/'gpu.png'),statistics=g['statistics'],offhand=ch['offhand'],main_vertices=ch['main_item_vertices'],offhand_vertices=ch['offhand_item_vertices'])
        print(json.dumps({'case':name,'complete':True}),flush=True)
    h,w,_=images['empty-forward'].shape
    # Empty right hand stays exactly the same when only the offhand changes.
    a=images['empty-forward'];b=images['offhand-only-forward'];roi=np.s_[int(h*.40):int(h*.78),int(w*.55):int(w*.97)]
    assert np.array_equal(a[roi],b[roi]),'Offhand changed unrelated right-hand/world pixels'
    roi=np.s_[int(h*.40):int(h*.78),int(w*.03):int(w*.45)];changed=int(np.any(a[roi]!=b[roi],axis=2).sum());assert changed>1000,changed
    assert sha(binary)==bh and all(sha(root/n)==v for n,v in hashes.items())
    (out/'summary.json').write_text(json.dumps(dict(complete=True,binary_sha256=bh,source_hashes=hashes,checks=checks,left_hand_changed_pixels=changed,right_hand_preserved=True,scope='Real maximized native client, shared first/third-person hand geometry, offhand-only view, schema-12 save/load, independent CPU triangle/depth/linear-light checks of left-held bucket contents in Forward/Deferred/hardware Ray lighting. Controller and authored UI are covered by verify_engine_controllers.py.'),indent=2)+'\n',encoding='utf-8')
if __name__=='__main__': main()
