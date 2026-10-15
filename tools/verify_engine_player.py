"""Native camera, skin UV pixels, held geometry, depth occlusion and actor/save isolation."""
import argparse,hashlib,json,math,subprocess
from pathlib import Path
import numpy as np
from PIL import Image

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p):return json.loads(p.read_bytes())
def skin_pixels(g,image,texture):
    vertices=g['character']['hand_geometry'];sx,sy,_,_=g['character']['skin_sprite'];height,width,_=image.shape;aspect=width/height;tan=math.tan(math.radians(35));triangles=[]
    for i in range(0,len(vertices),3):
        vs=vertices[i:i+3];p=np.array([v['position']for v in vs]);w=-p[:,2]
        if np.any(w<=.025):continue
        xy=np.column_stack(((p[:,0]/(w*tan*aspect)+1)*width/2,(1-p[:,1]/(w*tan))*height/2))
        a,b,c=xy;den=((b-a)[0]*(c-a)[1]-(b-a)[1]*(c-a)[0])
        if den>=-.001:continue
        depth=(-10/(10-.025)*p[:,2]-(10*.025)/(10-.025))/w
        triangles.append((xy,w,depth,np.array([v['uv']for v in vs]),np.array(vs[0]['color'][:3]),den))
    count=bad=maximum=0
    for y in range(int(height*.40),int(height*.91),13):
      for x in range(int(width*.52),int(width*.98),13):
        candidates=[];point=np.array([x+.5,y+.5])
        for xy,w,depth,uv,color,den in triangles:
            a,b,c=xy;b1=((point-a)[0]*(c-a)[1]-(point-a)[1]*(c-a)[0])/den;b2=((b-a)[0]*(point-a)[1]-(b-a)[1]*(point-a)[0])/den;weights=np.array([1-b1-b2,b1,b2])
            if np.min(weights)<-1e-7:continue
            sample=(weights[:,None]*uv/w[:,None]).sum(axis=0)/(weights/w).sum();tex=sample*1024-np.array([sx,sy]);fraction=tex-np.floor(tex)
            valid=np.min(weights)>.035 and not(np.any(fraction<.08)or np.any(fraction>.92))
            u,v=np.floor(tex).astype(int)
            if not(0<=u<texture.width and 0<=v<texture.height):continue
            rgba=np.array(texture.getpixel((u,v)))
            if rgba[3]<128:continue
            candidates.append((float(weights@depth),rgba[:3]/255,color,valid))
        if not candidates:continue
        _,rgb,shade,valid=min(candidates,key=lambda c:c[0]);
        if not valid:continue
        linear=np.where(rgb<=.04045,rgb/12.92,((rgb+.055)/1.055)**2.4)*shade;expected=np.rint(np.where(linear<=.0031308,linear*12.92,1.055*linear**(1/2.4)-.055)*255).astype(int)
        error=int(np.abs(image[y,x].astype(int)-expected).max());count+=1;bad+=error>3;maximum=max(maximum,error)
    assert count>100 and bad==0,(count,bad,maximum)
    return dict(samples=count,bad=bad,max_error=maximum)

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,required=True);args=ap.parse_args();root=Path(__file__).resolve().parents[1];out=args.out.resolve();binary=root/'target/release/mcbe-client.exe'
    if out.exists():raise ValueError('Fresh player proof folder required')
    out.mkdir(parents=True);binary_hash=sha(binary);files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'content').rglob('*')),Path(__file__)];sources={p.relative_to(root).as_posix():sha(p)for p in files if p.is_file()}
    config=read(root/'.local/player.json');skin=Path(config['skin']);skin_hash=sha(skin);images={};checks={};gpu={}
    def launch(case,home=root,name=None,extra=()):
        name=name or case;target=out/name;run=subprocess.run([str(binary),'--engine-player-smoke',str(target),'--engine-player-case',case,'--engine-home',str(home),*extra],cwd=root,capture_output=True,text=True,timeout=150)
        (out/(name+'.stdout.txt')).write_text(run.stdout,encoding='utf-8');(out/(name+'.stderr.txt')).write_text(run.stderr,encoding='utf-8')
        if run.returncode:raise ValueError(name+': '+run.stderr)
        g=read(target/'gpu.json');report=read(target/'player.json');window=read(target/'window.json');assert g['complete']and report['complete']and window['maximized']and not window['fullscreen']and g['statistics']['native_resolution']
        assert g['character']['skin']['read_only'];images[name]=np.array(Image.open(target/'gpu.png').convert('RGB'));checks[name]=dict(report=report,image_sha256=sha(target/'gpu.png'),character_uploads=g['character']['mesh_uploads']);gpu[name]=g
        print(json.dumps(dict(case=name,mode=g['camera']['mode'])),flush=True);return g
    for case in ['first','empty','slab','glass','back','front','free','blocked','occluded']:launch(case)
    launch('empty',root,'original-empty',('--engine-original',))
    for case in ['first','empty','slab','glass']:assert gpu[case]['character']['hand_vertices']==72 and gpu[case]['character']['body_vertices']==0 and gpu[case]['character']['mesh_uploads']<=2
    assert gpu['empty']['character']['item_vertices']==0
    for case in ['first','slab','glass']:assert gpu[case]['character']['item_vertices']==36
    assert checks['blocked']['report']['orbit_distance']<.5 and gpu['blocked']['character']['body_vertices']==0
    assert gpu['front']['character']['body_vertices']==432 and gpu['back']['character']['body_vertices']==432
    assert checks['free']['report']['free_actor_unchanged'] and checks['free']['report']['saved_actor_unchanged'] and checks['free']['report']['ctrl_s_save']
    for case in gpu:assert gpu[case]['character']['skin']['sha256']==skin_hash
    texture=Image.open(skin).convert('RGBA');pixels=skin_pixels(gpu['empty'],images['empty'],texture);original_pixels=skin_pixels(gpu['original-empty'],images['original-empty'],texture)
    h,w,_=images['first'].shape;roi=np.s_[int(h*.4):int(h*.90),int(w*.52):int(w*.98)]
    changed={case:int(np.any(images[case][roi]!=images['empty'][roi],axis=2).sum())for case in ['first','slab','glass']};assert all(v>1000 for v in changed.values()),changed
    assert np.any(images['first'][roi]!=images['slab'][roi],axis=2).sum()>1000
    occluded=images['occluded'][int(h*.49):int(h*.75),int(w*.47):int(w*.53)];assert not np.any((occluded[:,:,0]>occluded[:,:,1]*1.8)&(occluded[:,:,0]>100))
    # A known independent skin color verifies texture replacement, including slim geometry.
    home=out/'diagnostic-home';(home/'.local').mkdir(parents=True);diagnostic=home/'skin.png';Image.new('RGBA',(64,64),(230,30,80,255)).save(diagnostic);(home/'.local/player.json').write_text(json.dumps(dict(skin=str(diagnostic),slim=True)),encoding='utf-8');dg=launch('empty',home,'diagnostic-slim-hand');diagnostic_pixels=skin_pixels(dg,images['diagnostic-slim-hand'],Image.open(diagnostic).convert('RGBA'));assert dg['character']['skin']['slim']
    assert sha(binary)==binary_hash and sha(skin)==skin_hash and all(sha(root/n)==digest for n,digest in sources.items())
    result=dict(complete=True,binary_sha256=binary_hash,source_hashes=sources,skin_sha256=skin_hash,checks=checks,skin_pixels=pixels,original_skin_pixels=original_pixels,diagnostic_skin_pixels=diagnostic_pixels,held_changed_pixels=changed,scope='Actual F5 perspective controls, empty hand/full block/slab/glass, requested external skin sampled against independent perspective-correct CPU UV/alpha/depth calculations, alternate slim skin, native world occlusion, collision-shortened orbit, free-camera actor/control isolation and actual Ctrl+S save isolation. Existing terrain smoke scenes intentionally omit character passes; these player scenes enable the real player rendering path.')
    (out/'summary.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8');print(json.dumps(dict(complete=True,skin_pixels=pixels,original_skin_pixels=original_pixels,diagnostic_skin_pixels=diagnostic_pixels)),flush=True)
if __name__=='__main__':main()
