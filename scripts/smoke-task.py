"""Disposable task intake tests. --live invokes Claude with explicitly selected dangerous mode."""
import argparse,json,os,pathlib,select,subprocess,tempfile,time,uuid
p=argparse.ArgumentParser();p.add_argument('--live',action='store_true');a=p.parse_args()
root=pathlib.Path(tempfile.mkdtemp(prefix='rl-task-',dir='/tmp')).resolve();node=root/'node';node.mkdir(mode=0o700)
binary=os.environ.get('RUNLOOM_NODE_BINARY',str(pathlib.Path(__file__).resolve().parents[1]/'target/debug/runloom-node'))
profile=pathlib.Path.home()/'.claude' if a.live else root/'profile';profile.mkdir(exist_ok=True)
cli=str(pathlib.Path.home()/'.local/bin/claude') if a.live else str(root/'fake-claude')
if not a.live:
 pathlib.Path(cli).write_text('#!/bin/sh\nprintf TASK_LAUNCHED\nexit 0\n');pathlib.Path(cli).chmod(0o700)
def start():
 p=subprocess.Popen([binary,'--root',str(node),'serve','--host','task-fixture','--allow-interactive'],stdout=subprocess.PIPE,text=True)
 assert select.select([p.stdout],[],[],10)[0];assert json.loads(p.stdout.readline())['ready'];return p
def req(op,ok=True,**fields):
 out=subprocess.run([binary,'--root',str(node),'request'],input=json.dumps({'version':1,'op':op,**fields})+'\n',text=True,capture_output=True,timeout=20)
 r=json.loads(out.stdout);assert r['ok']==ok,r;return r.get('result',r)
server=start();rid=str(uuid.uuid4());marker='TASK_'+uuid.uuid4().hex;repo=root/'project'
spec={'workspace':'personal','request_id':rid,'directory':str(repo),'title':'Disposable task fixture','design':'Create answer.txt in the current directory containing exactly '+marker+'. No other file changes, no external operations. Do the work now.','acceptance':'Read answer.txt to verify its exact content and run a shell comparison against '+marker+'. Report TASK_VERIFIED only after the comparison passes.','permission':'dangerous','create_directory':True,'executable':cli,'config_root':str(profile)}
try:
 req('task_create',ok=False,spec=spec,dangerous_ack=False);assert not repo.exists()
 work=req('task_create',spec=spec,dangerous_ack=True);scope={'workspace':'personal','work':work['id']}
 assert req('task_create',spec=spec,dangerous_ack=True)['id']==work['id']
 req('task_create',ok=False,spec={**spec,'design':'different'},dangerous_ack=True)
 assert len(req('list',workspace='personal'))==1
 assert req('task_info',**scope)==spec
 req('task_info',ok=False,workspace='other',work=work['id'])
 proof=req('acquire',**scope,holder='fixture',ttl=300)['proof']
 req('task_start',ok=False,**scope,proof={'epoch':proof['epoch'],'token':'wrong'})
 first=req('task_start',**scope,proof=proof);execution=first['execution']['id']
 assert req('task_start',**scope,proof=proof)['execution']['id']==execution
 plan=req('resume_plan',**scope,executable=cli,headless=False)
 assert '--dangerously-skip-permissions' in plan['args'] and '--resume' in plan['args']
 # Client/server disconnect does not own the supervisor lifetime.
 server.kill();server.wait();server=start()
 text='';cursor=0;accepted=False;trusted=False;deadline=time.monotonic()+180
 while time.monotonic()<deadline:
  events=req('events',**scope,after=cursor,limit=100)
  for event in events:
   cursor=event['sequence']
   if event['kind']=='execution.output':text+=bytes(event['payload']['bytes']).decode(errors='replace')
  (root/'terminal.txt').write_text(text)
  if a.live:
   # Explicit danger acknowledgement in this disposable test authorizes only its
   # normal first-run CLI confirmation, not arbitrary terminal questions.
   clean=__import__('re').sub(r'\x1b\[[0-?]*[ -/]*[@-~]','',text)
   if not trusted and 'Yes, I trust this folder' in clean:
    req('input',input={**scope,'execution':execution,'proof':proof,'request_id':str(uuid.uuid4()),'bytes':list(b'\r')});trusted=True
   if not accepted and 'Yes, I accept' in clean:
    req('input',input={**scope,'execution':execution,'proof':proof,'request_id':str(uuid.uuid4()),'bytes':list(b'\x1b[B\r')});accepted=True
   if (repo/'answer.txt').exists() and (repo/'answer.txt').read_text().strip()==marker and any(row.get('type')=='result' and not row.get('is_error') and 'TASK_VERIFIED' in row.get('result','') for line in clean.splitlines() if line.startswith('{') for row in [json.loads(line)]):
    break
  elif 'TASK_LAUNCHED' in text:break
  time.sleep(.4)
 else:raise AssertionError('task did not reach acceptance; inspect '+str(root))
 # Independent verification, not just the assistant's completion claim.
 if a.live:assert (repo/'answer.txt').read_text().strip()==marker
 print(json.dumps({'result':'PASS','live':a.live,'evidence':str(root),'work':work['id'],'session':rid,'execution':execution,'server_restart_survived':True}))
finally:
 (root/'fixture.json').write_text(json.dumps({'root':str(root),'work':work,'proof':proof,'execution':execution}) if 'execution' in globals() else '{}')
 server.terminate();server.wait(timeout=5)
