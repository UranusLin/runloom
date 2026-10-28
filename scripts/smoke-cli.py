"""Run after cargo build; uses disposable metadata and no real provider session."""
import subprocess
import tempfile
import json
from pathlib import Path

binary = str(Path(__file__).resolve().parents[1] / 'target/debug/runloom')
with tempfile.TemporaryDirectory(prefix='runloom-smoke-') as directory:
    def call(*args, ok=True):
        result = subprocess.run([binary, '--db', directory + '/state.db', *args], capture_output=True, text=True)
        assert (result.returncode == 0) == ok, (result.returncode, result.stderr)
        return json.loads(result.stdout) if ok else result.stderr

    assert call('init')['schema'] == 4
    work = call('register', '--workspace', 'personal', '--host', 'fixture', '--provider', 'fixture-agent', '--config-root', '/fixture/profile', '--native-id', 'native-1', '--directory', '/fixture/project', '--title', 'Smoke')
    assert work['state'] == 'registered'
    assert len(call('list', '--workspace', 'personal')) == 1
    assert call('list', '--workspace', 'champ') == []
    args = ['note', '--workspace', 'personal', '--work', work['id'], '--request-id', 'note-1', '--text', 'hello']
    assert call(*args) == call(*args)
    assert 'different event' in call(*args[:-1], 'changed', ok=False)
    assert len(call('events', '--workspace', 'personal', '--work', work['id'])) == 2
    assert len(call('events', '--workspace', 'personal', '--work', work['id'], '--after', '1')) == 1
    call('events', '--workspace', 'champ', '--work', work['id'], ok=False)
    print('CLI smoke passed: persistence, workspace scope, deduplication, conflict, cursor')
