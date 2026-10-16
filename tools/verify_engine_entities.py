"""Native entity behaviors, bounded scheduling, persistence and independent instance pixel proofs."""
import argparse,hashlib,json,subprocess,math
from pathlib import Path
import numpy as np
from PIL import Image
CASES=['showcase','wire','follow','flee','path','interaction','occluded','occluded_clear','budget','save','natural','high','custom','showcase-deferred','showcase-ray','shadow_off-deferred','shadow_off-ray']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p):return json.loads(p.read_bytes())
def hits(g,image,step=4):
 h,width=image.shape[:2];yy,xx=np.mgrid[0:h:step,0:width:step];x=xx.ravel();y=yy.ravel();eye=np.array(g['camera']['eye']);forward=np.array(g['camera']['direction']);forward/=np.linalg.norm(forward);right=np.cross(forward,[0,1,0]);right/=np.linalg.norm(right);up=np.cross(right,forward);tan=math.tan(math.radians(g['settings']['fov']/2));directions=forward+right*((x+.5)/width*2-1)[:,None]*width/h*tan+up*(1-(y+.5)/h*2)[:,None]*tan
 count=len(x);best=np.full(count,np.inf);materials=np.full(count,-1);inside=np.zeros(count,bool);normals=np.zeros((count,3));shades=np.zeros(count);tints=np.ones((count,3));ids=np.zeros(count,dtype=np.uint64)
 for instance in g['entities']['instances']:
  matrix=np.array(instance['model']).T;inv=np.linalg.inv(matrix);origin=(inv@np.r_[eye,1])[:3];rays=directions@inv[:3,:3].T;safe=np.where(abs(rays)<1e-12,1e-12,rays);a=-origin/safe;b=(1-origin)/safe;low=np.minimum(a,b);high=np.maximum(a,b);near=low.max(axis=1);far=high.min(axis=1);valid=(near>0)&(near<far)&(near<best)
  axis=low.argmax(axis=1);points=origin+rays*near[:,None];interior=np.ones(count,bool)
  for v in range(3):interior&=(axis==v)|((points[:,v]>.08)&(points[:,v]<.92))
  local=np.zeros((count,3));local[np.arange(count),axis]=np.where(rays[np.arange(count),axis]>0,-1,1);normal=local@matrix[:3,:3].T;normal/=np.linalg.norm(normal,axis=1)[:,None];sun=np.array([-.4,.8,-.25]);sun/=np.linalg.norm(sun);shade=.5+.5*np.maximum(normal@sun,0);metadata=instance['metadata'];tint=[1,.35,.35] if metadata[1]&1 else ([.8,1,.85] if metadata[1]&2 else [1,1,1])
  best[valid]=near[valid];materials[valid]=metadata[0]//6;inside[valid]=interior[valid];normals[valid]=normal[valid];shades[valid]=shade[valid];tints[valid]=tint;ids[valid]=metadata[2]+(metadata[3]<<32)
 floor=np.divide(1-eye[1],directions[:,1],out=np.full(count,np.inf),where=directions[:,1]<-1e-9);mask=np.isfinite(best)&(best<floor-.001)&(y<h-110);mask&=~((abs(x-width/2)<14)&(abs(y-h/2)<14));return dict(x=x,y=y,mask=mask,inside=inside,material=materials,normal=normals,shade=shades,tint=tints,id=ids,eye=eye,directions=directions,floor_t=floor,best_t=best)
def actor_shadow_samples(g,reference):
 points=reference['eye']+reference['directions']*reference['floor_t'][:,None];points[:,1]+=0.025;sun=np.array(g['renderer']['frame']['sun'][:3]);shadow=np.zeros(len(points),bool)
 for instance in g['entities']['instances']:
  matrix=np.array(instance['model']).T
  if abs(np.linalg.det(matrix[:3,:3]))<.1:continue
  inv=np.linalg.inv(matrix);origin=points@inv[:3,:3].T+inv[:3,3];direction=inv[:3,:3]@sun;safe=np.where(abs(direction)<1e-12,1e-12,direction);a=-origin/safe;b=(1-origin)/safe;low=np.minimum(a,b);high=np.maximum(a,b);near=low.max(axis=1);far=high.min(axis=1);axis=low.argmax(axis=1);hit=origin+near[:,None]*direction;interior=np.ones(len(points),bool)
  for v in range(3):interior&=(axis==v)|((hit[:,v]>.2)&(hit[:,v]<.8))
  shadow|=(near>0)&(near<far)&interior
 h,w=g['statistics']['height'],g['statistics']['width'];mask=np.isfinite(reference['floor_t'])&(reference['floor_t']<reference['best_t']-.001)&(reference['y']>100)&(reference['y']<h-110)&shadow;mask&=~((abs(reference['x']-w/2)<16)&(abs(reference['y']-h/2)<16));return mask
def linear(v):return np.where(v<=.04045,v/12.92,((v+.055)/1.055)**2.4)
def srgb(v):return np.where(v<=.0031308,v*12.92,1.055*np.maximum(v,0)**(1/2.4)-.055)
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,required=True);args=ap.parse_args();root=Path(__file__).resolve().parents[1];out=args.out.resolve();binary=root/'target/release/mcbe-client.exe'
 if out.exists():raise ValueError('Fresh entity verification directory required')
 out.mkdir(parents=True);bh=sha(binary);files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'crates').rglob('Cargo.toml')),*sorted((root/'content').rglob('*')),*sorted((root/'assets').rglob('*')),*sorted((root/'vendor').rglob('*.rs')),Path(__file__)];hashes={p.relative_to(root).as_posix():sha(p) for p in files if p.is_file()};checks={};occlusion={};shadow_pairs={}
 for case in CASES:
  base=case.removesuffix('-deferred').removesuffix('-ray');home=root
  if case=='custom':
   home=out/'custom-home';(home/'content/base').mkdir(parents=True);(home/'assets/textures').mkdir(parents=True);registry=read(Path(read(out/'showcase/entities.json')['saved_world']))['registry'];azure=next(b for b in registry['blocks'] if b['id']=='azure');azure['texture_files']=['assets/textures/entity_probe.png']*6;(home/'assets/blocks.json').write_text(json.dumps(registry),encoding='utf-8');Image.new('RGBA',(16,16),(32,220,80,255)).save(home/'assets/textures/entity_probe.png');defs=read(root/'content/base/entities.json');comp=next(d for d in defs['kinds'] if d['id']=='companion');comp['body']['width']=.8;comp['body']['height']=1.9;comp['model'][1]['bounds']=[-.4,1.1,-.35,.4,1.9,.35];(home/'content/base/entities.json').write_text(json.dumps(defs),encoding='utf-8')
  target=out/case;method='ray_traced' if case.endswith('-ray') else ('deferred' if case.endswith('-deferred') else 'forward');result=subprocess.run([str(binary),'--engine-entity-smoke',str(target),'--engine-entity-case',base,'--engine-home',str(home),'--engine-original','--engine-renderer',method],cwd=root,capture_output=True,text=True,timeout=240)
  (out/(case+'.stdout.txt')).write_text(result.stdout,encoding='utf-8');(out/(case+'.stderr.txt')).write_text(result.stderr,encoding='utf-8');assert result.returncode==0,(case,result.stderr)
  p=read(target/'entities.json');g=read(target/'gpu.json');w=read(target/'window.json');assert p['complete'] and g['complete'] and p['collision_clear'] and p['saved_entities_equal'] and p['paused_clock_preserved'];assert w['maximized'] and not w['fullscreen'] and g['statistics']['native_resolution'];assert read(Path(p['saved_world']))['version']==13
  render=g['entities']['render'];assert 0<render['visible']<=128 and render['instances']<=128*32 and render['draws']>0 and g['entities']['unit_cube_vertices']==36;assert render['bytes']==g['entities']['instance_capacity']*g['entities']['instance_stride']+36*g['entities']['cube_stride'] and render['bytes']<1024*1024
  for t in p['trace']:assert t['stats']['physics_steps']<=128 and t['stats']['decisions']<=24 and t['stats']['paths']<=4 and t['stats']['path_nodes']<=512
  image=np.array(Image.open(target/'gpu.png').convert('RGB')).astype(np.int16);proof={};reference=None
  if base in ['showcase','shadow_off','wire','custom','occluded','occluded_clear']:
   reference=hits(g,image);mask=reference['mask']&reference['inside'];samples=int(mask.sum());assert samples>50,(case,samples)
   if base=='custom':
    azure=next(i for i,b in enumerate(read(Path(p['saved_world']))['registry']['blocks']) if b['id']=='azure');mask&=reference['material']==azure;scale=g['statistics']['gui_scale'];mask&=~((abs(reference['x']-image.shape[1]/2)<300*scale)&(abs(reference['y']-image.shape[0]/2-16*scale)<14*scale));expected=np.rint(srgb(linear(np.array([32,220,80])/255)*reference['shade'][:,None]*reference['tint'])*255);actual=image[reference['y'],reference['x']];error=np.max(abs(actual-expected),axis=1);samples=int(mask.sum());bad=int(((error>3)&mask).sum());assert samples>100 and bad==0,(case,samples,bad,float(error[mask].max()));proof['custom_png']={'samples':samples,'bad':bad,'max_error':float(error[mask].max())}
   if base=='wire':
    pixels=image[reference['y'],reference['x']];green=(pixels[:,1]>230)&(pixels[:,1]>pixels[:,0]+80)&(pixels[:,1]>pixels[:,2]+60);count=int((green&reference['mask']).sum());assert count>20,count;proof['wire_pixels']=count
   if case.endswith('-deferred') or case.endswith('-ray'):
    height,width=image.shape[:2];data=np.fromfile(target/'gbuffer-normal.bin',dtype=np.float16).astype(float).reshape(height,width,4)[reference['y'],reference['x']];error=np.max(abs(data[:,:3]-reference['normal']),axis=1);bad=int(((error>.003)&mask).sum());assert bad==0 and int(mask.sum())>100,(case,bad,float(error[mask].max()));assert np.all(abs(data[mask,3]-reference['material'][mask])<.01);proof['gbuffer_normals']={'samples':int(mask.sum()),'bad':bad,'max_error':float(error[mask].max())};assert render['shadow_draws']>0 if base!='shadow_off' else render['shadow_draws']==0
   if base in ['occluded','occluded_clear']:occlusion[base]=(image,reference)
   if base in ['showcase','shadow_off'] and method!='forward':shadow_pairs[case]=(g,image,reference)
  if case.endswith('-ray'):assert g['renderer']['genuine_ray_tracing'] and read(target/'ray-geometry.json')['complete']
  if base=='budget':assert p['world']['count']==201 and render['budget_culled']>0 and render['range_culled']>0
  if base=='high':assert p['world']['entities'][0]['transform']['position'][1]>=4097
  if base=='interaction':assert p['world']['entities'][0]['health']['current']==16 and p['world']['entities'][0]['brain']['following']
  if base in ['showcase','wire','budget','save','high','occluded','occluded_clear','custom']:assert render['uploads']<g['statistics']['frames'],(case,render)
  times=[t['ms'] for t in p['trace']];checks[case]={'count':p['world']['count'],'render':render,'tick_ms':{'maximum':max(times),'p95':float(np.percentile(times,95))},'pixels':proof,'image_sha256':sha(target/'gpu.png')};print(json.dumps({'case':case,**checks[case]}),flush=True)
 clear,ref=occlusion['occluded_clear'];hidden,_=occlusion['occluded'];mask=ref['mask']&ref['inside'];x=ref['x'];y=ref['y'];changed=int(((abs(clear[y,x]-hidden[y,x]).max(axis=1)>10)&mask).sum());assert changed>50,changed

 shadow_proof={}
 for method in ['deferred','ray']:
  g,on,ref=shadow_pairs['showcase-'+method];_,off,_=shadow_pairs['shadow_off-'+method];mask=actor_shadow_samples(g,ref);x=ref['x'];y=ref['y'];difference=(off[y,x]-on[y,x])@np.array([.2126,.7152,.0722]);count=int(mask.sum());dark=int(((difference>3)&mask).sum());assert count>30 and dark/count>.9,(method,count,dark,float(difference[mask].min()));shadow_proof[method]={'samples':count,'darker':dark,'minimum_difference':float(difference[mask].min())}
 assert sha(binary)==bh and all(sha(root/n)==h for n,h in hashes.items());(out/'summary.json').write_text(json.dumps({'complete':True,'binary_sha256':bh,'source_hashes':hashes,'checks':checks,'occlusion_changed_samples':changed,'dynamic_sun_shadows':shadow_proof,'scope':'Native typed mob components/AI/physics, bounded scheduling/pathfinding, lifecycle, high-altitude and custom-definition saves, shared actions, real instanced geometry in forward/deferred/ray rendering, independent PNG shading and rotated G-buffer normal/material pixels, wire triangles, terrain occlusion and static upload reuse. Ray secondary traversal covers static world geometry; dynamic actor sun shadows use the raster shadow map.'},indent=2)+'\n',encoding='utf-8');print(json.dumps({'complete':True,'cases':len(checks),'occlusion_samples':changed}),flush=True)
if __name__=='__main__':main()
