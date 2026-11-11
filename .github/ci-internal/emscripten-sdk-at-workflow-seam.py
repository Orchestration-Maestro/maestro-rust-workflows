import ast
import gzip
import hashlib
import io
import json
import os
import pathlib
import re
import shutil
import subprocess
import tarfile
import tempfile

root = pathlib.Path.cwd()
workflow = (root / '.github/workflows/ci.yml').read_text(encoding='utf-8')
document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
browser = document['jobs']['browser']
chromium = document['jobs']['browser-tests']
SDK = 'Emscripten SDK'
TARGET = 'Emscripten target'
BUILD = 'Build emscripten crates'
EMSCRIPTEN = 'wasm32-unknown-emscripten'
CANARY = 'canary-for-the-job-credential'


def steps(job):
    return {step['name']: step for step in job['steps']}


def opens(condition, context):
    expression = condition.removeprefix('${{').removesuffix('}}').strip().replace('!cancelled()', 'True')
    for key, value in context.items():
        expression = expression.replace(key, repr(value))
    expression = expression.replace('&&', ' and ').replace('||', ' or ')
    return settle(ast.parse(re.sub(r'!(?!=)', ' not ', expression).strip(), mode='eval').body)


def settle(node):
    # The workflow's conditions use only literals, comparisons, not, and, or.
    if isinstance(node, ast.Constant):
        return node.value
    if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.Not):
        return not settle(node.operand)
    if isinstance(node, ast.BoolOp):
        results = (settle(value) for value in node.values)
        return all(results) if isinstance(node.op, ast.And) else any(results)
    assert isinstance(node, ast.Compare) and len(node.ops) == 1, ast.dump(node)
    left, right = settle(node.left), settle(node.comparators[0])
    return left == right if isinstance(node.ops[0], ast.Eq) else left != right


def declared(directory, text):
    (directory / '.github/ci.toml').write_text('wasm_crates=[]\ncheck_targets=[]\n' + text, encoding='utf-8')
    output = directory / 'output'
    output.write_text('', encoding='utf-8')
    env = os.environ | {'GITHUB_OUTPUT': str(output), 'TEST_EVENT': 'merge_group', 'WARM': 'false'}
    result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', steps(document['jobs']['declaration'])['Read caller declaration']['run']],
                            cwd=directory, env=env, text=True, capture_output=True)
    lines = output.read_text(encoding='utf-8').splitlines()
    return result, dict(line.split('=', 1) for line in lines if '=' in line)


def add_isolated(directory):
    manifest = directory / 'isolated/Cargo.toml'
    (manifest.parent / 'src').mkdir(parents=True)
    (manifest.parent / 'src/lib.rs').write_text('', encoding='utf-8')
    manifest.write_text('[workspace]\n[package]\nname = "isolated"\nversion = "0.1.0"\nedition = "2021"\nlicense = "MIT"\n', encoding='utf-8')
    subprocess.run(['cargo', 'generate-lockfile', '--offline', '--manifest-path', str(manifest)], check=True, capture_output=True)
    return manifest


def isolated(directory):
    # A package of its own locked workspace is accepted only through the manifest that declares it.
    manifest = add_isolated(directory)
    listed = 'additional_locked_manifests=["isolated/Cargo.toml"]\n'
    result, values = declared(directory, listed + 'emscripten_crates=["witness", "isolated", "other"]\n')
    assert result.returncode == 0, result.stderr
    assert json.loads(values['emscripten_crates']) == [['witness', None], ['isolated', 'isolated/Cargo.toml'], ['other', None]], values
    # One package reached twice (repeated entry, spelling alias, member of the root workspace) has one owner.
    for entries, expected in [(['isolated/Cargo.toml'] * 2, [['isolated', 'isolated/Cargo.toml']]),
                              (['isolated/Cargo.toml', './isolated/../isolated/Cargo.toml'], [['isolated', 'isolated/Cargo.toml']]),
                              (['witness/Cargo.toml', 'isolated/Cargo.toml'], [['witness', None]])]:
        result, values = declared(directory, f'additional_locked_manifests={json.dumps(entries)}\nemscripten_crates={json.dumps([name for name, _ in expected])}\n')
        assert result.returncode == 0, (entries, result.stderr)
        assert json.loads(values['emscripten_crates']) == expected, (entries, values)
    result, _ = declared(directory, 'emscripten_crates=["isolated"]\n')
    assert result.returncode != 0
    assert result.stderr.strip() == ".github/ci.toml: emscripten_crates contains unknown package 'isolated'", result.stderr
    result, _ = declared(directory, listed + 'emscripten_crates=["missing"]\n')
    assert result.returncode != 0
    assert result.stderr.strip() == ".github/ci.toml: emscripten_crates contains unknown package 'missing'", result.stderr
    # A name that two manifests share would build one of them silently.
    manifest.write_text(manifest.read_text(encoding='utf-8').replace('"isolated"', '"witness"'), encoding='utf-8')
    subprocess.run(['cargo', 'generate-lockfile', '--offline', '--manifest-path', str(manifest)], check=True, capture_output=True)
    result, _ = declared(directory, listed + 'emscripten_crates=["witness"]\n')
    assert result.returncode != 0
    assert result.stderr.strip() == ".github/ci.toml: emscripten_crates package 'witness' is in more than one manifest", result.stderr


def the_declaration_checks_types_and_packages():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/wasm-witnesses', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        (directory / '.github').mkdir()
        result, values = declared(directory, '')
        assert result.returncode == 0, result.stderr
        assert values['emscripten_crates'] == '[]', values
        # Order and repetitions are retained, like wasm_crates.
        result, values = declared(directory, 'emscripten_crates=["witness", "other", "witness"]\n')
        assert result.returncode == 0, result.stderr
        assert json.loads(values['emscripten_crates']) == [['witness', None], ['other', None], ['witness', None]], values
        for value in ['"witness"', '["witness", 1]', '{witness=true}']:
            result, _ = declared(directory, 'emscripten_crates=' + value + '\n')
            assert result.returncode != 0, value
            assert result.stderr.strip() == '.github/ci.toml: emscripten_crates must be an array of strings', result.stderr
        for name in ['', 'unknown', ' witness', 'Witness', '$(touch injected)']:
            result, _ = declared(directory, 'emscripten_crates=[' + json.dumps(name) + ']\n')
            assert result.returncode != 0, name
            assert result.stderr.strip() == f'.github/ci.toml: emscripten_crates contains unknown package {name!r}', result.stderr
        assert not (directory / 'injected').exists()
        isolated(directory)
    schema = json.loads((root / '.github/ci.schema.json').read_text(encoding='utf-8'))
    assert schema['properties']['emscripten_crates']['type'] == 'array'
    assert schema['properties']['emscripten_crates']['items'] == {'type': 'string'}
    assert schema['properties']['emscripten_crates']['default'] == []
    assert 'emscripten_crates' not in schema['required']


def the_declaration_names_the_paths_that_decide_each_build():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/wasm-witnesses', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        (directory / '.github').mkdir()
        result, values = declared(directory, '')
        assert result.returncode == 0, result.stderr
        assert values['emscripten_paths'] == '[]', values
        # A root package is decided by its directory and the root manifest and lock file.
        result, values = declared(directory, 'emscripten_crates=["witness", "witness"]\n')
        assert result.returncode == 0, result.stderr
        assert json.loads(values['emscripten_paths']) == ['Cargo.lock', 'Cargo.toml', 'witness'], values
        # A package of an additional manifest is decided by its directory, that manifest and its lock file,
        # whichever way the declaration spells the manifest path.
        add_isolated(directory)
        for spelling in ['isolated/Cargo.toml', './isolated/../isolated/Cargo.toml']:
            result, values = declared(directory, f'additional_locked_manifests=[{json.dumps(spelling)}]\nemscripten_crates=["isolated", "other"]\n')
            assert result.returncode == 0, result.stderr
            assert json.loads(values['emscripten_paths']) == [
                'Cargo.lock', 'Cargo.toml', 'isolated', 'isolated/Cargo.lock', 'isolated/Cargo.toml', 'other'], (spelling, values)


def only_the_script_job_installs_the_sdk():
    # The script job builds the one artifact every consumer downloads, so no other job runs the compiler.
    script = steps(document['jobs']['sandbox-runtime'])
    for name in [SDK, TARGET]:
        assert name in script, name
    for name in ['browser', 'browser-tests']:
        assert not {SDK, TARGET, BUILD} & set(steps(document['jobs'][name])), (name, sorted({SDK, TARGET, BUILD} & set(steps(document['jobs'][name]))))
    assert steps(document['jobs']['sandbox-runtime'])[SDK]['env'] == {'GH_TOKEN': '${{ github.token }}'}
    order = [step['name'] for step in document['jobs']['sandbox-runtime']['steps']]
    assert order.index('Test tools') < order.index(SDK) < order.index(TARGET) < order.index('Build sandbox runtime script'), order
    # A caller that only builds for Emscripten still gets the browser job, and a caller without the key does not.
    job_context = {'inputs.browser-only': False, 'needs.declaration.outputs.coverage': 'true',
                   'needs.declaration.outputs.tests_reused': 'false', 'needs.declaration.outputs.wasm_examples': '[]',
                   'inputs.advisory-scan': False, 'inputs.fuzz-scan': False, 'inputs.warm-cache': False,
                   'github.event_name': 'push', 'github.ref': 'refs/heads/main',
                   "format('refs/heads/{0}', github.event.repository.default_branch)": 'refs/heads/main',
                   'needs.declaration.outputs.wasm_crates': '[]', 'needs.declaration.outputs.browser_build': 'false',
                   'needs.declaration.outputs.emscripten_crates': '["web"]',
                   'needs.declaration.result': 'success'}
    assert opens(browser['if'], job_context)
    assert not opens(browser['if'], job_context | {'needs.declaration.outputs.emscripten_crates': '[]'})


def a_pull_request_builds_only_what_its_change_reaches():
    pull_request = {'inputs.browser-only': False, 'needs.declaration.outputs.coverage': 'false',
                    'needs.declaration.outputs.tests_reused': 'false', 'needs.declaration.outputs.wasm_examples': '[]',
                    'inputs.advisory-scan': False, 'inputs.fuzz-scan': False, 'inputs.warm-cache': False,
                    'github.event_name': 'pull_request', 'github.ref': 'refs/pull/1/merge',
                    "format('refs/heads/{0}', github.event.repository.default_branch)": 'refs/heads/main',
                    'needs.declaration.outputs.wasm_crates': '[]', 'needs.declaration.outputs.browser_build': 'false',
                    'needs.declaration.outputs.emscripten_crates': '[["web", null]]',
                    'needs.declaration.outputs.emscripten_changed': 'true',
                    'needs.declaration.result': 'success'}
    untouched = pull_request | {'needs.declaration.outputs.emscripten_changed': 'false'}
    undeclared = pull_request | {'needs.declaration.outputs.emscripten_crates': '[]'}
    assert opens(browser['if'], pull_request), 'a pull request that reaches a listed package runs the job'
    assert not opens(browser['if'], untouched), 'a pull request that reaches none keeps it closed'
    assert not opens(browser['if'], undeclared), 'a caller without the key never runs it for this'
    # The Chromium job and the library sweep stay off a pull request.
    assert not opens(chromium['if'], pull_request | {'needs.declaration.outputs.browser_tests': 'true'})
    assert steps(browser)['Build wasm crates']['if'] and not opens(steps(browser)['Build wasm crates']['if'], pull_request)
    # Selection hands the verdict to the job: the declaration job exposes it, and the selection step reads the paths from the declaration.
    assert document['jobs']['declaration']['outputs']['emscripten_changed'] == '${{ steps.selection.outputs.emscripten_changed }}'
    assert steps(document['jobs']['declaration'])['Select test packages']['env']['EMSCRIPTEN_PATHS'] == '${{ steps.config.outputs.emscripten_paths }}'


def recorded(directory, tool, step, extra=None):
    # Runs a step's body with `tool` replaced by a recorder; the calls it receives are returned one per line.
    tools = directory / 'bin'
    tools.mkdir()
    (tools / tool).write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> "$FAKE_LOG"\n', encoding='utf-8')
    (tools / tool).chmod(0o700)
    env = os.environ | {'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'FAKE_LOG': str(directory / 'log')} | (extra or {})
    result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', steps(document['jobs']['sandbox-runtime'])[step]['run']],
                            cwd=directory, env=env, text=True, capture_output=True)
    log = directory / 'log'
    return result, log.read_text(encoding='utf-8').splitlines() if log.exists() else []


def the_target_comes_from_rustup():
    with tempfile.TemporaryDirectory() as temporary:
        result, calls = recorded(pathlib.Path(temporary), 'rustup', TARGET)
        assert result.returncode == 0, result.stderr
        assert calls == ['target add ' + EMSCRIPTEN], calls


def each_crate_builds_for_emscripten_in_order():
    names = [['b', None], ['a', 'x/Cargo.toml'], ['b', None], ['$(touch injected)', None], ['c', '$(touch injected)/Cargo.toml']]
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        result, calls = recorded(directory, 'cargo', BUILD, {'EMSCRIPTEN_CRATES': json.dumps(names)})
        assert result.returncode == 0, result.stderr
        assert calls == [f'build --locked --target {EMSCRIPTEN} ' + (f'--manifest-path {manifest} ' if manifest else '') + f'-p {name}'
                         for name, manifest in names], calls
        # A package name is an argument, never shell text.
        assert not (directory / 'injected').exists()

FAKE_CURL = '''#!/usr/bin/env python3
import json, os, pathlib, shutil, sys
args = sys.argv[1:]
url = next(arg for arg in args if arg.startswith('https://'))
with open(os.environ['FAKE_LOG'], 'a', encoding='utf-8') as log:
    log.write(json.dumps({'url': url, 'args': args}) + '\\n')
shutil.copyfile(json.loads(pathlib.Path(os.environ['FAKE_SERVED']).read_text(encoding='utf-8'))[url],
                args[args.index('--output') + 1])
'''

# Stands in for the real installer: it refuses to install unless both checksummed archives sit in its download folder.
FAKE_EMSDK = '''#!/usr/bin/env python3
import json, os, pathlib, sys
here = pathlib.Path(__file__).resolve().parent
with open(os.environ['FAKE_LOG'], 'a', encoding='utf-8') as log:
    log.write(json.dumps({'emsdk': sys.argv[1:], 'keep': os.environ.get('EMSDK_KEEP_DOWNLOADS'),
                          'token': os.environ.get('GH_TOKEN'), 'files': sorted(p.name for p in (here / 'downloads').iterdir())}) + '\\n')
if sys.argv[1] == 'install':
    emcc = here / 'upstream/emscripten/emcc'
    emcc.parent.mkdir(parents=True)
    emcc.write_text('#!/bin/sh\\necho "emcc (Emscripten) ' + os.environ['FAKE_EMCC_VERSION'] + ' (fake)"\\n', encoding='utf-8')
    emcc.chmod(0o755)
'''


def archive_of(path, entries):
    stream = io.BytesIO()
    with tarfile.TarFile(fileobj=stream, mode='w') as archive:
        for name, (content, mode) in entries.items():
            info = tarfile.TarInfo(name)
            info.size = len(content)
            info.mode = mode
            archive.addfile(info, io.BytesIO(content))
    path.write_bytes(gzip.compress(stream.getvalue(), mtime=0))


def installation(directory, mismatch=None, compiler='6.0.10'):
    # Serves a stand-in for each pinned URL and swaps its pinned digest for the stand-in's, except
    # for the archive at index `mismatch`, which keeps the real digest and so fails verification.
    body = steps(document['jobs']['sandbox-runtime'])[SDK]['run']
    pinned = re.findall(r'\b[0-9a-f]{64}\b', body)
    urls = re.findall(r'https://\S+', body)
    assert len(pinned) == 3 and len(urls) == 3 and '6.0.10' in urls[0], (pinned, urls)
    served = {}
    for index, url in enumerate(urls):
        path = directory / f'served-{index}'
        if index == 0:
            archive_of(path, {'emsdk-6.0.10/emsdk': (FAKE_EMSDK.encode(), 0o755)})
        else:
            path.write_bytes(f'archive {index}'.encode())
        served[url] = str(path)
        if index != mismatch:
            body = body.replace(pinned[index], hashlib.sha256(path.read_bytes()).hexdigest())
    (directory / 'served.json').write_text(json.dumps(served), encoding='utf-8')
    tools = directory / 'bin'
    tools.mkdir()
    (tools / 'curl').write_text(FAKE_CURL, encoding='utf-8')
    (tools / 'curl').chmod(0o700)
    for name in ['GITHUB_PATH', 'GITHUB_ENV', 'log']:
        (directory / name).write_text('', encoding='utf-8')
    temp = directory / 'runner-temp'
    temp.mkdir()
    env = {key: value for key, value in os.environ.items() if key != 'GH_TOKEN'} | {
        'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'GH_TOKEN': CANARY, 'RUNNER_TEMP': str(temp),
        'GITHUB_PATH': str(directory / 'GITHUB_PATH'), 'GITHUB_ENV': str(directory / 'GITHUB_ENV'),
        'FAKE_EMCC_VERSION': compiler, 'FAKE_LOG': str(directory / 'log'), 'FAKE_SERVED': str(directory / 'served.json')}
    result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', body], cwd=directory, env=env, text=True, capture_output=True)
    calls = [json.loads(line) for line in (directory / 'log').read_text(encoding='utf-8').splitlines()]
    return result, calls, urls


def the_sdk_is_downloaded_with_the_job_token_and_installed_from_verified_archives():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        result, calls, urls = installation(directory)
        assert result.returncode == 0, result.stdout + result.stderr
        requests = [call for call in calls if 'url' in call]
        assert [call['url'] for call in requests] == urls, requests
        for call in requests:
            args = call['args']
            # Every request refuses plain HTTP, including through redirects.
            assert args[args.index('--proto') + 1] == '=https' and args[args.index('--proto-redir') + 1] == '=https', args
            header = f'Authorization: Bearer {CANARY}'
            # Only the github.com request carries the job token.
            assert (header in args) == call['url'].startswith('https://github.com/'), args
        installer = [call for call in calls if 'emsdk' in call]
        assert [call['emsdk'] for call in installer] == [['install', '6.0.10'], ['activate', '6.0.10']], installer
        assert installer[0]['keep'] == '1', installer
        assert installer[0]['files'] == sorted(['node-v24.19.0-linux-x64.tar.xz',
                                                '666337b525e673e769121856d175f6f52b8ead64-wasm-binaries.tar.xz']), installer
        # The installer never sees the token.
        assert all(call['token'] is None for call in installer), installer
        exported = (directory / 'GITHUB_PATH').read_text(encoding='utf-8').splitlines()
        assert len(exported) == 1 and exported[0].endswith('/upstream/emscripten'), exported
        environment = dict(line.split('=', 1) for line in (directory / 'GITHUB_ENV').read_text(encoding='utf-8').splitlines())
        assert environment['EMSDK_NODE'] == environment['EMSDK'] + '/node/24.19.0_64bit/bin/node', environment
        assert pathlib.Path(exported[0]).parent.parent == pathlib.Path(environment['EMSDK']), (exported, environment)


def a_wrong_checksum_stops_the_install_before_the_installer_runs():
    for index in range(3):
        with tempfile.TemporaryDirectory() as temporary:
            result, calls, _ = installation(pathlib.Path(temporary), mismatch=index)
            assert result.returncode != 0, index
            assert not [call for call in calls if 'emsdk' in call], (index, calls)


def a_compiler_of_another_version_stops_the_install():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        result, _, _ = installation(directory, compiler='6.0.9')
        assert result.returncode != 0
        # Nothing reaches later steps.
        assert (directory / 'GITHUB_PATH').read_text(encoding='utf-8') == ''
        assert (directory / 'GITHUB_ENV').read_text(encoding='utf-8') == ''


for check in [the_declaration_checks_types_and_packages, the_declaration_names_the_paths_that_decide_each_build,
              only_the_script_job_installs_the_sdk, a_pull_request_builds_only_what_its_change_reaches,
              the_target_comes_from_rustup,
              the_sdk_is_downloaded_with_the_job_token_and_installed_from_verified_archives,
              a_wrong_checksum_stops_the_install_before_the_installer_runs, a_compiler_of_another_version_stops_the_install]:
    check()
    print('PASS ' + check.__name__, flush=True)
