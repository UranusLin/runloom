"""Two controllers, real interactive stdin, fencing, renewal, expiry, restart."""
import json,subprocess,tempfile,time,select
from pathlib import Path
binary=str(Path(__file__).resolve().parents[1]/'target/debug/runloom-node')
with tempfile.TemporaryDirectory(prefix='rl-control-',dir='/tmp') as root:
 servers=[]
 def serve():
  p=subprocess.Popen([binary,'--root',root,'serve','--host','fixture','--allow-interactive'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True);servers.append(p)
  assert select.select([p.stdout],[],[],10)[0];assert json.loads(p.stdout.readline())['ready'];return p
 def req(op,ok=True,**fields):
  p=subprocess.run([binary,'--root',root,'request'],input=json.dumps({'version':1,'op':op,**fields})+'\n',text=True,capture_output=True,timeout=10)
  r=json.loads(p.stdout);assert r['ok']==ok,r;return r.get('result',r.get('error'))
 try:
  server=serve();work=req('register',work={'workspace':'p','host':'fixture','provider':'shell','native_id':'one','config_root':'/fixture','directory':root,'title':'control'})
  scope={'workspace':'p','work':work['id']}
  first=req('acquire',**scope,holder='Pro',ttl=30)
  req('acquire',ok=False,**scope,holder='Air',ttl=30)
  cmd={'executable':'/bin/sh','args':['-c','while IFS= read -r line; do printf "%s\\n" "$line"; [ "$line" = quit ] && exit 0; done'],'directory':root,'interactive':True}
  start=req('start',**scope,request_id='start',command=cmd,proof=first['proof']);eid=start['execution']['id']
  deadline=time.monotonic()+10
  while not (Path(root)/(eid+'.sock')).exists():
   assert time.monotonic()<deadline;time.sleep(.02)
  def send(proof,key,text,ok=True):return req('input',ok=ok,input={**scope,'execution':eid,'request_id':key,'proof':proof,'bytes':list(text.encode())})
  assert send(first['proof'],'one','one\n')['newly_delivered']
  assert not send(first['proof'],'one','one\n')['newly_delivered']
  second=req('acquire',**scope,holder='Air',ttl=30,takeover_epoch=first['proof']['epoch'])
  send(first['proof'],'stale','must-not-appear\n',False)
  req('renew',ok=False,**scope,proof=first['proof'],ttl=30)
  send(second['proof'],'two','two\n')
  server.kill();server.wait();server=serve()
  send(second['proof'],'three','three\n')
  req('renew',**scope,proof=second['proof'],ttl=1);time.sleep(1.1)
  send(second['proof'],'expired','must-not-appear\n',False)
  third=req('acquire',**scope,holder='Pro',ttl=30)
  assert third['proof']['epoch']>second['proof']['epoch'];send(third['proof'],'quit','quit\n')
  while req('status',**scope,execution=eid)['state']!='exited':
   assert time.monotonic()<deadline;time.sleep(.05)
  assert not send(third['proof'],'quit','quit\n')['newly_delivered']
  assert req('input_status',**scope,execution=eid,request_id='quit')['state']=='delivered'
  events=req('events',**scope,after=0,limit=100)
  data=b''.join(bytes(e['payload']['bytes']) for e in events if e['kind']=='execution.output' and e['payload']['stream']=='stdout')
  assert data==b'one\ntwo\nthree\nquit\n',data
  print('PASS control: single writer, takeover, stale/expired rejection, renew, input dedupe, node restart, byte-exact output')
 finally:
  for p in servers:
   if p.poll() is None:p.terminate()
   p.wait(timeout=5)
