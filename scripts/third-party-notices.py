"""Collect registry package metadata and shipped license/notice files, offline."""
import argparse
import json
from pathlib import Path
import subprocess
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
meta = json.loads(subprocess.check_output(['cargo','metadata','--locked','--offline','--format-version','1']))
packages=sorted((p for p in meta['packages'] if (p.get('source') or '').startswith('registry+')),key=lambda p:(p['name'],p['version']))
lines=['# Third-party dependency notices', '', 'Generated from Cargo.lock with scripts/third-party-notices.py. Includes target-specific dependencies. Runloom is MIT; dependencies retain their own licenses. Upstream license texts below are distributed verbatim. This inventory records package notices; it does not verify license compatibility.', '']
missing=[]
for p in packages:
    root=Path(p['manifest_path']).parent
    candidates=set()
    for f in root.rglob('*'):
        relative=f.relative_to(root)
        if len(relative.parts)>3 or not f.is_file() or f.is_symlink():continue
        if f.name.upper().startswith(('LICENSE','LICENCE','COPYING','NOTICE','UNLICENSE')) and f.stat().st_size<1_000_000:candidates.add(f)
    if p.get('license_file'):
        f=root/p['license_file']
        if f.is_file() and f.resolve().is_relative_to(root.resolve()):candidates.add(f)
    lines += ['## '+p['name']+' '+p['version'], '', 'Declared license: `'+str(p.get('license') or 'See license file')+'`.', '', 'Source: https://crates.io/crates/'+p['name']+'/'+p['version'], '']
    if not candidates:
        missing.append(p['name']);lines += ['No standalone notice file shipped in the registry package; consult the declared license and upstream source above.', '']
    for f in sorted(candidates):
        lines += ['### '+str(f.relative_to(root)), '', '````text', f.read_text(errors='replace'), '````', '']
args.output.parent.mkdir(parents=True,exist_ok=True)
args.output.write_text('\n'.join(lines))
print(json.dumps({'packages':len(packages),'without_standalone_notice':missing,'output':str(args.output)}))
