import json
import os
import pathlib
import subprocess
import tempfile

workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
steps = {step['name']: step for step in document['jobs']['test-prepare']['steps']}
IDENTITY, RESTORE, PREPARE, SAVE = ('Test setup cache identity', 'Restore test setup cache',
                                    'Prepare test setup', 'Save test setup cache')

def execute(name, cwd, extra):
    env = {key: value for key, value in os.environ.items() if not key.startswith('TEST_PREPARE')} | extra
    return subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', steps[name]['run']],
                          cwd=cwd, env=env, text=True, capture_output=True)

def identity(directory, files, command='echo ready'):
    for name, text in files.items():
        (directory / name).write_text(text, encoding='utf-8')
    output = directory.parent / 'outputs'
    output.write_text('', encoding='utf-8')
    result = execute(IDENTITY, directory, {
        'TEST_PREPARE': command, 'TEST_PREPARE_INPUTS': json.dumps(sorted(files)), 'GITHUB_OUTPUT': str(output),
        'COMPILE_CACHE_PREFIX': 'maestro-compile-v1-abc-', 'GITHUB_WORKSPACE': '/work'})
    assert result.returncode == 0, result.stderr
    return dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())

def the_restore_falls_back_to_the_closest_earlier_bundle():
    restore, save = steps[RESTORE]['with'], steps[SAVE]['with']
    assert restore['key'] == '${{ steps.setup-identity.outputs.key }}'
    assert restore['restore-keys'] == '${{ steps.setup-identity.outputs.prefix }}'
    assert restore['path'] == save['path'] and save['key'] == restore['key']
    assert 'restore-keys' not in save
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary) / 'caller'
        directory.mkdir()
        first = identity(directory, {'a.txt': 'one\n'})
        changed = identity(directory, {'a.txt': 'two\n'})
        other = identity(directory, {'a.txt': 'one\n'}, command='echo other')
        assert first['key'].startswith('maestro-test-setup-v2-')
        assert first['key'].startswith(first['prefix']) and first['prefix'].endswith('-')
        # An edited input keeps the prefix and changes the key; another command is no base.
        assert changed['prefix'] == first['prefix'] and changed['key'] != first['key']
        assert other['prefix'] != first['prefix']

def bundle(directory, files):
    source = directory / 'source/test-setup'
    source.mkdir(parents=True)
    for name, text in files.items():
        (source / name).write_text(text, encoding='utf-8')
    subprocess.run(['tar', '-C', str(directory / 'source'), '-cf', str(directory / 'test-setup.tar'), 'test-setup'], check=True)

PROBE = ('{ echo "base=${TEST_PREPARE_BASE-unset}"; echo "output=$TEST_PREPARE_OUTPUT";'
         ' if [ -n "${TEST_PREPARE_BASE-}" ]; then echo "seen=$(cat "$TEST_PREPARE_BASE/kept")"; fi;'
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

def an_exact_hit_still_skips_preparation():
    assert steps[PREPARE]['if'] == "steps.setup-cache.outputs.cache-hit != 'true'"
    assert steps[SAVE]['if'] == ("${{ steps.cache-identity.outputs.writer == 'true' && steps.setup-cache.outputs.cache-hit != 'true'"
                                 " && needs.declaration.outputs.test_prepare_inputs != '[]' }}")

for check in [the_restore_falls_back_to_the_closest_earlier_bundle, prepare_gets_the_restored_bundle_as_its_base,
              prepare_without_a_restored_bundle_has_no_base, prepare_ignores_an_unreadable_bundle,
              an_exact_hit_still_skips_preparation]:
    check()
    print('PASS ' + check.__name__, flush=True)
