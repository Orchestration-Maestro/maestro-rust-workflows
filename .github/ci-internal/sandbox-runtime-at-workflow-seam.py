import ast
import json
import os
import pathlib
import re
import shutil
import subprocess
import tempfile

root = pathlib.Path.cwd()
workflow = (root / '.github/workflows/ci.yml').read_text(encoding='utf-8')
document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
jobs = document['jobs']
JOB = 'sandbox-runtime'
SCRIPT = 'target/sandbox-runtime/maestro-sandbox-runtime.js'
ARTIFACT = 'sandbox-runtime-${{ inputs.concurrency-key || \'root\' }}'
# Every job that compiles the caller's workspace or the fuzz manifest.
CONSUMERS = ['quality', 'docs', 'test-build', 'test-prepare', 'tests', 'checks', 'fuzz', 'targets', 'browser', 'browser-tests']


def steps(job):
    return {step['name']: step for step in job['steps']}


def opens(condition, context):
    expression = str(condition).removeprefix('${{').removesuffix('}}').strip()
    for key in sorted(context, key=len, reverse=True):
        expression = expression.replace(key, repr(context[key]))
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


def add_isolated(directory):
    # A package of its own locked workspace, reachable only through the manifest that declares it.
    manifest = directory / 'isolated/Cargo.toml'
    (manifest.parent / 'src').mkdir(parents=True)
    (manifest.parent / 'src/lib.rs').write_text('', encoding='utf-8')
    manifest.write_text('[workspace]\n[package]\nname = "isolated"\nversion = "0.1.0"\nedition = "2021"\nlicense = "MIT"\n', encoding='utf-8')
    subprocess.run(['cargo', 'generate-lockfile', '--offline', '--manifest-path', str(manifest)], check=True, capture_output=True)


def context(extra=None, crates='[["web", null]]'):
    values = {'inputs.browser-only': False, 'inputs.advisory-scan': False, 'inputs.fuzz-scan': False, 'inputs.warm-cache': False,
              'needs.declaration.outputs.emscripten_crates': crates}
    return values | (extra or {})


def the_job_exists_only_for_a_caller_that_declares_emscripten_crates():
    job = jobs[JOB]
    assert job['needs'] == 'declaration', job['needs']
    assert opens(job['if'], context()), 'a declared caller generates the script'
    assert opens(job['if'], context({'inputs.fuzz-scan': True, 'inputs.warm-cache': True})), 'fuzz and warm runs compile too'
    assert opens(job['if'], context({'inputs.browser-only': True})), 'the browser-only proof compiles the caller'
    assert not opens(job['if'], context(crates='[]')), 'a caller with an empty list installs and runs nothing'
    assert not opens(job['if'], context({'inputs.advisory-scan': True})), 'the advisory scan compiles nothing'
    # Only the cache key reads the inputs of the script, and the declaration is what names them.
    assert jobs['declaration']['outputs']['sandbox_runtime_paths'] == '${{ steps.config.outputs.sandbox_runtime_paths }}'


def the_cached_script_skips_every_step_that_prepares_it():
    job = steps(jobs[JOB])
    order = [step['name'] for step in jobs[JOB]['steps']]
    assert order == ['Runner hardware', 'Checkout', 'Script identity', 'Restore sandbox runtime', 'Toolchain', 'Mise bootstrap',
                     'Test tools', 'Emscripten SDK', 'Emscripten target', 'Build sandbox runtime script', 'Save sandbox runtime',
                     'Upload sandbox runtime'], order
    hit = {'steps.script-cache.outputs.cache-hit': 'true'}
    miss = {'steps.script-cache.outputs.cache-hit': 'false'}
    # A hit costs the checkout, the key and the restore: no toolchain, no tools, no SDK, no target and no build.
    for name in ['Toolchain', 'Mise bootstrap', 'Test tools', 'Emscripten SDK', 'Emscripten target', 'Build sandbox runtime script']:
        assert not opens(job[name]['if'], hit), (name, 'a hit runs nothing that prepares the script')
        assert opens(job[name]['if'], miss), (name, 'a miss prepares it')
    # The consumers still receive the file on a hit, and a file that never arrived fails the upload.
    upload = job['Upload sandbox runtime']
    assert 'if' not in upload, upload
    assert upload['uses'].startswith('actions/upload-artifact@')
    assert upload['with'] == {'name': ARTIFACT, 'path': "${{ inputs.working-directory || '.' }}/" + SCRIPT, 'if-no-files-found': 'error'}, upload['with']
    # One key for both: the entry the restore reads is the entry the build writes.
    script = "${{ inputs.working-directory || '.' }}/" + SCRIPT
    restore, save = job['Restore sandbox runtime'], job['Save sandbox runtime']
    assert restore['uses'].startswith('actions/cache/restore@') and restore['with'] == {'path': script, 'key': '${{ steps.script-identity.outputs.key }}'}, restore
    assert save['uses'].startswith('actions/cache/save@') and save['with'] == restore['with'], save
    # A cache that cannot be read or written costs the preparation, never the job.
    assert restore['continue-on-error'] is True and save['continue-on-error'] is True
    # Only a default-branch push writes, so a merge group reads what the last push saved.
    written = {'steps.script-cache.outputs.cache-hit': 'false', 'github.event_name': 'push', 'github.ref': 'refs/heads/main',
               'github.event.repository.default_branch': 'main', "format('refs/heads/{0}', github.event.repository.default_branch)": 'refs/heads/main'}
    assert opens(save['if'], written), save['if']
    for branch in [written | {'github.event_name': 'pull_request', 'github.ref': 'refs/pull/363/merge'},
                   written | {'github.ref': 'refs/heads/topic'}, written | {'github.event_name': 'merge_group'}]:
        assert not opens(save['if'], branch), (save['if'], branch)
    assert not opens(save['if'], written | {'steps.script-cache.outputs.cache-hit': 'true'}), 'a hit writes nothing'
    build = job['Build sandbox runtime script']['run'].splitlines()
    assert build[0] == 'mise exec -- just sandbox-runtime', build
    assert build[1] == 'test -f ' + SCRIPT, 'a recipe that writes nothing fails here'
    # The job reaches the toolchain and the tools through the shared step bodies, not a second copy.
    for name in ['Mise bootstrap', 'Test tools']:
        assert job[name]['run'] == steps(jobs['quality'])[name]['run'] and job[name]['env'] == steps(jobs['quality'])[name]['env'], name
    assert job['Emscripten SDK']['env'] == {'GH_TOKEN': '${{ github.token }}'}


DECLARATION = next(step for step in jobs['declaration']['steps'] if step['name'] == 'Read caller declaration')['run']
IDENTITY = steps(jobs[JOB])['Script identity']['run']


def declared(directory, text):
    (directory / '.github/ci.toml').write_text('wasm_crates=[]\ncheck_targets=[]\n' + text, encoding='utf-8')
    output = directory / 'declaration-output'
    output.write_text('', encoding='utf-8')
    env = os.environ | {'GITHUB_OUTPUT': str(output), 'TEST_EVENT': 'merge_group', 'WARM': 'false'}
    result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', DECLARATION], cwd=directory, env=env, text=True, capture_output=True)
    lines = output.read_text(encoding='utf-8').splitlines()
    return result, dict(line.split('=', 1) for line in lines if '=' in line)


WORKFLOW = 'c' * 40


def identity(directory, paths, sha=WORKFLOW, run='1'):
    output = directory.parent / 'identity-output'
    output.write_text('', encoding='utf-8')
    env = os.environ | {'SANDBOX_RUNTIME_PATHS': json.dumps(paths), 'WORKFLOW_SHA': sha, 'GITHUB_OUTPUT': str(output),
                        'GITHUB_RUN_ID': run, 'GITHUB_RUN_ATTEMPT': '1'}
    result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', IDENTITY], cwd=directory, env=env, text=True, capture_output=True)
    assert result.returncode == 0, result.stdout + result.stderr
    return dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())['key']


def the_cache_key_covers_every_input_of_the_script():
    # The listed crates and the packages the recipe runs generate the script; the tool pins and the recipe complete it.
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary, 'checkout')
        shutil.copytree(root / 'fixtures/wasm-witnesses', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        (directory / '.github').mkdir()
        (directory / 'justfile').write_text('sandbox-runtime:\n    @cargo run --quiet --locked -p witness -- sandbox-runtime\n', encoding='utf-8')
        (directory / 'mise.toml').write_text('[tools]\njust = "1"\n', encoding='utf-8')
        result, values = declared(directory, 'emscripten_crates=["other"]\n')
        assert result.returncode == 0, result.stdout + result.stderr
        paths = json.loads(values['sandbox_runtime_paths'])
        assert paths == ['Cargo.lock', 'Cargo.toml', 'justfile', 'mise.toml', 'other', 'rust-toolchain.toml',
                         'witness', 'witness-support'], paths
        key = identity(directory, paths)
        # Nothing that decides the script changed: the key stands, so the next run starts without preparing it.
        (directory / 'docs.md').write_text('Documentation\n', encoding='utf-8')
        assert identity(directory, paths) == key
        def changing(name, text, sha=WORKFLOW):
            original = (directory / name).read_text(encoding='utf-8')
            (directory / name).write_text(text, encoding='utf-8')
            try:
                return identity(directory, paths, sha)
            finally:
                (directory / name).write_text(original, encoding='utf-8')
        # Every input misses the cache: the generator's own sources and the path dependencies it declares first, then
        # the lock, the recipe and the tool pins.
        for name, text in [
                ('witness/src/lib.rs', 'pub fn changed() {}\n'),
                ('witness-support/src/lib.rs', 'pub fn changed() {}\n'),
                ('Cargo.lock', (directory / 'Cargo.lock').read_text(encoding='utf-8') + '# changed\n'),
                ('Cargo.toml', (directory / 'Cargo.toml').read_text(encoding='utf-8') + '# changed\n'),
                ('justfile', 'sandbox-runtime:\n    @cargo run --quiet --locked -p witness -- sandbox-runtime --check\n'),
                ('mise.toml', '[tools]\njust = "2"\n'),
                ('rust-toolchain.toml', (directory / 'rust-toolchain.toml').read_text(encoding='utf-8') + '# changed\n'),
                ('other/src/lib.rs', 'pub fn changed() {}\n')]:
            assert changing(name, text) != key, name
        # The workflow that runs the recipe decides the script too.
        assert identity(directory, paths, 'd' * 40) != key
        # A listed crate of an additional locked workspace is an input, with the manifest and lock file that declare it.
        add_isolated(directory)
        result, values = declared(directory, 'additional_locked_manifests=["isolated/Cargo.toml"]\nemscripten_crates=["isolated"]\n')
        assert result.returncode == 0, result.stdout + result.stderr
        assert json.loads(values['sandbox_runtime_paths']) == [
            'Cargo.lock', 'Cargo.toml', 'isolated', 'isolated/Cargo.lock', 'isolated/Cargo.toml', 'justfile', 'mise.toml',
            'rust-toolchain.toml', 'witness', 'witness-support'], values
        # A recipe that names no package leaves the whole checkout to decide the key, so it can never serve an old script.
        (directory / 'justfile').write_text('sandbox-runtime:\n    @sh build-sandbox-runtime.sh\n', encoding='utf-8')
        result, values = declared(directory, 'emscripten_crates=["other"]\n')
        assert result.returncode == 0, result.stdout + result.stderr
        whole = json.loads(values['sandbox_runtime_paths'])
        assert whole == ['.'], whole
        key = identity(directory, whole)
        (directory / 'docs.md').write_text('Documentation, changed\n', encoding='utf-8')
        assert identity(directory, whole) != key
        # A caller that lists no Emscripten crate names no input, so the job stays off.
        result, values = declared(directory, '')
        assert json.loads(values['sandbox_runtime_paths']) == [], values


def the_cache_key_uses_the_shared_workflow_revision():
    # A called workflow sees the caller's commit in `github.workflow_sha`, so the key takes the one revision this
    # workflow resolved for the run, and a revision that stays unresolved never matches a saved entry.
    assert steps(jobs[JOB])['Script identity']['env'] == {
        'SANDBOX_RUNTIME_PATHS': '${{ needs.declaration.outputs.sandbox_runtime_paths }}',
        'WORKFLOW_SHA': '${{ needs.declaration.outputs.workflow_revision }}'}, steps(jobs[JOB])['Script identity']['env']
    assert jobs['declaration']['outputs']['workflow_revision'] == '${{ steps.workflow-revision.outputs.revision }}'
    revision = steps(jobs['declaration'])['Shared workflow revision']
    # The script job runs on pull requests too, so the revision is resolved on every event.
    assert 'if' not in revision, revision
    assert revision['env'] == {'GH_TOKEN': '${{ github.token }}', 'CURRENT_WORKFLOW_REF': '${{ github.workflow_ref }}',
                               'CURRENT_WORKFLOW_SHA': '${{ github.workflow_sha }}'}, revision['env']
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary, 'checkout')
        directory.mkdir()
        (directory / 'input.txt').write_text('input\n', encoding='utf-8')
        assert identity(directory, ['input.txt']) == identity(directory, ['input.txt'], run='2')
        assert identity(directory, ['input.txt'], '') != identity(directory, ['input.txt'], '', run='2')


def every_compiling_job_waits_for_the_script_and_downloads_it_before_compiling():
    for name in CONSUMERS:
        job = jobs[name]
        needs = job['needs'] if isinstance(job['needs'], list) else [job['needs']]
        assert JOB in needs and 'declaration' in needs, (name, needs)
        order = [step['name'] for step in job['steps']]
        step = steps(job)['Sandbox runtime script']
        assert step['uses'].startswith('actions/download-artifact@'), name
        assert step['if'] == "needs.declaration.outputs.emscripten_crates != '[]'", name
        assert step['with'] == {'name': ARTIFACT, 'path': "${{ inputs.working-directory || '.' }}/target/sandbox-runtime"}, (name, step['with'])
        # Straight after the checkout, so no cargo command comes first.
        assert order.index('Sandbox runtime script') == order.index('Checkout') + 1, (name, order[:6])
    # The doctests job runs the same steps as the test shards.
    assert jobs['doctests']['steps'] == jobs['tests']['steps'] and JOB in jobs['doctests']['needs']


def a_skipped_generator_does_not_skip_a_job_and_a_failed_one_fails_it():
    for name in CONSUMERS + ['doctests']:
        condition = jobs[name]['if']
        # GitHub skips a job whose needed job was skipped unless its condition holds a status function.
        assert '!cancelled()' in condition, name
        if name != 'tests':
            # A broken declaration still stops everything, as it did when the job needed only the declaration.
            assert condition.startswith("${{ !cancelled() && needs.declaration.result == 'success' && (") and condition.endswith(") }}"), name
        # A failed generator lets the job start, and the download then fails it, so a required check never turns into a skip.
        assert 'needs.sandbox-runtime' not in condition, name


the_job_exists_only_for_a_caller_that_declares_emscripten_crates()
print('PASS the_job_exists_only_for_a_caller_that_declares_emscripten_crates', flush=True)
the_cached_script_skips_every_step_that_prepares_it()
print('PASS the_cached_script_skips_every_step_that_prepares_it', flush=True)
the_cache_key_covers_every_input_of_the_script()
print('PASS the_cache_key_covers_every_input_of_the_script', flush=True)
the_cache_key_uses_the_shared_workflow_revision()
print('PASS the_cache_key_uses_the_shared_workflow_revision', flush=True)
every_compiling_job_waits_for_the_script_and_downloads_it_before_compiling()
print('PASS every_compiling_job_waits_for_the_script_and_downloads_it_before_compiling', flush=True)
a_skipped_generator_does_not_skip_a_job_and_a_failed_one_fails_it()
print('PASS a_skipped_generator_does_not_skip_a_job_and_a_failed_one_fails_it', flush=True)
