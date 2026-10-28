"""Summarize measured Rust coverage without publishing absolute machine paths."""
import argparse
from datetime import datetime, timezone
import hashlib
import html
import json
from pathlib import Path
import re
import subprocess

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--docs', type=Path, help='Optionally refresh public docs and checked-in badges (e.g. docs or docs/public)')
args = parser.parse_args()
report_dir = root/'target/coverage'

def summarize(name):
    data = json.loads((report_dir/name).read_text())['data']
    if len(data) != 1: raise ValueError('Expected one coverage export')
    files = []
    for file in data[0]['files']:
        path = Path(file['filename']).resolve().relative_to(root)
        if path.parts[0] != 'crates': raise ValueError('Unexpected coverage source: '+str(path))
        lines = file['summary']['lines']
        files.append({'file':str(path), 'covered':lines['covered'], 'count':lines['count']})
    covered=sum(f['covered'] for f in files);count=sum(f['count'] for f in files)
    if not count: raise ValueError('Empty coverage report')
    return {'covered':covered,'count':count,'percent':round(100*covered/count,2),'files':files}

runs=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed;', (report_dir/'rust-tests.log').read_text())
if not runs: raise ValueError('Missing test result summaries')
passed=sum(int(a) for a,b in runs);failed=sum(int(b) for a,b in runs)
if failed or not passed: raise ValueError('Test suite did not pass')
# Fingerprint production/test Rust and fixture inputs, not a commit that includes this report.
paths=sorted([*root.glob('crates/**/*.rs'),*root.glob('crates/**/*.py'),*root.glob('crates/**/*.sql'),*root.glob('crates/**/*.toml'),root/'Cargo.toml',root/'Cargo.lock',*root.glob('scripts/smoke-*.py'),root/'scripts/test-native-materialization.py',root/'scripts/demo-local.py'])
# Only include scripts actually executed by coverage.sh, so private/live helpers do not affect exported fingerprint.
fixture_names={'smoke-cli.py','smoke-node.py','smoke-control.py','smoke-pty.py','smoke-checkpoint.py','smoke-checkpoint-concurrency.py','smoke-branch-control.py','smoke-task.py','test-native-materialization.py','demo-local.py'}
paths=[p for p in paths if p.parent != root/'scripts' or p.name in fixture_names]
h=hashlib.sha256()
for p in paths: h.update(str(p.relative_to(root)).encode()+b'\0'+p.read_bytes()+b'\0')
report={'measured_at':datetime.now(timezone.utc).isoformat(), 'source_sha256':h.hexdigest(), 'tool':subprocess.check_output(['cargo','llvm-cov','--version'],text=True).strip(), 'rustc':subprocess.check_output(['rustc','--version'],text=True).strip(), 'rust_tests':{'passed':passed,'failed':failed,'pass_percent':100.0}, 'rust_tests_line_coverage':summarize('rust-tests.json'), 'combined_line_coverage':summarize('combined.json'), 'scope':'Rust workspace source; Cargo unit/integration tests plus ten offline fixture scripts. Excludes Python, bundled C, external dependencies and test/example/bench files per cargo-llvm-cov defaults. Killed processes may not flush profiles. Not branch coverage or live acceptance.'}
(report_dir/'summary.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({k:v for k,v in report.items() if not k.endswith('line_coverage')},indent=2))
print('Rust tests line coverage:',report['rust_tests_line_coverage']['percent'])
print('Combined line coverage:',report['combined_line_coverage']['percent'])
if args.docs:
    docs=args.docs;docs.mkdir(parents=True,exist_ok=True);assets=docs/'assets';assets.mkdir(exist_ok=True)
    (docs/'coverage-summary.json').write_text(json.dumps(report,indent=2)+'\n')
    def badge(name,label,value,color):
        a=len(label)*7+20;b=len(value)*7+20
        svg=f'<svg xmlns="http://www.w3.org/2000/svg" width="{a+b}" height="24" role="img" aria-label="{html.escape(label+": "+value)}"><title>{html.escape(label+": "+value)}</title><rect width="{a+b}" height="24" rx="4" fill="#243641"/><path d="M{a} 0h{b}v24H{a}z" fill="{color}"/><g font-family="Verdana,sans-serif" font-size="11" text-anchor="middle"><text x="{a/2}" y="16" fill="#eff5f4">{html.escape(label)}</text><text x="{a+b/2}" y="16" fill="#10262b">{html.escape(value)}</text></g></svg>\n'
        (assets/name).write_text(svg)
    badge('tests.svg','Rust tests',f'{passed}/{passed+failed} passed','#80d8b9')
    badge('coverage.svg','Rust lines · local',f'{report["combined_line_coverage"]["percent"]:.1f}%','#e4bf7c')

    testing=docs/'TESTING.md'
    if testing.exists():
        rows=['| Rust crate | Rust tests | Tests + offline fixtures |','| --- | ---: | ---: |']
        totals={}
        for key in ['rust_tests_line_coverage','combined_line_coverage']:
            values={}
            for f in report[key]['files']:
                crate=f['file'].split('/')[1]
                v=values.setdefault(crate,[0,0]);v[0]+=f['covered'];v[1]+=f['count']
            totals[key]=values
        for crate in sorted(totals['combined_line_coverage']):
            vals=[]
            for key in totals:
                a,b=totals[key][crate];vals.append(f'{a}/{b} ({100*a/b:.2f}%)')
            rows.append('| '+crate+' | '+' | '.join(vals)+' |')
        for key in totals:
            v=report[key];totals[key]=f"{v['covered']}/{v['count']} ({v['percent']:.2f}%)"
        rows.append('| **Workspace** | **'+totals['rust_tests_line_coverage']+'** | **'+totals['combined_line_coverage']+'** |')
        block='Measured '+report['measured_at']+' with '+report['tool']+'; '+report['rustc']+'.\n\n'
        block+=f"Rust tests: **{passed}/{passed+failed} passed (100%)**.\n\n"+'\n'.join(rows)+'\n\n[Machine-readable summary](coverage-summary.json) includes the source fingerprint and per-file counts.'
        old=testing.read_text()
        updated,n=re.subn(r'<!-- coverage:start -->.*?<!-- coverage:end -->','<!-- coverage:start -->\n'+block+'\n<!-- coverage:end -->',old,flags=re.S)
        if n!=1: raise ValueError('Expected one coverage summary section')
        testing.write_text(updated)
