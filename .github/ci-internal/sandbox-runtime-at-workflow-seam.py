import ast
import json
import pathlib
import re
import subprocess

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
    assert 'emscripten_paths' not in jobs['declaration']['outputs'], 'only the cache key read it'


def the_script_is_built_on_every_run_with_the_installed_sdk():
    job = steps(jobs[JOB])
    # A cached script would hide a change to the generator, so no step restores, saves or skips on a cache.
    # Only the runner-hardware record carries a condition: the guards require it on every Linux job.
    assert not any('cache' in step.get('uses', '') or ('if' in step and step['name'] != 'Runner hardware')
                   for step in jobs[JOB]['steps']), 'every step runs on every run'
    # The generator reuses the one SDK, target and tool install of the other jobs.
    browser, quality = steps(jobs['browser']), steps(jobs['quality'])
    for name in ['Emscripten SDK', 'Emscripten target']:
        assert job[name]['run'] == browser[name]['run'] and job[name].get('env') == browser[name].get('env'), name
    for name in ['Mise bootstrap', 'Test tools']:
        assert job[name]['run'] == quality[name]['run'] and job[name]['env'] == quality[name]['env'], name
    order = [step['name'] for step in jobs[JOB]['steps']]
    assert order == ['Runner hardware', 'Checkout', 'Toolchain', 'Mise bootstrap', 'Test tools', 'Emscripten SDK', 'Emscripten target',
                     'Build sandbox runtime script', 'Upload sandbox runtime'], order
    build = job['Build sandbox runtime script']['run'].splitlines()
    assert build[0] == 'mise exec -- just sandbox-runtime', build
    assert build[1] == 'test -f ' + SCRIPT, 'a recipe that writes nothing fails here'
    upload = job['Upload sandbox runtime']
    assert upload['uses'].startswith('actions/upload-artifact@')
    assert upload['with'] == {'name': ARTIFACT, 'path': "${{ inputs.working-directory || '.' }}/" + SCRIPT, 'if-no-files-found': 'error'}, upload['with']


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
the_script_is_built_on_every_run_with_the_installed_sdk()
print('PASS the_script_is_built_on_every_run_with_the_installed_sdk', flush=True)
every_compiling_job_waits_for_the_script_and_downloads_it_before_compiling()
print('PASS every_compiling_job_waits_for_the_script_and_downloads_it_before_compiling', flush=True)
a_skipped_generator_does_not_skip_a_job_and_a_failed_one_fails_it()
print('PASS a_skipped_generator_does_not_skip_a_job_and_a_failed_one_fails_it', flush=True)
