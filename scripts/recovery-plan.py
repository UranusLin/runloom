#!/usr/bin/env python3
"""Read a work-scoped recovery plan. Never starts a service or applies repairs."""
import argparse
import json
import subprocess


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--node', default='runloom-node', help='Installed node executable')
    p.add_argument('--root', required=True)
    p.add_argument('--workspace', required=True)
    p.add_argument('--work', required=True)
    p.add_argument('--json', action='store_true')
    a = p.parse_args()
    def request(op, **fields):
        result = subprocess.run([a.node, '--root', a.root, 'request'],
            input=json.dumps({'version': 1, 'op': op, **fields})+'\n',
            text=True, capture_output=True, timeout=20)
        response = json.loads(result.stdout)
        if result.returncode or not response.get('ok'):
            raise RuntimeError('Node refused the read-only request; check host/root/work scope.')
        return response['result']
    try:
        if not request('ping').get('recovery_plan'):
            raise RuntimeError('This node does not support recovery_plan. Upgrade it first; no repair was attempted.')
        plan = request('recovery_plan', workspace=a.workspace, work=a.work)
        if a.json:
            print(json.dumps(plan, indent=2, ensure_ascii=False))
        else:
            print('Recovery plan — advisory only, no repair applied')
            print('Disposition:', plan['disposition'])
            for index, step in enumerate(plan['steps'], 1):
                print(f"{index}. [{step['code']}] {step['instruction']}")
            print('Not permitted:', ', '.join(plan['prohibited_actions']))
            print('Scope limits:', ', '.join(plan['limitations']))
            print('Use --json for scoped evidence IDs and paths (may contain private project information).')
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.TimeoutExpired) as error:
        p.exit(1, f'Recovery plan unavailable: {error}\n')


if __name__ == '__main__':
    main()
