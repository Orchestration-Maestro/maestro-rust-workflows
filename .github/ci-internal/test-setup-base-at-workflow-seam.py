import json
import os
import pathlib
import subprocess
import tempfile

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
