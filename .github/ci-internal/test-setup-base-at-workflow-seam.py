import ast
import json
import os
import pathlib
import subprocess
import tempfile
import textwrap

workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
steps = {step['name']: step for step in document['jobs']['test-prepare']['steps']}
PREPARE = 'Prepare test setup'

def execute(name, cwd, extra):
    env = {key: value for key, value in os.environ.items() if not key.startswith('TEST_PREPARE')} | extra
    return subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', steps[name]['run']],
                          cwd=cwd, env=env, text=True, capture_output=True)

def bundle(directory, files):
    source = directory / 'source/test-setup'
    source.mkdir(parents=True)
    for name, text in files.items():
        (source / name).write_text(text, encoding='utf-8')
    subprocess.run(['tar', '-C', str(directory / 'source'), '-cf', str(directory / 'test-setup.tar'), 'test-setup'], check=True)

PROBE = ('{ echo "base=${TEST_PREPARE_BASE-unset}"; echo "output=$TEST_PREPARE_OUTPUT";'
         ' if [ -n "${TEST_PREPARE_BASE-}" ]; then IFS=: read -ra bases <<< "$TEST_PREPARE_BASE";'
         ' echo "seen=$(for base in "${bases[@]}"; do cat "$base/kept"; done | paste -sd,)"; fi;'
         ' ls "$TEST_PREPARE_OUTPUT" | wc -l | sed "s/^/outputs=/"; } > "$PWD/probe"; echo new > "$TEST_PREPARE_OUTPUT/kept"')

def probe(runner, checkout):
    return dict(line.split('=', 1) for line in (checkout / 'probe').read_text(encoding='utf-8').splitlines())

def prepare_gets_the_restored_bundle_as_its_base():
    with tempfile.TemporaryDirectory() as temporary:
        runner = pathlib.Path(temporary) / 'runner'
        checkout = pathlib.Path(temporary) / 'caller'
        runner.mkdir()
        checkout.mkdir()
        bundle(runner, {'kept': 'old\n'})
        result = execute(PREPARE, checkout, {'RUNNER_TEMP': str(runner), 'TEST_PREPARE': PROBE})
        assert result.returncode == 0, result.stdout + result.stderr
        seen = probe(runner, checkout)
        base = pathlib.Path(seen['base'])
        # The base is its own directory, apart from the output, and the output starts empty.
        assert base != pathlib.Path(seen['output']) and base.is_relative_to(runner), seen
        assert seen['seen'] == 'old' and seen['outputs'] == '0', seen
        listing = subprocess.check_output(['tar', '-tf', str(runner / 'test-setup.tar')], text=True)
        assert 'test-setup/kept' in listing
        assert subprocess.check_output(['tar', '-xOf', str(runner / 'test-setup.tar'), 'test-setup/kept'], text=True) == 'new\n'

def prepare_without_a_restored_bundle_has_no_base():
    with tempfile.TemporaryDirectory() as temporary:
        runner = pathlib.Path(temporary) / 'runner'
        checkout = pathlib.Path(temporary) / 'caller'
        runner.mkdir()
        checkout.mkdir()
        result = execute(PREPARE, checkout, {'RUNNER_TEMP': str(runner), 'TEST_PREPARE': PROBE, 'TEST_PREPARE_BASE': '/inherited'})
        assert result.returncode == 0, result.stdout + result.stderr
        assert probe(runner, checkout)['base'] == 'unset'

def prepare_ignores_an_unreadable_bundle():
    with tempfile.TemporaryDirectory() as temporary:
        runner = pathlib.Path(temporary) / 'runner'
        checkout = pathlib.Path(temporary) / 'caller'
        runner.mkdir()
        checkout.mkdir()
        (runner / 'test-setup.tar').write_bytes(b'not a tarball')
        result = execute(PREPARE, checkout, {'RUNNER_TEMP': str(runner), 'TEST_PREPARE': PROBE})
        assert result.returncode == 0, result.stdout + result.stderr
        assert '::warning::' in result.stdout and probe(runner, checkout)['base'] == 'unset'

def prepare_lists_the_restored_bundle_then_each_candidate():
    with tempfile.TemporaryDirectory() as temporary:
        runner = pathlib.Path(temporary) / 'runner'
        checkout = pathlib.Path(temporary) / 'caller'
        runner.mkdir()
        checkout.mkdir()
        bundle(runner, {'kept': 'restored\n'})
        for run, text in [('11', 'first'), ('12', 'unreadable'), ('13', 'second')]:
            bundle(runner / 'test-setup-candidates' / run, {'kept': text + '\n'})
        (runner / 'test-setup-candidates/12/test-setup.tar').write_bytes(b'not a tarball')
        # Run 14 never downloaded: nothing to list.
        result = execute(PREPARE, checkout, {'RUNNER_TEMP': str(runner), 'TEST_PREPARE': PROBE, 'TEST_SETUP_CANDIDATE_RUNS': '11 12 13 14'})
        assert result.returncode == 0, result.stdout + result.stderr
        seen = probe(runner, checkout)
        assert seen['seen'] == 'restored,first,second' and len(seen['base'].split(':')) == 3, seen
        assert result.stdout.count('::warning::') == 1, result.stdout

def prepare_lists_candidates_without_a_restored_bundle():
    with tempfile.TemporaryDirectory() as temporary:
        runner = pathlib.Path(temporary) / 'runner'
        checkout = pathlib.Path(temporary) / 'caller'
        runner.mkdir()
        checkout.mkdir()
        bundle(runner / 'test-setup-candidates' / '11', {'kept': 'first\n'})
        result = execute(PREPARE, checkout, {'RUNNER_TEMP': str(runner), 'TEST_PREPARE': PROBE, 'TEST_SETUP_CANDIDATE_RUNS': '11'})
        assert result.returncode == 0, result.stdout + result.stderr
        assert probe(runner, checkout)['seen'] == 'first'

for check in [prepare_gets_the_restored_bundle_as_its_base, prepare_lists_the_restored_bundle_then_each_candidate,
              prepare_lists_candidates_without_a_restored_bundle, prepare_without_a_restored_bundle_has_no_base,
              prepare_ignores_an_unreadable_bundle]:
    check()
    print('PASS ' + check.__name__, flush=True)

def prepare_hands_the_coverage_build_environment_to_the_command():
    # A command that prepares a test child builds it with the instrumentation the coverage run uses.
    assert document['jobs']['test-prepare']['env']['COLLECT_COVERAGE'] == document['jobs']['test-build']['env']['COLLECT_COVERAGE']
    tools = next(step for step in document['jobs']['test-prepare']['steps'] if step['name'] == 'Coverage tools')
    assert tools['if'] == "env.COLLECT_COVERAGE == 'true'", tools
    with tempfile.TemporaryDirectory() as temporary:
        runner = pathlib.Path(temporary) / 'runner'
        checkout = pathlib.Path(temporary) / 'caller'
        stand_in = pathlib.Path(temporary) / 'tools'
        for directory in (runner, checkout, stand_in):
            directory.mkdir()
        (stand_in / 'cargo-llvm-cov').write_text(
            '#!/bin/sh\necho "$CARGO_LLVM_COV_TARGET_DIR $*" > "$STAND_IN_LOG"\n'
            "cat <<'ENV'\nexport LLVM_PROFILE_FILE='/shard/maestro-%p-%8m.profraw'\nexport RUSTC_WRAPPER=/tools/cargo-llvm-cov\n"
            'export CARGO_LLVM_COV=1\nexport CARGO_LLVM_COV_SHOW_ENV=1\nexport CARGO_LLVM_COV_TARGET_DIR=/shard\n'
            'export CARGO_LLVM_COV_BUILD_DIR=/shard\nENV\n', encoding='utf-8')
        (stand_in / 'cargo-llvm-cov').chmod(0o755)
        command = 'echo "environment=${TEST_PREPARE_COVERAGE_ENV-unset}" > "$PWD/probe"; if [ -n "${TEST_PREPARE_COVERAGE_ENV-}" ]; then cp "$TEST_PREPARE_COVERAGE_ENV" "$PWD/environment"; fi'
        extra = {'RUNNER_TEMP': str(runner), 'TEST_PREPARE': command, 'TOOLS': str(stand_in), 'STAND_IN_LOG': str(runner / 'log')}
        result = execute(PREPARE, checkout, extra | {'COLLECT_COVERAGE': 'false', 'TEST_PREPARE_COVERAGE_ENV': '/inherited'})
        assert result.returncode == 0, result.stdout + result.stderr
        assert probe(runner, checkout)['environment'] == 'unset' and not (runner / 'log').exists()
        result = execute(PREPARE, checkout, extra | {'COLLECT_COVERAGE': 'true'})
        assert result.returncode == 0, result.stdout + result.stderr
        assert (runner / 'log').read_text(encoding='utf-8') == f'{checkout}/target/llvm-cov-target llvm-cov show-env --sh\n'
        lines = (checkout / 'environment').read_text(encoding='utf-8').splitlines()
        assert 'export RUSTC_WRAPPER=/tools/cargo-llvm-cov' in lines and 'export CARGO_LLVM_COV=1' in lines, lines
        # Only show-env sets these; the compile cache hashes every CARGO_ variable, so they would miss every cached compilation.
        assert not [line for line in lines if line.startswith(('export CARGO_LLVM_COV_TARGET_DIR', 'export CARGO_LLVM_COV_BUILD_DIR', 'export CARGO_LLVM_COV_SHOW_ENV'))], lines

for check in [prepare_hands_the_coverage_build_environment_to_the_command]:
    check()
    print('PASS ' + check.__name__, flush=True)

def setup_cache_identity_separates_host_bundles():
    assert 'test_runners' in document['jobs']['test-prepare']['strategy']['matrix']['runner']
    assert 'steps.cache-identity.outputs.prefix' in steps['Test setup cache identity']['env']['COMPILE_CACHE_PREFIX']
    identity = next(step for step in document['jobs']['test-prepare']['steps'] if step['name'] == 'Compile cache identity')
    assert 'if' not in identity, 'Windows still needs a host-specific identity without compile caching'
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        (directory / 'input').write_text('bundle input', encoding='utf-8')
        # The identity hashes the pinned search programs, so stand-ins lead the path.
        programs = directory / 'programs'
        programs.mkdir()
        for name in ('rg', 'fd'):
            (programs / name).write_bytes(b'stand-in')
            (programs / name).chmod(0o755)
        def key(prefix, coverage='false'):
            output = directory / 'output'
            output.write_text('', encoding='utf-8')
            result = execute('Test setup cache identity', directory, {'COMPILE_CACHE_PREFIX': prefix, 'TEST_PREPARE_INPUTS': '["input"]',
                             'GITHUB_WORKSPACE': '/checkout', 'TEST_PREPARE': 'prepare', 'GITHUB_OUTPUT': str(output), 'COLLECT_COVERAGE': coverage,
                             'PATH': str(programs) + os.pathsep + os.environ['PATH']})
            assert result.returncode == 0, result.stderr
            return output.read_text(encoding='utf-8')
        assert key('linux-compiler') == key('linux-compiler')
        assert len({key(host + '-compiler') for host in ['linux', 'macos', 'windows']}) == 3
        # A bundle prepared for the coverage build holds instrumented programs, so it never answers a plain build.
        assert key('linux-compiler', 'true') != key('linux-compiler', 'false')

setup_cache_identity_separates_host_bundles()
print('PASS setup_cache_identity_separates_host_bundles')

# The helpers keep one owner: the caller checks step, which runs them against recording tools.
helpers = {}
code = pathlib.Path('.github/ci-internal/caller-checks-at-workflow-seam.py').read_text(encoding='utf-8')
tree = ast.parse(textwrap.dedent(code))
needed = {'workflow', 'root', 'credential_names', 'coverage_report_args'}
tree.body = [node for node in tree.body if isinstance(node, (ast.Import, ast.ImportFrom, ast.FunctionDef))
             or (isinstance(node, ast.Assign) and any(isinstance(target, ast.Name) and target.id in needed
                                                     for target in node.targets))]
exec(compile(tree, 'test prepare base helpers', 'exec'), helpers)  # noqa: S102

# The build host exports a non-empty base for every job; the workflow wires the same expression.
BASE = '$' + '{{ runner.temp }}/maestro-test-prepare-base'

def workspace_tests_run_with_a_prepare_base():
    for job, step in [('tests', 'Tests'), ('tests', 'Isolated tests'), ('doctests', 'Tests'), ('checks', 'Coverage')]:
        chunk, indent = helpers['workflow_steps']()[(job, step)]
        assert helpers['block_map'](chunk, indent).get('TEST_PREPARE_BASE') == BASE, (job, step)

def isolated_tests_forward_the_prepare_base_to_the_child():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        tools, log = helpers['install_recording_runner'](directory)
        base = temporary + '/maestro-test-prepare-base'
        result = helpers['execute']('Isolated tests', directory, {
            'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'TOOLS': str(tools),
            'RUNNER_TEMP': temporary, 'SHARD': '2', 'COLLECT_COVERAGE': 'false',
            'CARGO_COVERAGE_MIN_LINES': '', 'TEST_PACKAGES': '["sample"]', 'TEST_PREPARE_BASE': base})
        assert result.returncode == 0, result.stdout + result.stderr
        commands = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
        assert commands, result.stdout + result.stderr
        # The isolated child starts with a cleared environment, so the base reaches it only if the step forwards it.
        assert [command['test_prepare_base'] for command in commands] == [base] * len(commands), commands

workspace_tests_run_with_a_prepare_base()
print('PASS workspace_tests_run_with_a_prepare_base')
isolated_tests_forward_the_prepare_base_to_the_child()
print('PASS isolated_tests_forward_the_prepare_base_to_the_child')
