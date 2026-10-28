"""Check locked registry dependencies against OSV. Network required; no credentials sent."""
import argparse
import json
import pathlib
import subprocess
import sys
import urllib.request
from datetime import datetime, timezone

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=pathlib.Path)
args = parser.parse_args()
metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1']))
packages = sorted((p for p in metadata['packages'] if (p.get('source') or '').startswith('registry+')), key=lambda p: (p['name'], p['version']))
findings = []
for offset in range(0, len(packages), 100):
    batch = packages[offset:offset + 100]
    body = {'queries': [{'package': {'ecosystem': 'crates.io', 'name': p['name']}, 'version': p['version']} for p in batch]}
    request = urllib.request.Request('https://api.osv.dev/v1/querybatch', data=json.dumps(body).encode(), headers={'Content-Type': 'application/json'})
    with urllib.request.urlopen(request, timeout=30) as response:
        results = json.load(response)['results']
    if len(results) != len(batch) or any(r.get('next_page_token') for r in results):
        raise SystemExit('Incomplete OSV response; audit not passed')
    for package, result in zip(batch, results):
        for advisory in result.get('vulns', []):
            findings.append({'package': package['name'], 'version': package['version'], 'advisory': advisory['id']})
report = {'checked_at': datetime.now(timezone.utc).isoformat(), 'source': 'https://api.osv.dev/v1/querybatch', 'registry_packages': len(packages), 'findings': findings, 'limits': 'Known advisories only; not application reachability, bundled C inventory, or a security certification.'}
if args.output:
    args.output.write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2))
sys.exit(1 if findings else 0)
