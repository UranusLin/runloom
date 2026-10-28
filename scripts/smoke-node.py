"""Local real-process node acceptance. Uses disposable roots and shell fixtures only."""
import concurrent.futures
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import tempfile
import time

binary = str(Path(__file__).resolve().parents[1] / 'target/debug/runloom-node')
with tempfile.TemporaryDirectory(prefix='rl-', dir='/tmp') as folder:
    root = Path(folder)
    servers = []
    def serve(batch=True):
        args = [binary, '--root', folder, 'serve', '--host', 'fixture']
        if batch: args.append('--allow-batch')
        p = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        servers.append(p)
        assert select.select([p.stdout], [], [], 10)[0], 'server readiness timeout'
        line=p.stdout.readline()
        assert line and json.loads(line)['ready'], (line,p.stderr.read())
        return p
    def request(op, ok=True, version=1, **fields):
        data = json.dumps({'version':version,'op':op,**fields})+'\n'
        p = subprocess.run([binary,'--root',folder,'request'],input=data,text=True,capture_output=True,timeout=15)
        r = json.loads(p.stdout)
        assert r['ok']==ok, r
        assert (p.returncode==0)==ok, p.stderr
        return r['result'] if ok else r['error']
    try:
        server=serve(False)
        assert not request('ping')['batch_enabled']
        work=request('register',work={'workspace':'personal','host':'fixture','provider':'fixture-shell','config_root':'/fixture/profile','native_id':'smoke','directory':folder,'title':'fixture'})
        scope={'workspace':'personal','work':work['id']}
        cmd={'executable':'/bin/sh','args':['-c','echo once >> counter; printf begin; sleep 2; printf done; printf err >&2; exit 7'],'directory':folder}
        request('start',ok=False,**scope,request_id='start-1',command=cmd)
        request('ping',ok=False,version=99)
        server.terminate();server.wait(timeout=5)
        server=serve()
        def start(_):return request('start',**scope,request_id='start-1',command=cmd)
        with concurrent.futures.ThreadPoolExecutor(2) as pool: admissions=list(pool.map(start,range(2)))
        assert sum(a['newly_admitted'] for a in admissions)==1
        execution=admissions[0]['execution']['id']
        assert all(a['execution']['id']==execution for a in admissions)
        # Kill the actual service process while independent supervisor continues.
        server.kill();server.wait(timeout=5);server=serve()
        deadline=time.monotonic()+15
        while True:
            status=request('status',**scope,execution=execution)
            if status['state']=='exited':break
            assert time.monotonic()<deadline,status
            time.sleep(.1)
        assert status['exit_code']==7,status
        assert (root/'counter').read_text()=='once\n'
        assert not start(0)['newly_admitted']
        request('start',ok=False,**scope,request_id='start-1',command={**cmd,'args':['-c','echo changed']})
        request('status',ok=False,workspace='champ',work=work['id'],execution=execution)
        events=request('events',**scope,after=0,limit=100)
        output={'stdout':b'','stderr':b''}
        for e in events:
            if e['kind']=='execution.output':output[e['payload']['stream']]+=bytes(e['payload']['bytes'])
        assert output=={'stdout':b'begindone','stderr':b'err'},output
        assert request('events',**scope,after=events[-1]['sequence'],limit=100)==[]
        # Simulate an admission that committed just before the daemon crashed.
        import sqlite3
        with sqlite3.connect(root/'state.db') as db:
            db.execute("INSERT INTO executions(id,work_id,request_id,command_json,state) VALUES (?,?,?,?,?)",('00000000-0000-0000-0000-000000000000',work['id'],'uncertain',json.dumps(cmd),'queued'))
        uncertain=request('status',**scope,execution='00000000-0000-0000-0000-000000000000')
        assert uncertain['state']=='unknown'
        request('start',ok=False,**scope,request_id='must-not-restart',command=cmd)
        assert (root/'counter').read_text()=='once\n'
        print('PASS node: default disabled, version check, concurrent admission dedupe, client exit, service SIGKILL/restart, exit status, byte-exact output replay, unknown never auto-restarts')
    finally:
        for p in servers:
            if p.poll() is None:p.terminate()
            p.wait(timeout=5)
