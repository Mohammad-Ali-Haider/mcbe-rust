"""Checkpoint 48: native settings, variants, custom PNGs and independent glass pixels."""
import argparse, hashlib, json, math, subprocess
from pathlib import Path
import numpy as np
from PIL import Image

def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p): return json.loads(p.read_bytes())
def main():
    ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,required=True);a=ap.parse_args()
    root=Path(__file__).resolve().parents[1];out=a.out.resolve();binary=root/'target/release/mcbe-client.exe'
    if out.exists():raise ValueError('Fresh proof directory required')
    out.mkdir(parents=True)
    files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'assets').rglob('*')),*sorted((root/'content').rglob('*')),*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),Path(__file__)]
    hashes={p.relative_to(root).as_posix():sha(p)for p in files if p.is_file()};bh=sha(binary)
    def launch(name,flag,extra=()):
        d=out/name;r=subprocess.run([str(binary),flag,str(d),*map(str,extra)],cwd=root,capture_output=True,text=True,timeout=150)
        (out/(name+'.stdout.txt')).write_text(r.stdout,encoding='utf-8');(out/(name+'.stderr.txt')).write_text(r.stderr,encoding='utf-8')
        if r.returncode:raise ValueError(name+': '+r.stderr)
        g=read(d/'gpu.json');w=read(d/'window.json');assert g['complete'] and w['complete'] and w['maximized'] and not w['fullscreen'];assert g['statistics']['native_resolution']
        print(json.dumps({'native':name,'frames':g['statistics']['frames']}),flush=True);return d,g
    gallery,g=launch('gallery','--engine-features-smoke');report=read(gallery/'features.json');saved=Path(report['saved_world'])
    assert report['registry_blocks']==41 and report['variant_save_roundtrip'];assert g['statistics']['blended_vertices']>0
    # An idle native frame must reuse the two chunks containing blue glass.
    assert g['statistics']['blended_uploads']<=4 and g['statistics']['blended_sorts']<=4
    settings,sg=launch('settings','--engine-settings-smoke');trace=sg['settings_trace'];assert len(trace)==3
    assert trace[0]['stream']['radius']==8 and trace[0]['stream']['resident_columns']==289
    assert trace[1]['stream']['radius']==2 and trace[1]['stream']['resident_columns']==25
    assert trace[0]['position']==trace[1]['position']==sg['player_position'];assert trace[2]['all_vertex_bytes_match']
    assert sg['menu']=='Settings' and sg['settings']['frame_limit']==0 and sg['settings']['vsync'] is False
    assert read(settings/'settings.json')['render_distance']==8
    original,og=launch('original','--engine-features-smoke',['--engine-original']);original_saved=Path(read(original/'features.json')['saved_world'])
    sync,syncg=launch('sync','--engine-render-smoke',['--engine-sync-mesh','--engine-load',saved])
    assert sha(gallery/'gpu.png')==sha(sync/'gpu.png')
    doc=read(original_saved)
    doc['edits']=[e for e in doc['edits']if e['material'] not in ('glass','blue_glass')]
    noglass=out/'no-glass.blockworld';noglass.write_text(json.dumps(doc),encoding='utf-8')
    clear,cg=launch('no-glass','--engine-render-smoke',['--engine-original','--engine-load',noglass])
    # Independent camera rays select unobstructed interior pixels on the front wall.
    img=np.asarray(Image.open(original/'gpu.png').convert('RGB'));bg=np.asarray(Image.open(clear/'gpu.png').convert('RGB'));h,w=img.shape[:2]
    yy,xx=np.mgrid[0:h:4,0:w:4];pitch=-.02;t=math.tan(math.radians(70)/2);nx=((xx+.5)/w*2-1)*w/h*t;ny=(1-(yy+.5)/h*2)*t
    dy=math.sin(pitch)+ny*math.cos(pitch);dz=math.cos(pitch)-ny*math.sin(pitch)
    d=12.5/dz;px=.5-nx*d;py=5.62+dy*d
    backd=14.5/dz;bx=.5-nx*backd;by=5.62+dy*backd
    frac=lambda v:v-np.floor(v)
    common=(py>5.25)&(py<6.75)&(by>4.2)&(by<6.8)&(frac(bx)>.2)&(frac(bx)<.8)&(frac(by)>.2)&(frac(by)<.8)
    # Generated original-color glass has a two-texel border and a short diagonal.
    tx=np.floor(frac(1-px)*32);ty=np.floor(frac(1-py)*32)
    holes=(tx>=3)&(tx<29)&(ty>=3)&(ty<29)&~((abs(tx-ty)<2)&(tx<11))
    glass=common&(px> -4.8)&(px<.8)&holes
    actual=img[yy,xx];back=bg[yy,xx];err=np.max(abs(actual.astype(int)-back.astype(int)),axis=2)
    clear_count=int(glass.sum());clear_bad=int(((err>2)&glass).sum());assert clear_count>300 and clear_bad==0,(clear_count,clear_bad)
    linear=lambda v:np.where(v<=.04045,v/12.92,((v+.055)/1.055)**2.4)
    srgb=lambda v:np.where(v<=.0031308,v*12.92,1.055*v**(1/2.4)-.055)
    color=linear(np.array([64,132,224])/255)*.7;alpha=90/255
    expected=np.rint(srgb(color*alpha+linear(back/255)*(1-alpha))*255)
    blue=common&(px>1.2)&(px<4.8);blue_error=np.max(abs(actual.astype(int)-expected.astype(int)),axis=2)
    blue_count=int(blue.sum());blue_bad=int(((blue_error>3)&blue).sum());assert blue_count>300 and blue_bad==0,(blue_count,blue_bad,int(blue_error[blue].max()))
    # Own PNGs work in original mode, with no installed pack dependency.
    home=out/'custom-home';(home/'assets/textures').mkdir(parents=True);png=home/'assets/textures/custom.png';Image.new('RGBA',(16,16),(32,220,80,255)).save(png)
    custom=read(original_saved);custom['registry']['blocks'].append(dict(id='custom_png',label='Custom PNG',color=[1,1,1],pattern='solid',texture_files=['assets/textures/custom.png']*6))
    for e in custom['edits']:
        if e['material'] in ('glass','blue_glass'):e['material']='custom_png'
    custom_save=out/'custom.blockworld';custom_save.write_text(json.dumps(custom),encoding='utf-8')
    cp,cp_g=launch('custom-png','--engine-render-smoke',['--engine-original','--engine-home',home,'--engine-load',custom_save])
    ci=np.asarray(Image.open(cp/'gpu.png').convert('RGB'))[yy,xx];cm=common&(px> -4.8)&(px<4.8)
    expected_custom=np.rint(srgb(linear(np.array([32,220,80])/255)*.7)*255)
    ce=np.max(abs(ci.astype(int)-expected_custom.astype(int)),axis=2);assert cm.sum()>600 and np.max(ce[cm])<=3
    unchanged=sha(binary)==bh and all(sha(root/n)==v for n,v in hashes.items())
    result=dict(complete=unchanged,checkpoint=48,binary_sha256=bh,source_hashes=hashes,source_unchanged=unchanged,gallery=report,settings_trace=trace,async_sync_pixels_equal=True,idle_glass_cache=dict(uploads=g['statistics']['blended_uploads'],sorts=g['statistics']['blended_sorts']),pixels=dict(clear_count=clear_count,clear_bad=clear_bad,blue_count=blue_count,blue_bad=blue_bad,blue_max_error=int(blue_error[blue].max()),custom_count=int(cm.sum()),custom_max_error=int(ce[cm].max())),images={n:sha(out/n/'gpu.png')for n in ('gallery','settings','original','sync','no-glass','custom-png')},scope='Production menu actions expand/shrink residency 289 to 25 without moving the player; cache fingerprints after eviction; saved geometry variants; walk/flight sprint; independent ray-selected cutout, sRGB alpha-blend and own-PNG pixels; async/sync exact screenshot parity. Transparent face sorting assumes non-intersecting block surfaces. Performance counters are not a general FPS guarantee.')
    (out/'summary.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8');print(json.dumps({k:result[k]for k in ('complete','pixels','idle_glass_cache')}),flush=True)
    return 0 if unchanged else 1
if __name__=='__main__':raise SystemExit(main())
