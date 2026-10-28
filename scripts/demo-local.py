"""Run a disposable local handoff demonstration. No AI, credentials, SSH or sudo."""
import argparse
import json
from pathlib import Path
import select
import subprocess
import sys
import tempfile
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, default=Path(__file__).resolve().parents[1] / 'target/debug/runloom-node')
args = parser.parse_args()
binary = str(args.binary.resolve(strict=True))
with tempfile.TemporaryDirectory(prefix='runloom-demo-', dir='/tmp') as temp:
    root = Path(temp)
    node = root / 'node'
    project = root / 'project'
    project.mkdir()
    subprocess.run(['git', 'init', '-q', str(project)], check=True)
    (project / 'hello.txt').write_text('checkpoint baseline\n')
    subprocess.run(['git', '-C', str(project), 'add', 'hello.txt'], check=True)
    subprocess.run(['git', '-C', str(project), '-c', 'user.name=Runloom Demo', '-c', 'user.email=demo@example.invalid', 'commit', '-qm', 'demo baseline'], check=True)
    server = subprocess.Popen([binary, '--root', str(node), 'serve', '--host', 'local-demo', '--allow-interactive'], stdout=subprocess.PIPE, text=True)
    def request(op, expect=True, **fields):
        result = subprocess.run([binary, '--root', str(node), 'request'], input=json.dumps({'version': 1, 'op': op, **fields})+'\n', text=True, capture_output=True, timeout=15)
        reply = json.loads(result.stdout)
        if reply['ok'] != expect:
            raise RuntimeError(reply.get('error', 'Unexpected acceptance'))
        return reply.get('result', reply.get('error'))
    execution = None
    try:
        if not select.select([server.stdout], [], [], 10)[0] or not json.loads(server.stdout.readline())['ready']:
            raise RuntimeError('Demo node did not start')
        work = request('register', work={'workspace': 'demo', 'host': 'local-demo', 'provider': 'fixture', 'config_root': str(root), 'native_id': 'demo-session', 'directory': str(project), 'title': 'Local handoff demonstration'})
        scope = {'workspace': 'demo', 'work': work['id']}
        first = request('acquire', **scope, holder='laptop-one', ttl=60)
        print('1. Laptop one owns a registered work.', flush=True)
        program = "import select,sys; print('READY',flush=True); ready=select.select([sys.stdin],[],[],10)[0]; print('RECEIVED:'+sys.stdin.readline().strip() if ready else 'TIMEOUT',flush=True)"
        execution = request('start', **scope, request_id='demo-start', proof=first['proof'], command={'executable': sys.executable, 'args': ['-u', '-c', program], 'directory': str(project), 'interactive': True})['execution']['id']
        deadline = time.monotonic()+15
        def output():
            events = request('events', **scope, after=0, limit=100)
            return b''.join(bytes(e['payload']['bytes']) for e in events if e['kind']=='execution.output')
        while b'READY' not in output():
            if time.monotonic()>deadline: raise RuntimeError('Child startup timeout')
            time.sleep(.05)
        print('2. A real fixture process is running independently of the request client.', flush=True)
        second = request('acquire', **scope, holder='laptop-two', ttl=60, takeover_epoch=first['proof']['epoch'])
        def send(proof, key, text, expect=True):
            return request('input', expect=expect, input={**scope, 'execution': execution, 'request_id': key, 'proof': proof, 'bytes': list(text.encode())})
        send(first['proof'], 'old-owner', 'MUST_NOT_RUN\n', False)
        print('3. Laptop two takes over; the old controller is rejected.', flush=True)
        send(second['proof'], 'continue', 'CONTINUED\n')
        while request('status', **scope, execution=execution)['state'] != 'exited':
            if time.monotonic()>deadline: raise RuntimeError('Child exit timeout')
            time.sleep(.05)
        data = output()
        if b'RECEIVED:CONTINUED' not in data or b'MUST_NOT_RUN' in data: raise RuntimeError('Handoff validation failed')
        print('4. The same execution receives the new controller input; output replays correctly.', flush=True)
        (project / 'hello.txt').write_text('saved working change\n')
        capture = request('checkpoint_capture', **scope, proof=second['proof'], untracked=[], auto_native=False)
        destination = root / 'restored'
        request('checkpoint_restore', **scope, proof=second['proof'], checkpoint=capture['checkpoint'], destination=str(destination))
        if (destination / 'hello.txt').read_text() != 'saved working change\n': raise RuntimeError('Restore validation failed')
        print('5. Checkpoint restores the exact working file into a new directory.', flush=True)
        print('PASS — local simulation only; not physical-device, AI inference or reboot acceptance.')
    finally:
        # The fixture child has a ten-second self timeout even on failed demo steps.
        if execution:
            end=time.monotonic()+12
            while time.monotonic()<end:
                try:
                    if request('status', **scope, execution=execution)['state'] in ['exited','failed']: break
                except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired): break
                time.sleep(.1)
        server.terminate()
        server.wait(timeout=5)
