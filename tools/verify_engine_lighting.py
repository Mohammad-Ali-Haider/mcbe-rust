"""Native deferred geometry, BRDF/AO/PCF reference, material and renderer-switch proofs."""
import argparse,hashlib,json,math,subprocess
from pathlib import Path
import numpy as np
from PIL import Image

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p):return json.loads(p.read_bytes())
def norm(v):return v/np.maximum(np.linalg.norm(v,axis=-1,keepdims=True),1e-9)
def srgb(v):return np.where(v<=.0031308,v*12.92,1.055*np.maximum(v,0)**(1/2.4)-.055)
def smooth(lo,hi,x):t=np.clip((x-lo)/(hi-lo),0,1);return t*t*(3-2*t)
def reference(path):
    info=read(path/'lighting.json');f=info['frame'];w,h=info['size'];shadow_size=info['shadow_size'];albedo=np.fromfile(path/'gbuffer-albedo.bin',dtype=np.uint8).reshape(h,w,4);normal=np.fromfile(path/'gbuffer-normal.bin',dtype='<f2').reshape(h,w,4).astype(np.float32);depth=np.fromfile(path/'gbuffer-depth.bin',dtype='<f4').reshape(h,w);shadow=np.fromfile(path/'shadow-depth.bin',dtype='<f4').reshape(shadow_size,shadow_size)
    inverse=np.array(f['inverse'],dtype=np.float32).reshape(4,4,order='F');light=np.array(f['light'],dtype=np.float32).reshape(4,4,order='F');eye=np.array(f['eye'][:3],dtype=np.float32);sun=np.array(f['sun'][:3],dtype=np.float32);params=f['parameters'];sampling=f['sampling']
    def world(x,y,z):
        clip=np.stack([((x+.5)/w*2-1),(1-(y+.5)/h*2),z,np.ones_like(z)],axis=-1).astype(np.float32);p=clip@inverse.T;return p[:,:3]/p[:,3:4]
    yy,xx=np.mgrid[100:h-100:9,0:w:9];x=xx.ravel();y=yy.ravel();mask=(depth[y,x]<1)&(np.linalg.norm(normal[y,x,:3],axis=1)>.5);x=x[mask];y=y[mask];p=world(x,y,depth[y,x]);n=norm(normal[y,x,:3]);base=albedo[y,x,:3].astype(float)/255;materials=np.array(info['materials']);mat=materials[np.rint(normal[y,x,3]).astype(int)];v=norm(eye-p);half=norm(v+sun);nv=np.maximum(np.sum(n*v,axis=1),.001);nl=np.maximum(n@sun,.001);nh=np.maximum(np.sum(n*half,axis=1),0);vh=np.maximum(np.sum(v*half,axis=1),0);rough=mat[:,0];metal=mat[:,1];a2=rough**4;denom=nh*nh*(a2-1)+1;dist=a2/(np.pi*denom*denom);k=(rough+1)**2/8;geo=(nv/(nv*(1-k)+k))*(nl/(nl*(1-k)+k));f0=.04*(1-metal[:,None])+base*metal[:,None];fres=f0+(1-f0)*(1-vh[:,None])**5;brdf=(1-fres)*(1-metal[:,None])*base/np.pi+(dist*geo/(4*nv*nl))[:,None]*fres
    ao=np.ones(len(x));sample_points=np.array([[.35,.11],[-.12,.43],[-.52,-.16],[.19,-.61],[.67,.39],[-.38,.73],[-.79,-.43],[.51,-.78],[.91,-.12],[-.89,.31],[.11,.97],[-.16,-.98]],dtype=np.float32)
    if params[1]>.5:
        radius=np.minimum(64.,1.8*h/(np.maximum(np.linalg.norm(p-eye,axis=1),.1)*2*sampling[3]));occ=np.zeros(len(x))
        for offset in sample_points[:int(sampling[2])]:
            ox=np.rint(offset[0]*radius).astype(int)+x;oy=np.rint(offset[1]*radius).astype(int)+y;inside=(ox>=0)&(ox<w)&(oy>=0)&(oy<h);ox=np.clip(ox,0,w-1);oy=np.clip(oy,0,h-1);z=depth[oy,ox];delta=world(ox,oy,z)-p;length=np.linalg.norm(delta,axis=1);cos=np.sum(n*delta/np.maximum(length[:,None],1e-8),axis=1);ok=inside&(z<1)&(length>.03)&(length<1.8);occ+=np.where(ok,np.maximum(0,cos-.15)*(1-length/1.8),0)
        ao=np.clip(1-occ*2.2/sampling[2],.2,1)
    visibility=np.ones(len(x))
    if params[2]>.5:
        clip=np.concatenate([p+n*.025,np.ones((len(p),1))],axis=1).astype(np.float32)@light.T;q=clip[:,:3]/clip[:,3:4];uv=np.stack([q[:,0]*.5+.5,.5-q[:,1]*.5],axis=1);inside=(q[:,2]>=0)&(q[:,2]<=1)&np.all((uv>=0)&(uv<1),axis=1);sx=(uv[:,0]*shadow_size).astype(int);sy=(uv[:,1]*shadow_size).astype(int);total=np.zeros(len(x));radius=1 if sampling[2]>4.5 else 0
        for dy in range(-radius,radius+1):
            for dx in range(-radius,radius+1):total+=(q[:,2]-.0005<=shadow[np.clip(sy+dy,0,shadow_size-1),np.clip(sx+dx,0,shadow_size-1)])
        visibility=np.where(inside,total/(2*radius+1)**2,1);fade=smooth(sampling[0]*.65,sampling[0]*.88,np.linalg.norm(p[:,[0,2]]-eye[[0,2]],axis=1));visibility=visibility*(1-fade)+fade
    hemi=(n[:,1]*.5+.5)[:,None];ambient=(np.array([.11,.13,.16])*(1-hemi)+np.array([.40,.49,.62])*hemi)*base*(1-metal[:,None]*.65)*ao[:,None];direct=brdf*np.array([1.,.94,.84])*np.maximum(n@sun,0)[:,None]*3.2*visibility[:,None];hdr=ambient+direct+base*mat[:,2:3];rgb=1-np.exp(-hdr*params[0]);rgb=base if params[3]>.5 else rgb;sky=np.array(f['medium'][:3] if f['medium'][3]>.5 else [.62,.77,.89]);distance=np.linalg.norm(p-eye,axis=1) if f['fog'][2]<.5 or f['medium'][3]>.5 else np.linalg.norm(p[:,[0,2]]-eye[[0,2]],axis=1);fog=np.clip((distance-f['fog'][0])/(f['fog'][1]-f['fog'][0]),0,1)[:,None];expected=np.rint(srgb(rgb*(1-fog)+sky*fog)*255);image=np.array(Image.open(path/'gpu.png').convert('RGB'));error=np.max(abs(image[y,x].astype(float)-expected),axis=1)
    # Floating-point PCF lookup boundaries are excluded geometrically, before comparing colors.
    if params[2]>.5:
        fractions=np.mod(uv*shadow_size,1);stable=np.min(np.minimum(fractions,1-fractions),axis=1)>.025
    else:stable=np.ones(len(x),dtype=bool)
    bad=int(((error>4)&stable).sum());samples=int(stable.sum());assert samples>1000 and bad==0,(samples,bad,float(error[stable].max()))
    scene=read(path/'lighting-scene.json');floor=(normal[y,x,3]==scene['floor_id'])&(n[:,1]>.999)&(abs(p[:,1]-scene['floor_height'])<.003)
    assert floor.sum()>100;assert np.max(abs(p[floor,1]-4))<.003
    shadow_check=None
    if scene['case']=='lit':
        fp=p[floor]+np.array([0.,.025,0.]);boxes=[]
        for b in scene['occluders']:
            if b['layer']!='opaque':continue
            pos=np.array(b['position']);boxes += [(pos+np.array(q[:3]),pos+np.array(q[3:])) for q in b['boxes']]
        def blocked(margin):
            out=np.zeros(len(fp),dtype=bool)
            for lo,hi in boxes:
                lo=lo-margin;hi=hi+margin
                if np.any(lo>=hi):continue
                a=(lo-fp)/sun;b=(hi-fp)/sun;near=np.minimum(a,b).max(axis=1);far=np.maximum(a,b).min(axis=1);out|=(far>=np.maximum(near,.02))
            return out
        expected_shadow=blocked(0);robust=(expected_shadow==blocked(.15))&(expected_shadow==blocked(-.15));observed=visibility[floor]<.5;count=int(robust.sum());incorrect=int(((observed!=expected_shadow)&robust).sum());assert count>300 and incorrect==0,(count,incorrect);assert int((robust&expected_shadow).sum())>20;shadow_check=dict(samples=count,incorrect=incorrect,shadowed=int((robust&expected_shadow).sum()))
    return dict(samples=samples,incorrect=bad,max_error=float(error[stable].max()),mean_error=float(error[stable].mean()),ambient_darkened=int((ao<.98).sum()),shadow_darkened=int((visibility<.5).sum()),geometry_shadow=shadow_check)

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,required=True);a=ap.parse_args();root=Path(__file__).resolve().parents[1];out=a.out.resolve();binary=root/'target/release/mcbe-client.exe'
    if out.exists():raise ValueError('Fresh lighting proof folder required')
    out.mkdir(parents=True);bh=sha(binary);files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'content').rglob('*')),Path(__file__)];sources={p.relative_to(root).as_posix():sha(p) for p in files if p.is_file()};checks={};images={}
    cases=['forward','lit','unshadowed','noao','metal','emissive','low','high','wire','menu','toggle','resize','glass','water','cutout']
    for case in cases:
        target=out/case;run=subprocess.run([str(binary),'--engine-lighting-smoke',str(target),'--engine-lighting-case',case,'--engine-home',str(root),'--engine-original'],cwd=root,capture_output=True,text=True,timeout=180);(out/(case+'.stdout.txt')).write_text(run.stdout,encoding='utf-8');(out/(case+'.stderr.txt')).write_text(run.stderr,encoding='utf-8')
        if run.returncode:raise ValueError(case+': '+run.stderr)
        g=read(target/'gpu.json');w=read(target/'window.json');s=g['statistics'];scene=read(target/'lighting-scene.json');assert g['complete'] and s['meshes_ready'] and s['native_resolution'] and s['mesh_peak_jobs']<=2 and s['texture_uploads']==1 and w['maximized'] and not w['fullscreen'];assert scene['complete'] and scene['settings_persisted'] and scene['materials_saved'];save=read(Path(scene['saved_world']));assert save['version']==13
        check=dict(statistics=s,renderer=g['renderer'],image_sha256=sha(target/'gpu.png'));images[case]=np.array(Image.open(target/'gpu.png').convert('RGB'))
        if case in ['forward','toggle']:assert s['graphics_bytes']==0 and g['renderer']['method']=='forward'
        else:
            assert g['renderer']['method']=='deferred' and s['deferred_passes']>0 and s['graphics_bytes']==s['width']*s['height']*16+g['renderer']['shadow_size']**2*4+4096+224;assert g['renderer']['material_uploads']==1
            if case in ['lit','unshadowed','noao','metal','emissive','low','high','cutout']:check['pixels']=reference(target)
        if case=='toggle':check['switch']=read(target/'lighting-switch.json');assert check['switch']['complete'] and [r['stage'] for r in check['switch']['trace']]==['forward','deferred','released']
        if case=='menu':assert g['menu']=='Graphics' and g['settings']['rendering']['method']=='deferred'
        if case=='resize':assert g['renderer']['allocations']>=2 and len(g['window_transitions'])>=2
        checks[case]=check;print(json.dumps(dict(case=case,pixels=check.get('pixels'))),flush=True)
    assert checks['lit']['pixels']['ambient_darkened']>50 and checks['lit']['pixels']['shadow_darkened']>50
    assert np.max(abs(images['lit'].astype(int)-images['forward'].astype(int)))>20
    for case in ['unshadowed','noao','metal','emissive']:assert int((np.max(abs(images[case].astype(int)-images['lit'].astype(int)),axis=2)>3).sum())>100
    assert sha(binary)==bh and all(sha(root/n)==h for n,h in sources.items());result=dict(complete=True,binary_sha256=bh,source_hashes=sources,checks=checks,scope='Actual two-target G-buffer/depth lighting, independent BRDF/AO/PCF/exposure/fog pixels, analytic solid/partial geometry sun rays, registered materials, settings/menu switches, GPU allocation release and native resize. Transparency/players use the existing forward composition. Hardware ray-query lighting is verified separately by verify_engine_rays.py.')
    (out/'summary.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8');print(json.dumps(dict(complete=True,cases=len(checks))),flush=True)
if __name__=='__main__':main()
