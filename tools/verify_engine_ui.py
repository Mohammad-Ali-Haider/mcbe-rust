"""Native shared UI, Rust-built screen actions and owned texture/layout checks."""
import argparse,hashlib,json,subprocess
from pathlib import Path
from PIL import Image

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p):return json.loads(p.read_bytes())
def main():
    parser=argparse.ArgumentParser();parser.add_argument('--out',type=Path,required=True);args=parser.parse_args()
    root=Path(__file__).resolve().parents[1];out=args.out.resolve();binary=root/'target/release/mcbe-client.exe'
    if out.exists():raise ValueError('Fresh verification folder required')
    out.mkdir(parents=True);binary_hash=sha(binary)
    files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'content').rglob('*')),*sorted((root/'assets').rglob('*')),Path(__file__)]
    sources={p.relative_to(root).as_posix():sha(p) for p in files if p.is_file()};checks={}
    def launch(name,flag,home=root):
        target=out/name;run=subprocess.run([str(binary),flag,str(target),'--engine-home',str(home)],cwd=root,capture_output=True,text=True,timeout=150)
        (out/(name+'.stdout.txt')).write_text(run.stdout,encoding='utf-8');(out/(name+'.stderr.txt')).write_text(run.stderr,encoding='utf-8')
        if run.returncode:raise ValueError(name+': '+run.stderr)
        gpu=read(target/'gpu.json');window=read(target/'window.json');assert gpu['complete'] and window['complete'] and window['maximized'] and not window['fullscreen'] and gpu['palette_open'];assert gpu['statistics']['native_resolution']
        assert 'error' not in gpu['ui'];checks[name]=dict(image_sha256=sha(target/'gpu.png'),screen=gpu['ui']['screen'],ui_uploads=gpu['statistics']['ui_uploads'])
        print(json.dumps(dict(case=name,screen=gpu['ui']['screen'])),flush=True);return target,gpu
    inventory,ig=launch('inventory','--engine-ui-smoke');assert ig['ui']['screen']=='inventory' and bool(ig['player_preview']['vertices'])
    widgets=lambda g:{w['id']:w for w in g['ui']['widgets']}
    lab,lg=launch('code-screen','--engine-ui-lab-smoke');assert lg['ui']['screen']=='ui_lab' and lg['ui']['counter']==4
    def point(g,rect,fx=.5,fy=.5):
        s=g['statistics']['gui_scale'];return int((rect[0]+rect[2]*fx)*s),int((rect[1]+rect[3]*fy)*s)
    progress=widgets(lg)['progress']['rect'];image=Image.open(lab/'gpu.png').convert('RGB')
    srgb=lambda v:round(255*(v*12.92 if v<=.0031308 else 1.055*v**(1/2.4)-.055))
    green=tuple(srgb(v)for v in (.15,.68,.44));dark=tuple(srgb(v)for v in (.1,.12,.15))
    filled=image.getpixel(point(lg,progress,.2));empty=image.getpixel(point(lg,progress,.8));assert max(abs(a-b)for a,b in zip(filled,green))<=2,(filled,green);assert max(abs(a-b)for a,b in zip(empty,dark))<=2,(empty,dark)
    home=out/'custom-home';ui=home/'content/base/ui';ui.mkdir(parents=True)
    layout=read(root/'content/base/ui/inventory.json');theme=read(root/'content/base/ui/theme.json')
    def edit(node):
        if node['kind']=='panel':node['style']='probe_panel'
        if node['id']=='craft_0':node['layout']['offset']=[230,18]
        for child in node.get('children',[]):edit(child)
    edit(layout['root'])
    theme['textures']['probe']='content/base/ui/probe.png';theme['styles']['probe_panel']=dict(fill=[1]*4,text=[.25,.25,.25,1],border=[.1,.12,.15,1],sprite='probe')
    (ui/'inventory.json').write_text(json.dumps(layout),encoding='utf-8');(ui/'theme.json').write_text(json.dumps(theme),encoding='utf-8');Image.new('RGBA',(16,16),(32,220,80,255)).save(ui/'probe.png')
    custom,cg=launch('custom-layout-sprite','--engine-ui-smoke',home);cw=widgets(cg);base=widgets(ig);assert cw['craft_0']['rect'][0]!=base['craft_0']['rect'][0]
    image=Image.open(custom/'gpu.png').convert('RGB');root_rect=cw['inventory_root']['rect'];scale=cg['ui']['layout_scale'];actual=cw['craft_0']['rect'];panel=(32,220,80)
    new_pixel=image.getpixel(point(cg,actual));old_rect=[root_rect[0]+246*scale,root_rect[1]+18*scale,18*scale,18*scale];old_pixel=image.getpixel(point(cg,old_rect));assert old_pixel==panel and new_pixel!=panel,(old_pixel,new_pixel)
    assert sha(binary)==binary_hash and all(sha(root/name)==digest for name,digest in sources.items())
    result=dict(complete=True,binary_sha256=binary_hash,source_hashes=sources,checks=checks,code_screen_counter=4,progress_pixels=dict(filled=filled,empty=empty),custom_sprite_pixel=old_pixel,moved_slot_pixel=new_pixel,scope='Native default inventory/shared hit boxes, code-built interactive screen with mouse and keyboard actions, GPU progress pixels, custom JSON slot movement and owned PNG panel pixels. Rust tests cover clipping, authoring rejection and atomic failed reload. Controller input and station machines remain separate goal work.')
    (out/'summary.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8');print(json.dumps(dict(complete=True,cases=len(checks))),flush=True)
if __name__=='__main__':main()
