import json,subprocess,tempfile,time,select
from pathlib import Path
binary=str(Path(__file__).resolve().parents[1]/'target/debug/runloom-node')
with tempfile.TemporaryDirectory(prefix='rl-cp-',dir='/tmp') as root:
 root=Path(root);repo=root/'repo';repo.mkdir();node=root/'node';node.mkdir(mode=0o700)
 def git(*args):return subprocess.run(['git','-c','core.hooksPath=/dev/null','-C',str(repo),*args],check=True,capture_output=True,text=True)
 git('init','-q');git('config','user.name','Fixture');git('config','user.email','fixture@localhost')
 (repo/'a').write_text('base');(repo/'.env').write_text('EXCLUDED_SECRET');git('add','.');git('commit','-qm','base')
 (repo/'a').write_text('staged');git('add','a');(repo/'a').write_text('working');(repo/'new').write_text('new')
 server=subprocess.Popen([binary,'--root',str(node),'serve','--host','fixture'],stdout=subprocess.PIPE,text=True)
 try:
  assert select.select([server.stdout],[],[],10)[0];assert json.loads(server.stdout.readline())['ready']
  def req(op,ok=True,**fields):
   p=subprocess.run([binary,'--root',str(node),'request'],input=json.dumps({'version':1,'op':op,**fields})+'\n',capture_output=True,text=True,timeout=15);r=json.loads(p.stdout);assert r['ok']==ok,r;return r.get('result',r.get('error'))
  w=req('register',work={'workspace':'p','host':'fixture','provider':'fixture','native_id':'one','config_root':'/fixture','directory':str(repo),'title':'cp'});scope={'workspace':'p','work':w['id']}
  grant=req('acquire',**scope,holder='Pro',ttl=30)
  first=req('checkpoint_capture',**scope,proof=grant['proof'],untracked=['new'])['checkpoint']
  (repo/'a').write_text('second')
  second=req('checkpoint_capture',**scope,proof=grant['proof'],untracked=['new'],parent=first)['checkpoint']
  assert len(req('checkpoint_list',**scope))==2
  assert req('checkpoint_inspect',**scope,checkpoint=second)['parent']==first
  dest=root/'restored';req('checkpoint_restore',**scope,proof=grant['proof'],checkpoint=first,destination=str(dest))
  assert (dest/'a').read_text()=='working';assert (dest/'new').read_text()=='new';assert not (dest/'.env').exists()
  req('checkpoint_restore',ok=False,**scope,proof=grant['proof'],checkpoint=first,destination=str(dest))
  req('checkpoint_inspect',ok=False,workspace='other',work=w['id'],checkpoint=first)
  req('release',**scope,proof=grant['proof']);req('checkpoint_capture',ok=False,**scope,proof=grant['proof'])
  print('PASS checkpoint RPC: fenced capture, lineage/list, staged/working restore, exclusion, no overwrite, wrong-workspace/stale lease rejection')
 finally:server.terminate();server.wait(timeout=5)
