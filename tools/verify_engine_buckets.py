"""Native bucket use, measured quantities, persistence, held geometry and pixels."""
import argparse,hashlib,json,math,subprocess
from pathlib import Path
import numpy as np
from PIL import Image

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p):return json.loads(p.read_bytes())
def linear(v):return np.where(v<=.04045,v/12.92,((v+.055)/1.055)**2.4)
def srgb(v):return np.where(v<=.0031308,v*12.92,1.055*v**(1/2.4)-.055)
def hits(vertices,directions):
    best=np.full(len(directions),np.inf);shades=np.zeros(len(directions));layers=np.zeros(len(directions),dtype=int);interior=np.zeros(len(directions),dtype=bool)
    for i in range(0,len(vertices),3):
        tri=vertices[i:i+3];a,b,c=[np.array(v['position'],dtype=float) for v in tri];e1=b-a;e2=c-a;normal=np.cross(e1,e2);front=directions@normal<0
        h=np.cross(directions,e2);det=h@e1;valid=np.abs(det)>1e-10;inv=np.divide(1.,det,out=np.zeros_like(det),where=valid);s=-a;u=(h@s)*inv;q=np.cross(s,e1);v=(directions@q)*inv;t=(q@e2)*inv
        ok=valid&front&(u>=0)&(v>=0)&(u+v<=1)&(t>0)&(t<best)
        best[ok]=t[ok];shades[ok]=tri[0].get('shade',1.);layers[ok]=tri[0].get('layer',0);interior[ok]=(u[ok]>.06)&(v[ok]>.06)&(u[ok]+v[ok]<.94)
    return best,shades,layers,interior

def pixels(g,img,water_id,bucket_id):
    height,width,_=img.shape;yy,xx=np.mgrid[0:height:3,0:width:3];x=xx.ravel();y=yy.ravel();tan=math.tan(math.radians(35));directions=np.stack([((x+.5)/width*2-1)*width/height*tan,(1-(y+.5)/height*2)*tan,-np.ones(len(x))],axis=1)
    vertices=g['character']['item_geometry'];wet=[v for v in vertices if v['layer']&0x400000];solid=[v for v in vertices if not v['layer']&0x400000];tw,sw,_,inside=hits(wet,directions);tb,sb,lb,_=hits(solid,directions);th,_,_,_=hits(g['character']['hand_geometry'],directions)
    mask=inside&np.isfinite(tw)&np.isfinite(tb)&(tw<tb)&(tb<th)&((lb&0x3fffff)//6==bucket_id)&(y<height-110)
    # Uniform owned bucket material and uniform water material, blended in linear light.
    metal=linear(np.array([165.,175.,185.])/255)*sb[:,None];water=linear(np.array([45.,115.,195.])/255)*sw[:,None];alpha=90/255
    expected=np.rint(srgb(water*alpha+metal*(1-alpha))*255);actual=img[y,x].astype(float);error=np.max(abs(actual-expected),axis=1);samples=int(mask.sum());bad=int(((error>3)&mask).sum())
    assert samples>100 and bad==0,(samples,bad,float(error[mask].max()) if samples else None)
    assert all((v['layer']&0x3fffff)//6==water_id for v in wet)
    return dict(samples=samples,bad=bad,max_error=float(error[mask].max()))

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,required=True);a=ap.parse_args();root=Path(__file__).resolve().parents[1];out=a.out.resolve();binary=root/'target/release/mcbe-client.exe'
    if out.exists():raise ValueError('Fresh bucket verification directory required')
    out.mkdir(parents=True);bh=sha(binary);files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'content').rglob('*')),Path(__file__)];sources={p.relative_to(root).as_posix():sha(p) for p in files if p.is_file()};checks={};geometry={}
    for case in ['empty','partial','full','inventory']:
        target=out/case;run=subprocess.run([str(binary),'--engine-bucket-smoke',str(target),'--engine-bucket-case',case,'--engine-home',str(root)],cwd=root,capture_output=True,text=True,timeout=180)
        (out/(case+'.stdout.txt')).write_text(run.stdout,encoding='utf-8');(out/(case+'.stderr.txt')).write_text(run.stderr,encoding='utf-8')
        if run.returncode:raise ValueError(case+': '+run.stderr)
        report=read(target/'buckets.json');g=read(target/'gpu.json');w=read(target/'window.json');stats=g['statistics'];assert report['complete'] and report['mass']==10240 and report['actual_use_ray'] and report['creative_selection'] and report['save_reload_equal'];assert g['complete'] and stats['meshes_ready'] and stats['native_resolution'] and stats['mesh_peak_jobs']<=2 and stats['texture_uploads']==1 and w['maximized'] and not w['fullscreen']
        save=read(Path(report['saved_world']));assert save['version']==13;blocks=save['registry']['blocks'];water_id=next(i for i,b in enumerate(blocks) if b['id']=='water');bucket_id=next(i for i,b in enumerate(blocks) if b['id']=='bucket');held=g['character'];geometry[case]=held['item_vertices']
        if case=='empty':assert report['carried']==0 and held['item_data']['amount']==0
        if case=='partial':assert 0<report['carried']<1024 and held['item_data']['amount']==report['carried']
        if case=='full':assert report['carried']==1024 and held['item_data']['amount']==1024
        if case=='inventory':assert g['palette_open'] and g['ui']['screen']=='inventory' and save['inventory']['crafting'][3]['data']['amount']==report['carried']
        check=dict(report=report,statistics=stats,image_sha256=sha(target/'gpu.png'))
        if case in ['partial','full']:check['held_water_pixels']=pixels(g,np.array(Image.open(target/'gpu.png').convert('RGB')),water_id,bucket_id)
        checks[case]=check;print(json.dumps(dict(case=case,carried=report['carried'],pixels=check.get('held_water_pixels'))),flush=True)
    assert geometry['partial']==geometry['full']==geometry['empty']+6 and geometry['inventory']==0
    assert sha(binary)==bh and all(sha(root/n)==h for n,h in sources.items())
    result=dict(complete=True,binary_sha256=bh,source_hashes=sources,checks=checks,scope='Actual creative selection, fluid-specific use ray, measured collect/top-up/pour, inventory and Ctrl+S/F9 recovery; native geometry, independent held-water alpha pixels and bounded render jobs. Full fluid/distance/player regressions remain separate.')
    (out/'summary.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8');print(json.dumps(dict(complete=True,cases=len(checks))),flush=True)
if __name__=='__main__':main()
