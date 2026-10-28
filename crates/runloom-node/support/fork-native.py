"""Official CLI adapters. Input/output are private worker pipes, never credentials.
Codex uses experimental app-server fork-by-path; Claude uses documented transcript
resume + fork-session. No hand-rewriting of transcript identities or summary fallback.
"""
import datetime, json, os, pathlib, selectors, subprocess, sys, time, uuid

def materialize_codex(records, folder, cutoff=None, seen=None):
    seen = set() if seen is None else seen
    metadata = records[0].get('payload', {})
    identity = metadata.get('id')
    if identity in seen or len(seen) >= 32: raise RuntimeError('native dependency cycle/depth limit')
    seen.add(identity)
    inherited = []
    base = metadata.get('history_base')
    if base:
        parent_id = str(uuid.UUID(base['thread_id']))
        limit = base['end_ordinal_exclusive']
        if not isinstance(limit, int) or limit < 0: raise RuntimeError('invalid native history boundary')
        if cutoff is not None: limit = min(limit, cutoff)
        dependency = folder / (parent_id + '.jsonl')
        parent = [json.loads(line) for line in dependency.read_bytes().splitlines() if line.strip()]
        if parent[0].get('payload', {}).get('id') != parent_id: raise RuntimeError('native dependency identity mismatch')
        inherited = materialize_codex(parent, folder, limit, seen)
    own = []
    for row in records:
        if row.get('type') == 'session_meta': continue
        if cutoff is not None:
            ordinal = row.get('ordinal')
            if not isinstance(ordinal, int): raise RuntimeError('dependency lacks ordinal boundary')
            if ordinal >= cutoff: continue
        own.append(row)
    return inherited + own

def main():
    request = json.load(sys.stdin)
    work = request['source']
    root = pathlib.Path(work['config_root']).resolve(strict=True)
    snapshot = pathlib.Path(request['destination']) / '.runloom/session.jsonl'
    rows = [json.loads(line) for line in snapshot.read_bytes().splitlines() if line.strip()]
    if work['provider'] == 'codex':
        metas = [row.get('payload', {}) for row in rows if row.get('type') == 'session_meta']
        if len(metas) != 1 or metas[0].get('id') != work['native_id']:
            raise RuntimeError('Codex snapshot identity mismatch')
    elif work['provider'] == 'claude-code':
        messages = [row for row in rows if row.get('type') in ('user', 'assistant')]
        if not messages or any(row.get('sessionId') != work['native_id'] for row in messages):
            raise RuntimeError('Claude snapshot identity mismatch')
    env = dict(os.environ)
    child_id = str(uuid.uuid4())
    # Codex requires the imported rollout to live under its own config root.
    # Fresh private directory, no overwrite or writes to the parent's transcript.
    imported = root / 'runloom-imports' / request['request_id']
    imported.mkdir(mode=0o700, parents=True, exist_ok=False)
    stamp = datetime.datetime.now(datetime.timezone.utc)
    session_dir = imported / 'sessions' / stamp.strftime('%Y/%m/%d')
    session_dir.mkdir(mode=0o700, parents=True)
    transcript = session_dir / ('rollout-' + stamp.strftime('%Y-%m-%dT%H-%M-%S-') + work['native_id'] + '.jsonl')
    if work['provider'] == 'codex':
        version = subprocess.run([request['executable'], '--version'], capture_output=True, text=True, timeout=10)
        if version.returncode or version.stdout.strip() != 'codex-cli 0.164.0':
            raise RuntimeError('Codex checkpoint import validated for 0.164.0 only; adapter upgrade required')
        # 0.164 reference-backed paginated forks otherwise depend on the original
        # rollout. Import a self-contained legacy rollout through the OFFICIAL fork
        # reader, which materializes inherited records instead of a history_base.
        # Message/tool payloads and IDs are retained verbatim; source files untouched.
        rows = [rows[0]] + materialize_codex(rows, snapshot.parent / 'native-dependencies')
        for row in rows:
            row.pop('ordinal', None)
            if row.get('type') == 'session_meta':
                row['payload']['history_mode'] = 'legacy'
                row['payload'].pop('history_base', None)
                row['payload'].pop('forked_from_ordinal_exclusive', None)
        transcript.write_text(''.join(json.dumps(row, ensure_ascii=False) + '\n' for row in rows))
    else:
        transcript.write_bytes(snapshot.read_bytes())
    transcript.chmod(0o600)
    if work['provider'] == 'codex':
        env['CODEX_HOME'] = str(imported)
        process = subprocess.Popen([request['executable'], 'app-server'], cwd=request['destination'], env=env,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)
        buffer = bytearray()
        def rpc(number, method, params):
            process.stdin.write((json.dumps({'id':number,'method':method,'params':params}) + '\n').encode())
            process.stdin.flush()
            deadline = time.monotonic() + 45
            while time.monotonic() < deadline:
                if b'\n' not in buffer:
                    if not selector.select(max(0, deadline-time.monotonic())): break
                    chunk = os.read(process.stdout.fileno(), 65536)
                    if not chunk: raise RuntimeError('app-server closed before fork confirmation')
                    buffer.extend(chunk)
                    if len(buffer) > 8*1024*1024: raise RuntimeError('app-server response exceeds limit')
                    continue
                end = buffer.index(b'\n')
                line = bytes(buffer[:end]); del buffer[:end+1]
                value = json.loads(line)
                if value.get('id') == number:
                    if 'error' in value: raise RuntimeError(str(value['error']))
                    return value['result']
            raise RuntimeError('app-server fork outcome uncertain (timeout); do not retry automatically')
        try:
            rpc(1, 'initialize', {'clientInfo':{'name':'runloom','version':'0.1.0'},
                                 'capabilities':{'experimentalApi':True}})
            process.stdin.write(b'{"method":"initialized"}\n'); process.stdin.flush()
            result = rpc(2, 'thread/fork', {'threadId':work['native_id'], 'path':str(transcript),
                          'excludeTurns':True, 'deferGoalContinuation':True, 'cwd':request['destination'], 'approvalPolicy':'on-request', 'sandbox':'workspace-write'})
            child_id = result['thread']['id']
            child_path = pathlib.Path(result['thread']['path'])
            child_meta = json.loads(child_path.read_bytes().splitlines()[0])['payload']
            if child_meta.get('history_base'): raise RuntimeError('fork still depends on parent history; not publishable')
            relative = child_path.resolve().relative_to((imported / 'sessions').resolve())
            target = root / 'sessions' / relative
            target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            with target.open('xb') as output:
                output.write(child_path.read_bytes()); output.flush(); os.fsync(output.fileno())
            target.chmod(0o600)
            if child_id == work['native_id']: raise RuntimeError('provider reused source identity')
        finally:
            process.terminate()
            try: process.wait(timeout=5)
            except subprocess.TimeoutExpired: process.kill(); process.wait()
            selector.close()
    elif work['provider'] == 'claude-code':
        default = pathlib.Path.home() / '.claude'
        if root != default: env['CLAUDE_CONFIG_DIR'] = str(root)
        else: env.pop('CLAUDE_CONFIG_DIR', None)
        result = subprocess.run([request['executable'], '--permission-mode', 'manual',
            '--resume', str(transcript), '--fork-session', '--session-id', child_id,
            '--print', '--output-format', 'json', '--tools', '', '--',
            'This is a Runloom checkpoint branch in a new working directory. Preserve the restored conversation. '
            'Do not call tools or change files. Reply only: RUNLOOM_BRANCH_READY.'],
            cwd=request['destination'], env=env, capture_output=True, text=True, timeout=150)
        if result.returncode: raise RuntimeError('Claude fork rejected: ' + result.stderr[-1500:])
        value = json.loads(result.stdout)
        if value.get('is_error') or value.get('session_id') != child_id:
            raise RuntimeError('Claude did not confirm the requested new session identity')
        # The CLI must actually have persisted the child; JSON output alone is insufficient.
        matches = list((root/'projects').glob('*/' + child_id + '.jsonl'))
        if len(matches) != 1 or not matches[0].stat().st_size:
            raise RuntimeError('Claude fork transcript was not persisted')
    else:
        raise RuntimeError('unsupported native checkpoint provider')
    uuid.UUID(child_id)
    print(json.dumps({'native_id':child_id}))

if __name__ == '__main__':
    try: main()
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
