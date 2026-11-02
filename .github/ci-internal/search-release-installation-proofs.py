import ast
import json
import os
import pathlib
import re
import shlex
import shutil
import subprocess
import tempfile
import textwrap
import unittest
from collections import Counter
from typing import Any

workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
code = pathlib.Path('.github/ci-internal/caller-checks-at-workflow-seam.py').read_text(encoding='utf-8')
tree = ast.parse(textwrap.dedent(code))
tree.body = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == 'body']
exec(compile(tree, 'workflow body resolver', 'exec'))
body = globals()['body']

yq = shutil.which('yq') or ''
assert yq, 'Mike Farah yq is required to parse workflow controls'
subprocess.run([yq, '--version'], check=True)

def shfmt_install():
    command = body('Check tools')
    return '\n'.join(line for line in command.splitlines()
                     if not line.startswith('https://') or line.endswith(' shfmt'))

tool_directory = tempfile.TemporaryDirectory()
temporary = tool_directory.name
env = os.environ | {'RUNNER_TEMP': temporary, 'GITHUB_ENV': temporary + '/environment'}
subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', shfmt_install()], env=env, check=True)
tools = pathlib.Path(env['GITHUB_ENV']).read_text(encoding='utf-8').removeprefix('TOOLS=').strip()
shfmt = str(pathlib.Path(tools) / 'shfmt')
assert pathlib.Path(shfmt).is_file(), 'shfmt is required to parse workflow controls'
subprocess.run([shfmt, '--version'], check=True)

def syntax_nodes(value):
    if isinstance(value, dict):
        yield value
        for child in value.values():
            yield from syntax_nodes(child)
    elif isinstance(value, list):
        for child in value:
            yield from syntax_nodes(child)

control_consumers = {
    ('curl',): ('--retry', '--retry-all-errors', '--retry-delay', '--max-time'),
    ('cargo-nextest', 'nextest', 'run'): ('--retries',),
    ('cargo-llvm-cov', 'llvm-cov', 'nextest'): ('--retries',),
    ('timeout',): ('timeout',),
    ('sleep',): ('sleep',),
}
command_wrappers = {'run_isolated': 1, 'timeout': 2}

def command_name(word):
    def literal(part):
        if part['Type'] in ('Lit', 'SglQuoted'):
            return part['Value']
        if part['Type'] == 'DblQuoted':
            return ''.join(literal(child) for child in part.get('Parts', []))
        return '\0'
    name = ''.join(literal(part) for part in word.get('Parts', [])).rsplit('/', 1)[-1]
    return None if '\0' in name else name

def collect(files):
    found = []
    counts = Counter()
    def add(path, job, step, kind, value):
        key = (path, job, step, kind)
        counts[key] += 1
        found.append((*key, counts[key], value))
    for path, source in sorted(files.items()):
        result = subprocess.run([yq, 'eval-all', '-o=json', '-I=0', '[.]', '-'],
                                input=source, text=True, capture_output=True)
        assert result.returncode == 0, f'yq failed to parse {path}: {result.stderr}'
        documents = json.loads(result.stdout)
        assert len(documents) == 1, f'{path}: expected one workflow document'
        model = documents[0]
        if 'timeout-minutes' in model:
            add(path, '', '', 'timeout-minutes', str(model['timeout-minutes']))
        for job, node in model.get('jobs', {}).items():
            if 'timeout-minutes' in node:
                add(path, job, '', 'timeout-minutes', str(node['timeout-minutes']))
            default_shell = node.get('defaults', {}).get('run', {}).get(
                'shell', model.get('defaults', {}).get('run', {}).get('shell', 'bash'))
            for entry in node.get('steps', []):
                step = entry.get('name', '')
                if 'timeout-minutes' in entry:
                    add(path, job, step, 'timeout-minutes', str(entry['timeout-minutes']))
                if 'run' not in entry:
                    continue
                body = entry['run']
                shell = entry.get('shell', default_shell)
                python_sources = [body] if shell == 'python' else []
                if shell != 'python':
                    result = subprocess.run([shfmt, '-ln', 'bash', '--to-json'], input=body,
                                            text=True, capture_output=True)
                    assert result.returncode == 0, f'shfmt failed to parse {path}/{job}/{step}: {result.stderr}'
                    raw = body.encode('utf-8')
                    def spelling(word, raw=raw):
                        return raw[word['Pos']['Offset']:word['End']['Offset']].decode('utf-8')
                    for node in syntax_nodes(json.loads(result.stdout)):
                        command = node.get('Cmd', {})
                        if command.get('Type') != 'CallExpr':
                            continue
                        words = command.get('Args', [])
                        argv = [shlex.split(spelling(word))[0] for word in words]
                        if argv[:2] in (['python', '-'], ['python3', '-']):
                            for redirect in node.get('Redirs', []):
                                if 'Hdoc' in redirect and redirect.get('N', {}).get('Value', '0') == '0':
                                    python_sources.append(''.join(part['Value'] if part['Type'] == 'Lit' else spelling(part)
                                                                  for part in redirect['Hdoc'].get('Parts', [])))
                        elif argv[:1] == ['python3'] and any(argument.startswith('.github/ci-internal/') for argument in argv[1:2]):
                            python_sources.append(pathlib.Path(argv[1]).read_text(encoding='utf-8'))
                        prefix = []
                        for word, argument in zip(words, argv, strict=True):
                            if argument.startswith('--'):
                                break
                            prefix.append(spelling(word))
                        offset = 0
                        while offset < len(argv):
                            name = command_name(words[offset])
                            arguments = argv[offset:]
                            options = next((options for consumer, options in control_consumers.items()
                                            if name == consumer[0] and tuple(arguments[1:len(consumer)]) == consumer[1:]), ())
                            if name is None:
                                control_options = {option for options in control_consumers.values()
                                                   for option in options if option.startswith('--')}
                                assert not any(argument.partition('=')[0] in control_options for argument in arguments[1:]), (
                                    f'{path}/{job}/{step}: dynamic control consumer {spelling(words[offset])}')
                            for index, argument in enumerate(arguments):
                                kind, equal, value = argument.partition('=')
                                if kind == '--retry-all-errors' and kind in options:
                                    value = 'true'
                                elif kind in options and kind.startswith('--'):
                                    value = value if equal else arguments[index + 1] if index + 1 < len(arguments) else ''
                                elif index == 0 and name in options:
                                    kind, value = name, arguments[1] if len(arguments) > 1 else ''
                                else:
                                    continue
                                owner = step + ' / ' + ' '.join(prefix) if kind == '--retries' else step
                                add(path, job, owner, kind, value)
                            if name not in command_wrappers:
                                break
                            offset += command_wrappers[name]
                for python_source in python_sources:
                    for node in ast.walk(ast.parse(python_source)):
                        if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) and node.func.attr in ('run', 'wait', 'communicate'):
                            for keyword in node.keywords:
                                if keyword.arg == 'timeout':
                                    add(path, job, step, 'python timeout', ast.unparse(keyword.value))
                if 'f"-timeout={os.environ[\'TIMEOUT\']}"' in body:
                    add(path, job, step, 'caller fuzz timeout', 'TIMEOUT')
    return found

authority = {}
def permit(path, job, step, kind, value, occurrence=1):
    reason = ('Approved release-report polling' if kind == 'sleep' else
              'Caller-declared fuzz limits; no shared deadline' if kind == 'caller fuzz timeout' else
              'Approved nextest retry policy' if kind == '--retries' else
              'Approved transient download retry' if kind in ('--retry', '--retry-all-errors', '--retry-delay') else
              'Approved test/coverage bound' if value == '45' else
              'Approved installation bound; compiled-cache tool' if step == 'Compile cache' else
              'Approved installation bound; fuzz tools' if job == 'fuzz' else
              'Approved installation/self-test download bound')
    authority[('.github/workflows/' + path + '.yml', job, step, kind, occurrence, value)] = reason
for job, names in {
 'quality': ['Compile cache', 'Mise bootstrap', 'Test tools'],
 'docs': ['Compile cache', 'Mise bootstrap', 'Test tools'],
 'test-prepare': ['Compile cache', 'Search tools', 'Mise bootstrap', 'Test tools'],
 'test-build': ['Compile cache', 'Coverage tools', 'Search tools', 'Mise bootstrap', 'Test tools'],
 'tests': ['Compile cache', 'Coverage tools', 'Search tools', 'Mise bootstrap', 'Test tools'],
 'checks': ['Check tools', 'Compile cache'],
 'fuzz': ['Check tools'], 'advisories': ['Check tools'],
 'targets': ['Compile cache', 'Mise bootstrap', 'Test tools'],
 'browser': ['Compile cache', 'Browser mise bootstrap', 'Browser tools'],
 'browser-tests': ['Compile cache'],
}.items():
    for name in names:
        permit('ci', job, name, 'timeout-minutes', '10')
for job, names in {
 'quality': ['Compile cache', 'Mise bootstrap'],
 'docs': ['Compile cache', 'Mise bootstrap'],
 'targets': ['Compile cache', 'Mise bootstrap'],
 'test-prepare': ['Compile cache', 'Search tools', 'Mise bootstrap'],
 'test-build': ['Compile cache', 'Coverage tools', 'Search tools', 'Mise bootstrap'],
 'tests': ['Compile cache', 'Coverage tools', 'Search tools', 'Mise bootstrap'],
 'checks': ['Check tools', 'Compile cache'],
 'fuzz': ['Check tools'], 'advisories': ['Check tools'],
 'browser': ['Compile cache', 'Browser mise bootstrap'],
 'browser-tests': ['Compile cache', 'Browser test mise bootstrap'],
}.items():
    for name in names:
        permit('ci', job, name, '--retry', '3')
for occurrence in range(1, 5):
    permit('ci', 'browser-tests', 'Browser runtime tools', '--retry', '3', occurrence)
for name in ['Tests', 'Isolated tests']:
    permit('ci', 'tests', name, 'timeout-minutes', '45')
permit('ci', 'test-prepare', 'Prepare test setup', 'timeout-minutes', '45')
permit('ci', 'test-build', 'Build test archive', 'timeout-minutes', '45')
permit('ci', 'checks', 'Coverage', 'timeout-minutes', '45')
for name in ['Coverage release installation', 'Search release installation proofs', 'Real search tools']:
    permit('ci-internal', 'seams', name, 'timeout-minutes', '10')
for name in ['Coverage tools', 'Caller tools']:
    permit('ci-proof', 'comparison', name, 'timeout-minutes', '10')
permit('ci-proof', 'comparison', 'Single-job coverage baseline', 'timeout-minutes', '45')
permit('ci-internal', 'browser-qualification', 'Compile cache', 'timeout-minutes', '10')
permit('ci', 'tests', 'Tests / "$TOOLS/cargo-nextest" nextest run', '--retries', '2')
permit('ci', 'tests', 'Isolated tests / run_isolated "$TOOLS/cargo-llvm-cov" llvm-cov nextest', '--retries', '2')
permit('ci', 'tests', 'Isolated tests / run_isolated "$TOOLS/cargo-nextest" nextest run', '--retries', '2')
permit('ci', 'checks', 'Coverage / "$TOOLS/cargo-llvm-cov" llvm-cov nextest', '--retries', '2')
permit('ci', 'fuzz', 'Fuzz target', 'caller fuzz timeout', 'TIMEOUT')
permit('release-please', 'evidence', 'Export the dependency graph SBOM', 'sleep', '5')
permit('content', 'content', 'Mise bootstrap', '--retry', '3')

for path, job, step, kind, occurrence, _value in list(authority):
    if kind == '--retry':
        permit(path.removeprefix('.github/workflows/').removesuffix('.yml'), job, step, '--retry-all-errors', 'true', occurrence)
        permit(path.removeprefix('.github/workflows/').removesuffix('.yml'), job, step, '--retry-delay', '30', occurrence)

# Doctests reuse the test route's step definitions and bounds.
for (path, job, step, kind, occurrence, value), reason in list(authority.items()):
    if job == 'tests':
        authority[(path, 'doctests', step, kind, occurrence, value)] = reason

def require_recorded_controls(sources, reasons=authority):
    observed = collect(sources)
    assert Counter(observed) == Counter(reasons.keys()) and all(reason.strip() for reason in reasons.values()), (
        'Unlisted, stale, duplicate or unexplained workflow control',
        list((Counter(observed) - Counter(reasons.keys())).elements()),
        list((Counter(reasons.keys()) - Counter(observed)).elements()))
    return '\n'.join(' | '.join(map(str, row)) + ' | ' + reasons[row] for row in observed)

files = {str(path): path.read_text(encoding='utf-8')
         for path in pathlib.Path('.github/workflows').iterdir() if path.suffix in ('.yml', '.yaml')}

class SearchReleases(unittest.TestCase):
    def test_pinned_archives(self):
        self.assertNotIn('apt-get', workflow)
        rows = re.findall(r'^https://github.com/\S+ (\w{64}) (rg|fd)$', body('Search tools'), re.MULTILINE)
        self.assertEqual(len(rows), 2)
        self.assertIn('/15.2.0/ripgrep-15.2.0-x86_64-unknown-linux-musl.tar.gz', body('Search tools'))
        self.assertIn('/v10.5.0/fd-v10.5.0-x86_64-unknown-linux-musl.tar.gz', body('Search tools'))

    def test_real_versions(self):
        self.assertIn('sha256sum --check', body('Search tools'))
        with tempfile.TemporaryDirectory() as temporary:
            env = os.environ | {'RUNNER_TEMP': temporary, 'GITHUB_PATH': temporary + '/path'}
            result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', body('Search tools')],
                                    env=env, capture_output=True, text=True, encoding='utf-8')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn('ripgrep 15.2.0', result.stdout)
            self.assertIn('fd 10.5.0', result.stdout)
            bin_dir = pathlib.Path(env['GITHUB_PATH']).read_text(encoding='utf-8').strip()
            self.assertTrue(pathlib.Path(bin_dir).is_absolute())
            for binary in ['rg', 'fd']:
                self.assertIn('"$bin/' + binary + '" --version', body('Search tools'))
                self.assertTrue((pathlib.Path(bin_dir) / binary).is_file())

    def test_reset_and_gateway_bursts_preserve_verified_downloads(self):
        self.check_download_outcomes([
            ('receive_reset', ['reset', 200], 0, 2, False, {}),
            *[('gateway_burst_' + str(code), [code, code, 200], 0, 3, False, {})
              for code in [502, 503, 504]],
            ('reset_exhaustion', ['reset'], 56, 4, False, {}),
            ('reset_then_corrupt_bytes', ['reset', 200], 1, 2, True, {}),
        ])

    def check_download_outcomes(self, cases):
        import contextlib
        import hashlib
        import http.server
        import threading
        peer = pathlib.Path('.github/ci-internal/cache-storage-failures-keep-builds-running.py').read_text(encoding='utf-8')
        tree = ast.parse(peer)
        tree.body = [node for node in tree.body
                     if isinstance(node, (ast.ClassDef, ast.FunctionDef)) and node.name in ('Storage', 'storage')]
        namespace: dict[str, Any] = {'contextlib': contextlib, 'http': http, 'threading': threading}
        exec(compile(tree, 'existing HTTP peer lifecycle', 'exec'), namespace)

        class ResetStorage(namespace['Storage']):
            def do_GET(self):
                import socket
                import struct
                index = min(len(self.server.requests), len(self.server.responses) - 1)
                if self.server.responses[index][0] != 'reset':
                    return super().do_GET()
                self.server.requests.append(self.path)
                self.send_response(200)
                self.send_header('Content-Length', '1000000')
                self.end_headers()
                self.wfile.write(b'partial tool bytes')
                self.wfile.flush()
                self.connection.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack('ii', 1, 0))
                self.close_connection = True
                self.rfile.close()
                self.connection.close()

        namespace['Storage'] = ResetStorage
        command = body('Compile cache')
        parsed = subprocess.check_output([shfmt, '-ln', 'bash', '--to-json'], input=command, text=True)
        raw = command.encode('utf-8')
        calls = [node['Cmd']['Args'] for node in syntax_nodes(json.loads(parsed))
                 if node.get('Cmd', {}).get('Type') == 'CallExpr'
                 and node['Cmd'].get('Args') and command_name(node['Cmd']['Args'][0]) == 'curl']
        self.assertEqual(len(calls), 1)
        argv = [shlex.split(raw[word['Pos']['Offset']:word['End']['Offset']].decode())[0]
                for word in calls[0]]
        executable = shutil.which('curl')
        assert executable is not None
        # Keep native curl's policy; scale only its delay so loopback proofs finish under 30 seconds.
        if '--retry-delay' in argv:
            argv[argv.index('--retry-delay') + 1] = '1'
        payload = b'verified tool payload\n'
        digest = hashlib.sha256(payload).hexdigest()
        for name, statuses, status, attempts, corrupt, headers in cases:
            with self.subTest(name=name), namespace['storage']() as server, tempfile.TemporaryDirectory() as temporary:
                server.responses = [(code, b'corrupt' if corrupt else payload, headers) for code in statuses]
                output = pathlib.Path(temporary) / 'download'
                url = f'http://127.0.0.1:{server.server_port}/pinned-tool'
                actual = [executable, *argv[1:]]
                actual[actual.index('--location') + 1] = url
                actual[actual.index('--output') + 1] = str(output)
                command = shlex.join(actual) + '\nprintf "%s  %s\\n" ' + shlex.quote(digest) + ' ' + shlex.quote(str(output))
                command += ' | sha256sum --check --status\nprintf "INSTALLED\\n"'
                result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', command], capture_output=True, text=True)
                print(f"DOWNLOAD {name}: exit={result.returncode} requests={len(server.requests)} installed={result.stdout.strip() == 'INSTALLED'}", flush=True)
                self.assertEqual(result.returncode, status)
                self.assertEqual(server.requests, ['/pinned-tool'] * attempts)
                self.assertEqual(result.stdout, 'INSTALLED\n' if status == 0 else '')

    def test_transient_download_retry_outcomes(self):
        self.check_download_outcomes([
            ('initial_success', [200], 0, 1, False, {}),
            *[('transient_' + str(status), [status, 200], 0, 2, False, {})
              for status in [408, 429, 500, 502, 503, 504]],
            ('last_retry_succeeds', [500, 500, 500, 200], 0, 4, False, {}),
            ('exhaustion_stays_fatal', [500], 22, 4, False, {}),
            ('permanent_404_stays_fatal', [404], 22, 4, False, {}),
            ('nontransient_501_stays_fatal', [501], 22, 4, False, {}),
            ('retry_after_header_accepted', [503, 200], 0, 2, False, {'Retry-After': '0'}),
            ('retried_corrupt_payload_rejected', [500, 200], 1, 2, True, {}),
        ])

    def test_every_pinned_tool_download_retains_the_transport_retry_policy(self):
        for path in ['.github/workflows/ci.yml', '.github/workflows/content.yml']:
            commands = [shlex.split(line) for line in files[path].splitlines() if line.strip().startswith('curl ')]
            self.assertEqual(len(commands), 9 if path.endswith('/ci.yml') else 1)
            for argv in commands:
                with self.subTest(path=path, argv=argv):
                    self.assertIn('--retry-all-errors', argv)
                    self.assertEqual(argv[argv.index('--retry') + 1], '3')
                    self.assertEqual(argv[argv.index('--retry-delay') + 1], '30')

    def test_workflow_controls_match_authority(self):
        report = require_recorded_controls(files)
        self.assertEqual(len(report.splitlines()), 161)
        print(report)

    def assert_policy(self, sources, admitted):
        if admitted:
            require_recorded_controls(sources, authority)
        else:
            with self.assertRaises(AssertionError):
                require_recorded_controls(sources, authority)

    def test_unlisted_step_deadline_is_rejected(self):
        source = files['.github/workflows/ci.yml']
        candidate = source.replace('      - name: Format\n', '      - name: Format\n        timeout-minutes: 10\n', 1)
        self.assertNotEqual(candidate, source)
        self.assert_policy(files | {'.github/workflows/ci.yml': candidate}, False)

    def test_unlisted_job_and_workflow_deadlines_are_rejected(self):
        source = files['.github/workflows/ci.yml']
        candidate = source.replace('  quality:\n', '  quality:\n    timeout-minutes: 10\n', 1)
        self.assertNotEqual(candidate, source)
        self.assert_policy(files | {'.github/workflows/ci.yml': candidate}, False)
        for suffix in ['yml', 'yaml']:
            for key in ['timeout-minutes: 10', '"timeout-minutes": 10', "'timeout-minutes': 10", 'extra: {timeout-minutes: 10}']:
                with self.subTest(suffix=suffix, key=key):
                    source = ('jobs:\n  extra: {timeout-minutes: 10, runs-on: ubuntu-latest, steps: [{run: echo valid}]}\n' if key.startswith('extra:') else 'jobs:\n  new:\n    ' + key + '\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo valid\n')
                    self.assert_policy(files | {'.github/workflows/new.' + suffix: source}, False)

    def test_changed_deadline_value_is_rejected(self):
        source = files['.github/workflows/ci.yml']
        for old, value in [('10', '11'), ('45', '44'), ('10', '$' + '{{ inputs.minutes }}'), ('10', '')]:
            with self.subTest(old=old, value=value):
                candidate = source.replace('timeout-minutes: ' + old, 'timeout-minutes: ' + value, 1)
                self.assertNotEqual(candidate, source)
                self.assert_policy(files | {'.github/workflows/ci.yml': candidate}, False)

    def test_download_retry_changes_are_rejected_per_call(self):
        source = files['.github/workflows/ci.yml']
        positions = [match.start() for match in re.finditer('--retry 3', source)]
        self.assertEqual(len(positions), 9)
        self.assert_policy(files, True)
        for offset in positions:
            for replacement in ['', '--retry 2', '--retry 3 --retry 3']:
                with self.subTest(offset=offset, replacement=replacement):
                    candidate = source[:offset] + source[offset:].replace('--retry 3', replacement, 1)
                    self.assert_policy(files | {'.github/workflows/ci.yml': candidate}, False)

    def test_retry_changes_are_rejected_per_branch(self):
        source = files['.github/workflows/ci.yml']
        positions = [m.start() for m in re.finditer('--retries 2', source)]
        self.assertEqual(len(positions), 4)
        for offset in positions:
            for replacement in ['--retries 3', '--retries 0', '--retries 2 --retries 2']:
                with self.subTest(offset=offset, replacement=replacement):
                    candidate = source[:offset] + source[offset:].replace('--retries 2', replacement, 1)
                    self.assert_policy(files | {'.github/workflows/ci.yml': candidate}, False)

    def test_authority_is_bound_to_its_owner(self):
        copied = 'jobs:\n  other:\n    steps:\n      - name: Search tools\n        timeout-minutes: 10\n        run: echo valid\n'
        self.assert_policy(files | {'.github/workflows/new.yml': copied}, False)
        self.assert_policy(files | {'.github/workflows/ci.yml': files['.github/workflows/ci.yml'].replace('jobs:\n', copied, 1)}, False)
        moved = files.copy()
        moved['.github/workflows/moved.yml'] = moved.pop('.github/workflows/ci.yml')
        self.assert_policy(moved, False)
        removed = files['.github/workflows/ci.yml'].replace('        timeout-minutes: 10\n', '', 1)
        self.assert_policy(files | {'.github/workflows/ci.yml': removed}, False)

    def test_script_deadlines_are_rejected(self):
        for fragment in ['timeout 3 true', 'curl --max-time 15 https://example.test', 'curl --retry 3 https://example.test']:
            with self.subTest(fragment=fragment):
                source = 'jobs:\n  new:\n    steps:\n      - name: Example\n        run: |\n          ' + fragment + '\n'
                self.assert_policy(files | {'.github/workflows/new.yml': source}, False)
        source = 'jobs:\n  new:\n    steps:\n      - name: Example\n        shell: python\n        run: |\n          subprocess.run(["true"], timeout=60)\n'
        self.assert_policy(files | {'.github/workflows/new.yml': source}, False)

    def test_retry_cannot_move_between_commands(self):
        source = files['.github/workflows/ci.yml']
        coverage = '              run_isolated "$TOOLS/cargo-llvm-cov" llvm-cov nextest'
        plain = '              run_isolated "$TOOLS/cargo-nextest" nextest run'
        start = source.index(plain)
        end = source.index('\n', source.index('--retries 2', start))
        command = source[start:end]
        candidate = source.replace(command, command + '\n' + command, 1)
        start = candidate.index(coverage)
        offset = candidate.index('--retries 2', start)
        candidate = candidate[:offset] + candidate[offset:].replace('--retries 2', '', 1)
        self.assertEqual(candidate.count('--retries 2'), 4)
        self.assert_policy(files | {'.github/workflows/ci.yml': candidate}, False)

    def test_deadline_after_steps_keeps_job_ownership(self):
        source = files['.github/workflows/ci-proof.yml']
        candidate = source.replace('        timeout-minutes: 45\n', '', 1) + '\n    timeout-minutes: 45\n'
        self.assertNotEqual(candidate, source)
        self.assert_policy(files | {'.github/workflows/ci-proof.yml': candidate}, False)

    def test_run_forms_reject_unlisted_controls(self):
        for header, command in [('run: timeout 3 true', ''), ('- run: |', 'timeout 3 true'),
                                ('- run: timeout 3 true', ''), ('run: | # command', 'timeout 3 true'),
                                ('run: \"curl --retry 3 https://example.test\"', ''),
                                ("run: 'curl --max-time 15 https://example.test'", '')]:
            with self.subTest(header=header):
                step = ('      ' + header if header.startswith('-') else '      - name: Example\n        ' + header)
                source = 'jobs:\n  new:\n    steps:\n' + step + '\n'
                if command:
                    source += '          ' + command + '\n'
                self.assert_policy(files | {'.github/workflows/new.yml': source}, False)
        source = files['.github/workflows/ci.yml']
        candidate = source.replace('run: cargo fmt --all --check', 'run: timeout 3 cargo fmt --all --check', 1)
        self.assertNotEqual(candidate, source)
        self.assert_policy(files | {'.github/workflows/ci.yml': candidate}, False)

    def test_shell_reader_fails_closed(self):
        source = 'jobs:\n  new:\n    steps:\n      - run: "case"\n'
        with self.assertRaisesRegex(AssertionError, 'shfmt failed to parse'):
            collect({'example.yml': source})
        executable = pathlib.Path(shfmt)
        executable.rename(executable.with_suffix('.hidden'))
        try:
            with self.assertRaises(FileNotFoundError):
                collect({'example.yml': 'jobs: {new: {steps: [{run: echo ok}]}}'})
        finally:
            executable.with_suffix('.hidden').rename(executable)

    def test_shfmt_release_is_verified(self):
        row = next(line for line in shfmt_install().splitlines() if line.startswith('https://'))
        self.assertEqual(row, 'https://github.com/mvdan/sh/releases/download/v3.14.1/shfmt_v3.14.1_linux_amd64 76e77641faa025814b77f153b29796b8e6fa2fca03e0c76a691608b86c7ea7bf shfmt')
        self.assertEqual(subprocess.check_output([shfmt, '--version'], text=True), 'v3.14.1\n')
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            curl = directory / 'curl'
            curl.write_text('#!/bin/sh\nwhile [ "$1" != --output ]; do shift; done\nprintf corrupt > "$2"\n', encoding='utf-8')
            curl.chmod(0o755)
            env = os.environ | {'RUNNER_TEMP': temporary, 'GITHUB_ENV': temporary + '/environment',
                                'PATH': temporary + ':' + os.environ['PATH']}
            result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', shfmt_install()],
                                    env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(pathlib.Path(env['GITHUB_ENV']).exists())
            self.assertFalse(list(directory.glob('maestro-check-tools.*/shfmt')))

    def test_controls_belong_to_their_consuming_command(self):
        for executable in ['echo', 'printf', 'grep']:
            for option in ['--retry', '--retry-all-errors', '--retry-delay', '--retries', '--max-time']:
                with self.subTest(executable=executable, option=option):
                    command = f"{executable} 'curl' '{option}' '8'"
                    source = 'jobs: {new: {steps: [{run: ' + json.dumps(command) + '}]}}'
                    self.assertEqual(collect({'example.yml': source}), [])
        for command, kind, value in [("curl '--retry' '3'", '--retry', '3'),
                                     ('curl --retry-all-errors example', '--retry-all-errors', 'true'),
                                     ('curl --retry-delay=30 example', '--retry-delay', '30'),
                                     ('curl --max-time=15 example', '--max-time', '15'),
                                     ('timeout 3 curl --retry 2 example', 'timeout', '3')]:
            with self.subTest(command=command):
                source = 'jobs: {new: {steps: [{run: ' + json.dumps(command) + '}]}}'
                rows = collect({'example.yml': source})
                self.assertIn(('example.yml', 'new', '', kind, 1, value), rows)
        for command in ['run_isolated "$TOOLS/cargo-nextest" nextest run --retries 2',
                        'run_isolated "$TOOLS/cargo-llvm-cov" llvm-cov nextest --retries 2',
                        'timeout 3 "$TOOLS/cargo-nextest" nextest run --retries 2']:
            with self.subTest(command=command):
                source = 'jobs: {new: {steps: [{name: Example, run: ' + json.dumps(command) + '}]}}'
                rows = collect({'example.yml': source})
                self.assertIn(('example.yml', 'new', 'Example / ' + command.split(' --')[0], '--retries', 1, '2'), rows)
        source = 'jobs: {new: {steps: [{run: \'"$TOOLS/cargo-llvm-cov" llvm-cov report --retries 2\'}]}}'
        self.assertEqual(collect({'example.yml': source}), [])

    def test_dynamic_control_consumer_fails_closed(self):
        for command in ['"$COMMAND" --retry 3', 'run_isolated "$COMMAND" --retries 2',
                        'timeout 3 "$(which curl)" --max-time 15']:
            with self.subTest(command=command):
                source = 'jobs: {new: {steps: [{run: ' + json.dumps(command) + '}]}}'
                with self.assertRaisesRegex(AssertionError, 'dynamic control consumer'):
                    collect({'example.yml': source})

    def test_quoted_shell_payload_is_data(self):
        command = "cat <<'DATA'\nsleep 99\ncurl --retry 9 example\nDATA\necho 'curl --retry 8'\ncase x in x) sleep 5 ;; esac\n"
        source = 'jobs:\n  new:\n    steps:\n      - name: Example\n        run: |\n' + textwrap.indent(command, '          ')
        self.assertEqual(collect({'example.yml': source}), [('example.yml', 'new', 'Example', 'sleep', 1, '5')])

    def test_unquoted_heredoc_substitution_is_executable(self):
        command = 'cat <<DATA\n$(sleep 6)\nDATA\n'
        source = 'jobs:\n  new:\n    steps:\n      - name: Example\n        run: |\n' + textwrap.indent(command, '          ')
        self.assertEqual(collect({'example.yml': source}), [('example.yml', 'new', 'Example', 'sleep', 1, '6')])

    def test_python_heredoc_uses_own_language(self):
        command = "python3 - <<'PYTHON'\nimport subprocess\nexpected = ['--retries', '9']\nsubprocess.run(['true'], timeout=60)\nPYTHON\n"
        source = 'jobs:\n  new:\n    steps:\n      - name: Example\n        run: |\n' + textwrap.indent(command, '          ')
        self.assertEqual(collect({'example.yml': source}), [('example.yml', 'new', 'Example', 'python timeout', 1, '60')])

    def test_script_file_uses_own_language(self):
        source = 'jobs:\n  new:\n    steps:\n      - name: Example\n        run: python3 .github/ci-internal/probe.py\n'
        with tempfile.TemporaryDirectory() as directory:
            probe = pathlib.Path(directory, '.github/ci-internal/probe.py')
            probe.parent.mkdir(parents=True)
            probe.write_text("import subprocess\nsubprocess.run(['true'], timeout=60)\n", encoding='utf-8')
            previous = os.getcwd()
            os.chdir(directory)
            try:
                rows = collect({'example.yml': source})
            finally:
                os.chdir(previous)
        self.assertEqual(rows, [('example.yml', 'new', 'Example', 'python timeout', 1, '60')])

    def test_workflow_reader_fails_closed(self):
        for source, message in [('jobs: [', 'yq failed to parse'),
                                ('jobs: {}\n---\njobs: {}\n', 'expected one workflow document')]:
            with self.subTest(source=source), self.assertRaisesRegex(AssertionError, message):
                collect({'.github/workflows/new.yml': source})

    def test_parsed_workflow_ownership(self):
        source = files['.github/workflows/ci.yml']
        original = '      - name: Format\n        run: cargo fmt --all --check\n'
        for replacement in [
            '      - {name: Format, run: timeout 3 cargo fmt --all --check}\n',
            '      - name: Format\n        run: |2\n          timeout 3 cargo fmt --all --check\n',
            '      - name: Format\n        run: &format timeout 3 cargo fmt --all --check\n      - run: *format\n',
            '      - name: Format\n        run: \'import subprocess; subprocess.run(["true"], timeout=60)\'\n        shell: python\n',
        ]:
            with self.subTest(replacement=replacement):
                candidate = source.replace(original, replacement, 1)
                self.assertNotEqual(candidate, source)
                self.assert_policy(files | {'.github/workflows/ci.yml': candidate}, False)
        candidate = source.replace('run: &sccache-stats |\n', 'run: &sccache-stats |\n          timeout 3 true\n', 1)
        self.assertNotEqual(candidate, source)
        owners = [row[1] for row in collect({'.github/workflows/ci.yml': candidate}) if row[3] == 'timeout']
        self.assertCountEqual(owners, ['quality', 'docs', 'test-build', 'test-prepare', 'tests', 'doctests', 'checks', 'targets', 'browser', 'browser-tests'])

    def test_literal_fixture_values_are_not_policy(self):
        source = ('jobs:\n  new:\n'
                  '    # timeout-minutes: 999\n'
                  '    name: "timeout-minutes: 999"\n'
                  '    env: {timeout-minutes: 999}\n'
                  '    steps:\n      - name: Example\n        shell: python\n        run: |\n'
                  '          expected = "timeout-minutes: 45"\n'
                  '          expected_args = ["--retries", "2"]\n'
                  '          sample = dict(timeout=5)\n')
        for candidate in [source, source.replace('env: {timeout-minutes: 999}', 'env:\n      timeout-minutes: 999'),
                          source.replace('env: {timeout-minutes: 999}', 'with:\n      timeout-minutes: 999'),
                          'jobs:\n  new:\n    steps:\n      - name: Example\n        run: |\n          # curl --retry 3\n          echo valid\n']:
            with self.subTest(source=candidate):
                self.assert_policy(files | {'.github/workflows/new.yml': candidate}, True)

    def test_existing_polling_and_fuzz_limits_are_preserved(self):
        self.assertEqual([r[-1] for r in collect(files) if r[3] == 'caller fuzz timeout'], ['TIMEOUT'])
        source = files['.github/workflows/release-please.yml']
        step = source.split('      - name: Export the dependency graph SBOM\n', 1)[1].split('      - name:', 1)[0]
        command = textwrap.dedent(step.split('        run: |\n', 1)[1])
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            stubs = {
                'gh': '#!/bin/sh\nprintf "%s\\n" "https://api.github.test/repos/owner/repo/dependency-graph/sbom/fetch-report/example"\n',
                'curl': ('#!/bin/sh\nn=0\n'
                         '[ ! -f "$TEST_ROOT/calls" ] || n=$(cat "$TEST_ROOT/calls")\n'
                         'n=$((n + 1))\n[ "$n" -le 2 ] || exit 70\n'
                         'printf "%s" "$n" > "$TEST_ROOT/calls"\n'
                         'while [ "$1" != --output ]; do shift; done\n'
                         'printf \'{"spdxVersion":"SPDX-2.3"}\' > "$2"\n'
                         'if [ "$n" -eq 1 ]; then printf 202; else printf 200; fi\n'),
                'sleep': '#!/bin/sh\nprintf "%s\\n" "$1" >> "$TEST_ROOT/sleeps"\n',
            }
            for name, script in stubs.items():
                stub = directory / (name + '.source')
                stub.write_text(script, encoding='utf-8')
                subprocess.run(['install', '-m', '0700', str(stub), str(directory / name)], check=True)
            env = os.environ | {'PATH': str(directory) + ':' + os.environ['PATH'], 'TEST_ROOT': temporary,
                                'GITHUB_REPOSITORY': 'owner/repo', 'GITHUB_API_URL': 'https://api.github.test',
                                'ASSETS': str(directory / 'assets'), 'GH_TOKEN': 'fixture'}
            executable = shutil.which('bash')
            self.assertEqual(pathlib.Path(executable).resolve(), pathlib.Path('/usr/bin/bash'))
            result = subprocess.run([executable, '-e', '-o', 'pipefail', '-c', command],
                                    env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((directory / 'calls').read_text(encoding='utf-8'), '2')
            self.assertEqual((directory / 'sleeps').read_text(encoding='utf-8'), '5\n')
            self.assertEqual(json.loads((directory / 'assets/sbom.spdx.json').read_text(encoding='utf-8')),
                             {'spdxVersion': 'SPDX-2.3'})

    def test_inventory_requires_a_reason_for_each_occurrence(self):
        report = require_recorded_controls(files, authority)
        self.assertEqual(len(report.splitlines()), 161)
        for row in authority:
            self.assertIn(' | '.join(map(str, row)) + ' | ' + authority[row], report)
        for row in [next(iter(authority)), next(reversed(authority))]:
            with self.assertRaises(AssertionError):
                require_recorded_controls(files, authority | {row: ''})

    def test_corrupt_archives(self):
        command = body('Search tools')
        rows = re.findall(r'^https://github.com/\S+ \w{64} (?:rg|fd)$', command, re.MULTILINE)
        self.assertEqual(len(rows), 2)
        for row in rows:
            with self.subTest(archive=row.split()[0]), tempfile.TemporaryDirectory() as temporary:
                directory = pathlib.Path(temporary)
                curl = directory / 'curl'
                curl.write_text('#!/bin/sh\nwhile [ "$1" != --output ]; do shift; done\nprintf corrupt > "$2"\n', encoding='utf-8')
                curl.chmod(0o755)
                tar = directory / 'tar'
                tar.write_text('#!/bin/sh\ntouch "$RUNNER_TEMP/extracted"\n', encoding='utf-8')
                tar.chmod(0o755)
                # Exercise each published digest as the first download.
                other = next(value for value in rows if value != row)
                env = os.environ | {'RUNNER_TEMP': temporary, 'GITHUB_PATH': temporary + '/path',
                                    'PATH': temporary + ':' + os.environ['PATH']}
                result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', command.replace(other + '\n', '')],
                                        env=env, capture_output=True, text=True, encoding='utf-8')
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse((directory / 'extracted').exists())
                self.assertFalse((directory / 'path').exists())
                self.assertFalse(list(directory.glob('maestro-search-bin.*/rg')))
                self.assertFalse(list(directory.glob('maestro-search-bin.*/fd')))

suite = unittest.defaultTestLoader.loadTestsFromTestCase(SearchReleases)
try:
    result = unittest.TextTestRunner(verbosity=2).run(suite)
finally:
    tool_directory.cleanup()
if not result.wasSuccessful():
    raise SystemExit(1)
