#!/usr/bin/env python3
"""Build a clean commit into a versioned Apple Silicon preview bundle."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]
TARGET = 'aarch64-apple-darwin'

def output(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tag', help='When releasing, require this tag to match version and HEAD')
    args = parser.parse_args()
    if platform.system() != 'Darwin' or platform.machine() != 'arm64':
        parser.error('first binary preview builds and validates on native macOS arm64 only')
    metadata = json.loads(output('cargo', 'metadata', '--no-deps', '--format-version', '1', '--locked'))
    versions = {p['version'] for p in metadata['packages'] if p['id'] in metadata['workspace_members']}
    if len(versions) != 1:
        parser.error('workspace package versions must agree')
    version = versions.pop()
    if not re.fullmatch(r'\d+\.\d+\.\d+-alpha\.\d+', version):
        parser.error('this preview publisher only accepts alpha versions')
    if output('git', 'status', '--porcelain', '--untracked-files=normal'):
        parser.error('commit changes first; release builds require a clean source tree')
    commit = output('git', 'rev-parse', 'HEAD')
    if args.tag:
        if args.tag != 'v'+version or output('git', 'rev-parse', args.tag+'^{commit}') != commit:
            parser.error('tag/version/HEAD mismatch')
    slug = f'runloom-{version}-{TARGET}'
    dist = ROOT/'dist';dist.mkdir(exist_ok=True)
    archive = dist/(slug+'.tar.gz');checksum = dist/(slug+'.tar.gz.sha256')
    if archive.exists() or checksum.exists():
        parser.error('release assets already exist; move them aside before rebuilding')
    env = dict(os.environ, MACOSX_DEPLOYMENT_TARGET='14.0', CARGO_TARGET_DIR=str(ROOT/'target'))
    env.pop('CARGO_ENCODED_RUSTFLAGS', None)
    env['RUSTFLAGS'] = f'--remap-path-prefix={ROOT}=/runloom --remap-path-prefix={Path.home()}=/builder'
    subprocess.run(['cargo','build','--release','--workspace','--locked','--target',TARGET],cwd=ROOT,env=env,check=True)
    if output('git', 'rev-parse', 'HEAD') != commit or output('git', 'status', '--porcelain', '--untracked-files=normal'):
        raise ValueError('source changed during build; refusing release assets')
    with tempfile.TemporaryDirectory(prefix='runloom-package-') as temp:
        bundle = Path(temp)/slug;bundle.mkdir();(bundle/'bin').mkdir()
        for binary in ('runloom','runloom-node'):
            source = ROOT/'target'/TARGET/'release'/binary
            if output(str(source),'--version') != binary+' '+version:
                raise ValueError('binary version mismatch')
            shutil.copy2(source, bundle/'bin'/binary)
        for name in ('LICENSE','THIRD_PARTY_NOTICES.md'):
            shutil.copyfile(ROOT/name,bundle/name)
        shutil.copyfile(ROOT/'scripts/install-release.py',bundle/'install.py')
        # Public trees keep release docs under docs; private authoring uses docs/public.
        notes = ROOT/'docs/RELEASE.md'
        if not notes.exists():notes=ROOT/'docs/public/RELEASE.md'
        shutil.copyfile(notes,bundle/'RELEASE.md')
        hashes={str(p.relative_to(bundle)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(bundle.rglob('*')) if p.is_file()}
        metadata={'format':1,'version':version,'source_commit':commit,'target':TARGET,
                  'macos_deployment_target':'14.0','rustc':output('rustc','--version'),
                  'built_at':datetime.now(timezone.utc).isoformat(),'sha256':hashes,
                  'signing':'No Developer ID signing or notarization; compiler ad-hoc signatures only',
                  'schema_version':4,'protocol_version':1}
        (bundle/'BUILD.json').write_text(json.dumps(metadata,indent=2)+'\n')
        # Tar metadata excludes local account names and absolute build paths.
        def clean(info):
            info.uid=info.gid=0;info.uname=info.gname='';return info
        with tarfile.open(archive,'w:gz') as tar:tar.add(bundle,arcname=slug,filter=clean)
    checksum.write_text(hashlib.sha256(archive.read_bytes()).hexdigest()+'  '+archive.name+'\n')
    print(json.dumps({'archive':str(archive),'version':version,'source_commit':commit,'target':TARGET}))

if __name__ == '__main__':main()
