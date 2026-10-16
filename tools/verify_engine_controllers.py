"""Native controller routing, shared UI transactions, contextual glyphs and real XInput worker."""
import argparse,hashlib,json,subprocess
from pathlib import Path
import numpy as np
from PIL import Image
CASES=['motion','inventory','keyboard','menu','lab','table','furnace','disconnect','takeover','keyboard-ray','inventory-custom']
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p):return json.loads(p.read_bytes())
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,required=True);a=ap.parse_args();root=Path(__file__).resolve().parents[1];out=a.out.resolve();binary=root/'target/release/mcbe-client.exe'
 if out.exists():raise ValueError('Fresh controller proof directory required')
 out.mkdir(parents=True);files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'crates').rglob('Cargo.toml')),*sorted((root/'content').rglob('*')),*sorted((root/'assets').rglob('*')),*sorted((root/'vendor').rglob('*.rs')),Path(__file__)];hashes={p.relative_to(root).as_posix():sha(p) for p in files if p.is_file()};bh=sha(binary);checks={}
 home=out/'custom-home';ui=home/'content/base/ui';ui.mkdir(parents=True)
 theme=read(root/'content/base/ui/theme.json');theme['textures']['pad_probe']='content/base/ui/pad_probe.png';theme['styles']['pad_a']={'fill':[1,1,1,1],'text':[1,1,1,1],'border':[0,0,0,1],'sprite':'pad_probe'}
 (ui/'theme.json').write_text(json.dumps(theme),encoding='utf-8');Image.new('RGBA',(16,16),(245,38,198,255)).save(ui/'pad_probe.png')
 layout=read(root/'content/base/ui/inventory.json')
 def move(node):
  if node['id']=='craft_0':node['layout']['offset']=[230,18]
  for child in node.get('children',[]):move(child)
 move(layout['root']);(ui/'inventory.json').write_text(json.dumps(layout),encoding='utf-8')
 for case in CASES:
  base=case.removesuffix('-ray').removesuffix('-custom');target=out/case
  command=[str(binary),'--engine-controller-smoke',str(target),'--engine-controller-case',base,'--engine-home',str(home if case.endswith('-custom') else root),'--engine-original',*(['--engine-renderer','ray_traced'] if case.endswith('-ray') else [])]
  result=subprocess.run(command,cwd=root,capture_output=True,text=True,timeout=180)
  (out/(case+'.stdout.txt')).write_text(result.stdout,encoding='utf-8');(out/(case+'.stderr.txt')).write_text(result.stderr,encoding='utf-8');assert result.returncode==0,(case,result.stderr)
  g=read(target/'gpu.json');p=read(target/'controller.json');w=read(target/'window.json');assert g['complete'] and p['complete'] and w['maximized'] and not w['fullscreen'] and g['statistics']['native_resolution']
  assert p['input']['injected_raw_state'] and p['input']['injections']>=3 and p['input']['worker_started'] and p['input']['poll_calls']>0
  assert g['input']['injected_raw_state'] and g['statistics']['ui_uploads']<g['statistics']['frames'],(case,g['statistics'])
  device='keyboard_mouse' if base in ['disconnect','takeover'] else 'gamepad';assert g['input']['active_device']==device,(case,g['input'])
  image=np.array(Image.open(target/'gpu.png').convert('RGB')).astype(np.int16);h,width=image.shape[:2];scale=g['statistics']['gui_scale'];roi=image[max(0,int(h-33*scale)):]
  left=roi[:,:width//3];right=roi[:,2*width//3:];white=lambda a:int(((a[:,:,0]>225)&(a[:,:,1]>225)&(a[:,:,2]>225)).sum());pixels=[white(left),white(right)];assert min(pixels)>30,(case,pixels)
  pink=int(((roi[:,:,0]==245)&(roi[:,:,1]==38)&(roi[:,:,2]==198)).sum())
  if case.endswith('-custom'):assert pink>40,(case,pink)
  if base=='keyboard':
   assert p['keyboard_open'] and p['filter']=='ab';k=read(target/'controller.json')['keyboard_pointer'];x,y=round(k[0]*scale),round(k[1]*scale);patch=image[y-8:y+8,x-8:x+8];green=int(((patch[:,:,1]>160)&(patch[:,:,1]>patch[:,:,0]+30)&(patch[:,:,1]>patch[:,:,2]+25)).sum());assert green>10,(case,k,green)
  if base=='menu':assert g['menu']=='Settings'
  if base=='lab':assert g['ui']['screen']=='ui_lab' and g['ui']['counter']==1
  if base in ['table','furnace']:assert g['ui']['screen']==('crafting_table' if base=='table' else 'furnace')
  if case.endswith('-ray'):assert g['renderer']['genuine_ray_tracing'] and read(target/'ray-geometry.json')['complete']
  checks[case]={'proof':p,'statistics':g['statistics'],'hint_text_pixels':pixels,'custom_glyph_pixels':pink,'image_sha256':sha(target/'gpu.png')};print(json.dumps({'case':case,'device':device,'hint_pixels':pixels,'custom_pixels':pink}),flush=True)
 assert sha(binary)==bh and all(sha(root/n)==h for n,h in hashes.items());(out/'summary.json').write_text(json.dumps({'complete':True,'binary_sha256':bh,'source_hashes':hashes,'checks':checks,'scope':'Injected raw XInput-compatible state routing; real XInput worker lifecycle/polling and native rendering. Physical controller is not connected/tested. Gameplay, double-tap flight, UI safety/focus, shared catalog/slot/crafting/furnace transactions, virtual/physical search, custom code screen, custom PNG controller glyph and hardware-ray overlay.'},indent=2)+'\n',encoding='utf-8');print(json.dumps({'complete':True,'cases':len(checks)}),flush=True)
if __name__=='__main__':main()
