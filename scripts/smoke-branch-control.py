"""No inference: exercise fork admission, crash/reconnect, scope and ownership."""
import concurrent.futures, json, os, pathlib, select, subprocess, tempfile, time, uuid
root=pathlib.Path(tempfile.mkdtemp(prefix='rl-branch-control-',dir='/tmp')).resolve()
binary=str(pathlib.Path(__file__).resolve().parents[1]/'target/debug/runloom-node')
repo=root/'repo';repo.mkdir();profile=root/'profile';(profile/'sessions').mkdir(parents=True)
def run(cmd,**kw):return subprocess.run(cmd,check=True,capture_output=True,text=True,timeout=20,**kw)
for args in [('init','-q'),('config','user.name','Fixture'),('config','user.email','fixture@localhost')]:run(['git','-C',str(repo),*args])
(repo/'a').write_text('base');run(['git','-C',str(repo),'add','.']);run(['git','-C',str(repo),'commit','-qm','base'])
native=str(uuid.uuid4());source=profile/'sessions'/('rollout-2026-01-01T00-00-00-'+native+'.jsonl');source.write_text(json.dumps({'type':'session_meta','payload':{'id':native}})+'\n')
fake=root/'fake-codex';fake.write_text('''#!/usr/bin/python3
import json,os,pathlib,sys,time,uuid
if '--version' in sys.argv:
 print('codex-cli 0.164.0');sys.exit()
for line in sys.stdin:
 r=json.loads(line)
 if r.get('method')=='initialize': print(json.dumps({'id':r['id'],'result':{}}),flush=True)
 elif r.get('method')=='thread/fork':
  time.sleep(2)
  ident=str(uuid.uuid4());folder=pathlib.Path(os.environ['CODEX_HOME'])/'sessions'/'2026'/'01'/'01';folder.mkdir(parents=True,exist_ok=True)
  path=folder/('rollout-2026-01-01T00-00-00-'+ident+'.jsonl');path.write_text(json.dumps({'type':'session_meta','payload':{'id':ident}})+'\\n')
  print(json.dumps({'id':r['id'],'result':{'thread':{'id':ident,'path':str(path)}}}),flush=True)
''');fake.chmod(0o700)
node=root/'node';node.mkdir(mode=0o700)
def start():
 p=subprocess.Popen([binary,'--root',str(node),'serve','--host','fixture','--allow-interactive'],stdout=subprocess.PIPE,text=True)
 assert select.select([p.stdout],[],[],10)[0];assert json.loads(p.stdout.readline())['ready'];return p
def req(op,ok=True,**fields):
 p=subprocess.run([binary,'--root',str(node),'request'],input=json.dumps({'version':1,'op':op,**fields})+'\n',text=True,capture_output=True,timeout=20)
 r=json.loads(p.stdout);assert r['ok']==ok,r;return r.get('result',r)
server=start()
try:
 work=req('register',work={'workspace':'p','host':'fixture','provider':'codex','config_root':str(profile),'native_id':native,'directory':str(repo),'title':'fixture'})
 scope={'workspace':'p','work':work['id']};proof=req('acquire',**scope,holder='A',ttl=300)['proof']
 checkpoint=req('checkpoint_capture',**scope,proof=proof,auto_native=True)['checkpoint']
 rid=str(uuid.uuid4());args={**scope,'request_id':rid,'checkpoint':checkpoint,'destination':str(root/'branch'),'executable':str(fake)}
 stale={'epoch':proof['epoch'],'token':'invalid'}
 req('checkpoint_fork',ok=False,request=args,proof=stale);assert not (node/'branches'/f'{rid}.json').exists()
 req('checkpoint_fork',ok=False,request={**args,'destination':str(repo/'nested')},proof=proof)
 req('checkpoint_fork',request=args,proof=proof)
 # The async worker must fence the newly created destination until completion.
 deadline=time.monotonic()+10
 while not (root/'branch').is_dir():
  assert time.monotonic()<deadline,'branch destination was not created';time.sleep(.01)
 intruder=req('register',work={'workspace':'p','host':'fixture','provider':'fixture','config_root':str(profile),'native_id':'destination-writer','directory':str(root/'branch'),'title':'destination writer'})
 intruder_scope={'workspace':'p','work':intruder['id']}
 intruder_proof=req('acquire',**intruder_scope,holder='fixture',ttl=300)['proof']
 rejected=req('start',ok=False,**intruder_scope,proof=intruder_proof,request_id='during-branch',command={'executable':'/bin/sh','args':['-c','printf unexpected'],'directory':str(root/'branch'),'interactive':True})
 assert 'reserved by checkpoint' in rejected['error'],rejected
 req('checkpoint_fork',request=args,proof=proof)
 req('checkpoint_fork',ok=False,request={**args,'destination':str(root/'different')},proof=proof)
 req('branch_status',ok=False,workspace='wrong',work=work['id'],request_id=rid)
 server.kill();server.wait();server=start()
 end=time.monotonic()+20
 while True:
  job=req('branch_status',**scope,request_id=rid)
  if job['state'] in ('ready','needs_inspection'):break
  assert time.monotonic()<end,job;time.sleep(.1)
 assert job['state']=='ready',job
 assert req('checkpoint_fork',request=args,proof=proof)['result']['id']==job['result']['id']
 assert len(req('list',workspace='p'))==3
 assert len(req('branch_list',**scope))==1
 # A second stopped-worker record is intentionally unknown, never automatically launched.
 lost=str(uuid.uuid4());record={**job,'request_id':lost,'state':'creating','result':None}
 (node/'branches'/f'{lost}.json').write_text(json.dumps(record))
 assert req('branch_status',**scope,request_id=lost)['state']=='unknown'
 assert req('checkpoint_fork',request={**args,'request_id':lost},proof=proof)['state']=='creating'
 assert len(req('list',workspace='p'))==3
 # Conflicting UUIDs on unrelated work must never overwrite a durable job.
 repo2=root/'repo2'
 run(['git','clone','-q',str(repo),str(repo2)])
 native2=str(uuid.uuid4())
 (profile/'sessions'/('rollout-2026-01-01T00-00-00-'+native2+'.jsonl')).write_text(json.dumps({'type':'session_meta','payload':{'id':native2}})+'\n')
 work2=req('register',work={'workspace':'p','host':'fixture','provider':'codex','config_root':str(profile),'native_id':native2,'directory':str(repo2),'title':'second fixture'})
 scope2={'workspace':'p','work':work2['id']}
 proof2=req('acquire',**scope2,holder='B',ttl=300)['proof']
 checkpoint2=req('checkpoint_capture',**scope2,proof=proof2,auto_native=True)['checkpoint']
 shared=str(uuid.uuid4())
 requests=[({**args,'request_id':shared,'destination':str(root/'race-a')},proof),
           ({**args,**scope2,'checkpoint':checkpoint2,'request_id':shared,'destination':str(root/'race-b')},proof2)]
 def race(item):
  request,lease=item
  p=subprocess.run([binary,'--root',str(node),'request'],input=json.dumps({'version':1,'op':'checkpoint_fork','request':request,'proof':lease})+'\n',text=True,capture_output=True,timeout=20)
  return json.loads(p.stdout)
 with concurrent.futures.ThreadPoolExecutor(2) as pool:
  responses=list(pool.map(race,requests))
 assert sum(r['ok'] for r in responses)==1,responses
 winner=next(i for i,r in enumerate(responses) if r['ok'])
 assert 'request ID conflict' in responses[1-winner]['error'],responses
 chosen=requests[winner][0]
 end=time.monotonic()+20
 while True:
  race_job=req('branch_status',workspace='p',work=chosen['work'],request_id=shared)
  if race_job['state'] in ('ready','needs_inspection'):break
  assert time.monotonic()<end,race_job;time.sleep(.1)
 assert race_job['state']=='ready',race_job
 assert race_job['work']==chosen['work'] and race_job['destination']==chosen['destination']
 assert not pathlib.Path(requests[1-winner][0]['destination']).exists()
 assert len(req('list',workspace='p'))==5
 print(json.dumps({'result':'PASS','evidence':str(root),'dedupe':True,'stale_rejected':True,'restart_survives':True,'unknown_not_replayed':True}))
finally:server.terminate();server.wait(timeout=5)
