"""Native local chat/commands, travel persistence and anatomical hand projection."""
import argparse,hashlib,json,subprocess
from pathlib import Path
import numpy as np
from PIL import Image

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p):return json.loads(p.read_bytes())
def anatomy(g,proof):
    ch=g['character'];body=np.array([v['position'] for v in ch['body_geometry']]);items=np.array([v['position'] for v in ch['item_geometry']]);camera=g['camera'];eye=np.array(camera['eye']);forward=np.array(camera['direction']);right=np.cross(forward,[0,1,0]);right/=np.linalg.norm(right);origin=np.array([g['player_position'][a] for a in ['x','y','z']]);expected=1 if ch['mode']=='third_back' else -1
    result={}
    for bone,sign in [('rightArm',expected),('leftArm',-expected)]:
        i=proof['body_bones'].index(bone);center=body[i*36:(i+1)*36].mean(axis=0);x=float((center-origin)@right);assert x*sign>.25,(bone,x);result[bone]=x
    n=ch['main_item_vertices'];main=float((items[:n].mean(axis=0)-origin)@right);off=float((items[n:].mean(axis=0)-origin)@right);assert main*expected>.25 and off*expected<-.25,(main,off);result.update(main=main,offhand=off)
    assert ch['selected']=='blue_glass' and ch['offhand']=='bucket' and ch['offhand_data']['amount']==331
    return result

def inspect(target):
    g=read(target/'gpu.json');w=read(target/'window.json');p=read(target/'chat.json');s=g['statistics'];assert p['complete'] and g['complete'] and w['maximized'] and not w['fullscreen'] and s['native_resolution'] and p['idle_ui_cached'] and p['terrain_unchanged_by_chat_and_save'] and p['given_items_roundtrip'] and p['dimensions_roundtrip'] and p['pose_preserved'];assert g['chat']==p['chat']
    save=read(Path(p['save']));assert save['version']==13
    case=p['case'];result=dict(image_sha256=sha(target/'gpu.png'),statistics=s,chat=p['chat'])
    if case in ['back','front']:result['anatomy']=anatomy(g,p)
    elif case!='controller':
        assert p['chat']['open'];im=np.array(Image.open(target/'gpu.png').convert('RGB'));h,width=im.shape[:2];scale=s['gui_scale'];panel=(min(420,width/scale-8))*scale;y=h-48*scale;input_roi=im[int(y-2*scale):int(y+10*scale),int(4*scale):int(4*scale+panel)];assert input_roi.size>0;dark=int((input_roi.max(axis=2)<150).sum());text=int((input_roi.min(axis=2)>160).sum());assert dark>input_roi.shape[0]*input_roi.shape[1]*.5 and text>10,(dark,text);result['input_pixels']=dict(dark=dark,text=text,bottom_left=True)
        if case=='errors':
            roi=im[max(0,int(y-110*scale)):int(y-8*scale),:int(4*scale+panel)].astype(float);red=(roi[:,:,0]>120)&(roi[:,:,0]>roi[:,:,1]*1.3)&(roi[:,:,0]>roi[:,:,2]*1.3);assert int(red.sum())>20;result['error_pixels']=int(red.sum())
    if case in ['give','travel','cancel','editing']:
        assert sum(v['count'] for v in save['inventory']['slots'] if v and v['block']=='glass')==70
    if case in ['give','travel','cancel']:
        buckets=[v for v in save['inventory']['slots'] if v and v['block']=='bucket'];assert len(buckets)==2 and all(v['count']==1 and v['data']['amount']==0 for v in buckets)
    if case=='travel':assert p['route']==['cavern','islands','surface'] and g['loading']['counters']['applied']==3 and len(save['dimensions']['inactive'])==2
    if case=='cancel':assert g['loading']['counters']['cancelled']==1 and g['loading']['counters']['applied']==0
    if case=='completion':assert p['chat']['input']=='/travel islands '
    if case=='message':assert any(m['text']=='<Player> Hello islands!' for m in p['chat']['messages'])
    return result

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--out',required=True,type=Path);a=ap.parse_args();root=Path(__file__).resolve().parents[1];out=a.out.resolve();binary=root/'target/release/mcbe-client.exe';assert not out.exists(),'Fresh chat proof directory required';out.mkdir(parents=True)
    files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'content').rglob('*')),*sorted((root/'assets').rglob('*')),Path(__file__)]
    hashes={p.relative_to(root).as_posix():sha(p) for p in files if p.is_file()};bh=sha(binary);checks={}
    cases=[(c,'forward',True) for c in ['open','free','message','give','errors','editing','completion','travel','cancel','controller','back','front']]+[('back','deferred',True),('front','ray_traced',True),('back','forward',False)]
    for case,renderer,original in cases:
        name=case+'-'+renderer+('' if original else '-installed');target=out/name;args=[str(binary),'--engine-chat-smoke',str(target),'--engine-chat-case',case,'--engine-home',str(root),'--engine-render-distance',('255' if case=='cancel' else '2'),'--engine-renderer',renderer]
        if original:args.append('--engine-original')
        process=subprocess.run(args,cwd=root,capture_output=True,text=True,timeout=180);(out/(name+'.stdout.txt')).write_text(process.stdout,encoding='utf-8');(out/(name+'.stderr.txt')).write_text(process.stderr,encoding='utf-8');assert process.returncode==0,(name,process.stderr)
        checks[name]=inspect(target)
        if renderer=='ray_traced':assert read(target/'gpu.json')['renderer']['genuine_ray_tracing']
        print(json.dumps(dict(case=name,complete=True,anatomy=checks[name].get('anatomy'))),flush=True)
    assert sha(binary)==bh and all(sha(root/n)==h for n,h in hashes.items()),'Source/binary changed during native verification'
    result=dict(complete=True,binary_sha256=bh,source_hashes=hashes,checks=checks,scope='Real maximized native-resolution client, same text handler as keyboard events, local chat/registry/give/travel, error feedback, history/completion, input isolation, actual three-world worker roundtrip and cancellation, item/dimension saves and cached idle HUD. Independent screenshot checks locate the bottom-left input and errors; renderer geometry projects anatomical arms/main/offhand in rear/front views and supported lighting. No physical-device automation or multiplayer messaging is claimed.')
    (out/'summary.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
if __name__=='__main__':main()
