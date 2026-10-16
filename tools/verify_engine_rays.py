"""Native DX12 ray-query sunlight, offscreen reflection and indirect-light proofs.

Use --inspect to recheck existing evidence; never silently rerun a captured case.
This suite covers controlled fixtures. Streaming/edit/eviction stress is separate.
"""
import argparse, hashlib, json, os, subprocess
from pathlib import Path
import numpy as np
from PIL import Image

CASES=['ray_sun','ray_unshadowed','ray_mirror','ray_no_reflections','ray_indirect','ray_no_indirect','ray_low','ray_high','ray_cutout','ray_glass','ray_water','ray_menu','ray_toggle','fallback']
def read(p):return json.loads(p.read_bytes())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def norm(v):return v/np.maximum(np.linalg.norm(v,axis=-1,keepdims=True),1e-9)
def srgb(v):return np.where(v<=.0031308,v*12.92,1.055*np.maximum(v,0)**(1/2.4)-.055)
def buffers(path):
 g=read(path/'gpu.json');w,h=g['renderer']['size'];n=np.fromfile(path/'gbuffer-normal.bin',dtype='<f2').reshape(h,w,4).astype(np.float32);z=np.fromfile(path/'gbuffer-depth.bin',dtype='<f4').reshape(h,w)
 assert np.isfinite(n).all() and np.isfinite(z).all()
 return g,w,h,n,z

def positions(g,w,h,x,y,z):
 inverse=np.array(g['renderer']['frame']['inverse'],dtype=np.float32).reshape(4,4,order='F');clip=np.stack([((x+.5)/w*2-1),1-(y+.5)/h*2,z,np.ones_like(z)],axis=-1).astype(np.float32);p=clip@inverse.T;return p[:,:3]/p[:,3:4]

def sunlight(path):
 g,w,h,normal,depth=buffers(path);f=g['renderer']['frame'];fixture=read(path/'lighting-scene.json');yy,xx=np.mgrid[100:h-100:9,0:w:9];x=xx.ravel();y=yy.ravel();p=positions(g,w,h,x,y,depth[y,x]);eye=np.array(f['eye'][:3]);radius=g['renderer']['ray_scene']['effective_range']
 ok=(depth[y,x]<1)&(normal[y,x,3]==fixture['floor_id'])&(normal[y,x,1]>.999)&(abs(p[:,1]-4)<.003)&(np.linalg.norm(p-eye,axis=1)<radius*.62);x=x[ok];y=y[ok];p=p[ok];n=norm(normal[y,x,:3]);sun=np.array(f['sun'][:3]);origin=p+n*.035
 boxes=[]
 for b in fixture['occluders']:
  if b['layer']=='opaque':
   pos=np.array(b['position']);boxes.extend((pos+np.array(q[:3]),pos+np.array(q[3:])) for q in b['boxes'])
 def blocked(margin):
  result=np.zeros(len(p),dtype=bool)
  for lo,hi in boxes:
   lo=lo-margin;hi=hi+margin
   if np.any(lo>=hi):continue
   a=(lo-origin)/sun;b=(hi-origin)/sun;near=np.minimum(a,b).max(axis=1);far=np.maximum(a,b).min(axis=1);result|=(far>=np.maximum(near,.02))
  return result
 shadow=blocked(0);robust=(shadow==blocked(.03))&(shadow==blocked(-.03));assert int(robust.sum())>500
 visible=np.ones(len(p)) if f['parameters'][2]<.5 else (~shadow).astype(float)
 albedo=np.fromfile(path/'gbuffer-albedo.bin',dtype=np.uint8).reshape(h,w,4);base=albedo[y,x,:3]/255.;rows=np.array(read(path/'lighting.json')['materials']);mat=rows[np.rint(normal[y,x,3]).astype(int)];v=norm(eye-p);half=norm(v+sun);nv=np.maximum(np.sum(n*v,axis=1),.001);nl=np.maximum(n@sun,.001);nh=np.maximum(np.sum(n*half,axis=1),0);vh=np.maximum(np.sum(v*half,axis=1),0);rough=mat[:,0];metal=mat[:,1];a2=rough**4;denom=nh*nh*(a2-1)+1;distribution=a2/(np.pi*denom*denom);k=(rough+1)**2/8;geometry=(nv/(nv*(1-k)+k))*(nl/(nl*(1-k)+k));f0=.04*(1-metal[:,None])+base*metal[:,None];fres=f0+(1-f0)*(1-vh[:,None])**5;brdf=(1-fres)*(1-metal[:,None])*base/np.pi+(distribution*geometry/(4*nv*nl))[:,None]*fres
 hemi=(n[:,1]*.5+.5)[:,None];ambient=(np.array([.11,.13,.16])*(1-hemi)+np.array([.4,.49,.62])*hemi)*base*(1-metal[:,None]*.65);direct=brdf*np.array([1,.94,.84])*np.maximum(n@sun,0)[:,None]*3.2*visible[:,None];rgb=1-np.exp(-(ambient+direct+base*mat[:,2:3])*f['parameters'][0]);distance=np.linalg.norm(p-eye,axis=1);fog=np.clip((distance-f['fog'][0])/(f['fog'][1]-f['fog'][0]),0,1);rgb=rgb*(1-fog[:,None])+np.array([.62,.77,.89])*fog[:,None];expected=np.rint(srgb(rgb)*255);actual=np.array(Image.open(path/'gpu.png').convert('RGB'))[y,x];error=np.max(abs(actual-expected),axis=1);bad=int(((error>2)&robust).sum());assert bad==0,(path.name,bad,float(error[robust].max()));assert int((robust&shadow).sum())>50
 return dict(samples=int(robust.sum()),incorrect=bad,max_channel_error=float(error[robust].max()),geometric_shadow_samples=int((robust&shadow).sum()))

def compare_reflections(out):
 path=out/'ray_mirror';g,w,h,n,z=buffers(path);yy,xx=np.mgrid[:h,:w];x=xx.ravel();y=yy.ravel();p=positions(g,w,h,x,y,z.ravel());valid=z.ravel()<1;assert int(((p[:,2]<-17)&valid).sum())==0,'The reflection emitter must be outside primary visibility'
 mirror=((p[:,2]>13.9)&(p[:,2]<14.1)&(n[:,:,2].ravel()<-.99)&valid).reshape(h,w);mirror[:100]=False;mirror[-100:]=False
 enabled=np.array(Image.open(path/'gpu.png').convert('RGB')).astype(int);disabled=np.array(Image.open(out/'ray_no_reflections/gpu.png').convert('RGB')).astype(int);delta=enabled-disabled;affected=mirror&(delta[:,:,2]>5)&(delta[:,:,2]>delta[:,:,0]+2);assert int(affected.sum())>500,(int(mirror.sum()),int(affected.sum()))
 return dict(mirror_pixels=int(mirror.sum()),offscreen_blue_reflection_pixels=int(affected.sum()),emitter_primary_pixels=0)

def inspect(root,out,cases):
 manifest=read(out/'run.json');binary=root/'target/release/mcbe-client.exe';assert sha(binary)==manifest['binary_sha256'];assert all(sha(root/n)==h for n,h in manifest['source_hashes'].items());checks={}
 for case in cases:
  p=out/case;g=read(p/'gpu.json');s=g['statistics'];r=g['renderer'];assert g['complete'] and s['native_resolution'] and s['meshes_ready'];assert read(p/'window.json')['maximized'];check=dict(timing=g['timing'],renderer=r)
  if case=='fallback':assert r['method']=='deferred' and not r['genuine_ray_tracing'] and 'unavailable' in r['fallback'] and not s['hardware_ray_tracing']
  elif case=='ray_toggle':
   switch=read(p/'lighting-switch.json');assert switch['complete'];assert [v['stage'] for v in switch['trace']]==['forward','deferred','ray_traced','ray_quality','released'];assert r['method']=='forward' and s['graphics_bytes']==0;check['switch']=switch
  else:
   assert r['method']=='ray_traced' and r['genuine_ray_tracing'] and s['hardware_ray_tracing'];scene=r['ray_scene'];assert scene['ready'] and scene['pending']==0 and scene['vertices']>0 and scene['build_budget']==2;assert scene['reserved_vertex_bytes']<=2097024*40;assert scene['builds']==scene['instances'];buffers(p);geometry=read(p/'ray-geometry.json');assert geometry['complete'] and geometry['vertices']==scene['vertices'] and len(geometry['parts'])==scene['instances'];check['geometry']=geometry
   if case in ['ray_sun','ray_unshadowed']:check['independent_sunlight']=sunlight(p)
   if case=='ray_menu':assert g['menu']=='Graphics' and g['settings']['rendering']['method']=='ray_traced'
  checks[case]=check
 if all(c in cases for c in ['ray_mirror','ray_no_reflections']):checks['reflection_comparison']=compare_reflections(out)
 if all(c in cases for c in ['ray_indirect','ray_no_indirect']):
  a=np.array(Image.open(out/'ray_indirect/gpu.png').convert('RGB')).astype(int);b=np.array(Image.open(out/'ray_no_indirect/gpu.png').convert('RGB')).astype(int);affected=int((np.max(a-b,axis=2)>2).sum());assert affected>100;checks['indirect_comparison']=dict(affected_pixels=affected)
 result=dict(complete=True,cases=cases,checks=checks,**manifest,scope='Native DX12 hardware-query execution; independent analytic solid/partial-geometry sunlight and BRDF pixels; reflections from an emitter absent from primary visibility; indirect-light switch; quality/cutouts/transparency; actual settings transitions/resource release; FXC fallback. Streaming, edits and eviction stress are separate required checks.')
 (out/'summary.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8');print(json.dumps(dict(complete=True,cases=len(cases),sunlight={k:v.get('independent_sunlight') for k,v in checks.items() if k in ['ray_sun','ray_unshadowed']},reflections=checks.get('reflection_comparison'))),flush=True)

def main():
 ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,required=True);ap.add_argument('--inspect',action='store_true');ap.add_argument('--cases',nargs='+',choices=CASES,default=CASES);a=ap.parse_args();root=Path(__file__).resolve().parents[1];out=a.out.resolve();binary=root/'target/release/mcbe-client.exe'
 if not a.inspect:
  if out.exists():raise ValueError('Fresh proof directory required; use --inspect for existing captures')
  out.mkdir(parents=True);files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'content').rglob('*')),*sorted((root/'vendor').rglob('*.rs')),root/'vendor/wgpu-hal/Cargo.toml',Path(__file__)];manifest=dict(binary_sha256=sha(binary),source_hashes={p.relative_to(root).as_posix():sha(p) for p in files if p.is_file()});(out/'run.json').write_text(json.dumps(manifest,indent=2),encoding='utf-8')
  for case in a.cases:
   target=out/case;environment=os.environ.copy();environment.pop('WGPU_DX12_COMPILER',None)
   if case=='fallback':environment['WGPU_DX12_COMPILER']='fxc'
   run=subprocess.run([str(binary),'--engine-lighting-smoke',str(target),'--engine-lighting-case','ray_sun' if case=='fallback' else case,'--engine-home',str(root),'--engine-original'],cwd=root,env=environment,capture_output=True,text=True,timeout=240);(out/(case+'.stdout.txt')).write_text(run.stdout,encoding='utf-8');(out/(case+'.stderr.txt')).write_text(run.stderr,encoding='utf-8');assert run.returncode==0,(case,run.stderr);print(json.dumps(dict(captured=case)),flush=True)
 inspect(root,out,a.cases)
if __name__=='__main__':main()
