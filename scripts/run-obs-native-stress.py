"""Record and observe a local, isolated OBS/Spout session. No streaming."""
import json,time,runpy,argparse,hashlib
from pathlib import Path
import psutil
OBS=runpy.run_path(str(Path(__file__).with_name('obs-native-probe.py')))['OBS']
def main():
 parser=argparse.ArgumentParser();parser.add_argument('root',type=Path);parser.add_argument('--rounds',type=int,default=3);parser.add_argument('--focus-history',action='store_true');a=parser.parse_args();root=a.root;obs=OBS(root.parent);pid=int((root/'app.pid').read_text());proc=psutil.Process(pid);obsproc=psutil.Process(int((root.parent/'obs.pid').read_text()))
 # Wait for the actual native sender, rather than recording DLL/host startup.
 deadline=time.monotonic()+60
 while True:
  try:
   if json.loads((root/'live.json').read_text()).get('sends',0)>=2:break
  except (ValueError,FileNotFoundError):pass
  if not proc.is_running() or time.monotonic()>deadline:raise RuntimeError('native sender did not become ready')
  time.sleep(.1)
 seq=100;actions=[];samples=[];start=time.monotonic();last=0
 def snapshot():
  nonlocal last
  if time.monotonic()-last<1:return
  last=time.monotonic();sample={'t':last-start,'app_private':proc.memory_info().private,'app_handles':proc.num_handles(),'app_threads':proc.num_threads(),'obs_private':obsproc.memory_info().private,'obs_handles':obsproc.num_handles(),'obs':obs.call('GetStats')}
  live=root/'live.json'
  try:sample['native']=json.loads(live.read_text())
  except (ValueError,FileNotFoundError):pass
  samples.append(sample);(root/'samples.json').write_text(json.dumps(samples))
 def wait(seconds):
  end=time.monotonic()+seconds
  while time.monotonic()<end:snapshot();time.sleep(.1)
 def command(op,**args):
  nonlocal seq
  seq+=1;v={'op':op,**args};p=root/'commands'/f'{seq:05d}.tmp';p.write_text(json.dumps(v,ensure_ascii=False));p.rename(p.with_suffix('.json'));actions.append({'t':time.monotonic()-start,'command':v});(root/'timeline.json').write_text(json.dumps(actions,ensure_ascii=False,indent=2));print(v,flush=True)
 try:
  info=obs.call('CreateInput',{'sceneName':'Probe','inputName':'ProbeBackground','inputKind':'color_source_v3','inputSettings':{'color':0xff303030,'width':1920,'height':1080},'sceneItemEnabled':True});obs.call('SetSceneItemIndex',{'sceneName':'Probe','sceneItemId':info['sceneItemId'],'sceneItemIndex':0})
 except AssertionError:pass
 obs.call('SetRecordDirectory',{'recordDirectory':str(root.parent/'recordings')})
 before=obs.call('GetStats');obs.call('StartRecord');(root/'record-start.json').write_text(json.dumps({'monotonic':start,'stats':before,'version':obs.call('GetVersion')}));wait(2)
 command('send',text='[@name=NATIVE-PROBE]OBS Spout 实测：边框、正文与动态装饰 0123456789');wait(.2);command('skip');wait(8)
 # Keep an older row visible even when a skin deliberately hides the current row.
 command('send',text='[@name=NATIVE-PROBE]第二条测试正文：历史区域保持非空');wait(.2);command('skip');wait(2)
 if a.focus_history:
  for _ in range(a.rounds):
   for skin in ['echo_vanilla','echo_void','forest','tidal']:
    command('skin',id='nanatype.'+skin);wait(1.5)
  obs.screenshot('Probe',root/'final.png');after=obs.call('GetStats');stop=obs.call('StopRecord');(root/'record-result.json').write_text(json.dumps({'before':before,'after':after,'record':stop}));command('outputs',enabled=False);command('visible',visible=False);wait(3);command('quit');return
 ids=[p.stem for p in sorted(Path('crates/theme/builtin').glob('*.toml'))]
 for round in range(a.rounds):
  for skin in ids:
   command('skin',id='nanatype.'+skin);wait(1.5)
  command('skin',id='nanatype.tidal');wait(5)
  obs.screenshot('Probe',root/f'round-{round}.png')
 command('skin',id='user.nativeprobe');wait(3)
 source=Path('crates/theme/builtin/tidal/artwork.wgsl').read_text();effect=root/'app/data/skins/user.nativeprobe/effect.wgsl'
 for i in range(20):
  text='fn effect(i:EffectInput)->vec4<f32>{loop{} return vec4(0.);}' if i%5==0 else source+f'\n// native reload revision {i}\n'
  effect.write_text(text);actions.append({'t':time.monotonic()-start,'hot_reload':i,'valid':i%5!=0});(root/'timeline.json').write_text(json.dumps(actions,ensure_ascii=False,indent=2));wait(1.5)
 command('skin',id='nanatype.tidal');wait(3);command('visible',visible=False);wait(5);obs.screenshot('Probe',root/'hidden-output.png');command('visible',visible=True);wait(3)
 for i in range(8):command('size',width=960+64*(i%2));wait(1.5)
 command('size',width=960);wait(3)
 for i in range(5):command('outputs',enabled=False);wait(1);command('outputs',enabled=True);wait(2)
 command('motion',enabled=False);wait(5);command('motion',enabled=True);wait(5)
 command('skin',id='nanatype.halo');wait(5);command('skin',id='nanatype.tidal');wait(5)
 obs.screenshot('Probe',root/'final.png');after=obs.call('GetStats');stop=obs.call('StopRecord');(root/'record-result.json').write_text(json.dumps({'before':before,'after':after,'record':stop}));command('outputs',enabled=False);command('visible',visible=False);wait(6)
 command('quit');print(stop,flush=True)
if __name__=='__main__':main()
