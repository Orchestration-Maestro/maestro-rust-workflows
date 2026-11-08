import itertools
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile

workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
jobs = document['jobs']
PARSE = {step['name']: step for step in jobs['declaration']['steps']}['Read caller declaration']['run']
SHA = 'a' * 40

# The matrix legs of the jobs that can hold a macOS runner; the other keys are what each leg's matrix context carries.
LEGS = {
    'test-build': [{'runner': 'ubuntu-latest'}, {'runner': 'macos-latest'}],
    'test-prepare': [{'runner': 'ubuntu-latest'}, {'runner': 'macos-latest'}],
    'tests': [{'runner': 'ubuntu-latest', 'shard': '1', 'count': '2'}, {'runner': 'ubuntu-latest', 'shard': '2', 'count': '2'},
              {'runner': 'macos-latest', 'shard': '1', 'count': '2'}, {'runner': 'macos-latest', 'shard': '2', 'count': '2'}],
    'doctests': [{'runner': 'ubuntu-latest', 'shard': 'doctests', 'count': 0}, {'runner': 'macos-latest', 'shard': 'doctests', 'count': 0}],
    'targets': [{'target': 'aarch64-apple-darwin', 'runner': 'macos-latest'}, {'target': 'x86_64-apple-darwin', 'runner': 'macos-latest'},
                {'target': 'x86_64-pc-windows-msvc', 'runner': 'windows-latest'}],
}

def parse(text):
    """Read a GitHub expression into nested tuples; supports the literals, contexts, !, ==, !=, &&, || and calls the workflow uses."""
    tokens = re.findall(r"'(?:[^']|'')*'|[A-Za-z_][\w-]*(?:\.[A-Za-z_][\w-]*)*|\d+|==|!=|&&|\|\||[()!,]", text)
    position = 0

    def take():
        nonlocal position
        position += 1
        return tokens[position - 1]

    def peek():
        return tokens[position] if position < len(tokens) else None

    def binary(operators, operand):
        left = operand()
        while peek() in operators:
            operator = take()
            left = (operator, left, operand())
        return left

    def atom():
        word = take()
        if word == '(':
            inner = either()
            assert take() == ')'
            return inner
        if word == '!':
            return ('!', atom())
        if word.startswith("'"):
            return ('literal', word[1:-1].replace("''", "'"))
        if peek() == '(':
            take()
            arguments = []
            while peek() != ')':
                arguments.append(either())
                if peek() == ',':
                    take()
            take()
            return ('call', word, arguments)
        return ('literal', int(word)) if word.isdigit() else ('context', word)

    either = lambda: binary({'||'}, lambda: binary({'&&'}, lambda: binary({'==', '!='}, atom)))  # noqa: E731
    tree = either()
    assert position == len(tokens), (text, tokens[position:])
    return tree

def evaluate(tree, contexts):
    kind = tree[0]
    if kind == 'literal':
        return tree[1]
    if kind == 'context':
        value = contexts
        for part in tree[1].split('.'):
            value = (value or {}).get(part)
        return value
    if kind == '!':
        return not evaluate(tree[1], contexts)
    if kind == 'call':
        if tree[1] == 'cancelled':
            return False
        assert tree[1] == 'format', tree
        template, *values = (evaluate(argument, contexts) for argument in tree[2])
        return re.sub(r'\{(\d+)\}', lambda found: str(values[int(found[1])]), template)
    left = evaluate(tree[1], contexts)
    if kind == '&&':
        return evaluate(tree[2], contexts) if left else left
    if kind == '||':
        return left if left else evaluate(tree[2], contexts)
    right = evaluate(tree[2], contexts)
    return (str(left).lower() == str(right).lower()) == (kind == '==')

def expression(text):
    text = str(text).strip()
    assert text.startswith('${{') and text.endswith('}}'), text
    return parse(text[3:-2])

def merge_group_pull_request(event, ref):
    """Run the declaration job's own script with the runner's event variables; return its output and exit status."""
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        output = directory / 'output'
        output.write_text('', encoding='utf-8')
        env = {key: value for key, value in os.environ.items() if not key.startswith('GITHUB_')} | {
            'GITHUB_OUTPUT': str(output), 'GITHUB_EVENT_NAME': event, 'GITHUB_REF': ref, 'TEST_EVENT': event, 'WARM': 'false'}
        result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', PARSE], cwd=directory, env=env, text=True, capture_output=True)
        lines = output.read_text(encoding='utf-8').splitlines()
        return result, dict(line.split('=', 1) for line in lines if '=' in line)

def queue_ref(number, base=SHA, branch='main'):
    return f'refs/heads/gh-readonly-queue/{branch}/pr-{number}-{base}'

def groups(event, run_id, ref, key=''):
    """Every held concurrency group of one run, keyed by job and leg, with the cancel setting; the run's own declaration output feeds `needs`."""
    result, outputs = merge_group_pull_request(event, ref)
    assert result.returncode == 0, result.stdout + result.stderr
    contexts = {'github': {'event_name': event, 'workflow': 'CI', 'run_id': run_id, 'ref': ref}, 'inputs': {'concurrency-key': key},
                'needs': {'declaration': {'outputs': outputs}}}
    held = {}
    for name, legs in LEGS.items():
        block = jobs[name].get('concurrency')
        assert block is not None, f'{name} holds macOS runners without a concurrency group'
        for leg in legs:
            leg_contexts = contexts | {'matrix': leg}
            held[name, json.dumps(leg, sort_keys=True)] = (evaluate(expression(block['group']), leg_contexts),
                                                           evaluate(expression(block['cancel-in-progress']), leg_contexts))
    return held

def a_rebuilt_group_cancels_the_macos_jobs_of_the_run_it_replaces():
    old = groups('merge_group', 101, queue_ref(7, 'a' * 40))
    new = groups('merge_group', 102, queue_ref(7, 'b' * 40))
    # Same pull request, new base: every leg lands in the group its predecessor holds, and the newcomer cancels it.
    assert old.keys() == new.keys() and all(old[leg][0] == new[leg][0] for leg in old), (old, new)
    assert all(cancel is True for _, cancel in new.values()), new
    assert any('macos-latest' in leg[1] for leg in new), new
    # Branch names with slashes still yield the pull request number.
    assert groups('merge_group', 103, queue_ref(7, branch='release/1.x')) == groups('merge_group', 104, queue_ref(7, 'c' * 40))

def live_groups_never_cancel_each_other():
    seven, eight = groups('merge_group', 101, queue_ref(7)), groups('merge_group', 102, queue_ref(8))
    names = [name for name, _ in seven.values()]
    assert len(set(names)) == len(names), 'two legs of one run share a group: ' + str(sorted(names))
    assert not set(names) & {name for name, _ in eight.values()}, 'two live pull requests share a group'
    # Two calls of the shared workflow in one run stay apart by their key.
    keyed = groups('merge_group', 101, queue_ref(7), key='sample')
    assert not set(names) & {name for name, _ in keyed.values()}, 'calls with different keys share a group'

def pull_requests_and_pushes_keep_their_runs_uncancelled():
    for event, ref in [('pull_request', 'refs/pull/7/merge'), ('push', 'refs/heads/main')]:
        first, second = groups(event, 101, ref), groups(event, 102, ref)
        names = [name for name, _ in first.values()]
        assert len(set(names)) == len(names), (event, sorted(names))
        assert not set(names) & {name for name, _ in second.values()}, f'a newer {event} run shares a group with an older one'
        assert not any(cancel for _, cancel in itertools.chain(first.values(), second.values())), (event, first)
        result, outputs = merge_group_pull_request(event, ref)
        assert result.returncode == 0 and outputs['merge_group_pull_request'] == '', (event, result.stderr, outputs)

def only_the_macos_job_legs_carry_a_group():
    assert {name for name, job in jobs.items() if 'concurrency' in job} == set(LEGS)
    # Every job whose runner comes from its matrix can hold a macOS runner, and those are the jobs listed in LEGS.
    assert {name for name, job in jobs.items() if 'matrix.runner' in str(job.get('runs-on'))} == set(LEGS)

def an_unreadable_queue_ref_stops_the_declaration():
    for ref in ['refs/heads/main', queue_ref('x'), 'refs/heads/gh-readonly-queue/main/pr-7', 'refs/heads/gh-readonly-queue/main/pr--' + SHA]:
        result, _ = merge_group_pull_request('merge_group', ref)
        assert result.returncode != 0 and ref in result.stderr, (ref, result.stdout, result.stderr)
    result, outputs = merge_group_pull_request('merge_group', queue_ref(4358, '988f4a7' + '0' * 33))
    assert result.returncode == 0 and outputs['merge_group_pull_request'] == '4358', (result.stderr, outputs)

REPOSITORY = 'Orchestration-Maestro/maestro'
STEP_NAME = 'Stop when the merge group was replaced'

def stop_step(job):
    """The job's steps that carry the replaced-group check, by position."""
    return [(index, step) for index, step in enumerate(jobs[job]['steps']) if step.get('name') == STEP_NAME]

def run_stop_step(reply, status, ref):
    """Run the check's own script with a stand-in `gh` that answers like the API; return the exit status, output and the call made."""
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        calls = directory / 'calls'
        gh = directory / 'gh'
        gh.write_text(f'#!{sys.executable}\nimport sys\nopen({str(calls)!r}, "w").write(" ".join(sys.argv[1:]))\n'
                      f'sys.stderr.write({reply!r})\nsys.exit({status})\n', encoding='utf-8')
        gh.chmod(0o755)
        env = {key: value for key, value in os.environ.items() if not key.startswith('GITHUB_')} | {
            'PATH': f'{directory}{os.pathsep}{os.environ["PATH"]}', 'GITHUB_REPOSITORY': REPOSITORY, 'GITHUB_REF': ref}
        step = stop_step('declaration')[0][1]
        result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', step['run']], cwd=directory, env=env, text=True, capture_output=True)
        return result, calls.read_text(encoding='utf-8')

def a_replaced_group_stops_and_a_live_group_continues():
    ref = queue_ref(7)
    gone, call = run_stop_step('gh: Not Found (HTTP 404)', 1, ref)
    assert gone.returncode != 0 and ref in gone.stdout + gone.stderr, (gone.stdout, gone.stderr)
    assert call == f'api repos/{REPOSITORY}/git/ref/{ref.removeprefix("refs/")}', call
    live, _ = run_stop_step('', 0, ref)
    assert live.returncode == 0, (live.stdout, live.stderr)
    # An API outage says nothing about the group, so the run goes on.
    outage, _ = run_stop_step('gh: Bad Gateway (HTTP 502)', 1, ref)
    assert outage.returncode == 0, (outage.stdout, outage.stderr)

def a_replaced_run_never_joins_a_group_the_live_run_holds():
    # The check sits right after the declaration read and as the last step of the two jobs that gate `tests`.
    declaration = jobs['declaration']['steps']
    (index, step), = stop_step('declaration')
    assert declaration[index - 1]['name'] == 'Read caller declaration', declaration[index - 1]['name']
    assert step['if'] == "${{ github.event_name == 'merge_group' }}", step
    for name in ['test-build', 'test-prepare']:
        (index, copy), = stop_step(name)
        assert index == len(jobs[name]['steps']) - 1 and copy == step, (name, index)
    # A run that failed the check skips every job that needs the declaration, and its `tests` legs never start.
    for name in LEGS:
        assert 'declaration' in str(jobs[name]['needs']), name
    # `doctests` and `targets` carry no status function at all; `tests` uses `!cancelled()` and is checked by `starts`.
    for name in ['doctests', 'targets']:
        condition = str(jobs[name].get('if'))
        assert not any(call in condition for call in ('always()', 'failure()', 'cancelled()')), (name, condition)
    tests_if = expression(jobs['tests']['if'])
    def starts(build, prepare):
        contexts = {'github': {'repository_id': '1'}, 'inputs': {'working-directory': ''}, 'needs': {
            'declaration': {'outputs': {'coverage': 'true'}}, 'test-build': {'result': build}, 'test-prepare': {'result': prepare}}}
        return bool(evaluate(tests_if, contexts))
    assert starts('success', 'success') and starts('success', 'skipped')
    assert not starts('success', 'failure') and not starts('failure', 'success') and not starts('failure', 'failure')

for check in [a_rebuilt_group_cancels_the_macos_jobs_of_the_run_it_replaces, live_groups_never_cancel_each_other,
              pull_requests_and_pushes_keep_their_runs_uncancelled, only_the_macos_job_legs_carry_a_group,
              an_unreadable_queue_ref_stops_the_declaration, a_replaced_group_stops_and_a_live_group_continues,
              a_replaced_run_never_joins_a_group_the_live_run_holds]:
    check()
    print('PASS ' + check.__name__, flush=True)
