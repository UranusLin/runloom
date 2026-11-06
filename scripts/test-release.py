"""Installer failure boundaries; no real profile or service is touched."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
spec=importlib.util.spec_from_file_location('installer',Path(__file__).with_name('install-release.py'))
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
class InstallTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name).resolve();self.bundle=self.root/'bundle';self.bundle.mkdir()
        hashes={}
        for name in module.FILES:
            p=self.bundle/name;p.parent.mkdir(exist_ok=True);p.write_text('fixture')
            hashes[name]=hashlib.sha256(p.read_bytes()).hexdigest()
        self.manifest={'sha256':hashes,'version':'0.1.0-alpha.1','source_commit':'fixture'}
        self.save()
    def save(self):(self.bundle/'BUILD.json').write_text(json.dumps(self.manifest))
    def test_new_prefix_and_no_overwrite(self):
        prefix=self.root/'installed';module.install(self.bundle,prefix)
        self.assertEqual((prefix/'bin/runloom').read_text(),'fixture')
        with self.assertRaises(ValueError):module.install(self.bundle,prefix)
    def test_tamper_before_any_install(self):
        (self.bundle/'bin/runloom').write_text('tampered')
        with self.assertRaises(ValueError):module.install(self.bundle,self.root/'new')
        self.assertFalse((self.root/'new').exists())
    def test_manifest_path_escape_rejected(self):
        self.manifest['sha256']['../escape']='fake';self.save()
        with self.assertRaises(ValueError):module.install(self.bundle,self.root/'new')
    def test_symlink_source_and_destination_rejected(self):
        source=self.bundle/'bin/runloom';source.unlink();source.symlink_to(self.bundle/'LICENSE')
        with self.assertRaises(ValueError):module.install(self.bundle,self.root/'new')
        source.unlink();source.write_text('fixture')
        link=self.root/'link';link.symlink_to(self.root/'missing')
        with self.assertRaises(ValueError):module.install(self.bundle,link)
if __name__=='__main__':unittest.main()
