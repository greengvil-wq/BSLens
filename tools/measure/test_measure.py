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

    def test_nonfinite_nonpositive_time_rejected_before_process_or_timer(self):
        for value in [float('nan'), float('inf'), -float('inf'), 0, -1]:
            with self.subTest(value=value), mock.patch.object(measure.subprocess, 'Popen') as spawn, mock.patch.object(measure.threading, 'Timer') as timer:
                for call in [lambda: measure.Git(self.repo, value),
                             lambda: measure.preflight(self.repo, self.commit, seconds=value),
                             lambda: measure.timed('open', ['unused'], value),
                             lambda: measure.first_tool('unused', 'unused', 'unused', 1, '0'*64, timeout=value)]:
                    with self.assertRaisesRegex(measure.Refusal, 'positive_finite'):
                        call()
                spawn.assert_not_called(); timer.assert_not_called()
        for value in ['nan', '+inf', '-inf', '0', '-1']:
            for args in [['preflight', '--repo', str(self.repo), '--commit', self.commit, '--seconds='+value],
                         ['time', '--phase', 'open', '--timeout='+value, '--', '/missing'],
                         ['first-tool', '--binary', '/missing', '--snapshot', '/missing', '--staging-dir', '/missing', '--max-sqlite-bytes', '1', '--expected-sha256', '0'*64, '--timeout='+value]]:
                r=subprocess.run([sys.executable, str(Path(measure.__file__)), *args], capture_output=True)
                self.assertEqual(r.returncode, 1)
                self.assertEqual(json.loads(r.stdout)['error'], 'positive_finite_seconds_required')
                self.assertEqual(r.stderr, b'')

    def test_seconds_upper_bound_before_spawn_and_boundary(self):
        for value in [3600.000001, 3601, 1e300]:
            with self.subTest(value=value), mock.patch.object(measure.subprocess, 'Popen') as spawn, mock.patch.object(measure.threading, 'Timer') as timer:
                for call in [lambda: measure.Git(self.repo, value),
                             lambda: measure.preflight(self.repo, self.commit, seconds=value),
                             lambda: measure.timed('open', ['unused'], value),
                             lambda: measure.first_tool('unused', 'unused', 'unused', 1, '0'*64, timeout=value)]:
                    with self.assertRaisesRegex(measure.Refusal, 'seconds_limit_3600'):
                        call()
                spawn.assert_not_called(); timer.assert_not_called()
            for args in [['preflight', '--repo', str(self.repo), '--commit', self.commit, '--seconds='+str(value)],
                         ['time', '--phase', 'open', '--timeout='+str(value), '--', '/missing'],
                         ['first-tool', '--binary', '/missing', '--snapshot', '/missing', '--staging-dir', '/missing', '--max-sqlite-bytes', '1', '--expected-sha256', '0'*64, '--timeout='+str(value)]]:
                with mock.patch.object(sys, 'argv', ['measure.py', *args]), mock.patch.object(measure.subprocess, 'Popen') as spawn, mock.patch.object(measure.threading, 'Timer') as timer:
                    with self.assertRaisesRegex(measure.Refusal, 'seconds_limit_3600'):
                        measure.main()
                    spawn.assert_not_called(); timer.assert_not_called()
        self.assertEqual(measure.positive_seconds(3600), 3600)
        self.assertEqual(measure.preflight(self.repo, self.commit, seconds=3600)['commit'], self.commit)
        self.assertEqual(measure.timed('open', [sys.executable, '-c', 'pass'], 3600)['exit_code'], 0)
        self.assertEqual(measure.first_tool(self.fake_mcp('ok'), '/synthetic', '/synthetic', 1,
                                           '0'*64, timeout=3600)['tool'], 'metadata')
        result=subprocess.run([sys.executable, str(Path(measure.__file__)), 'time', '--phase', 'open',
                               '--timeout=3600', '--', sys.executable, '-c', 'pass'], capture_output=True)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(json.loads(result.stdout)['exit_code'], 0)

    def fake_mcp(self, mode):
        path=self.root/'synthetic-mcp'
        path.write_text('#!'+sys.executable+'\n'+'''import json, sys, time, os
assert os.environ['PATH'] == ''
assert '--snapshot' in sys.argv and '--repo' not in sys.argv
mode = '''+repr(mode)+'''
a=json.loads(sys.stdin.readline())
assert a['method']=='initialize' and a['params']['protocolVersion']=='2025-11-25'
if mode=='timeout': time.sleep(5)
if mode=='eof': sys.exit(1)
if mode=='oversize': print('x'*20000, flush=True); time.sleep(5)
if mode=='malformed': print('not json', flush=True); time.sleep(5)
print(json.dumps({'jsonrpc':'2.0','id':1,'result':{'protocolVersion':'2025-11-25'}}), flush=True)
assert json.loads(sys.stdin.readline())['method']=='notifications/initialized'
a=json.loads(sys.stdin.readline())
assert a['method']=='tools/call' and a['params']=={'name':'metadata','arguments':{}}
time.sleep(.02)
r={'jsonrpc':'2.0','id':2,'result':{'isError':False,'structuredContent':{'snapshot':{'source_id':'private-user-secret'},'data':{'status':'ok'}}}}
if mode=='rpc_error': r={'jsonrpc':'2.0','id':2,'error':{'message':'private-user-secret'}}
if mode=='tool_error': r['result']['isError']=True
if mode=='wrong_id': r['id']=9
print(json.dumps(r), flush=True)
time.sleep(5)
''')
        path.chmod(0o700)
        return str(path)

    def test_first_tool_protocol_timings_and_redaction(self):
        r=measure.first_tool(self.fake_mcp('ok'), '/synthetic', '/synthetic', 1, '0'*64, timeout=2)
        self.assertGreater(r['start_to_initialize_seconds'], 0)
        self.assertGreaterEqual(r['request_to_first_useful_seconds'], .02)
        self.assertGreater(r['start_to_first_useful_seconds'], r['request_to_first_useful_seconds'])
        self.assertNotIn('private-user-secret', json.dumps(r))
        self.assertNotIn('/synthetic', json.dumps(r))

    def test_first_tool_errors_and_budgets(self):
        for mode, code in [('timeout','mcp_timeout'), ('eof','mcp_eof'), ('oversize','mcp_response_budget'),
                           ('malformed','mcp_invalid_json'), ('rpc_error','mcp_response_error'),
                           ('tool_error','mcp_tool_error'), ('wrong_id','mcp_response_error')]:
            with self.subTest(mode=mode), self.assertRaisesRegex(measure.Refusal, code):
                measure.first_tool(self.fake_mcp(mode), '/synthetic', '/synthetic', 1, '0'*64,
                                   timeout=.3, max_response_bytes=4096)

    @unittest.skipUnless(os.environ.get('BSLENS_MCP_BIN') and os.environ.get('BSLENS_V5_BIN'),
                         'external exact S5 MCP and builder binaries required')
    def test_real_first_tool_snapshot_without_git(self):
        stage=self.root/'mcp-stage'; stage.mkdir(mode=0o700)
        result=json.loads(subprocess.check_output([os.environ['BSLENS_V5_BIN'], 'build',
            str(self.repo), self.commit, 'synthetic:measure', str(self.root/'mcp-build'),
            '100', '1048576', '4194304', '8388608'], stderr=subprocess.DEVNULL))
        r=measure.first_tool(os.environ['BSLENS_MCP_BIN'], result['path'], stage, 8388608,
                             result['file_sha256'], timeout=30)
        self.assertGreater(r['request_to_first_useful_seconds'], 0)
        self.assertEqual(r['tool'], 'metadata')
        self.assertFalse(r['git_requested'])
