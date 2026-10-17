"""Local OBS WebSocket control for isolated native Spout validation."""
import base64,hashlib,json,time,argparse
from pathlib import Path
import websocket
class OBS:
 def __init__(self,root):
  auth=json.loads((Path(root)/'obs-auth.json').read_text());self.ws=websocket.create_connection('ws://127.0.0.1:'+str(auth['port']),timeout=10);hello=json.loads(self.ws.recv())['d'];identify={'rpcVersion':1,'eventSubscriptions':0}
  if 'authentication' in hello:
   a=hello['authentication'];secret=base64.b64encode(hashlib.sha256((auth['password']+a['salt']).encode()).digest()).decode();identify['authentication']=base64.b64encode(hashlib.sha256((secret+a['challenge']).encode()).digest()).decode()
  self.ws.send(json.dumps({'op':1,'d':identify}));assert json.loads(self.ws.recv())['op']==2;self.seq=0
 def call(self,kind,data=None):
  self.seq+=1;request=str(self.seq);self.ws.send(json.dumps({'op':6,'d':{'requestType':kind,'requestId':request,'requestData':data or {}}}))
  while True:
   message=json.loads(self.ws.recv())
   if message['op']==7 and message['d']['requestId']==request:
    d=message['d'];assert d['requestStatus']['result'],(kind,d['requestStatus']);return d.get('responseData',{})
 def screenshot(self,source,path):
  data=self.call('GetSourceScreenshot',{'sourceName':source,'imageFormat':'png'})['imageData'];Path(path).write_bytes(base64.b64decode(data.split(',',1)[1]))
 def setup(self):
  for i in self.call('GetInputList')['inputs']:
   try:self.call('SetInputMute',{'inputName':i['inputName'],'inputMuted':True})
   except AssertionError:pass
  profiles=self.call('GetProfileList');print('profiles',profiles)
  if any(p=='NanaTypeProbe' for p in profiles['profiles']):self.call('SetCurrentProfile',{'profileName':'NanaTypeProbe'})
  self.call('SetVideoSettings',{'fpsNumerator':60,'fpsDenominator':1,'baseWidth':1920,'baseHeight':1080,'outputWidth':1920,'outputHeight':1080})
  print('version',self.call('GetVersion'));print('kinds',[k for k in self.call('GetInputKindList')['inputKinds'] if 'spout' in k.lower()])
if __name__=='__main__':
 parser=argparse.ArgumentParser();parser.add_argument('root',type=Path);parser.add_argument('action',choices=['setup','stats','sources','shot','stop']);a=parser.parse_args();o=OBS(a.root)
 if a.action=='setup':o.setup()
 elif a.action=='stats':print(json.dumps(o.call('GetStats')))
 elif a.action=='sources':print(json.dumps(o.call('GetInputList'),ensure_ascii=False));print(json.dumps(o.call('GetSceneList'),ensure_ascii=False))
 elif a.action=='shot':o.screenshot('Probe',a.root/'obs-preview.png')
 elif a.action=='stop':print(o.call('StopRecord'))
