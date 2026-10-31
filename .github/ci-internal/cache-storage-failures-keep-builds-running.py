import contextlib
import hashlib
import http.server
import json
import os
import pathlib
import re
import shlex
import shutil
import subprocess
import tempfile
import textwrap
import threading

root = pathlib.Path.cwd()
workflow = (root / '.github/workflows/ci.yml').read_text(encoding='utf-8')

def body(name):
    step = next(s for s in workflow.split('      - name: ')[1:] if s.splitlines()[0] == name)
    return textwrap.dedent(re.search(r'^        run: (?:&[\w-]+ )?\|\n((?:          .*\n|\n)+)', step, re.M)[1])

install, startup = body('Compile cache').split('if [ "$CACHE_RESTORE_OUTCOME"', 1)
startup = 'if [ "$CACHE_RESTORE_OUTCOME"' + startup

def run(command, env, cwd=root):
    return subprocess.run(command, env=env, cwd=cwd, capture_output=True, text=True)

def shell(source, env):
    return run(['bash', '-e', '-o', 'pipefail', '-c', source], env)

class Storage(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.server.requests.append(self.path)
        index = min(len(self.server.requests) - 1, len(self.server.responses) - 1)
        status, payload, headers = self.server.responses[index]
        self.send_response(status)
        self.send_header('Content-Length', str(len(payload)))
        for name, value in headers.items():
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(payload)

    def do_POST(self):
        self.rfile.read(int(self.headers.get('Content-Length', '0')))
        self.server.requests.append(self.path)
        response = b'{"code":"unavailable","msg":"Egress is over the account limit"}'
        self.send_response(503)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(response)))
        self.end_headers()
        self.wfile.write(response)

    def log_message(self, *args):
        pass

@contextlib.contextmanager
def storage():
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Storage)
    server.requests = []
    thread = threading.Thread(target=server.serve_forever)
    thread.start()
    try:
        yield server
    finally:
        server.shutdown()
        thread.join()
        server.server_close()

def environment(directory):
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(('SCCACHE_', 'ACTIONS_', 'RUSTC_', 'CARGO_'))
           and k not in ('GITHUB_API_URL', 'GITHUB_SERVER_URL')}
    return env | {'RUNNER_TEMP': str(directory), 'GITHUB_ENV': str(directory / 'env'),
                  'GITHUB_PATH': str(directory / 'paths'), 'SCCACHE_SERVER_UDS': str(directory / 'daemon.sock'),
                  'SCCACHE_CONF': str(directory / 'no-config'), 'SCCACHE_CACHED_CONF': str(directory / 'cached-config'),
                  'SCCACHE_DIR': str(directory / 'maestro-sccache'), 'CARGO_TARGET_DIR': str(directory / 'target'),
                  'GITHUB_OUTPUT': str(directory / 'outputs'), 'CACHE_RESTORE_OUTCOME': 'success',
                  'CARGO_INCREMENTAL': '0', 'CARGO_BUILD_JOBS': '3'}

@contextlib.contextmanager
def daemon(directory, outcome='success', matched='false'):
    env = environment(directory) | {'bin': str(binary.parent), 'PATH': str(binary.parent) + os.pathsep + os.environ['PATH'],
                                    'CACHE_RESTORE_OUTCOME': outcome, 'CACHE_RESTORE_MATCHED': matched}
    try:
        started = shell(startup, env)
        assert started.returncode == 0, (started.returncode, started.stdout, started.stderr)
        (directory / 'restore-message').write_text(started.stdout, encoding='utf-8')
        env.update(line.split('=', 1) for line in (directory / 'env').read_text(encoding='utf-8').splitlines())
        yield env
    finally:
        run([str(binary), '--stop-server'], env)

def compile_sample(env, fixture='sample', lint=False):
    command = ['cargo', 'clippy' if lint else 'build', '--locked', '--manifest-path', str(root / f'fixtures/{fixture}/Cargo.toml')]
    if lint:
        command += ['--', '-D', 'warnings']
    if os.environ.get('MAESTRO_LOCAL_CAPPED'):
        command.insert(0, os.path.expanduser('~/.local/bin/capped'))
    return run(command, env)

def stats(env, hits, misses):
    result = shell(body('Compile cache statistics'), env)
    assert result.returncode == 0, result.stdout + result.stderr
    assert re.search(rf'^Cache hits\s+{hits}$', result.stdout, re.M), result.stdout
    assert re.search(rf'^Cache misses\s+{misses}$', result.stdout, re.M), result.stdout
    return result.stdout

def printed_log(env, output):
    log = pathlib.Path(env['RUNNER_TEMP'], 'sccache-error.log').read_text(encoding='utf-8')
    assert '503' in log and 'Egress is over the account limit' in log
    marker = re.search(r'^::stop-commands::([0-9a-f]{64})$', output, re.M)
    assert marker, 'Missing Actions command-suppression marker'
    token = marker[1]
    assert f'::stop-commands::{token}\n{log}\n::{token}::\n' in output

def snapshot_keys(**changes):
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        env = environment(directory) | {
            'CALLER_REPOSITORY': 'example/caller', 'CALLER_DIRECTORY': '.',
            'HOST_OS': 'Linux', 'HOST_ARCH': 'X64', 'CACHE_JOB': 'tests', 'CACHE_INDEX': '0',
            'CACHE_CALL': 'consumer', 'CACHE_COVERAGE': 'true', 'CACHE_ENABLED': 'true', 'CACHE_WARM': 'false',
            'SHARD': '1', 'GITHUB_RUN_ID': '10', 'GITHUB_RUN_ATTEMPT': '1', 'CACHE_TARGET': '',
            'GITHUB_EVENT_NAME': 'push', 'GITHUB_REF': 'refs/heads/main', 'CACHE_DEFAULT_BRANCH': 'main'}
        compiler = changes.pop('compiler', None)
        if compiler:
            source = directory / 'source'
            source.write_text('#!/bin/sh\nprintf "%s\\n" "different compiler"\n', encoding='utf-8')
            subprocess.run(['install', '-m', '0700', str(source), str(directory / 'rustc')], check=True)
            env['PATH'] = str(directory) + os.pathsep + os.environ['PATH']
        result = shell(body('Compile cache identity'), env | changes)
        assert result.returncode == 0, result.stderr
        return dict(line.split('=', 1) for line in (directory / 'outputs').read_text(encoding='utf-8').splitlines())

def snapshot_key_matches_consuming_shape():
    coverage = snapshot_keys()['prefix']
    assert snapshot_keys(CACHE_WARM='true', SHARD='warm', CACHE_COVERAGE='false')['prefix'] == coverage
    for shard in map(str, range(1, 17)):
        assert snapshot_keys(SHARD=shard)['prefix'] == coverage
    assert snapshot_keys(CACHE_JOB='test-build', SHARD='')['prefix'] == coverage
    plain = snapshot_keys(CACHE_COVERAGE='false')['prefix']
    assert snapshot_keys(CACHE_JOB='doctests', SHARD='doctests')['prefix'] == plain
    assert snapshot_keys(CACHE_JOB='test-build', CACHE_COVERAGE='false', SHARD='')['prefix'] == plain
    assert plain != coverage and snapshot_keys(SHARD='doctests')['prefix'] == plain
    assert snapshot_keys(CACHE_COVERAGE='', SHARD='')['prefix'] == plain
    for event, collect in [('pull_request', 'false'), ('push', 'true'), ('merge_group', 'true')]:
        for shard in [*map(str, range(1, 17)), 'doctests']:
            observed = snapshot_keys(CACHE_ENABLED='false', CACHE_COVERAGE=collect, SHARD=shard)['prefix']
            assert observed == plain, (event, shard, 'undeclared tests must use the plain snapshot')
    assert 'CACHE_ENABLED: $' + '{{ needs.declaration.outputs.enabled }}' in workflow
    shapes = [snapshot_keys(CACHE_JOB=job)['prefix'] for job in
              ['quality', 'docs', 'checks', 'browser', 'browser-tests']]
    assert len(set(shapes + [plain, coverage])) == 7
    assert snapshot_keys(CACHE_JOB='targets', CACHE_TARGET='aarch64-unknown-linux-gnu')['prefix'] != snapshot_keys(CACHE_JOB='targets', CACHE_TARGET='x86_64-unknown-linux-gnu')['prefix']

    apple = snapshot_keys(CACHE_JOB='targets', CACHE_TARGET='aarch64-apple-darwin', HOST_OS='macOS', HOST_ARCH='ARM64')['prefix']
    assert apple == snapshot_keys(CACHE_JOB='targets', CACHE_TARGET='aarch64-apple-darwin', HOST_OS='macOS', HOST_ARCH='ARM64')['prefix']
    assert apple != snapshot_keys(CACHE_JOB='targets', CACHE_TARGET='aarch64-apple-darwin', HOST_OS='Linux', HOST_ARCH='ARM64')['prefix']
    assert apple != snapshot_keys(CACHE_JOB='targets', CACHE_TARGET='x86_64-apple-darwin', HOST_OS='macOS', HOST_ARCH='ARM64')['prefix']

def snapshot_key_preserves_caller_isolation():
    base = snapshot_keys()['prefix']
    for change in [dict(CALLER_REPOSITORY='other/caller'), dict(CALLER_DIRECTORY='nested'),
                   dict(HOST_OS='Windows'), dict(HOST_ARCH='ARM64'), dict(compiler=True)]:
        assert snapshot_keys(**change)['prefix'] != base, change
    for change in [dict(CACHE_CALL='warm'), dict(TEST_PACKAGES='["other"]'), dict(SHARD='16')]:
        assert snapshot_keys(**change)['prefix'] == base, change
    key = snapshot_keys(CALLER_DIRECTORY='space and é/child')['key']
    assert key.isascii() and len(key) < 512 and 'space' not in key

def snapshot_only_default_branch_writers_save():
    plain_prefix = snapshot_keys(CACHE_COVERAGE='false')['prefix']
    jobs = ['quality', 'docs', 'test-build', 'test-prepare', 'tests', 'checks', 'targets', 'browser', 'browser-tests']
    for job in jobs:
        for event in ['pull_request', 'merge_group', 'workflow_dispatch', 'push']:
            for ref in ['refs/pull/119/merge', 'refs/heads/feature', 'refs/heads/main', 'refs/tags/main']:
                observed = snapshot_keys(CACHE_JOB=job, GITHUB_EVENT_NAME=event, GITHUB_REF=ref)
                expected = 'true' if event == 'push' and ref == 'refs/heads/main' else 'false'
                assert observed.get('writer') == expected, (job, event, ref, observed)
    assert snapshot_keys(GITHUB_EVENT_NAME='pull_request', TEST_EVENT='push')['writer'] == 'false'
    assert snapshot_keys(GITHUB_EVENT_NAME='merge_group', TEST_EVENT='push')['writer'] == 'false'
    assert snapshot_keys(CACHE_DEFAULT_BRANCH='trunk', GITHUB_REF='refs/heads/trunk')['writer'] == 'true'
    assert snapshot_keys(CACHE_DEFAULT_BRANCH='trunk')['writer'] == 'false'
    for enabled, coverage, expected in [('true', 'true', ['1', 'doctests']),
                                        ('true', 'false', ['1']), ('false', 'true', ['1'])]:
        writers = [shard for shard in [*map(str, range(1, 17)), 'doctests']
                   if snapshot_keys(CACHE_ENABLED=enabled, CACHE_COVERAGE=coverage, SHARD=shard)['writer'] == 'true']
        assert writers == expected, (enabled, coverage, writers)
        prefixes = [snapshot_keys(CACHE_ENABLED=enabled, CACHE_COVERAGE=coverage, SHARD=shard)['prefix']
                    for shard in writers]
        assert len(prefixes) == len(set(prefixes)), 'at most one writer per actual shape'
    for event in ['pull_request', 'merge_group', 'workflow_dispatch', 'push']:
        for ref in ['refs/pull/119/merge', 'refs/heads/feature', 'refs/heads/main', 'refs/tags/main']:
            warm_doctests = snapshot_keys(CACHE_JOB='doctests', CACHE_WARM='true', SHARD='doctests',
                                          CACHE_ENABLED='false', CACHE_COVERAGE='false',
                                          GITHUB_EVENT_NAME=event, GITHUB_REF=ref)
            expected = 'true' if event == 'push' and ref == 'refs/heads/main' else 'false'
            assert warm_doctests['writer'] == expected, (event, ref)
            assert warm_doctests['prefix'] == plain_prefix, (event, ref)
    assert snapshot_keys(CACHE_WARM='true', SHARD='warm')['writer'] == 'true'
    assert snapshot_keys(CACHE_WARM='true', SHARD='warm', GITHUB_EVENT_NAME='pull_request')['writer'] == 'false'
    assert 'CACHE_DEFAULT_BRANCH: $' + '{{ github.event.repository.default_branch }}' in workflow

def snapshot_save_keys_are_run_unique():
    base = snapshot_keys()
    for change in [dict(GITHUB_RUN_ID='11'), dict(GITHUB_RUN_ATTEMPT='2'), dict(CACHE_INDEX='1'), dict(CACHE_CALL='warm')]:
        observed = snapshot_keys(**change)
        assert observed['key'] != base['key'] and observed['prefix'] == base['prefix'], change
    warm = snapshot_keys(CACHE_WARM='true', SHARD='warm', CACHE_CALL='warm')
    assert warm['prefix'] == base['prefix'] and warm['key'] != base['key']
    assert len({snapshot_keys(CACHE_INDEX=str(index))['key'] for index in range(16)}) == 16
    # Independent canonical-JSON digest pins field order and ASCII encoding.
    compiler = subprocess.check_output(['rustc', '-vV'], text=True)
    identity = json.dumps(['example/caller', '.', 'Linux', 'X64', compiler, 'tests-coverage'], separators=(',', ':'), ensure_ascii=True)
    assert base['prefix'] == 'maestro-compile-v1-' + hashlib.sha256(identity.encode('utf-8')).hexdigest() + '-'

def snapshot_size_bound_skips_only_ineligible_saves():
    for size in [0, 1073741823, 1073741824, 1073741825, None]:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            source = directory / 'source'
            source.write_text('#!/bin/sh\nexit 0\n', encoding='utf-8')
            subprocess.run(['install', '-m', '0700', str(source), str(directory / 'sccache')], check=True)
            cache = directory / 'cache'
            cache.mkdir()
            if size:
                with (cache / 'payload').open('wb') as payload:
                    payload.truncate(size)
            if size is None:
                cache.rmdir()
                cache.write_text('not a directory', encoding='utf-8')
            env = environment(directory) | {'PATH': str(directory) + os.pathsep + os.environ['PATH'], 'SCCACHE_DIR': str(cache)}
            result = shell(body('Seal compile cache'), env)
            assert result.returncode == 0, result.stderr
            output = (directory / 'outputs').read_text(encoding='utf-8') if (directory / 'outputs').exists() else ''
            assert ('eligible=true' in output) == (size in [1073741823, 1073741824])
            if size is None:
                assert result.stdout == 'Compile cache snapshot save skipped: cache inspection failed\n', result.stdout
            else:
                expected = f'Compile cache snapshot: {int(bool(size))} files, {size} bytes\n'
                if size == 0:
                    expected += 'Compile cache snapshot save skipped: empty cache\n'
                elif size > 1073741824:
                    expected += 'Compile cache snapshot save skipped: exceeds 1073741824 bytes\n'
                assert result.stdout == expected, result.stdout
    # Instrument the actual inspection body, not a second size implementation.
    script = body('Seal compile cache').split("<<'PYTHON'\n", 1)[1].rsplit('\nPYTHON', 1)[0]
    from unittest import mock
    for count in [16, 32, 64]:
        with tempfile.TemporaryDirectory() as temporary:
            cache = pathlib.Path(temporary)
            for index in range(count):
                (cache / str(index)).write_bytes(b'x')
            original = pathlib.Path.lstat
            visits = []
            def tracked(path):
                visits.append(path)
                return original(path)
            with mock.patch.dict(os.environ, {'SCCACHE_DIR': temporary, 'GITHUB_OUTPUT': str(cache.parent / (cache.name + '.output'))}), mock.patch.object(pathlib.Path, 'lstat', tracked):
                exec(compile(script, 'snapshot inspection', 'exec'), {})
            assert len(visits) <= count * 2
            repeated_scan = sum(1 for _ in range(count) for _ in cache.iterdir())
            assert repeated_scan > count * 2
            pathlib.Path(cache.parent, cache.name + '.output').unlink()

def snapshot_seal_stops_before_measurement():
    for status in [0, 7]:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            source = directory / 'source'
            # Successful stop creates the payload, so enumeration must follow it.
            source.write_text('#!/bin/sh\ntest "$1" = --stop-server\nif [ "$STOP_STATUS" = 0 ]; then mkdir "$SCCACHE_DIR"; printf sealed > "$SCCACHE_DIR/payload"; fi\nexit "$STOP_STATUS"\n', encoding='utf-8')
            subprocess.run(['install', '-m', '0700', str(source), str(directory / 'sccache')], check=True)
            env = environment(directory) | {'PATH': str(directory) + os.pathsep + os.environ['PATH'], 'STOP_STATUS': str(status)}
            result = shell(body('Seal compile cache'), env)
            assert result.returncode == 0, result.stderr
            if status:
                assert result.stdout == 'Compile cache snapshot save skipped: server stop failed\n'
                assert not (directory / 'outputs').exists()
            else:
                assert result.stdout == 'Compile cache snapshot: 1 files, 6 bytes\n'
                assert (directory / 'outputs').read_text(encoding='utf-8') == 'eligible=true\n'

def artifact(env):
    return next(pathlib.Path(env['CARGO_TARGET_DIR'], 'debug/deps').glob('libsample-*.rlib')).read_bytes()

def snapshot_native_entries_are_reused():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        with daemon(directory) as env:
            result = compile_sample(env)
            assert result.returncode == 0, result.stderr
            expected = artifact(env)
            stats(env, '0', '1')
            sealed = shell(body('Seal compile cache'), env)
            assert sealed.returncode == 0 and 'eligible=true' in (directory / 'outputs').read_text(encoding='utf-8'), sealed.stdout + sealed.stderr
        cache = pathlib.Path(env['SCCACHE_DIR'])
        snapshot = directory / 'snapshot'
        shutil.copytree(cache, snapshot)
        shutil.rmtree(cache)
        shutil.rmtree(directory / 'target')
        shutil.copytree(snapshot, cache)
        with daemon(directory) as env:
            result = compile_sample(env)
            assert result.returncode == 0, result.stderr
            assert artifact(env) == expected
            assert 'Local disk:' in stats(env, '1', '0')
            assert 'service=ghac' not in (directory / 'sccache-error.log').read_text(encoding='utf-8')
        return expected

def snapshot_missing_entries_build_uncached():
    with tempfile.TemporaryDirectory() as temporary:
        with daemon(pathlib.Path(temporary)) as env:
            result = compile_sample(env)
            assert result.returncode == 0 and artifact(env) == warm_artifact, result.stderr
            stats(env, '0', '1')

def snapshot_partial_restore_is_discarded():
    for outcome, matched in [('success', 'false'), ('success', 'true'), ('failure', 'false'), ('success', '')]:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            with daemon(directory) as env:
                built = compile_sample(env)
                assert built.returncode == 0, built.stdout + built.stderr
            shutil.rmtree(directory / 'target')
            with daemon(directory, outcome, matched) as env:
                result = compile_sample(env)
                assert result.returncode == 0 and artifact(env) == warm_artifact, result.stderr
                kept = outcome == 'success' and matched in ['true', 'false']
                stats(env, '1' if kept else '0', '0' if kept else '1')
                if not kept:
                    assert 'Compile cache snapshot restore failed; using an empty local cache' in (directory / 'restore-message').read_text(encoding='utf-8')

def action(role, directory, server):
    env = environment(directory) | {
        'ACTIONS_CACHE_URL': f'http://127.0.0.1:{server.server_port}/',
        'ACTIONS_RESULTS_URL': f'http://127.0.0.1:{server.server_port}/',
        'ACTIONS_CACHE_SERVICE_V2': 'true', 'ACTIONS_RUNTIME_TOKEN': 'loopback-test-only',
        'GITHUB_REF': 'refs/heads/main', 'GITHUB_REPOSITORY': 'example/caller',
        'GITHUB_WORKSPACE': str(directory), 'INPUT_KEY': 'snapshot-outage',
        'INPUT_PATH': str(directory / 'maestro-sccache'), 'INPUT_FAIL-ON-CACHE-MISS': 'false'}
    (directory / 'outputs').touch()
    result = run(['node', str(action_directory / (role + '.js'))], env)
    assert result.returncode == 0 and '503' in result.stdout + result.stderr, result.stdout + result.stderr
    print(f'{role}: ' + result.stdout, flush=True)
    endpoint = 'GetCacheEntryDownloadURL' if role == 'restore' else 'CreateCacheEntry'
    assert any(path.endswith('/' + endpoint) for path in server.requests), server.requests
    return dict(line.split('=', 1) for line in (directory / 'outputs').read_text(encoding='utf-8').splitlines() if '=' in line)

def snapshot_restore_outage_builds_uncached():
    with tempfile.TemporaryDirectory() as temporary, storage() as server:
        directory = pathlib.Path(temporary)
        outputs = action('restore', directory, server)
        with daemon(directory, matched=outputs.get('cache-hit', '')) as env:
            result = compile_sample(env)
            assert result.returncode == 0 and artifact(env) == warm_artifact, result.stderr
            assert 'Local disk:' in stats(env, '0', '1')
            assert 'service=ghac' not in (directory / 'sccache-error.log').read_text(encoding='utf-8')

def snapshot_save_outage_keeps_build_success():
    with tempfile.TemporaryDirectory() as temporary, storage() as server:
        directory = pathlib.Path(temporary)
        with daemon(directory) as env:
            built = compile_sample(env)
            assert built.returncode == 0, built.stderr
            before = artifact(env)
            sealed = shell(body('Seal compile cache'), env)
            assert sealed.returncode == 0, sealed.stdout + sealed.stderr
        action('save', directory, server)
        assert artifact(env) == before == warm_artifact

def snapshot_restart_preserves_isolated_local_policy():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        with daemon(directory) as env:
            isolated = body('Isolated tests')
            setup = isolated[isolated.index('cargo_home='):isolated.index('cargo fetch --locked')]
            # Only the test daemon address is injected after isolation, never cache policy.
            command = setup + '\nrun_isolated env\nrun_isolated env SCCACHE_SERVER_UDS="$SCCACHE_SERVER_UDS" cargo build --locked --manifest-path "$SAMPLE_MANIFEST" --target-dir "$CARGO_TARGET_DIR"\n'
            isolated_env = env | {'RUSTUP_TOOLCHAIN': subprocess.check_output(['rustup', 'show', 'active-toolchain'], text=True).split()[0],
                                  'SAMPLE_MANIFEST': str(root / 'fixtures/sample/Cargo.toml'),
                                  'OPENAI_API_KEY': 'not-a-credential-canary'}
            warm = shell(command, isolated_env)
            assert warm.returncode == 0, warm.stderr
            stopped = run([str(binary), '--stop-server'], env)
            assert stopped.returncode == 0, stopped.stdout + stopped.stderr
            shutil.rmtree(directory / 'target')
            result = shell(command, isolated_env)
            assert result.returncode == 0, result.stdout + result.stderr
            assert 'OPENAI_API_KEY=' not in result.stdout and 'ACTIONS_RUNTIME_TOKEN=' not in result.stdout
            for key in ['CARGO_BUILD_RUSTC_WRAPPER', 'SCCACHE_DIR', 'SCCACHE_CACHE_SIZE', 'SCCACHE_LOCAL_RW_MODE',
                        'SCCACHE_GHA_ENABLED', 'SCCACHE_SKIP_CACHE_CHECK', 'SCCACHE_ERROR_LOG', 'SCCACHE_LOG']:
                assert f'{key}={env[key]}\n' in result.stdout, key
            assert artifact(env) == warm_artifact
            stats(env, '1', '0')
            assert 'service=ghac' not in (directory / 'sccache-error.log').read_text(encoding='utf-8')

def cache_statistics_print_error_log():
    for exists, status, errors in [(True, 0, 0), (False, 0, 1), (True, 7, 120), (True, 0, 120)]:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            source = directory / 'source'
            source.write_text('#!/bin/sh\nprintf "Cache hits 1\\nCache write errors %s\\n" "$WRITE_ERRORS"\nexit "$STATS_STATUS"\n', encoding='utf-8')
            subprocess.run(['install', '-m', '0700', str(source), str(directory / 'sccache')], check=True)
            log = '503 Egress is over the account limit\n::error::literal cache response'
            if exists:
                (directory / 'sccache-error.log').write_text(log, encoding='utf-8')
            env = environment(directory) | {'PATH': str(directory) + os.pathsep + os.environ['PATH'],
                                             'CARGO_BUILD_RUSTC_WRAPPER': 'sccache', 'STATS_STATUS': str(status), 'WRITE_ERRORS': str(errors)}
            result = shell(body('Compile cache statistics'), env)
            assert result.returncode == status, result.stderr
            assert result.stdout.startswith('Cache hits 1\n')
            if status == 0:
                assert (directory / 'outputs').read_text(encoding='utf-8') == f'write_errors={str(errors > 0).lower()}\n'
            else:
                assert not (directory / 'outputs').exists()
            if exists and status == 0:
                printed_log(env, result.stdout)
            else:
                assert 'stop-commands' not in result.stdout and 'Egress' not in result.stdout

def cache_error_artifacts_are_selected_and_scoped():
    jobs = dict(re.findall(r'^  ([\w-]+):\n(.*?)(?=^  [\w-]+:\n|\Z)',
                           workflow.split('\njobs:\n', 1)[1], re.M | re.S))
    owners = {'quality', 'docs', 'test-build', 'test-prepare', 'tests', 'checks', 'targets', 'browser', 'browser-tests'}
    uploads = {name: job.split('      - name: Upload compile cache errors\n')[1].split('      - name:', 1)[0]
               for name, job in jobs.items() if '      - name: Upload compile cache errors\n' in job}
    assert set(uploads) == owners
    condition = re.search(r'if: &sccache-artifact-condition (.*)', uploads['quality'])[1]
    assert condition == ("always() && (steps.cache-statistics.outputs.write_errors == 'true' || "
                         "(inputs.concurrency-key == 'maestro-proof' && steps.cache-statistics.outcome == 'success'))")
    expression = condition.replace('always()', 'True').replace('&&', 'and').replace('||', 'or')
    for errors in ['false', 'true']:
        for call in ['ordinary', 'maestro-selected', 'maestro-proof']:
            for outcome in ['success', 'failure', 'skipped']:
                actual = expression.replace('steps.cache-statistics.outputs.write_errors', repr(errors))
                actual = actual.replace('inputs.concurrency-key', repr(call)).replace('steps.cache-statistics.outcome', repr(outcome))
                assert eval(actual) == (errors == 'true' or (call == 'maestro-proof' and outcome == 'success'))
    scope = "compile-cache-errors-$" + "{{ inputs.concurrency-key || 'root' }}-$" + "{{ github.job }}-$" + "{{ strategy.job-index || 0 }}-$" + "{{ github.run_attempt }}"
    assert 'name: ' + scope in uploads['quality']
    assert 'path: $' + '{{ runner.temp }}/sccache-error.log' in uploads['quality']
    assert 'if-no-files-found: error' in uploads['quality']
    for name in owners - {'quality'}:
        assert 'if: *sccache-artifact-condition' in uploads[name], name
        assert 'with: *sccache-artifact' in uploads[name], name
    assert all('uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02' in step for step in uploads.values())

def cache_outage_keeps_compiler_failure():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        with daemon(directory) as env:
            result = compile_sample(env, 'clippy-warning', lint=True)
            assert result.returncode != 0 and 'error:' in result.stderr, result.stdout + result.stderr
            # Clippy can reject the crate before a cacheable compilation occurs.
            built = compile_sample(env)
            assert built.returncode == 0, built.stderr
            stats(env, '0', '[1-9][0-9]*')

def cache_wrapper_is_cargo_configuration():
    # Coverage owns RUSTC_WRAPPER, so the cache travels as cargo configuration.
    for step in ['Compile cache', 'Compile cache statistics', 'Isolated tests']:
        assert not re.search(r'\bRUSTC_WRAPPER\b', body(step)), step
    assert 'echo CARGO_BUILD_RUSTC_WRAPPER=sccache;' in body('Compile cache')
    assert 'test "$CARGO_BUILD_RUSTC_WRAPPER" = sccache' in body('Compile cache statistics')
    assert 'CARGO_BUILD_RUSTC_WRAPPER="${CARGO_BUILD_RUSTC_WRAPPER-}"' in body('Isolated tests')

def builds_without_the_coverage_wrapper_keep_the_cache():
    # A build a test starts drops RUSTC_WRAPPER to stay uninstrumented and must still reach the cache.
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        with daemon(directory) as env:
            shown = run(['cargo', 'llvm-cov', 'show-env', '--sh'], env, root / 'fixtures/sample')
            assert shown.returncode == 0, shown.stderr
            coverage = env | dict(shlex.split(line)[1].split('=', 1)
                                  for line in shown.stdout.splitlines() if line.startswith('export '))
            started = {key: value for key, value in coverage.items() if key != 'RUSTC_WRAPPER'}
            for build, instrumented, misses in [(coverage, True, '1'), (started, False, '2')]:
                shutil.rmtree(directory / 'target', ignore_errors=True)
                result = compile_sample(build)
                assert result.returncode == 0, result.stderr
                rlib = next((directory / 'target/debug/deps').glob('libsample-*.rlib')).read_bytes()
                assert (b'__llvm_covfun' in rlib) == instrumented, instrumented
                stats(env, '0', misses)

def target_check_adds_no_compile_wrapper():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shim = directory / 'shim'
        shim.mkdir()
        source = directory / 'source'
        source.write_text('#!/usr/bin/env python3\nimport json, os, pathlib, sys\n'
                          'with open(os.environ["TRACE"], "a") as output:\n'
                          '    output.write(json.dumps([pathlib.Path(sys.argv[0]).name, sys.argv[1:], '
                          '{key: os.environ[key] for key in ["RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER"] if key in os.environ}]) + "\\n")\n',
                          encoding='utf-8')
        for tool in ['cargo', 'rustup']:
            subprocess.run(['install', '-m', '0700', str(source), str(shim / tool)], check=True)
        env = environment(directory) | {'PATH': str(shim) + os.pathsep + os.environ['PATH'],
                                        'TARGET': 'x86_64-pc-windows-msvc', 'TRACE': str(directory / 'trace')}
        command = body('Check target')
        for wrapper in [{}, {'CARGO_BUILD_RUSTC_WRAPPER': 'sccache'}]:
            (directory / 'trace').write_text('', encoding='utf-8')
            result = shell(command, env | wrapper)
            assert result.returncode == 0, result.stderr
            calls = [json.loads(line) for line in (directory / 'trace').read_text(encoding='utf-8').splitlines()]
            assert calls == [
                ['rustup', ['target', 'add', 'x86_64-pc-windows-msvc'], wrapper],
                ['rustup', ['component', 'add', 'clippy'], wrapper],
                ['cargo', ['clippy', '--workspace', '--all-targets', '--locked', '--target',
                           'x86_64-pc-windows-msvc', '--', '-D', 'warnings'], wrapper]]

def cache_install_selects_pinned_host_asset():
    hosts = [('Linux', 'x86_64', 'x86_64-unknown-linux-musl'),
             ('Linux', 'aarch64', 'aarch64-unknown-linux-musl'),
             ('Darwin', 'arm64', 'aarch64-apple-darwin')]
    installer = body('Compile cache')
    assert "hashlib.file_digest(source, 'sha256')" in installer
    for forbidden in ['mise', 'yq', 'CARGO_TARGET_DIR']:
        assert forbidden not in installer
    with tempfile.TemporaryDirectory(prefix='cache install space ') as temporary:
        archives = pathlib.Path(temporary)
        for host, arch, platform in hosts:
            archive = archives / (platform + '.tar.gz')
            url = f'https://github.com/mozilla/sccache/releases/download/v0.18.0/sccache-v0.18.0-{platform}.tar.gz'
            subprocess.run(['curl', '--fail', '--location', url, '--output', str(archive)], check=True)
        for host, arch, platform in hosts + [('MINGW64_NT-10.0-26100', 'x86_64', ''),
                                             ('Darwin', 'x86_64', ''), ('FreeBSD', 'amd64', '')]:
            for corrupt in ([False, True] if platform else [False]):
                with tempfile.TemporaryDirectory(prefix='cache host space ') as trial:
                    directory = pathlib.Path(trial)
                    shim = directory / 'shim'
                    shim.mkdir()
                    # Host routing runs on Linux; substitute execution only after extracting verified release bytes.
                    tools = {
                        'uname': 'if [ "$1" = -s ]; then echo "$HOST"; else echo "$ARCH"; fi\n',
                        'curl': 'printf "%s\\n" "$@" > "$TRACE/download"\nwhile [ "$1" != --output ]; do shift; done\nif [ "$CORRUPT" = true ]; then printf corrupt > "$2"; else cp "$ARCHIVE" "$2"; fi\n',
                        'tar': 'printf "%s\\n" "$@" > "$TRACE/extract"\n"$REAL_TAR" "$@" || exit\nwhile [ "$1" != -C ]; do shift; done\nbin=$2\nprintf \'#!/bin/sh\\nprintf "%%s\\\\n" "$@" >> "$TRACE/server"\\n\' > "$bin/sccache"\nchmod +x "$bin/sccache"\n',
                        'mise': 'exit 99\n', 'yq': 'exit 99\n'}
                    for tool, content in tools.items():
                        source = directory / 'source'
                        source.write_text('#!/bin/sh\n' + content, encoding='utf-8')
                        subprocess.run(['install', '-m', '0700', str(source), str(shim / tool)], check=True)
                    env = environment(directory) | {
                        'HOST': host, 'ARCH': arch,
                        'CORRUPT': str(corrupt).lower(), 'ARCHIVE': str(archives / (platform + '.tar.gz')),
                        'TRACE': str(directory), 'TMPDIR': str(directory), 'CACHE_RESTORE_MATCHED': 'false',
                        'PATH': str(shim) + os.pathsep + os.environ['PATH'], 'REAL_TAR': shutil.which('tar')}
                    result = shell(installer, env)
                    if corrupt or not platform:
                        assert result.returncode != 0, f'{host} {arch} {corrupt}\n' + result.stdout + result.stderr
                        assert not (directory / 'extract').exists() and not (directory / 'server').exists()
                        assert not (directory / 'paths').exists() and not (directory / 'env').exists()
                        if not platform:
                            assert not (directory / 'download').exists()
                            assert result.stderr == 'Unsupported compile cache runner host\n'
                        continue
                    assert result.returncode == 0, (host, result.stderr)
                    name = f'sccache-v0.18.0-{platform}'
                    assert (directory / 'download').read_text(encoding='utf-8').splitlines() == [
                        '--fail', '--retry', '3', '--location', f'https://github.com/mozilla/sccache/releases/download/v0.18.0/{name}.tar.gz',
                        '--output', (directory / 'extract').read_text(encoding='utf-8').splitlines()[1]]
                    extraction = (directory / 'extract').read_text(encoding='utf-8').splitlines()
                    assert extraction[0] == '-xzf' and extraction[2] == '-C'
                    assert extraction[4:] == ['--strip-components=1', f'{name}/sccache']
                    assert (directory / 'server').read_text(encoding='utf-8') == '--start-server\n'
                    exported = dict(line.split('=', 1) for line in (directory / 'env').read_text(encoding='utf-8').splitlines())
                    assert exported['CARGO_BUILD_RUSTC_WRAPPER'] == 'sccache' and exported['CARGO_INCREMENTAL'] == '0'
                    assert 'CARGO_TARGET_DIR' not in exported
                    assert (directory / 'paths').read_text(encoding='utf-8') == extraction[3] + '\n'
                    assert exported['SCCACHE_DIR'] == str(directory / 'maestro-sccache')

def cache_installation_failure_stays_fatal():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shim = directory / 'shim'
        shim.mkdir()
        source = directory / 'source'
        trace = directory / 'extracted'
        for tool, content in [('curl', '#!/bin/sh\nwhile [ "$1" != --output ]; do shift; done\nprintf corrupt > "$2"\n'),
                              ('tar', '#!/bin/sh\ntouch ' + str(trace) + '\nexit 99\n')]:
            source.write_text(content, encoding='utf-8')
            subprocess.run(['install', '-m', '0700', str(source), str(shim / tool)], check=True)
        env = environment(directory) | {'PATH': str(shim) + os.pathsep + os.environ['PATH']}
        result = shell(body('Compile cache'), env)
        assert result.returncode != 0, result.stdout + result.stderr
        assert not trace.exists() and not (directory / 'daemon.sock').exists()
        assert not (directory / 'env').exists() and not (directory / 'paths').exists()

for test in [snapshot_key_matches_consuming_shape, snapshot_key_preserves_caller_isolation,
             snapshot_save_keys_are_run_unique, snapshot_only_default_branch_writers_save,
             snapshot_size_bound_skips_only_ineligible_saves,
             snapshot_seal_stops_before_measurement, cache_statistics_print_error_log,
             cache_error_artifacts_are_selected_and_scoped, cache_wrapper_is_cargo_configuration,
             target_check_adds_no_compile_wrapper, cache_install_selects_pinned_host_asset]:
    test()
    print('PASS ' + test.__name__, flush=True)

with tempfile.TemporaryDirectory() as temporary:
    directory = pathlib.Path(temporary)
    result = shell(install, environment(directory))
    assert result.returncode == 0, result.stdout + result.stderr
    binary = pathlib.Path((directory / 'paths').read_text(encoding='utf-8').strip()) / 'sccache'
    assert run([str(binary), '--version'], environment(directory)).stdout.strip() == 'sccache 0.18.0'
    action_directory = directory / 'actions'
    action_directory.mkdir()
    for role in ['restore', 'save']:
        result = run(['curl', '--fail', '--location',
                      f'https://raw.githubusercontent.com/actions/cache/0400d5f644dc74513175e3cd8d07132dd4860809/dist/{role}-only/index.js',
                      '--output', str(action_directory / (role + '.js'))], environment(directory))
        assert result.returncode == 0, result.stderr
    warm_artifact = snapshot_native_entries_are_reused()
    print('PASS snapshot_native_entries_are_reused', flush=True)
    for test in [snapshot_missing_entries_build_uncached, snapshot_partial_restore_is_discarded,
                 snapshot_restore_outage_builds_uncached, snapshot_save_outage_keeps_build_success,
                 snapshot_restart_preserves_isolated_local_policy, cache_outage_keeps_compiler_failure,
                 builds_without_the_coverage_wrapper_keep_the_cache, cache_installation_failure_stays_fatal]:
        test()
        print('PASS ' + test.__name__, flush=True)
