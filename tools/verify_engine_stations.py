"""Native station interactions, save schema, GPU progress pixels and editable layout."""
import argparse, hashlib, json, subprocess
from pathlib import Path
from PIL import Image

def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p): return json.loads(p.read_bytes())
def main():
    parser=argparse.ArgumentParser(); parser.add_argument('--out',type=Path,required=True); args=parser.parse_args()
    root=Path(__file__).resolve().parents[1]; binary=root/'target/release/mcbe-client.exe'; out=args.out.resolve()
    if out.exists(): raise ValueError('Fresh station verification folder required')
    out.mkdir(parents=True); binary_hash=sha(binary)
    files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'content').rglob('*')),*sorted((root/'assets').rglob('*')),Path(__file__)]
    sources={p.relative_to(root).as_posix():sha(p) for p in files if p.is_file()}; checks={}
    def launch(name,flag,home=root):
        target=out/name; run=subprocess.run([str(binary),flag,str(target),'--engine-home',str(home)],cwd=root,capture_output=True,text=True,timeout=150)
        (out/(name+'.stdout.txt')).write_text(run.stdout,encoding='utf-8');(out/(name+'.stderr.txt')).write_text(run.stderr,encoding='utf-8')
        if run.returncode: raise ValueError(name+': '+run.stderr)
        gpu=read(target/'gpu.json'); window=read(target/'window.json'); report=read(target/'stations.json')
        assert gpu['complete'] and window['complete'] and report['complete'] and report['raycast_open']
        assert window['maximized'] and not window['fullscreen'] and gpu['statistics']['native_resolution']
        assert gpu['palette_open'] and gpu['ui']['screen']==report['station'] and 'error' not in gpu['ui']
        checks[name]=dict(report=report,image_sha256=sha(target/'gpu.png'))
        print(json.dumps(dict(case=name,complete=True)),flush=True); return target,gpu
    table,tg=launch('table','--engine-table-smoke'); assert checks['table']['report']['furnace_recipe_taken'] and checks['table']['report']['close_returns_inputs']
    furnace,fg=launch('furnace','--engine-furnace-smoke'); report=checks['furnace']['report']
    assert report['runtime_ui_open_updates'] and report['timer_preserves_terrain_revision'] and report['output_withdrawal']
    saved=read(furnace/'processing.blockworld'); assert saved['version']==13
    machine=saved['machines']['furnaces'][0]; assert machine['cook_ms']==2500 and machine['burn_ms']==2500
    assert machine['slots']==[dict(block='sand',count=2),dict(block='oak_planks',count=1),dict(block='glass',count=1)]
    widgets=lambda g:{w['id']:w for w in g['ui']['widgets']}
    def pixel(g,image,rect,fx):
        s=g['statistics']['gui_scale'];return image.getpixel((int((rect[0]+rect[2]*fx)*s),int((rect[1]+rect[3]/2)*s)))
    srgb=lambda v:round(255*(v*12.92 if v<=.0031308 else 1.055*v**(1/2.4)-.055))
    image=Image.open(furnace/'gpu.png').convert('RGB'); pixels={}
    for name,color,fraction in [('cook',(.15,.68,.44),.25),('burn',(1,.32,.04),.5)]:
        assert abs(fg['ui']['model'][name+'_progress']-fraction)<1e-6
        rect=widgets(fg)[name]['rect']; filled=pixel(fg,image,rect,.15); empty=pixel(fg,image,rect,.8)
        expected=tuple(srgb(v)for v in color); dark=tuple(srgb(v)for v in (.1,.12,.15))
        assert max(abs(a-b)for a,b in zip(filled,expected))<=2,(name,filled,expected)
        assert max(abs(a-b)for a,b in zip(empty,dark))<=2,(name,empty,dark)
        pixels[name]=dict(filled=filled,empty=empty)
    home=out/'custom-home'; ui=home/'content/base/ui';ui.mkdir(parents=True)
    layout=read(root/'content/base/ui/furnace.json'); theme=read(root/'content/base/ui/theme.json')
    for widget in layout['root']['children']:
        if widget['id']=='machine_2':widget['layout']['offset'][0]=145
    (ui/'furnace.json').write_text(json.dumps(layout),encoding='utf-8');(ui/'theme.json').write_text(json.dumps(theme),encoding='utf-8')
    custom,cg=launch('custom-furnace-layout','--engine-furnace-smoke',home)
    assert widgets(cg)['machine_2']['rect'][0]!=widgets(fg)['machine_2']['rect'][0]
    assert checks['custom-furnace-layout']['report']['output_withdrawal']
    assert sha(binary)==binary_hash and all(sha(root/n)==h for n,h in sources.items())
    result=dict(complete=True,binary_sha256=binary_hash,source_hashes=sources,checks=checks,progress_pixels=pixels,scope='Native raycast station opening, shared mouse/shift transactions, 3x3 crafting and close return, actual App machine updates with UI open, saved furnace timers/items, GPU progress fill/empty pixels, and moved JSON furnace slot input. Rust tests additionally cover scheduler budget/fairness, frame-rate clock, output blocking, chunk eviction, concurrent snapshots and registry remapping. No claim for the remaining camera/terrain/render/controller/entity goal systems.')
    (out/'summary.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8');print(json.dumps(dict(complete=True,cases=len(checks))),flush=True)
if __name__=='__main__':main()
