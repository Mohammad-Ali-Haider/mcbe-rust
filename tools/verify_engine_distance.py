"""Bind native 255-chunk LOD, menu, shrink and save/reload checks to a build."""
import argparse,hashlib,json,subprocess
from pathlib import Path
from PIL import Image

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p):return json.loads(p.read_bytes())
def main():
    ap=argparse.ArgumentParser();ap.add_argument('--out',type=Path,required=True);a=ap.parse_args();root=Path(__file__).resolve().parents[1];out=a.out.resolve();binary=root/'target/release/mcbe-client.exe'
    if out.exists():raise ValueError('Fresh proof directory required')
    out.mkdir(parents=True);bh=sha(binary)
    files=[root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'crates').rglob('*.rs')),*sorted((root/'crates').rglob('*.wgsl')),*sorted((root/'assets').rglob('*')),Path(__file__)]
    hashes={p.relative_to(root).as_posix():sha(p)for p in files if p.is_file()};checks={}
    def launch(name,flag,extra=()):
        p=out/name;r=subprocess.run([str(binary),flag,str(p),*map(str,extra)],cwd=root,capture_output=True,text=True,timeout=150)
        (out/(name+'.stdout.txt')).write_text(r.stdout,encoding='utf-8');(out/(name+'.stderr.txt')).write_text(r.stderr,encoding='utf-8')
        if r.returncode:raise ValueError(name+': '+r.stderr)
        g=read(p/'gpu.json');w=read(p/'window.json');s=g['statistics'];assert g['complete'] and w['complete'] and s['native_resolution'] and w['maximized'] and not w['fullscreen']
        assert s['mesh_peak_jobs']<=2 and g['streaming']['resident_columns']<=289 and s['distant_ready']
        checks[name]=dict(complete=True,image_sha256=sha(p/'gpu.png'),statistics=s,streaming=g['streaming']);print(json.dumps(dict(case=name,vertices=s['distant_vertices'],detail_columns=g['streaming']['resident_columns'])),flush=True);return p,g
    maximum,g=launch('maximum','--engine-distance-smoke');s=g['statistics'];assert s['distant_jobs']==1 and 0<s['distant_vertices']<=3145728 and s['distant_bytes']<=3145728*40 and s['distant_area_blocks']==(511*16)**2-256**2
    assert s['distant_trees']>1000 and s['distant_stitched_edges']>0 and s['distant_fine_vertices']>0
    assert g['settings']['render_distance']==255 and g['streaming']['full_detail_radius']==8 and g['streaming']['resident_columns']==289
    doc=read(maximum/'distance.json');saved=Path(doc['saved_world']);assert doc['saved_radius']==255 and doc['saved_residents']==289 and read(maximum/'settings.json')['render_distance']==255
    low=read(saved);low['player']['y']=20.;low['pitch']=-.04;low_path=out/'low-view.blockworld';low_path.write_text(json.dumps(low),encoding='utf-8')
    ground,gg=launch('ground-view','--engine-render-smoke',['--engine-load',low_path,'--engine-render-distance','255']);assert gg['player_position']['y']==20.
    image=Image.open(ground/'gpu.png').convert('RGB');width,height=image.size;sky=image.getpixel((10,10));band=image.crop((0,int(height*.497),width,int(height*.657)));gaps=sum(p==sky for p in band.getdata());assert gaps<=max(32,width*height//100000), ('ground transition sky gaps',gaps)
    menu,mg=launch('menu','--engine-distance-settings-smoke');assert mg['menu']=='Settings' and mg['settings']['render_distance']==255
    shrink,sh=launch('shrink','--engine-distance-shrink-smoke');t=sh['settings_trace'];assert len(t)==3 and t[0]['stream']['radius']==255 and t[0]['distant']['vertices']>0
    assert t[1]['stream']['radius']==2 and t[1]['stream']['resident_columns']==25 and t[1]['distant']['vertices']==0 and t[2]['all_vertex_bytes_match']
    assert s['distant_vertices']>0 and sh['statistics']['distant_vertices']==0
    ref,rg=launch('sync-reference','--engine-render-smoke',['--engine-load',saved,'--engine-render-distance','255','--engine-sync-mesh']);assert sha(ref/'gpu.png')==sha(maximum/'gpu.png')
    startup,st=launch('startup-reload','--engine-startup-smoke',['--engine-load',saved,'--engine-render-distance','255']);assert sha(startup/'gpu.png')==sha(maximum/'gpu.png')
    assert read(startup/'startup-native.json')['geometry']['all_vertex_bytes_match']
    small,sm=launch('small','--engine-render-smoke',['--engine-stream']);assert sm['settings']['render_distance']==3 and sm['statistics']['distant_vertices']==0 and sm['statistics']['distant_jobs']==0
    motion,mo=launch('motion','--engine-distance-motion-smoke');mt=mo['lod_motion_trace'];ms=mo['statistics']
    assert mt and mt[-1]['stage']==5 and ms['distant_crossfades']==4 and ms['distant_retained_updates']==4
    assert all(t['vertices']>0 for t in mt), 'LOD vanished during a chunk crossing'
    assert all(any(t['stage']==stage and t['previous_vertices']>0 for t in mt)for stage in range(1,5))
    assert mt[-1]['previous_vertices']==0 and mt[-1]['ready']
    bad=subprocess.run([str(binary),'--engine','--engine-render-distance','256'],cwd=root,capture_output=True,text=True,timeout=10);assert bad.returncode!=0 and '2..255' in bad.stderr
    unchanged=sha(binary)==bh and all(sha(root/n)==h for n,h in hashes.items());assert unchanged
    result=dict(complete=True,patch_of_checkpoint=48,feature='Stitched, detailed LOD with spatial and temporal blending',binary_sha256=bh,source_hashes=hashes,source_unchanged=unchanged,saved_world=str(saved),checks=checks,maximum=dict(radius=255,full_detail_radius=8,full_detail_columns=289,distant_vertices=s['distant_vertices'],distant_bytes=s['distant_bytes'],distant_ground_area=s['distant_area_blocks'],trees=s['distant_trees'],stitched_edges=s['distant_stitched_edges'],fine_vertices=s['distant_fine_vertices']),ground_band_sky_pixels=gaps,exact_sync_and_startup_pixels=True,shrink_trace=t,motion_trace=mt,invalid_distance_rejected=True,scope='Native maximum range and settings, 255 preference/save roundtrip, exact async/sync/startup pixels, shrink-to-2, and four chunk crossings with retained old meshes and completed crossfades. Fine voxel contours overlap a progressively sampled, edge-stitched heightfield; generated tree silhouettes continue to96 chunks. Rust tests check area, bounds, winding, identical shared heights, surface edit sampling and cancellation. Sparse opaque/custom objects are represented within96 chunks; transparent objects and full collision stay nearby. Deep caves and distant chopped-tree edits are not represented. Vertex budget is per mesh, with at most two adopted GPU meshes during180ms blending. No guaranteed-FPS claim.')
    (out/'summary.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8');print(json.dumps(dict(complete=True,maximum=result['maximum'])),flush=True)
if __name__=='__main__':main()
