"""Запуск: python3 -m unittest discover -s spikes/linux-v5 -v."""
import hashlib
import sqlite3
import tempfile
import unittest
from pathlib import Path
from fixture import ROOT, SOURCES, ACCEPTED, UNPARSED, ORPHAN, ROOT_MODULES, populate, physical_lines, read_range


class SchemaTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        self.addCleanup(self.db.close)
        populate(self.db)

    def reject(self, sql, args=()):
        self.db.execute('SAVEPOINT invalid')
        try:
            with self.assertRaises(sqlite3.IntegrityError):
                self.db.execute(sql, args)
        finally:
            self.db.execute('ROLLBACK TO invalid')
            self.db.execute('RELEASE invalid')

    def branch(self, level, parent_id=None, kind=None, module_path=None):
        return self.db.execute((ROOT / 'branch.sql').read_text(),
            dict(level=level, parent_id=parent_id, kind=kind, module_path=module_path)).fetchall()

    def test_versions_and_separation(self):
        self.assertEqual(self.db.execute('PRAGMA user_version').fetchone(), (5,))
        self.assertEqual(self.db.execute('SELECT schema_version,profile,facts_sha256,relations FROM metadata').fetchone(),
            (5, 'linux-v5-spike-xml221-bsl-r09', None, 'not_analyzed'))
        for assignment in ["schema_version=4", "profile='synthetic-bsl-utf8-lines-v2'",
                           "snapshot_id='bslens-v4-x'", "relations='none'", 'singleton=2']:
            self.reject('UPDATE metadata SET ' + assignment)

    def test_tables_and_integrity(self):
        self.assertEqual({r[0] for r in self.db.execute("SELECT name FROM sqlite_schema WHERE type='table'")},
            {'metadata', 'sources', 'objects', 'modules', 'methods', 'source_lines'})
        self.assertEqual(self.db.execute('PRAGMA integrity_check').fetchall(), [('ok',)])
        self.assertEqual(self.db.execute('PRAGMA foreign_keys').fetchone(), (1,))
        self.assertEqual(self.db.execute('PRAGMA foreign_key_check').fetchall(), [])

    def test_foreign_keys_and_kinds(self):
        self.reject('UPDATE modules SET owner_id=999')
        self.reject("UPDATE objects SET xml_path=? WHERE object_id=2", (ACCEPTED,))
        self.reject("UPDATE modules SET path='Configuration.xml' WHERE path=?", (ORPHAN,))
        self.reject('DELETE FROM sources WHERE path=?', (ACCEPTED,))
        self.reject('UPDATE methods SET module_path=?', (UNPARSED,))
        self.reject('UPDATE source_lines SET path=? WHERE path=?', ('Configuration.xml', ORPHAN))

    def test_status_and_method_invariants(self):
        self.reject("UPDATE modules SET reason_code=NULL WHERE path=?", (UNPARSED,))
        self.reject("UPDATE modules SET reason_code='Limit' WHERE path=?", (ACCEPTED,))
        self.reject("UPDATE modules SET reason_code='unstable prose' WHERE path=?", (UNPARSED,))
        self.reject("UPDATE modules SET parse_status='unknown' WHERE path=?", (ORPHAN,))
        for assignment in ['exported=2', "kind='unknown'", 'start_line=0', 'end_line=0', 'end_line=999',
                           'byte_start=-1', 'byte_end=0']:
            self.reject('UPDATE methods SET ' + assignment)

    def test_source_line_invariants(self):
        for assignment in ['line_number=0', 'byte_start=-1', 'byte_end=byte_start']:
            self.reject('UPDATE source_lines SET ' + assignment)
        self.reject('INSERT INTO source_lines SELECT * FROM source_lines LIMIT 1')
        self.reject("UPDATE sources SET content='not a blob'")
        self.reject("UPDATE sources SET mode='120000'")
        self.reject("UPDATE sources SET path='../escape' WHERE path=?", (ORPHAN,))

    def test_tree_kind_level_order_and_empty_object(self):
        self.assertEqual(self.branch('configuration', 1),
            [('module', p, p, 'accepted', None) for p in ROOT_MODULES] +
            [('kind', 'Catalog', 'Catalog', None, None), ('kind', 'Document', 'Document', None, None),
             ('unconfirmed', 'unconfirmed', 'Владелец не подтверждён', None, None)])
        self.assertEqual(self.branch('kind', 1, 'Catalog'),
            [('object', '4', 'Empty', None, None), ('object', '2', 'Items', None, None)])
        self.assertEqual(self.branch('object', 4), [])
        self.assertEqual(self.branch('object', 3), [('module', UNPARSED, UNPARSED, 'unparsed', 'RegionHeader')])
        self.assertEqual(self.branch('object', 2), [('module', ACCEPTED, ACCEPTED, 'accepted', None)])
        self.assertEqual(self.branch('module', module_path=UNPARSED), [])
        self.assertEqual(self.branch('module', module_path=ACCEPTED), [('method', '1', 'ЯПервый', 'accepted', None),
             ('method', '2', 'АВторой', 'accepted', None)])
        self.assertEqual(self.branch('unconfirmed'), [('module', ORPHAN, ORPHAN, 'accepted', None)])

    def test_all_bsl_lines_and_source_hashes(self):
        for path, data in SOURCES.items():
            actual, digest, oid = self.db.execute('SELECT content,sha256,blob_oid FROM sources WHERE path=?', (path,)).fetchone()
            self.assertEqual(actual, data)
            self.assertEqual(digest, hashlib.sha256(data).hexdigest())
            self.assertEqual(oid, hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest())
            rows = self.db.execute('SELECT line_number,byte_start,byte_end FROM source_lines WHERE path=? ORDER BY line_number', (path,)).fetchall()
            if path.endswith('.bsl'):
                self.assertEqual(rows, list(physical_lines(data)))
                self.assertEqual(rows[0][1], 0)
                self.assertEqual(rows[-1][2], len(data))
                self.assertTrue(all(a[2] == b[1] for a, b in zip(rows, rows[1:])))
            else:
                self.assertEqual(rows, [])

    def test_unparsed_exact_bytes_by_line_index(self):
        expected = [b'\xef\xbb\xbf' + '// Синтетика\r\n'.encode(),
                    '#Область\r\n'.encode(), 'ПоследняяСтрока'.encode()]
        for start in range(1, 4):
            for end in range(start, 4):
                self.assertEqual(read_range(self.db, UNPARSED, start, end), b''.join(expected[start-1:end]))
        for start, end in [(0,1), (1,4), (3,2)]:
            with self.assertRaises(ValueError):
                read_range(self.db, UNPARSED, start, end)
        self.assertEqual(read_range(self.db, ACCEPTED, 1, 4), SOURCES[ACCEPTED])

    def test_lexical_offsets_are_independent_of_physical_lines(self):
        data = SOURCES[ACCEPTED]
        for start_line, end_line, start, end in self.db.execute(
                'SELECT start_line,end_line,byte_start,byte_end FROM methods ORDER BY start_line'):
            first_start, first_end = self.db.execute(
                'SELECT byte_start,byte_end FROM source_lines WHERE path=? AND line_number=?',
                (ACCEPTED, start_line)).fetchone()
            last_start, last_end = self.db.execute(
                'SELECT byte_start,byte_end FROM source_lines WHERE path=? AND line_number=?',
                (ACCEPTED, end_line)).fetchone()
            self.assertTrue(first_start < start < first_end)
            self.assertTrue(last_start < end < last_end)
            self.assertTrue(data[start:end].startswith('Процедура'.encode()))
            self.assertTrue(data[start:end].endswith('КонецПроцедуры'.encode()))
            self.assertEqual(read_range(self.db, ACCEPTED, start_line, end_line), data[first_start:last_end])
        self.assertTrue(read_range(self.db, ACCEPTED, 1, 2).startswith(b'\xef\xbb\xbf  '))
        self.assertTrue(read_range(self.db, ACCEPTED, 1, 2).endswith(b'\r\n'))
        # Межтабличное попадание байтов в строку проверят S3/verify, не FK.
        self.db.execute('UPDATE methods SET byte_start=1 WHERE method_id=1')
        self.assertEqual(self.db.execute('PRAGMA foreign_key_check').fetchall(), [])

    def test_build_date_is_utc_and_separate_from_identity(self):
        before = self.db.execute('SELECT snapshot_id,facts_sha256 FROM metadata').fetchone()
        self.assertEqual(self.db.execute('SELECT built_at_utc FROM metadata').fetchone(),
                         ('2026-11-19T00:00:00Z',))
        for invalid in ['2026-11-19', '2026-11-19T03:00:00+03:00', '2026-13-19T00:00:00Z']:
            self.reject('UPDATE metadata SET built_at_utc=?', (invalid,))
        self.db.execute("UPDATE metadata SET built_at_utc='2026-11-20T00:00:00Z'")
        self.assertEqual(self.db.execute('SELECT snapshot_id,facts_sha256 FROM metadata').fetchone(), before)

    def test_physical_line_edge_cases(self):
        for data, expected in [(b'', []), (b'\n', [(1,0,1)]),
                (b'a\r\nb', [(1,0,3),(2,3,4)]), (b'a\r', [(1,0,2)])]:
            self.assertEqual(list(physical_lines(data)), expected)

    def test_reproducible_logical_fixture(self):
        other = sqlite3.connect(':memory:')
        self.addCleanup(other.close)
        populate(other)
        self.assertEqual(list(self.db.iterdump()), list(other.iterdump()))

    def test_standalone_sqlite_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'fixture.sqlite'
            db = sqlite3.connect(path)
            populate(db)
            db.close()
            self.assertEqual([p.name for p in path.parent.iterdir()], ['fixture.sqlite'])
            with sqlite3.connect(f'file:{path}?mode=ro', uri=True) as reopened:
                self.assertEqual(read_range(reopened, UNPARSED, 2, 3),
                    '#Область\r\nПоследняяСтрока'.encode())
