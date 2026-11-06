"""Unpack/check/install real release in an empty HOME and exercise CLI + node."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import sys
import tarfile
import tempfile

p=argparse.ArgumentParser();p.add_argument('archive',type=Path);a=p.parse_args()
archive=a.archive.resolve()
expected=Path(str(archive)+'.sha256').read_text().split()[0]
assert hashlib.sha256(archive.read_bytes()).hexdigest()==expected,'archive checksum mismatch'
with tempfile.TemporaryDirectory(prefix='rl-install-') as temp:
    root=Path(temp).resolve();home=root/'home';home.mkdir();unpack=root/'unpack';unpack.mkdir()
    with tarfile.open(archive) as tar:
        # Only our own bounded regular files/directories, never links or traversal.
        members=tar.getmembers();assert len(members)<30
        for m in members:
            assert not Path(m.name).is_absolute() and '..' not in Path(m.name).parts
            assert m.isfile() or m.isdir()
        assert len({m.name for m in members})==len(members)
        assert sum(m.size for m in members)<256*1024*1024
        for m in members:
            dest=unpack/m.name
            if m.isdir():dest.mkdir(parents=True,exist_ok=True)
            else:
                dest.parent.mkdir(parents=True,exist_ok=True)
                with tar.extractfile(m) as source, dest.open('xb') as target:
                    shutil.copyfileobj(source,target)
                dest.chmod(0o700 if '/bin/' in m.name else 0o600)
    bundles=list(unpack.iterdir());assert len(bundles)==1;bundle=bundles[0]
    build=json.loads((bundle/'BUILD.json').read_text());prefix=home/'installed'
    env={**os.environ,'HOME':str(home),'XDG_CONFIG_HOME':str(home/'.config')}
    subprocess.run([sys.executable,str(bundle/'install.py'),'--prefix',str(prefix)],env=env,check=True)
    def run(args,input=None):return subprocess.check_output([str(x) for x in args],input=input,text=True,env=env,timeout=15).strip()
    for name in ('runloom','runloom-node'):
        assert run([prefix/'bin'/name,'--version'])==name+' '+build['version']
    cli=prefix/'bin/runloom';db=home/'metadata.db'
    run([cli,'--db',db,'init']);assert json.loads(run([cli,'--db',db,'list','--workspace','fixture']))==[]
    node=prefix/'bin/runloom-node';data=home/'node';data.mkdir(mode=0o700)
    server=subprocess.Popen([str(node),'--root',str(data),'serve','--host','release-fixture'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,env=env)
    try:
        assert select.select([server.stdout],[],[],15)[0]
        assert json.loads(server.stdout.readline())['ready']
        response=json.loads(run([node,'--root',data,'request'],json.dumps({'version':1,'op':'ping'})+'\n'))
        assert response['ok']
        health=response['result']
        assert health['package_version']==build['version'] and health['schema_version']==build['schema_version']
        assert health['recovery_plan']
    finally:
        server.terminate();server.wait(timeout=10)
    print(json.dumps({'pass':True,'version':build['version'],'source_commit':build['source_commit'],'isolated_home_install':True,'cli_and_node':True,'new_machine_or_gatekeeper_verified':False,'provider_inference':False}))
