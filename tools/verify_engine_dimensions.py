"""Independent analytic generator checks and actual native dimension/GPU proofs."""
import argparse, hashlib, json, math, shutil, subprocess
from pathlib import Path
import numpy as np
from PIL import Image
VERTEX=np.dtype([('position','<f4',(3,)),('uv','<f4',(2,)),('layer','<u4'),('shade','<f4'),('bary','<f4',(3,))])
MASK=(1<<64)-1

def read(p): return json.loads(Path(p).read_bytes())
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def noise_hash(x,z,seed,salt=0):
    x=np.asarray(x,dtype=np.int64).astype(np.uint64);z=np.asarray(z,dtype=np.int64).astype(np.uint64)
    with np.errstate(over='ignore'):
        n=np.uint64((seed^salt)&MASK)^(x*np.uint64(0x9e3779b97f4a7c15))^(z*np.uint64(0xbf58476d1ce4e5b9))
        n=(n^(n>>np.uint64(30)))*np.uint64(0xbf58476d1ce4e5b9);n=(n^(n>>np.uint64(27)))*np.uint64(0x94d049bb133111eb)
        return n^(n>>np.uint64(31))
def noise2(x,z,scale,seed,salt):
    x=np.asarray(x,dtype=np.int64);z=np.asarray(z,dtype=np.int64);gx=x//scale;gz=z//scale
    fade=lambda t:t*t*t*(t*(t*6-15)+10)
    u=fade((x%scale)/scale);v=fade((z%scale)/scale)
    h=lambda a,b:(noise_hash(a,b,seed,salt)>>np.uint64(11)).astype(np.float64)/(1<<53)*2-1
    a=h(gx,gz)*(1-u)+h(gx+1,gz)*u;b=h(gx,gz+1)*(1-u)+h(gx+1,gz+1)*u
    return a*(1-v)+b*v

def columns(terrain,seed,x,z):
    x=np.asarray(x,dtype=np.int64);z=np.asarray(z,dtype=np.int64);g=terrain['generator'];o=g['options'];kind=g['kind'];sea=terrain['sea_level'];nan=lambda:np.full(x.shape,np.nan)
    bottom=nan();ceiling=nan();roof=nan();water=nan();present=np.ones(x.shape,bool);coverage=np.ones(x.shape);wet=np.zeros(x.shape,np.int8)
    n=lambda scale,salt:noise2(x,z,scale,seed,salt)
    if kind=='surface':
        c=n(o['continent_scale'],0x1001);ridges=(1-n(o['mountain_scale'],0x1006).__abs__())**3
        height=sea+o['base_height']+c*o['continent_height']+n(o['hill_scale'],0x1004)*o['hill_height']+n(o['detail_scale'],0x1005)*o['detail_height']+np.clip((c-.05)*2,0,1)*ridges*o['mountain_height']
        ocean=o['oceans']&(c<o['ocean_threshold']);river=np.abs(n(o['river_scale'],0x1007));level=sea+(np.maximum(c,0)*4).astype(np.int64)
        active=o['rivers']&~ocean&(river<o['river_width'])&(height>level-o['river_depth']);t=np.clip(river/o['river_width'],0,1);t=t*t*t*(t*(t*6-15)+10)
        height=np.where(active,(level-o['river_depth'])*(1-t)+height*t,height);top=np.floor(height).astype(np.int64)
        wet=np.where(ocean,1,np.where(o['rivers']&(river<o['river_width']*.385),2,0)).astype(np.int8)
        water=np.where((wet==1)&(top<sea),sea,np.where((wet==2)&(top<level),level,np.nan))
        if terrain['ravines']:
            cx=x//512;cz=z//512;h=noise_hash(cx,cz,seed,0x2001);mid=cz*512+256+(noise2(x,np.zeros_like(z),128,seed,0x2002)*80).astype(np.int64)
            width=4+np.abs(n(64,0x2003))*3;distance=np.abs(z-mid);active=(wet==0)&~np.isfinite(water)&(h%np.uint64(3)==0)&((x%512)>=48)&((x%512)<464)&(distance<width)
            drop=np.floor((1-(distance/width)**3)*34).astype(np.int64);top=top-np.where(active,drop,0)
    elif kind=='cavern':
        top=o['floor']+np.floor(n(o['floor_scale'],0x4101)*o['floor_height']).astype(np.int64)
        ceiling=top+o['clearance']+np.floor(n(o['ceiling_scale'],0x4102)*o['ceiling_height']).astype(np.int64);roof=ceiling+o['roof_thickness']
        if o['water_level'] is not None:water=np.where((o['water_level']>top)&(o['water_level']<ceiling),o['water_level'],np.nan)
    elif kind=='islands':
        spacing=o['spacing'];radius=o['radius'];gx=x//spacing;gz=z//spacing;best=np.full(x.shape,np.inf);chosen=np.zeros(x.shape,np.uint64);jitter=max(spacing-radius*2,0)//3
        for a in [-1,0,1]:
            for b in [-1,0,1]:
                h=noise_hash(gx+a,gz+b,seed,0x5101)
                dx=((h>>np.uint64(8))%np.uint64(2*jitter+1)).astype(np.int64)-jitter;dz=((h>>np.uint64(24))%np.uint64(2*jitter+1)).astype(np.int64)-jitter
                cx=(gx+a)*spacing+spacing//2+dx;cz=(gz+b)*spacing+spacing//2+dz
                rx=radius*(.78+((h>>np.uint64(40))&np.uint64(255)).astype(np.float64)/255*.22);rz=radius*(.78+((h>>np.uint64(48))&np.uint64(255)).astype(np.float64)/255*.22)
                q=((x-cx)/rx)**2+((z-cz)/rz)**2;select=q<best;best=np.where(select,q,best);chosen=np.where(select,h,chosen)
        variation=((chosen>>np.uint64(32))%np.uint64(o['altitude_variation']*2+1)).astype(np.int64)-o['altitude_variation'];shape=np.sqrt(np.maximum(1-best,0))
        top=o['altitude']+variation+np.floor(shape*o['relief']+n(o['detail_scale'],0x5102)*o['detail_height']).astype(np.int64)
        bottom=top-2-np.floor(shape*o['thickness']).astype(np.int64);present=best<1;coverage=1-best
    else:raise ValueError('Reference generator not implemented: '+kind)
    return dict(top=top,bottom=bottom,ceiling=ceiling,roof_top=roof,water=water,present=present,coverage=coverage,wet=wet)

def check_columns(report):
    rows=report['columns'];t=report['generator']['terrain'];seed=report['seed'];x=np.array([r['x'] for r in rows]);z=np.array([r['z'] for r in rows]);expected=columns(t,seed,x,z)
    for key in ['top','bottom','ceiling','roof_top','water']:
        actual=np.array([np.nan if r[key] is None else r[key] for r in rows]);e=expected[key];assert np.array_equal(np.isnan(actual),np.isnan(e)),key
        assert np.all(actual[np.isfinite(e)]==e[np.isfinite(e)]),(key,np.max(abs(actual[np.isfinite(e)]-e[np.isfinite(e)])))
    assert np.array_equal(np.array([r['present']for r in rows]),expected['present'])
    assert np.max(abs(np.array([r['coverage']for r in rows])-expected['coverage']))<1e-10
    climate=t['generator']['climate'];temp=noise2(x,z,climate['temperature_scale'],seed,0x1002);moist=noise2(x,z,climate['moisture_scale'],seed,0x1003)
    biomes=[]
    for i in range(len(rows)):
        hint=climate['ocean'] if expected['wet'][i]==1 else climate['river'] if expected['wet'][i]==2 else None
        if hint:biomes.append(hint);continue
        matches=[b['id'] for b in t['biomes'] if b['id'] not in [climate['ocean'],climate['river']] and b['temperature'][0]<=temp[i]<=b['temperature'][1] and b['moisture'][0]<=moist[i]<=b['moisture'][1] and b['altitude'][0]<=expected['top'][i]<=b['altitude'][1]]
        biomes.append(matches[0] if matches else climate['fallback'])
    assert biomes==[r['biome'] for r in rows]
    return dict(samples=len(rows),biomes=sorted(set(biomes)),void_columns=int((~expected['present']).sum()))

def field(terrain,seed,x,z,center,layer,step=None):
    x=np.asarray(x,dtype=np.float64);z=np.asarray(z,dtype=np.float64)
    def value(a,b):
        c=columns(terrain,seed,a,b)
        if layer=='coverage':return np.clip(c['coverage'],-1,1)
        return np.where(np.isfinite(c[layer]),c[layer],c['top']).astype(np.float64)
    def bilinear(step):
        x0=(np.floor(x).astype(np.int64)//step)*step;z0=(np.floor(z).astype(np.int64)//step)*step;u=(x-x0)/step;v=(z-z0)/step
        a=value(x0,z0);b=value(x0+step,z0);c=value(x0,z0+step);d=value(x0+step,z0+step)
        return (a*(1-u)+b*u)*(1-v)+(c*(1-u)+d*u)*v
    if step is not None:return bilinear(step)
    distance=np.maximum(abs(x-center[0]),abs(z-center[1]));lod=np.log2(np.clip(distance/32,1,64));size=(2**np.floor(lod)).astype(np.int64);weight=lod-np.floor(lod);weight=weight*weight*(3-2*weight)
    return bilinear(size)*(1-weight)+bilinear(np.minimum(size*2,64))*weight

def check_gpu(path,report,g):
    vertices=np.fromfile(path/'distant-solid.bin',dtype=VERTEX);water=np.fromfile(path/'distant-water.bin',dtype=VERTEX)
    s=g['statistics'];assert len(vertices)+len(water)==s['distant_vertices'] and len(water)==s['distant_water_vertices'];assert (len(vertices)+len(water))*40==s['distant_bytes'];assert len(vertices)+len(water)<=3145728
    layered=(vertices['layer']&0x18000000)==0x18000000
    if not np.any(layered):return dict(layered=False,actual_gpu_vertices=len(vertices))
    data=vertices[layered];assert np.isfinite(data['position']).all();tri=data.reshape(-1,3);normals=np.cross(tri['position'][:,1]-tri['position'][:,0],tri['position'][:,2]-tri['position'][:,0]);face=(tri['layer'][:,0]&0x3fffff)%6;nonzero=np.linalg.norm(normals,axis=1)>1e-7
    assert np.all(normals[(face==0)&nonzero,1]>0) and np.all(normals[(face==1)&nonzero,1]<0),'Horizontal LOD winding'
    chosen=data[::17];p=chosen['position'].astype(np.float64);face=(chosen['layer']&0x3fffff)%6
    center=(np.array(g['streaming']['center'],dtype=np.float64)*16+8);dist=np.maximum(abs(p[:,0]-center[0]),abs(p[:,2]-center[1]));integer=np.max(abs(p[:,[0,2]]-np.rint(p[:,[0,2]])),axis=1)<1e-5
    mask=integer&(dist>=144)&(face<2);p=p[mask];face=face[mask];dist=dist[mask];t=report['generator']['terrain'];seed=report['seed'];kind=t['generator']['kind']
    coverage=field(t,seed,p[:,0],p[:,2],center,'coverage');mask=coverage>.25;p=p[mask];face=face[mask];dist=dist[mask]
    far=np.clip((dist-128)/128,0,1);far=far*far*(3-2*far)
    expected=lambda key:field(t,seed,p[:,0],p[:,2],center,key,1)*(1-far)+field(t,seed,p[:,0],p[:,2],center,key)*far
    if kind=='cavern':error=np.where(face==1,abs(p[:,1]-expected('ceiling')),np.minimum(abs(p[:,1]-expected('top')),abs(p[:,1]-expected('roof_top'))))
    else:error=np.where(face==1,abs(p[:,1]-expected('bottom')),abs(p[:,1]-expected('top')))
    assert len(error)>50 and np.max(error)<.005,(len(error),float(np.max(error)) if len(error) else None)
    return dict(layered=True,actual_gpu_vertices=len(vertices),analytic_height_samples=len(error),max_height_error=float(np.max(error)),winding_triangles=int(nonzero.sum()))

def inspect(path):
    path=Path(path);r=read(path/'dimensions.json');g=read(path/'gpu.json');w=read(path/'window.json');s=g['statistics'];assert r['complete'] and g['complete'] and w['maximized'] and not w['fullscreen'] and s['native_resolution'] and s['meshes_ready'] and s['distant_ready'];assert r['all_worlds_roundtrip'] and r['shared_inventory_preserved'] and r['save_instance_preserved'] and r['save_meshes_unchanged'];assert r['near_geometry']['all_vertex_bytes_match'] and r['gpu_readback']['actual_gpu_buffer_readback']
    saved=read(r['saved_world']);assert saved['version']==13 and saved['dimensions']['active']==r['active'];assert len(saved['dimensions']['definitions']['dimensions'])>=3
    result=dict(columns=check_columns(r),gpu=check_gpu(path,r,g),statistics=s,image_sha256=sha(path/'gpu.png'))
    image=np.array(Image.open(path/'gpu.png').convert('RGB'));height,width=image.shape[:2];color=np.array(next(d for d in r['definitions']['dimensions'] if d['id']==r['active'])['environment']['fog_color']);sky=np.rint(np.where(color<=.0031308,color*12.92,1.055*color**(1/2.4)-.055)*255)
    if r['generator']['terrain']['generator']['kind']=='cavern':
        region=image[int(height*.02):int(height*.15),int(width*.35):int(width*.65)];holes=int(np.all(abs(region.astype(float)-sky)<=2,axis=2).sum());assert holes<=32,('Cavern ceiling sky gaps',holes);result['ceiling_sky_pixels']=holes
    if r['source_return_equal']:assert [e['active']for e in r['trace']]==['cavern','islands','surface']
    if r['case']=='cancel':assert r['cancellation_source_preserved'] and g['loading']['counters']['cancelled']>=1
    if r['case']=='controller-return':assert r['input']['active_device']=='gamepad' and r['input']['injected_raw_state']
    if r['case']=='reload-invalid':assert r['reload_rejected']
    return result

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--inspect',type=Path);ap.add_argument('--out',type=Path);a=ap.parse_args()
    if a.inspect:print(json.dumps(inspect(a.inspect),indent=2));return
    if not a.out:ap.error('--out or --inspect required')
    root=Path(__file__).resolve().parents[1];out=a.out.resolve();binary=root/'target/release/mcbe-client.exe'
    assert not out.exists(),'Fresh dimension proof directory required'
    out.mkdir(parents=True)
    files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'content').rglob('*')),*sorted((root/'tools').glob('verify_engine_*.py'))]
    hashes={p.relative_to(root).as_posix():sha(p) for p in files if p.is_file()};bh=sha(binary)
    home=out/'custom-home';shutil.copytree(root/'content',home/'content')
    index=read(home/'content/base/world/dimensions.json');islands=read(home/'content/base/world/dimensions/islands.json')
    for i in range(7):
        d=json.loads(json.dumps(islands));d['id']='sky_gardens' if i==0 else 'extra_'+str(i);d['label']='Sky Gardens' if i==0 else 'Example '+str(i)
        if i==0:d['terrain']['generator']['options'].update(altitude=224,spacing=160,radius=56)
        name='dimensions/'+d['id']+'.json';index['dimensions'].append(name);(home/'content/base/world'/name).write_text(json.dumps(d),encoding='utf-8')
    (home/'content/base/world/dimensions.json').write_text(json.dumps(index),encoding='utf-8')
    invalid=out/'invalid-home';shutil.copytree(root/'content',invalid/'content')
    cases=[('surface','showcase',16,'forward',root,True),('cavern','showcase',16,'forward',root,True),('islands','showcase',16,'forward',root,True),('islands','underside',16,'forward',root,True),('cavern','wire',16,'forward',root,True),('islands','wire',16,'forward',root,True),('cavern','showcase',255,'forward',root,True),('islands','showcase',255,'forward',root,True),('cavern','showcase',16,'deferred',root,True),('islands','showcase',16,'deferred',root,True),('islands','showcase',16,'ray_traced',root,True),('surface','picker',16,'forward',root,True),('surface','travel-cavern',16,'forward',root,True),('surface','travel-islands',16,'forward',root,True),('surface','travel-return',16,'forward',root,True),('surface','controller-return',16,'forward',root,True),('surface','cancel',255,'forward',root,True),('sky_gardens','showcase',16,'forward',home,True),('surface','pages',16,'forward',home,True),('surface','travel-custom',16,'forward',home,True),('surface','reload-invalid',16,'forward',invalid,True),('islands','motion',16,'forward',root,True),('surface','travel-return',16,'ray_traced',root,True),('surface','travel-return',16,'forward',root,False)]
    checks={}
    for dimension,case,distance,renderer,source,original in cases:
        name=f'{dimension}-{case}-{distance}-{renderer}'+('' if original else '-installed');target=out/name
        args=[str(binary),'--engine-dimensions-smoke',str(target),'--engine-dimension',dimension,'--engine-dimensions-case',case,'--engine-home',str(source),'--engine-render-distance',str(distance),'--engine-renderer',renderer]
        if original:args.append('--engine-original')
        process=subprocess.run(args,cwd=root,capture_output=True,text=True,timeout=240)
        (out/(name+'.stdout.txt')).write_text(process.stdout,encoding='utf-8');(out/(name+'.stderr.txt')).write_text(process.stderr,encoding='utf-8')
        assert process.returncode==0,(name,process.stderr)
        checks[name]=inspect(target)
        if renderer=='ray_traced':assert read(target/'gpu.json')['renderer']['genuine_ray_tracing']
        print(json.dumps(dict(case=name,complete=True,gpu=checks[name]['gpu'])),flush=True)
    assert sha(binary)==bh and all(sha(root/n)==h for n,h in hashes.items()),'Proof source/binary changed during verification'
    result=dict(complete=True,binary_sha256=bh,source_hashes=hashes,checks=checks,scope='Actual maximized native client at native resolution: three authored generators, custom and paginated dimensions, keyboard/raw XInput travel, per-world persistence and shared offhand, async cancellation and rejected F6 reload, layered GPU geometry independently sampled against generator equations, solid ceiling coverage, 255-chunk budgets, forward/deferred/hardware-ray rendering and installed textures. CPU tests additionally check closed distant shells. Synthetic controller input uses the real router; a physical controller has not been tested. Capture timing includes diagnostic GPU readback and save validation; it is not a gameplay FPS benchmark.')
    (out/'summary.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(dict(complete=True,captures=len(checks),binary_sha256=bh)),flush=True)
if __name__=='__main__':main()
