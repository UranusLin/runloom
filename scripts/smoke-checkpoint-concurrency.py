"""Regression: slow checkpoint A must not break B; reservations survive crashes.
Only disposable local repositories, shell fixtures and processes are used.
"""
import concurrent.futures
import json
import os
from pathlib import Path
import select
import signal
import socket
import sqlite3
import subprocess
import tempfile
import time

binary = str(Path(__file__).resolve().parents[1] / 'target/debug/runloom-node')
with tempfile.TemporaryDirectory(prefix='rl-cp-concurrent-', dir='/tmp') as folder:
    root = Path(folder)
    node, source, active = root / 'node', root / 'source', root / 'active'
    source.mkdir(); active.mkdir()
    def git(*args):
        return subprocess.run(['/usr/bin/git', '-C', str(source), *args], check=True,
                              capture_output=True, text=True)
    git('init', '-q'); (source / 'file').write_text('fixture\n')
    git('add', 'file')
    git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@localhost', 'commit', '-qm', 'fixture')
    monitor = root / 'monitor'
    monitor.write_text('#!/bin/sh\nsleep 7\nprintf "token\\000"\n')
    monitor.chmod(0o700)
    git('config', 'core.fsmonitor', str(monitor))
    servers = []
    def serve():
        p = subprocess.Popen([binary, '--root', str(node), 'serve', '--host', 'fixture', '--allow-batch'],
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=True)
        servers.append(p)
        assert select.select([p.stdout], [], [], 10)[0], 'node did not start'
        assert json.loads(p.stdout.readline())['ready']
        return p
    def request(op, ok=True, **fields):
        with socket.socket(socket.AF_UNIX) as stream:
            stream.settimeout(45)
            stream.connect(str(node / 'node.sock'))
            stream.sendall((json.dumps({'version': 1, 'op': op, **fields}) + '\n').encode())
            with stream.makefile('rb') as reader:
                response = json.loads(reader.readline())
            assert response['ok'] == ok, response
            return response['result'] if ok else response['error']
    def register(directory, native):
        return request('register', work=dict(workspace='fixture', host='fixture', provider='fixture',
                       config_root=str(root), native_id=native, directory=str(directory), title=native))['id']
    def wait_reservation():
        deadline=time.monotonic()+5
        while time.monotonic()<deadline:
            with sqlite3.connect(node/'state.db', timeout=1) as db:
                if db.execute('SELECT count(*) FROM checkpoint_reservations').fetchone()[0]: return
            time.sleep(.02)
        raise AssertionError('checkpoint reservation was not persisted')
    def command(directory, script):
        return dict(executable='/bin/sh', args=['-c', script], directory=str(directory))
    pool = concurrent.futures.ThreadPoolExecutor(1)
    try:
        server=serve()
        a,b,alias=register(source,'source'),register(active,'active'),register(source,'same-directory')
        scope_a=dict(workspace='fixture',work=a); scope_b=dict(workspace='fixture',work=b)
        pa=request('acquire',**scope_a,holder='review-a',ttl=120)['proof']
        pb=request('acquire',**scope_b,holder='review-b',ttl=3)['proof']
        script='i=0; while [ "$i" -lt 80 ]; do printf "tick\\n"; sleep .25; i=$((i+1)); done'
        execution=request('start',**scope_b,request_id='writer',command=command(active,script))['execution']['id']
        time.sleep(.5)
        started=time.monotonic()
        job=pool.submit(request,'checkpoint_capture',**scope_a,proof=pa,untracked=[],auto_native=False)
        wait_reservation()
        blocked=request('start',ok=False,workspace='fixture',work=alias,request_id='overlap',command=command(source,'true'))
        assert 'reserved by checkpoint' in blocked,blocked
        polls=0;worst=0
        while not job.done():
            before=time.monotonic()
            request('renew',**scope_b,proof=pb,ttl=3)
            state=request('status',**scope_b,execution=execution)['state']
            assert state=='running',state
            worst=max(worst,time.monotonic()-before);polls+=1;time.sleep(.2)
        assert job.result()['checkpoint']
        assert time.monotonic()-started>12,'fixture did not exercise slow I/O'
        assert worst<2,f'control plane blocked {worst}s'
        assert polls>20,'insufficient concurrent lease/status observations'
        deadline=time.monotonic()+15
        while request('status',**scope_b,execution=execution)['state']!='exited':
            assert time.monotonic()<deadline,'writer did not exit'
            time.sleep(.1)
        output=b'';cursor=0
        while True:
            events=request('events',**scope_b,after=cursor,limit=100)
            if not events: break
            for event in events:
                if event['kind']=='execution.output': output+=bytes(event['payload']['bytes'])
            cursor=events[-1]['sequence']
        assert output==b'tick\n'*80,(len(output),output)
        with sqlite3.connect(node/'state.db') as db:
            assert db.execute('SELECT count(*) FROM checkpoint_reservations').fetchone()[0]==0
        # A normal operation error releases the reservation without permitting
        # retry of process effects. (Capture has no child agent side effects.)
        request('checkpoint_capture',ok=False,**scope_a,proof=pa,untracked=['.env'],auto_native=False)
        with sqlite3.connect(node/'state.db') as db:
            assert db.execute('SELECT count(*) FROM checkpoint_reservations').fetchone()[0]==0
        # Kill only this disposable server/process group while its Git scan runs.
        crash=pool.submit(request,'checkpoint_capture',**scope_a,proof=pa,untracked=[],auto_native=False)
        wait_reservation();os.killpg(server.pid,signal.SIGKILL);server.wait(timeout=5)
        try: crash.result(timeout=5)
        except (ValueError,ConnectionError,OSError): pass
        else: raise AssertionError('crashed checkpoint unexpectedly returned')
        server=serve()
        blocked=request('start',ok=False,workspace='fixture',work=alias,request_id='after-crash',command=command(source,'true'))
        assert 'reserved by checkpoint' in blocked,blocked
        request('checkpoint_capture',ok=False,**scope_a,proof=pa,untracked=[],auto_native=False)
        other=request('start',**scope_b,request_id='unrelated-after-crash',command=command(active,'printf recovered'))['execution']['id']
        deadline=time.monotonic()+5
        while request('status',**scope_b,execution=other)['state']!='exited':
            assert time.monotonic()<deadline
            time.sleep(.05)
        print(f'PASS slow checkpoint: {polls} lease/status polls, max latency {worst:.3f}s, 80/80 output ticks; overlap fencing, normal cleanup and crash/restart reservation retained')
    finally:
        for p in servers:
            if p.poll() is None:
                os.killpg(p.pid,signal.SIGTERM)
            p.wait(timeout=5)
        pool.shutdown(wait=True)
