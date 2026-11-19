"""Только авторская синтетика. Реальные выгрузки не читаются."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import measure

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('s2_fixture', ROOT/'spikes/linux-v5/fixture.py')
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


class Measures(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root/'repo'; self.repo.mkdir()
        self.git('init', '-q')
        for name, content in fixture.SOURCES.items():
            p = self.repo/name; p.parent.mkdir(parents=True, exist_ok=True); p.write_bytes(content)
        (self.repo/'ignored-secret-name.txt').write_text('synthetic only')
        self.git('add', '.')
        self.git('-c', 'user.name=Synthetic', '-c', 'user.email=synthetic@invalid', 'commit', '-qm', 'synthetic')
        self.commit = self.git('rev-parse', 'HEAD')

    def git(self, *args):
        return subprocess.check_output(['/usr/bin/git','-C',str(self.repo),*args], stderr=subprocess.DEVNULL,
            env={**os.environ,'GIT_CONFIG_GLOBAL':'/dev/null','GIT_CONFIG_NOSYSTEM':'1','GIT_AUTHOR_DATE':'2026-11-19T00:00:00Z','GIT_COMMITTER_DATE':'2026-11-19T00:00:00Z'}).decode().strip()

    def artifact(self):
        path = self.root/'original.sqlite'
        with sqlite3.connect(path) as db:
            fixture.populate(db)
            db.execute('UPDATE metadata SET commit_oid=?', (self.commit,))
            # Большой BSL BLOB не меняет заданные факты: это методический контрпример.
            db.execute("UPDATE sources SET content=? WHERE path=?", (b'// synthetic\n'*10000,fixture.ORPHAN))
        return path, hashlib.sha256(path.read_bytes()).hexdigest()

    def test_preflight_exact_git_aggregate_only_and_no_worktree_change(self):
        before = self.git('status', '--porcelain')
        result = measure.preflight(self.repo,self.commit)
        self.assertEqual(result['selected']['bsl']['count'],5)
        self.assertEqual(result['full_tree']['regular'],len(fixture.SOURCES)+1)
        self.assertEqual(result['xml_root_versions']['2.21'],4)
        self.assertEqual(result['hard_blockers'],[])
        self.assertEqual(self.git('status','--porcelain'),before)
        text=json.dumps(result)
        self.assertNotIn('ignored-secret-name',text);self.assertNotIn('Catalogs',text)
        (self.repo/'Configuration.xml').write_text('uncommitted garbage')
        self.assertEqual(result,measure.preflight(self.repo,self.commit))
        self.assertEqual(before,'')
        with self.assertRaises(measure.Refusal): measure.preflight(self.repo,'HEAD')
        with self.assertRaises(measure.Refusal): measure.preflight(self.repo,self.commit,max_entries=1)

    def test_preflight_detects_hard_git_guards_before_selected_limits(self):
        extras = self.repo/'synthetic-excluded'; extras.mkdir()
        for i in range(4090):
            (extras/f'{i}.txt').write_bytes(b'x')
        with (extras/'large.bin').open('wb') as output:
            for _ in range(33):
                output.write(b'x'*measure.MIB)
        self.git('add','.')
        self.git('-c','user.name=Synthetic','-c','user.email=s@invalid','commit','-qm','guards')
        result=measure.preflight(self.repo,self.git('rev-parse','HEAD'))
        self.assertIn('git_files_4096',result['hard_blockers'])
        self.assertIn('git_total_32MiB',result['hard_blockers'])
        self.assertIn('git_object_8MiB',result['hard_blockers'])
        self.assertNotIn('xml_inputs_32MiB',result['hard_blockers'])
        self.assertNotIn('synthetic-excluded',json.dumps(result))

    def test_xml220_and_lexical_size_are_separate_from_build_blockers(self):
        p=self.repo/'Configuration.xml';p.write_bytes(p.read_bytes().replace(b'2.21',b'2.20'))
        (self.repo/'Large.bsl').write_bytes(b'x'*(measure.MIB+1))
        self.git('add','.');self.git('-c','user.name=Synthetic','-c','user.email=s@invalid','commit','-qm','220')
        r=measure.preflight(self.repo,self.git('rev-parse','HEAD'))
        self.assertIn('xml_format_2_20',r['hard_blockers']);self.assertEqual(r['lexical_size_refusals'],1)

    def test_pair_normalized_same_facts_original_untouched(self):
        path,digest=self.artifact();before=path.read_bytes()
        report=measure.pair(path,digest,self.commit,self.root)
        self.assertEqual(path.read_bytes(),before)
        self.assertEqual(report['original_bytes'],len(before))
        self.assertGreater(report['paired_difference_bytes'],0)
        self.assertNotEqual(report['paired_difference_bytes'],report['raw_bsl_payload_bytes'])
        self.assertEqual(report['normalized_with_bsl']['page_size'],report['normalized_without_bsl']['page_size'])
        run=next(self.root.glob('measurement-only-*'))
        self.assertEqual(run.stat().st_mode&0o777,0o700)
        for file in run.iterdir():self.assertEqual(file.stat().st_mode&0o777,0o600)
        with sqlite3.connect(run/'measurement-only-without-bsl.db') as db:
            self.assertEqual(db.execute("SELECT sum(length(content)) FROM sources WHERE kind='bsl'").fetchone()[0],0)
        self.assertNotIn('Catalogs',json.dumps(report))
        self.assertTrue((run/'MEASUREMENT_ONLY.json').exists())

    def test_pair_refuses_hash_sidecar_and_nonprivate_directory(self):
        path,digest=self.artifact()
        with self.assertRaises(measure.Refusal): measure.pair(path,'0'*64,self.commit,self.root)
        sidecar=Path(str(path)+'-wal');sidecar.touch()
        with self.assertRaises(measure.Refusal): measure.pair(path,digest,self.commit,self.root)
        sidecar.unlink();os.chmod(self.root,0o755)
        with self.assertRaises(measure.Refusal): measure.pair(path,digest,self.commit,self.root)
        os.chmod(self.root,0o700)

    def test_dbstat_unavailable_is_explicit(self):
        fake=mock.Mock();fake.execute.side_effect=sqlite3.OperationalError('no dbstat')
        self.assertEqual(measure.line_storage(fake)['available'],False)
        self.assertIsNone(measure.line_storage(fake)['bytes'])

    def test_timing_redacts_output_and_records_timeout(self):
        r=measure.timed('open',[sys.executable,'-c','print("secret-name")'],5)
        self.assertEqual(r['exit_code'],0);self.assertGreater(r['maxrss_kib'],0)
        self.assertNotIn('secret-name',json.dumps(r))
        r=measure.timed('first-tool',[sys.executable,'-c','import time;time.sleep(3)'],.05)
        self.assertTrue(r['timed_out']);self.assertNotEqual(r['exit_code'],0)

    @unittest.skipUnless(os.environ.get('BSLENS_V5_BIN'),'Rust CLI integration enabled explicitly')
    def test_real_synthetic_builder_pair_and_phase_hooks(self):
        binary=os.environ['BSLENS_V5_BIN'];output=self.root/'build';stage=self.root/'stage';stage.mkdir(mode=0o700)
        argv=[binary,'build',str(self.repo),self.commit,'synthetic:measure',str(output),'100','1048576','4194304','8388608']
        result=json.loads(subprocess.check_output(argv,stderr=subprocess.DEVNULL))
        report=measure.pair(result['path'],result['file_sha256'],self.commit,self.root)
        self.assertEqual(report['original_sha256'],result['file_sha256'])
        common=[result['path'],'--staging-dir',str(stage),'--max-sqlite-bytes','8388608','--expected-sha256',result['file_sha256']]
        phases = {}
        for phase in ['open','verify']:
            extra=[] if phase=='open' else ['--max-sources','100','--max-source-bytes','1048576','--max-total-bytes','4194304','--max-scratch-bytes','8388608']
            r=measure.timed(phase,[binary,phase,*common,*extra],30)
            self.assertEqual(r['exit_code'],0)
            phases[phase] = r
        # Для измерения build требуется свежий output; повтор в том же каталоге = reuse.
        argv[5]=str(self.root/'fresh-build')
        phases['build'] = measure.timed('build',argv,30)
        self.assertEqual(phases['build']['exit_code'],0)
        if destination := os.environ.get('BSLENS_MEASURE_REPORT'):
            Path(destination).write_text(json.dumps({'synthetic_only': True, 'profile': 'debug Rust 1.85.1 Linux x86_64', 'preflight': measure.preflight(self.repo,self.commit), 'pair': report, 'phases': phases, 'first_tool': 'not_measured_S5_driver_required'},indent=2)+'\n')
