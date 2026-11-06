#!/usr/bin/env python3
"""Install a verified extracted bundle into a NEW user-owned prefix. No service changes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil

FILES = ('bin/runloom', 'bin/runloom-node', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'RELEASE.md', 'install.py')

def install(bundle, prefix):
    manifest = json.loads((bundle / 'BUILD.json').read_text())
    if set(manifest['sha256']) != set(FILES):
        raise ValueError('unexpected bundle manifest')
    for name in FILES:
        source = bundle / name
        if source.is_symlink() or not source.is_file() or source.resolve() != bundle.resolve() / name:
            raise ValueError('invalid bundle file: '+name)
        if hashlib.sha256(source.read_bytes()).hexdigest() != manifest['sha256'][name]:
            raise ValueError('checksum mismatch: '+name)
    if prefix.exists() or prefix.is_symlink():
        raise ValueError('prefix already exists; never overwrite an installed version')
    if not prefix.parent.is_dir() or prefix.parent.resolve() != prefix.parent:
        raise ValueError('parent must be an existing canonical directory')
    prefix.mkdir(mode=0o700)  # Exclusive reservation, no overwrite or merge.
    try:
        for name in (*FILES, 'BUILD.json'):
            target = prefix / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(bundle / name, target)
            target.chmod(0o700 if name.startswith('bin/') else 0o600)
    except BaseException:
        shutil.rmtree(prefix)
        raise
    return manifest

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prefix', type=Path, required=True)
    args = parser.parse_args()
    if os.geteuid() == 0:
        parser.error('run as your own user, without sudo')
    result = install(Path(__file__).resolve().parent, args.prefix.expanduser().absolute())
    print(json.dumps({'installed':str(args.prefix), 'version':result['version'], 'source_commit':result['source_commit'], 'services_changed':False}))

if __name__ == '__main__':
    main()
