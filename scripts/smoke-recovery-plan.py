"""Disposable recovery evidence fixture; no providers and no production roots."""
import json
from pathlib import Path
import select
import sqlite3
import subprocess
import tempfile

repo = Path(__file__).resolve().parents[1]
binary = str(repo/'target/debug/runloom-node')
with tempfile.TemporaryDirectory(prefix='rl-recovery-', dir='/tmp') as folder:
    server = subprocess.Popen([binary,'--root',folder,'serve','--host','fixture'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    def request(op, ok=True, **fields):
        p = subprocess.run([binary,'--root',folder,'request'],input=json.dumps({'version':1,'op':op,**fields})+'\n',text=True,capture_output=True,timeout=15)
        r = json.loads(p.stdout)
        assert r['ok']==ok, r
        return r.get('result')
    try:
        assert select.select([server.stdout],[],[],10)[0]
        assert json.loads(server.stdout.readline())['ready']
        assert request('ping')['recovery_plan']
        w=request('register',work={'workspace':'fixture','host':'fixture','provider':'fixture','config_root':'/fixture','native_id':'fixture','directory':folder,'title':'fixture'})
        scope={'workspace':'fixture','work':w['id']}
        assert request('recovery_plan',**scope)['disposition']=='no_recorded_blockers'
        with sqlite3.connect(Path(folder)/'state.db') as db:
            db.execute("INSERT INTO checkpoint_reservations VALUES ('fence',?,'/fixture/destination',1,1)",(w['id'],))
            db.execute("INSERT INTO executions(id,work_id,request_id,command_json,state) VALUES ('exec',?,'request','SECRET_COMMAND','running')",(w['id'],))
            db.execute("INSERT INTO input_receipts VALUES ('exec','input',?,'uncertain')",(b'SECRET_INPUT',))
        plan=request('recovery_plan',**scope)
        assert plan['disposition']=='manual_investigation_required' and not plan['apply_supported']
        assert not plan['evidence']['process_liveness_checked']
        assert 'SECRET' not in json.dumps(plan)
        request('recovery_plan',ok=False,workspace='other',work=w['id'])
        request('recovery_apply',ok=False,**scope)
        cli=subprocess.run(['python3',str(repo/'scripts/recovery-plan.py'),'--node',binary,'--root',folder,'--workspace','fixture','--work',w['id']],text=True,capture_output=True,timeout=20)
        assert cli.returncode==0 and 'reconcile_uncertain_input' in cli.stdout,cli.stderr
        assert 'SECRET' not in cli.stdout
        with sqlite3.connect(Path(folder)/'state.db') as db:
            assert db.execute('SELECT count(*) FROM checkpoint_reservations').fetchone()[0]==1
            assert db.execute('SELECT state FROM input_receipts').fetchone()[0]=='uncertain'
            assert db.execute('SELECT state FROM executions').fetchone()[0]=='running'
        print('PASS recovery plan: scoped advisory decisions, no secrets/replay/unlock, CLI rendering, unsupported apply rejected')
    finally:
        server.terminate();server.wait(timeout=10)
