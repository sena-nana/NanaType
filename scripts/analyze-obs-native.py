"""Analyze actual OBS recording plus native sender/fence observations."""
import argparse,json,subprocess,hashlib,collections
from pathlib import Path
import numpy as np
import imageio_ffmpeg

def dist(values):
 v=sorted(values)
 return {'samples':len(v),'median':v[len(v)//2] if v else None,'p95':v[len(v)*95//100] if v else None,'max':v[-1] if v else None}
def main():
 parser=argparse.ArgumentParser();parser.add_argument('root',type=Path);parser.add_argument('--video-offset',type=float,default=.32);parser.add_argument('--history-rows',type=int,default=1);parser.add_argument('--active-after',type=float,default=4);a=parser.parse_args();r=a.root
 native=json.loads((r/'native-report.json').read_text());samples=json.loads((r/'samples.json').read_text());actions=json.loads((r/'timeline.json').read_text());actions=[{**x,'t':x['t']-a.video_offset} for x in actions];record=json.loads((r/'record-result.json').read_text())
 video=record['record']['outputPath'];ffmpeg=imageio_ffmpeg.get_ffmpeg_exe();reader=subprocess.Popen([ffmpeg,'-v','error','-i',video,'-an','-vf','scale=480:270','-f','rawvideo','-pix_fmt','rgb24','pipe:1'],stdout=subprocess.PIPE)
 disabled=[];off=None
 for action in actions:
  c=action.get('command',{})
  if c.get('op')=='outputs':
   if not c['enabled']:off=action['t']-1/60
   elif off is not None:disabled.append((off,action['t']+.5));off=None
 if off is not None:disabled.append((off,float('inf')))
 blanks={'display':[],'history':[]};expected_history_empty=[];max_same={'display':0,'history':0};run={'display':0,'history':0};previous={};frames=0;changes=collections.Counter();groups={'display':(slice(5,85),slice(10,250)),'history':(slice(105,195),slice(10,250))};animated={'gothic','celestial','forest','tidal','lantern','citrus','night','rpg','bubble','livechat','terminal','pixel','sakura'}
 events=[x for x in actions if 'command' in x];current=0;skin='tidal';motion=True
 while True:
  raw=reader.stdout.read(480*270*3)
  if not raw:break
  assert len(raw)==480*270*3
  t=frames/60;frames+=1
  while current<len(events) and events[current]['t']<=t:
   c=events[current]['command'];current+=1
   if c['op']=='skin':skin=c['id'].split('.')[-1]
   if c['op']=='motion':motion=c['enabled']
  image=np.frombuffer(raw,dtype=np.uint8).reshape(270,480,3)
  active=t>a.active_after and not any(x<=t<=y for x,y in disabled)
  for name,region in groups.items():
   crop=image[region];bg=image[250,450].astype(np.int16);pixels=int(np.count_nonzero(np.max(abs(crop.astype(np.int16)-bg),axis=2)>10))
   if active and pixels<50 and name=='history' and skin=='echo_void' and a.history_rows==1:expected_history_empty.append(frames-1)
   elif active and pixels<50:blanks[name].append({'frame':frames-1,'t':t,'pixels':pixels,'skin':skin})
   digest=hashlib.blake2b(crop.tobytes(),digest_size=8).digest()
   moving=active and name=='display' and motion and skin in animated
   if moving and previous.get(name)==digest:run[name]+=1
   else:
    max_same[name]=max(max_same[name],run[name]);run[name]=0
    if moving:changes[skin]+=1
   previous[name]=digest
 reader.wait();assert reader.returncode==0
 disabled=[(x,min(y,frames/60)) for x,y in disabled]
 bywindow={}
 for window in sorted({x['window'] for x in native['frames']}):
  rows=[x for x in native['frames'] if x['window']==window];warm=[x['send_cpu_ms'] for x in rows if x['ms']>3000]
  sent=[x for x in native['sends'] if x['window']==window];bywindow[str(window)]={'frames':len(rows),'publish_cpu_ms':dist(warm),'outcomes':dict(collections.Counter(x['status'] for x in sent)),'max_pool_opens':max(x['pool_opens'] for x in sent),'native_generations':len({x['generation'] for x in sent}),'failed_counter_max':max(x['failed'] for x in sent),'skipped_counter_max':max(x['skipped'] for x in sent)}
 summary={'video':video,'video_frames':frames,'video_pts_offset_s':a.video_offset,'expected_empty_history_frames':len(expected_history_empty),'blank_candidate_frames':{k:len(v) for k,v in blanks.items()},'blank_details':blanks,'max_identical_animated_frames':max_same,'animated_changed_frames':dict(changes),'native':native['summary'],'per_window':bywindow,'release_observed_ms':dist(native['release_observed_ms']),'ready_to_release_observed_ms':dist(native['ready_to_release_observed_ms']),'obs_render_skipped_delta':record['after']['renderSkippedFrames']-record['before']['renderSkippedFrames'],'obs_render_frames_delta':record['after']['renderTotalFrames']-record['before']['renderTotalFrames'],'memory_samples':[{k:s[k] for k in ['t','app_private','app_handles','obs_private','obs_handles']} for s in samples], 'intentional_output_off_intervals':disabled,'note':'60 fps OBS canvas, 30 fps WGSL is expected. Identical-pixel spans also depend on artwork contrast and encoding; they are not transport frame timestamps. Fence observations are 1 ms polling upper bounds.'}
 (r/'analysis.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2));print(json.dumps({k:v for k,v in summary.items() if k not in ['memory_samples','native','blank_details']},ensure_ascii=False,indent=2))
if __name__=='__main__':main()
