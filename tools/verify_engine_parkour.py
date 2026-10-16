"""Native collision-driven traversal, keyboard/HUD and transient-save checks."""
import argparse,hashlib,json,subprocess
from pathlib import Path
import numpy as np
from PIL import Image
CASES=['climb','hang','mantle','run','jump','drop','ceiling','removed','slab','hang-ray','mantle-ray']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p):return json.loads(p.read_bytes())
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,required=True);a=ap.parse_args();root=Path(__file__).resolve().parents[1];out=a.out.resolve();binary=root/'target/release/mcbe-client.exe'
 if out.exists():raise ValueError('Fresh parkour proof directory required')
 out.mkdir(parents=True);files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'content').rglob('*')),*sorted((root/'vendor').rglob('*.rs')),Path(__file__)];hashes={p.relative_to(root).as_posix():sha(p) for p in files if p.is_file()};bh=sha(binary);checks={}
 for case in CASES:
  base_case=case.removesuffix('-ray');target=out/case;r=subprocess.run([str(binary),'--engine-parkour-smoke',str(target),'--engine-parkour-case',base_case,'--engine-home',str(root),'--engine-original',*(['--engine-renderer','ray_traced'] if case.endswith('-ray') else [])],cwd=root,capture_output=True,text=True,timeout=180);(out/(case+'.stdout.txt')).write_text(r.stdout,encoding='utf-8');(out/(case+'.stderr.txt')).write_text(r.stderr,encoding='utf-8');assert r.returncode==0,(case,r.stderr)
  g=read(target/'gpu.json');p=read(target/'parkour.json');w=read(target/'window.json');assert g['complete'] and p['complete'] and p['collision_clear'] and p['transient_attachment_cleared_on_load'];assert w['maximized'] and not w['fullscreen'] and g['statistics']['native_resolution'];assert all(t['grab'] for t in p['trace']);assert g['settings']['rendering']['method']==('ray_traced' if case.endswith('-ray') else 'forward');
  if case.endswith('-ray'):assert g['renderer']['genuine_ray_tracing'] and read(target/'ray-geometry.json')['complete']
  assert p['action']=={'climb':'climbing','hang':'hanging','mantle':'idle','run':'wall_running','jump':'wall_jump','drop':'idle','ceiling':'hanging','removed':'idle','slab':'hanging'}[base_case]
  pos=p['position'];trace=p['trace'];assert all(np.isfinite([v for t in trace for v in t['position'].values()]));pixels=None
  if p['action']!='idle':
   img=np.array(Image.open(target/'gpu.png').convert('RGB'));h,width=img.shape[:2];scale=g['statistics']['gui_scale'];y=int(h-49*scale);roi=img[y:y+int(10*scale),width//2:];mask=(roi[:,:,1]>245)&(roi[:,:,1].astype(int)>roi[:,:,0].astype(int)+12)&(roi[:,:,1].astype(int)>roi[:,:,2].astype(int)+8);pixels=int(mask.sum());assert pixels>30,(case,pixels)
  if base_case=='mantle':assert abs(pos['y']-6)<.002 and pos['z']>2.3 and any(t['action']=='mantling' for t in trace)
  if base_case=='hang':assert abs(pos['y']-4.6)<.002
  if case=='slab':assert abs(pos['y']-4.1)<.002
  if case=='jump':assert pos['x']<.9 and pos['y']>4 and any(t['action']=='wall_running' for t in trace)
  if case=='run':assert pos['z']>3.3
  if case=='drop':assert pos['y']<4.1
  if case=='ceiling':assert abs(pos['y']-4.6)<.002 and not any(t['action']=='mantling' for t in trace)
  checks[case]={'proof':p,'hint_pixels':pixels,'statistics':g['statistics'],'image_sha256':sha(target/'gpu.png')};print(json.dumps({'case':case,'action':p['action'],'hint_pixels':pixels}),flush=True)
 assert sha(binary)==bh and all(sha(root/n)==h for n,h in hashes.items());(out/'summary.json').write_text(json.dumps({'complete':True,'binary_sha256':bh,'source_hashes':hashes,'checks':checks,'scope':'Native fixed-tick ordinary-keyboard mapping; wall climb/run/jump, ledge hang/pull-up/drop; current registered collision geometry and slab tops; removed support/ceiling guards; contextual HUD pixels and transient save behavior.'},indent=2)+'\n',encoding='utf-8');print(json.dumps({'complete':True,'cases':len(checks)}),flush=True)
if __name__=='__main__':main()
