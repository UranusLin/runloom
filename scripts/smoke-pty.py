import json,subprocess,tempfile,time,select
from pathlib import Path
binary=str(Path(__file__).resolve().parents[1]/'target/debug/runloom-node')
with tempfile.TemporaryDirectory(prefix='rl-pty-',dir='/tmp') as root:
 p=subprocess.Popen([binary,'--root',root,'serve','--host','fixture','--allow-interactive'],stdout=subprocess.PIPE,text=True)
 try:
  assert select.select([p.stdout],[],[],10)[0];assert json.loads(p.stdout.readline())['ready']
  def req(op,ok=True,**fields):
   r=subprocess.run([binary,'--root',root,'request'],input=json.dumps({'version':1,'op':op,**fields})+'\n',text=True,capture_output=True,timeout=10);v=json.loads(r.stdout);assert v['ok']==ok,v;return v.get('result',v.get('error'))
  work=req('register',work={'workspace':'p','host':'fixture','provider':'shell','native_id':'pty','config_root':'/fixture','directory':root,'title':'pty'})
  scope={'workspace':'p','work':work['id']};grant=req('acquire',**scope,holder='Pro',ttl=30)
  cmd={'executable':'/bin/sh','args':['-c','stty -echo; test -t 0 || exit 12; printf READY; read x; stty size; printf "got:%s" "$x"'],'directory':root,'interactive':True,'tty':True}
  eid=req('start',**scope,request_id='start',command=cmd,proof=grant['proof'])['execution']['id']
  def output():return b''.join(bytes(e['payload']['bytes']) for e in req('events',**scope,after=0,limit=100) if e['kind']=='execution.output')
  deadline=time.monotonic()+10
  while b'READY' not in output():
   assert time.monotonic()<deadline;time.sleep(.05)
  resize={**scope,'execution':eid,'proof':grant['proof'],'cols':93,'rows':27}
  req('resize',resize=resize)
  req('resize',ok=False,resize={**resize,'cols':0})
  second=req('acquire',**scope,holder='Air',ttl=30,takeover_epoch=grant['status']['epoch'])
  req('resize',ok=False,resize=resize)
  req('input',ok=False,input={**scope,'execution':eid,'request_id':'stale','proof':grant['proof'],'bytes':[10]})
  grant=second
  req('resize',resize={**resize,'proof':grant['proof']})
  req('input',input={**scope,'execution':eid,'request_id':'input','proof':grant['proof'],'bytes':list(b'PTY_MARKER\n')})
  while req('status',**scope,execution=eid)['state']!='exited':
   assert time.monotonic()<deadline;time.sleep(.05)
  assert b'got:PTY_MARKER' in output() and b'27 93' in output(),output()
  print('PASS PTY: real controlling terminal, fenced input/resize, stale owner rejected, output replay, clean exit')
 finally:p.terminate();p.wait(timeout=5)
