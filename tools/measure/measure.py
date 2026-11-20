#!/usr/bin/env python3
"""Локальная диагностика S8. Только агрегаты; не валидатор продукта."""
import argparse
import contextlib
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import resource
import select
import signal
import sqlite3
import stat
import subprocess
import tempfile
import threading
import time
import xml.parsers.expat

BASE = '5c91f662ba6a398e841ba51b39cb3b6cd3d22c55'
MIB = 1024 * 1024


class Refusal(Exception):
    pass


def exact_sha(value):
    if not re.fullmatch(r'[0-9a-f]{40}|[0-9a-f]{64}', value):
        raise Refusal('full_sha_required')
    return value


def positive_seconds(value):
    if not math.isfinite(value) or value <= 0:
        raise Refusal("positive_finite_seconds_required")
    return value


class Git:
    def __init__(self, repo, seconds):
        positive_seconds(seconds)
        self.command = ['/usr/bin/git', '--no-replace-objects', '-C', str(repo)]
        self.env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
        self.env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL='/dev/null',
                        GIT_NO_REPLACE_OBJECTS='1', GIT_NO_LAZY_FETCH='1', GIT_TERMINAL_PROMPT='0')
        self.deadline = time.monotonic() + seconds
        self.children = []

    def spawn(self, *args, stdin=None):
        if time.monotonic() >= self.deadline:
            raise Refusal('preflight_deadline')
        p = subprocess.Popen(self.command + list(args), env=self.env, stdin=stdin,
                             stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        timer = threading.Timer(max(.001, self.deadline - time.monotonic()), p.kill)
        timer.start()
        self.children.append((p, timer))
        return p

    def finish(self, p):
        entry = next(item for item in self.children if item[0] is p)
        entry[1].cancel()
        entry[1].join()
        p.wait()
        if p.stdin:
            p.stdin.close()
        p.stdout.close()
        self.children.remove(entry)

    def small(self, *args):
        p = self.spawn(*args)
        value = p.stdout.read(4097)
        if len(value) > 4096 or p.wait() != 0:
            raise Refusal('git_object_or_deadline')
        self.finish(p)
        return value.strip()

    def close(self):
        for p, timer in self.children:
            timer.cancel()
            timer.join()
            if p.poll() is None:
                p.kill()
            p.wait()
            if p.stdin:
                p.stdin.close()
            p.stdout.close()


def records(stream):
    pending = b''
    while True:
        chunk = stream.read(65536)
        if not chunk:
            if pending:
                raise Refusal('git_truncated_record')
            return
        chunks = (pending + chunk).split(b'\0')
        pending = chunks.pop()
        if len(pending) > 1024 * 1024:
            raise Refusal('preflight_record_budget')
        for record in chunks:
            if len(record) > 1024*1024:
                raise Refusal("preflight_record_budget")
            yield record


def preflight(repo, commit, max_entries=200000, max_xml_scan=1024*MIB, seconds=300):
    exact_sha(commit)
    git = Git(repo, seconds)
    try:
        if git.small('cat-file', '-t', commit) != b'commit':
            raise Refusal('not_commit')
        tree = git.small('rev-parse', commit + '^{tree}').decode('ascii')
        check = git.spawn('cat-file', '--batch-check=%(objecttype) %(objectsize)', stdin=subprocess.PIPE)

        def size(oid, kind):
            check.stdin.write(oid + b'\n'); check.stdin.flush()
            line = check.stdout.readline(128).split()
            if len(line) != 2 or line[0] != kind or not line[1].isdigit():
                raise Refusal('git_object_or_deadline')
            return int(line[1])

        commit_bytes = size(commit.encode(), b'commit')
        tree_bytes = size(tree.encode(), b'tree')
        all_bytes = commit_bytes + tree_bytes
        counts = dict(entries=0, directories=0, regular=0, regular_bytes=0, max_regular_bytes=0, unsupported=0, excluded=0)
        selected = {k: dict(count=0, bytes=0, max_bytes=0) for k in ('xml', 'bsl')}
        versions = {'2.21': 0, '2.20': 0, 'other_or_unknown': 0}
        over = dict(git_object_bytes=int(commit_bytes > 8*MIB)+int(tree_bytes > 8*MIB), path_bytes=0, depth=0, xml_bytes=0, lexical_bytes=0)
        xml_scanned = 0
        configuration_present = False
        listing = git.spawn('ls-tree', '-r', '-t', '-z', '--full-tree', commit)
        for record in records(listing.stdout):
            counts['entries'] += 1
            if counts['entries'] > max_entries:
                raise Refusal('preflight_entry_budget')
            header, path = record.split(b'\t', 1)
            mode, kind, oid = header.split()
            configuration_present |= path == b'Configuration.xml' and mode in (b'100644', b'100755')
            over['path_bytes'] += len(path) > 1024
            over['depth'] += path.count(b'/') > 64
            if mode == b'040000':
                counts['directories'] += 1
            elif mode in (b'100644', b'100755'):
                counts['regular'] += 1
            else:
                counts['unsupported'] += 1
                continue
            n = size(oid, kind)
            all_bytes += n
            over['git_object_bytes'] += n > 8*MIB
            if mode == b'040000':
                tree_bytes += n
                continue
            counts['regular_bytes'] += n
            counts['max_regular_bytes'] = max(counts['max_regular_bytes'], n)
            category = 'xml' if path.endswith(b'.xml') else 'bsl' if path.endswith(b'.bsl') else None
            if category is None:
                counts['excluded'] += 1
                continue
            group = selected[category]
            group['count'] += 1; group['bytes'] += n; group['max_bytes'] = max(group['max_bytes'], n)
            if category == 'bsl':
                over['lexical_bytes'] += n > MIB
                continue
            over['xml_bytes'] += n > 2*MIB
            # Только первый XML-элемент, максимум 64 KiB. Не парсим DTD/содержимое.
            xml_scanned += min(n, 65536)
            if xml_scanned > max_xml_scan:
                raise Refusal('preflight_xml_scan_budget')
            blob = git.spawn('cat-file', 'blob', oid.decode('ascii'))
            prefix = blob.stdout.read(min(n, 65536))
            blob.kill() if blob.poll() is None else None
            git.finish(blob)
            parser = xml.parsers.expat.ParserCreate(namespace_separator='}')
            found = []
            class RootFound(Exception):
                pass
            def root(name, attrs):
                if name == 'http://v8.1c.ru/8.3/MDClasses}MetaDataObject':
                    found.append(attrs.get('version'))
                raise RootFound()
            def doctype(*args):
                raise RootFound()
            parser.StartElementHandler = root
            parser.StartDoctypeDeclHandler = doctype
            try:
                parser.Parse(prefix, False)
            except (RootFound, xml.parsers.expat.ExpatError):
                pass
            version = found[0] if found and found[0] in ('2.20', '2.21') else 'other_or_unknown'
            versions[version] += 1
        if listing.wait() != 0:
            raise Refusal('git_object_or_deadline')
        total_count = sum(v['count'] for v in selected.values())
        total_bytes = sum(v['bytes'] for v in selected.values())
        blockers = []
        for code, failed in [
            ('xml_configuration_missing', not configuration_present),
            ('git_files_4096', counts['regular'] > 4096),
            ('git_objects_8192', 2 + counts['directories'] + counts['regular'] > 8192),
            ('git_total_32MiB', all_bytes > 32*MIB),
            ('git_object_8MiB', over['git_object_bytes']),
            ('git_type', counts['unsupported']), ('path_1024', over['path_bytes']),
            ('tree_depth_64', over['depth']), ('xml_inputs_4096', total_count > 4096),
            ('xml_inputs_32MiB', total_bytes > 32*MIB), ('xml_file_2MiB', over['xml_bytes']),
            ('xml_format_2_20', versions['2.20']), ('xml_format_unknown', versions['other_or_unknown'])]:
            if failed:
                blockers.append(code)
        return dict(contract='measurement-preflight-v1', guard_base=BASE, commit=commit, tree=tree,
                    full_tree=counts, commit_object_bytes=commit_bytes, tree_object_bytes_per_traversal=tree_bytes,
                    git_object_bytes_per_traversal=all_bytes, selected=selected,
                    xml_root_versions=versions, over_guard_counts=over, hard_blockers=blockers,
                    readiness='blocked' if blockers else 'requires_build_and_verify',
                    not_checked=['git_layout_and_30s_runtime', 'xml_semantics_depth_events_nodes',
                                 'lexical_lines_tokens_regions_syntax', 'identity_rehash'],
                    lexical_size_refusals=over['lexical_bytes'])
    finally:
        git.close()


def private_dir(path):
    path = Path(path).absolute()
    if str(path).startswith('/mnt/'):
        raise Refusal('linux_private_filesystem_required')
    for component in [*reversed(path.parents), path]:
        info = component.lstat()
        if stat.S_ISLNK(info.st_mode):
            raise Refusal('symlink_directory')
        if component != path and (info.st_uid not in (0, os.geteuid()) or
                (info.st_mode & 0o022 and not (info.st_uid == 0 and info.st_mode & stat.S_ISVTX))):
            raise Refusal('unsafe_directory_parent')
    info = path.stat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.geteuid() or stat.S_IMODE(info.st_mode) != 0o700:
        raise Refusal('private_directory_0700_required')
    return path


def file_hash(stream, maximum=1024*MIB):
    h = hashlib.sha256()
    stream.seek(0)
    total = 0
    while chunk := stream.read(65536):
        total += len(chunk)
        if total > maximum:
            raise Refusal('snapshot_file_budget')
        h.update(chunk)
    return h.hexdigest()


def open_db(path):
    db = sqlite3.connect(path)
    # Только доверенный собственный артефакт с ожидаемой SHA, не reader S4.
    # SQLite 3.40 требует trusted_schema для json_valid в CHECK при VACUUM.
    db.enable_load_extension(False)
    db.execute('PRAGMA trusted_schema=ON')
    return db


def line_storage(db):
    try:
        size = db.execute("SELECT coalesce(sum(pgsize),0) FROM dbstat WHERE name IN (SELECT name FROM sqlite_schema WHERE tbl_name='source_lines')").fetchone()[0]
        return dict(available=True, method='dbstat table_and_all_indexes_allocated_pages', bytes=size)
    except sqlite3.OperationalError:
        return dict(available=False, method='dbstat_unavailable', bytes=None)


def unchanged_digest(db):
    """Локальная проверка пары, НЕ facts_sha256 продукта; BSL content исключён."""
    h = hashlib.sha256()
    for table, order in [('metadata', 'singleton'), ('objects', 'object_id'), ('modules', 'path'),
                         ('methods', 'method_id'), ('source_lines', 'path,line_number')]:
        for row in db.execute(f'SELECT * FROM {table} ORDER BY {order}'):
            data = json.dumps([table, row], ensure_ascii=True, separators=(',', ':')).encode()
            h.update(len(data).to_bytes(8, 'big')); h.update(data)
    for rowid, path, kind, mode, oid, sha in db.execute('SELECT rowid,path,kind,mode,blob_oid,sha256 FROM sources ORDER BY path'):
        data = json.dumps([path, kind, mode, oid, sha], ensure_ascii=True).encode()
        h.update(len(data).to_bytes(8, 'big')); h.update(data)
        if kind != 'bsl':
            with db.blobopen('sources', 'content', rowid, readonly=True) as blob:
                h.update(len(blob).to_bytes(8, 'big'))
                while chunk := blob.read(65536):
                    h.update(chunk)
    return h.hexdigest()


def pair(snapshot, expected_sha256, commit, workdir, max_bytes=1024*MIB):
    exact_sha(commit)
    if not re.fullmatch('[0-9a-f]{64}', expected_sha256):
        raise Refusal('expected_sha256_required')
    for suffix in ('-wal', '-shm', '-journal'):
        if Path(str(snapshot)+suffix).exists():
            raise Refusal('snapshot_sidecar')
    work = private_dir(workdir)
    run = Path(tempfile.mkdtemp(prefix='measurement-only-', dir=work))
    # Копии остаются локальными и потенциально содержат исходники. В stdout только агрегаты.
    marker = run / 'MEASUREMENT_ONLY.json'
    marker.write_text('{"measurement_only":true,"never_use_with_mcp_or_verify":true}')
    os.chmod(marker, 0o600)
    fd = os.open(snapshot, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as original:
        info = os.fstat(original.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size > max_bytes:
            raise Refusal('snapshot_file_budget_or_type')
        if file_hash(original, max_bytes) != expected_sha256:
            raise Refusal('snapshot_sha256')
        copies = [run / 'measurement-only-with-bsl.db', run / 'measurement-only-without-bsl.db']
        for path in copies:
            original.seek(0)
            with os.fdopen(os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600), 'wb') as target:
                copied = 0
                while chunk := original.read(65536):
                    copied += len(chunk)
                    if copied > max_bytes:
                        raise Refusal('snapshot_file_budget')
                    target.write(chunk)
        if file_hash(original, max_bytes) != expected_sha256:
            raise Refusal('snapshot_changed')
    baseline = None
    results = []
    for i, path in enumerate(copies):
        with contextlib.closing(open_db(path)) as db:
            if db.execute('SELECT commit_oid FROM metadata').fetchone() != (commit,):
                raise Refusal('snapshot_commit')
            if db.execute("SELECT count(*) FROM sqlite_schema WHERE type IN ('trigger','view')").fetchone()[0]:
                raise Refusal('snapshot_schema')
            current = unchanged_digest(db)
            if baseline is None:
                baseline = current
                original_lines = line_storage(db)
                raw_bsl = db.execute("SELECT coalesce(sum(length(content)),0) FROM sources WHERE kind='bsl'").fetchone()[0]
            elif current != baseline:
                raise Refusal('copy_mismatch')
            db.execute('PRAGMA journal_mode=DELETE')
            db.execute('PRAGMA page_size=4096')
            db.execute('PRAGMA auto_vacuum=NONE')
            if i:
                db.execute("UPDATE sources SET content=x'' WHERE kind='bsl'")
                db.commit()
            db.execute('VACUUM')
            if unchanged_digest(db) != baseline:
                raise Refusal('pair_facts_changed')
            results.append(dict(bytes=path.stat().st_size, source_lines=line_storage(db),
                                page_size=db.execute('PRAGMA page_size').fetchone()[0]))
    report = dict(contract='measurement-pair-v1', measurement_only=True, commit=commit,
                  original_sha256=expected_sha256, original_bytes=info.st_size,
                  sqlite_version=sqlite3.sqlite_version, compaction='VACUUM; auto_vacuum=NONE; DELETE; page_size=4096',
                  normalized_with_bsl=results[0], normalized_without_bsl=results[1],
                  paired_difference_bytes=results[0]['bytes']-results[1]['bytes'],
                  raw_bsl_payload_bytes=raw_bsl, original_source_lines=original_lines,
                  unchanged_non_bsl_logical_digest=baseline)
    output = run / 'aggregates.json'
    output.write_text(json.dumps(report, indent=2) + '\n'); os.chmod(output, 0o600)
    return report


def timed(phase, command, timeout):
    positive_seconds(timeout)
    if not command:
        raise Refusal('command_required')
    runner_rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    started = time.monotonic()
    p = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    expired = threading.Event()
    def stop():
        expired.set()
        try:
            os.killpg(p.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    timer = threading.Timer(timeout, stop); timer.start()
    try:
        _, status, usage = os.wait4(p.pid, 0)
        p.returncode = os.waitstatus_to_exitcode(status)
    finally:
        timer.cancel()
        timer.join()
    return dict(contract='measurement-time-v1', phase=phase, wall_seconds=time.monotonic()-started,
                maxrss_kib=usage.ru_maxrss, exit_code=p.returncode, timed_out=expired.is_set(),
                output_discarded=True, cache_state='uncontrolled', runner_peak_rss_kib_before_spawn=runner_rss,
                rss_floor_warning=usage.ru_maxrss <= runner_rss,
                rss_caveat='includes child pre-exec lifetime; runner floor may dominate small commands; do not subtract peaks',
                rss_method='wait4 Linux ru_maxrss, not sum of concurrent processes')


def first_tool(binary, snapshot, staging_dir, max_sqlite_bytes, expected_sha256,
               timeout=30, max_response_bytes=MIB):
    """Фактический snapshot stdio PR #4; ответы остаются только в памяти."""
    positive_seconds(timeout)
    if max_response_bytes <= 0 or max_sqlite_bytes <= 0:
        raise Refusal('positive_budget_required')
    if not re.fullmatch('[0-9a-f]{64}', expected_sha256):
        raise Refusal('expected_sha256_required')
    command = [str(binary), '--snapshot', str(snapshot), '--staging-dir', str(staging_dir),
               '--max-sqlite-bytes', str(max_sqlite_bytes), '--expected-sha256', expected_sha256]
    started = time.monotonic()
    deadline = started + timeout
    p = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, start_new_session=True,
                         env={**os.environ, 'PATH': ''}, bufsize=0)
    received = 0
    pending = bytearray()
    def ready(stream, writing=False):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise Refusal('mcp_timeout')
        read, write, _ = select.select([] if writing else [stream], [stream] if writing else [], [], remaining)
        if not (read or write):
            raise Refusal('mcp_timeout')
    def send(value):
        data = json.dumps(value, separators=(',', ':')).encode() + b'\n'
        while data:
            ready(p.stdin, True)
            data = data[os.write(p.stdin.fileno(), data):]
    def response(identifier):
        nonlocal received
        while b'\n' not in pending:
            ready(p.stdout)
            chunk = os.read(p.stdout.fileno(), min(65536, max_response_bytes - received + 1))
            if not chunk:
                raise Refusal('mcp_eof')
            received += len(chunk)
            if received > max_response_bytes:
                raise Refusal('mcp_response_budget')
            pending.extend(chunk)
        line, _, tail = pending.partition(b'\n')
        pending[:] = tail
        arrived = time.monotonic()
        try:
            value = json.loads(line)
        except (ValueError, UnicodeError, RecursionError):
            raise Refusal('mcp_invalid_json') from None
        if (not isinstance(value, dict) or value.get('jsonrpc') != '2.0'
                or type(value.get('id')) is not int or value['id'] != identifier
                or 'error' in value or not isinstance(value.get('result'), dict)):
            raise Refusal('mcp_response_error')
        return value['result'], arrived
    try:
        os.set_blocking(p.stdin.fileno(), False)
        os.set_blocking(p.stdout.fileno(), False)
        send({'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {
            'protocolVersion': '2025-11-25', 'capabilities': {},
            'clientInfo': {'name': 'bslens-measure', 'version': '1'}}})
        initialized, init_at = response(1)
        if initialized.get('protocolVersion') != '2025-11-25':
            raise Refusal('mcp_protocol')
        send({'jsonrpc': '2.0', 'method': 'notifications/initialized'})
        requested = time.monotonic()
        send({'jsonrpc': '2.0', 'id': 2, 'method': 'tools/call',
              'params': {'name': 'metadata', 'arguments': {}}})
        useful, useful_at = response(2)
        content = useful.get('structuredContent')
        if (useful.get('isError') is not False or not isinstance(content, dict)
                or not isinstance(content.get('snapshot'), dict)
                or not isinstance(content.get('data'), dict) or content['data'].get('status') != 'ok'):
            raise Refusal('mcp_tool_error')
        return dict(contract='measurement-first-tool-v1', tool='metadata',
                    start_to_initialize_seconds=init_at-started,
                    request_to_first_useful_seconds=useful_at-requested,
                    start_to_first_useful_seconds=useful_at-started,
                    clock='monotonic', received_bytes=received, output_discarded=True,
                    cache_state='uncontrolled', git_requested=False)
    finally:
        # Завершение сервиса не входит в latency; не ждать EOF от неисправного сервиса.
        try:
            os.killpg(p.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        p.wait()
        p.stdin.close()
        p.stdout.close()


class SafeParser(argparse.ArgumentParser):
    def error(self, message):
        raise Refusal('arguments')


def main():
    parser = SafeParser(description=__doc__)
    sub = parser.add_subparsers(dest='mode', required=True)
    p = sub.add_parser('preflight'); p.add_argument('--repo', required=True); p.add_argument('--commit', required=True)
    p.add_argument('--max-entries', type=int, default=200000); p.add_argument('--max-xml-scan', type=int, default=1024*MIB)
    p.add_argument('--seconds', type=float, default=300)
    p = sub.add_parser('pair'); p.add_argument('--snapshot', required=True); p.add_argument('--expected-sha256', required=True)
    p.add_argument('--commit', required=True); p.add_argument('--workdir', required=True); p.add_argument('--max-bytes', type=int, default=1024*MIB)
    p = sub.add_parser('time'); p.add_argument('--phase', choices=['build','open','verify','first-tool'], required=True)
    p.add_argument('--timeout', type=float, default=300); p.add_argument('command', nargs=argparse.REMAINDER)
    p = sub.add_parser('first-tool'); p.add_argument('--binary', required=True)
    p.add_argument('--snapshot', required=True); p.add_argument('--staging-dir', required=True)
    p.add_argument('--max-sqlite-bytes', type=int, required=True); p.add_argument('--expected-sha256', required=True)
    p.add_argument('--timeout', type=float, default=30); p.add_argument('--max-response-bytes', type=int, default=MIB)
    args = vars(parser.parse_args()); mode = args.pop('mode')
    if platform.system() != 'Linux' or platform.machine() != 'x86_64':
        raise Refusal('linux_x86_64_required')
    if mode == 'time' and args['command'][:1] == ['--']:
        args['command'].pop(0)
    result = {'preflight': preflight, 'pair': pair, 'time': timed, 'first-tool': first_tool}[mode](**args)
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    try:
        main()
    except Refusal as error:
        print(json.dumps({'error': str(error)}))
        raise SystemExit(1)
    except (OSError, ValueError, sqlite3.Error, subprocess.SubprocessError):
        # Ни exception text, ни пути/argv/содержимое не публикуются.
        print('{"error":"measurement_refused","details":"check_local_prerequisites_and_budgets"}')
        raise SystemExit(1)
