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
SHA = 'a' * 40

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
        assert tree[1] == 'cancelled', tree
        return False
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

def queue_ref(number, base=SHA, branch='main'):
    return f'refs/heads/gh-readonly-queue/{branch}/pr-{number}-{base}'

REPOSITORY = 'Orchestration-Maestro/maestro'
STEP_NAME = 'Stop when the merge group was replaced'
CANCELLER = 'replaced-groups'
CANCEL_STEP = 'Cancel replaced merge groups'

def stand_in_gh(directory, replies):
    """Install a `gh` that answers each exact argument list from `replies` and logs every call; any other call fails the test."""
    calls, table = directory / 'calls', directory / 'replies.json'
    table.write_text(json.dumps(replies), encoding='utf-8')
    gh = directory / 'gh'
    gh.write_text(f'#!{sys.executable}\nimport json, sys\n'
                  f'with open({str(calls)!r}, "a") as log: log.write(json.dumps(sys.argv[1:]) + "\\n")\n'
                  f'status, out, err = json.load(open({str(table)!r})).get(" ".join(sys.argv[1:]), [97, "", "unexpected call"])\n'
                  'sys.stdout.write(out)\nsys.stderr.write(err)\nsys.exit(status)\n', encoding='utf-8')
    gh.chmod(0o755)
    calls.write_text('', encoding='utf-8')
    return calls

def run_step(script, replies, variables):
    """Run a step's own script with a stand-in `gh`; return the exit status, output and every call made."""
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        calls = stand_in_gh(directory, replies)
        env = {key: value for key, value in os.environ.items() if not key.startswith('GITHUB_')} | {
            'PATH': f'{directory}{os.pathsep}{os.environ["PATH"]}', 'GITHUB_REPOSITORY': REPOSITORY} | variables
        result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', script], cwd=directory, env=env, text=True, capture_output=True)
        return result, [json.loads(line) for line in calls.read_text(encoding='utf-8').splitlines()]

def stop_step(job):
    """The job's steps that carry the replaced-group check, by position."""
    return [(index, step) for index, step in enumerate(jobs[job]['steps']) if step.get('name') == STEP_NAME]

def ref_call(ref):
    return f'api repos/{REPOSITORY}/git/ref/{ref.removeprefix("refs/")} --jq .object.sha'

def run_stop_step(reply, ref):
    """Run the declaration's check for a run of `ref` at commit SHA, with the ref read answered by `reply`."""
    status, text = reply
    replies = {ref_call(ref): [status, text if status == 0 else '', text if status else '']}
    return run_step(stop_step('declaration')[0][1]['run'], replies, {'GITHUB_REF': ref, 'GITHUB_SHA': SHA})

def a_replaced_group_stops_and_a_live_group_continues():
    ref = queue_ref(7)
    gone, calls = run_stop_step((1, 'gh: Not Found (HTTP 404)'), ref)
    assert gone.returncode != 0 and ref in gone.stdout + gone.stderr, (gone.stdout, gone.stderr)
    # The queue reuses a group's name when it queues the same pull request on the same base again.
    moved, _ = run_stop_step((0, 'b' * 40 + '\n'), ref)
    assert moved.returncode != 0 and ref in moved.stdout + moved.stderr, (moved.stdout, moved.stderr)
    live, _ = run_stop_step((0, SHA + '\n'), ref)
    assert live.returncode == 0, (live.stdout, live.stderr)
    # An API outage says nothing about the group, so the run goes on.
    outage, _ = run_stop_step((1, 'gh: Bad Gateway (HTTP 502)'), ref)
    assert outage.returncode == 0, (outage.stdout, outage.stderr)
    assert [' '.join(call) for call in calls] == [ref_call(ref)], calls

def a_replaced_run_never_starts_its_test_shards():
    # The check sits right after the declaration read and as the last step of the two jobs that gate `tests`.
    declaration = jobs['declaration']['steps']
    (index, step), = stop_step('declaration')
    assert declaration[index - 1]['name'] == 'Read caller declaration', declaration[index - 1]['name']
    assert step['if'] == "${{ github.event_name == 'merge_group' }}", step
    for name in ['test-build', 'test-prepare']:
        (index, copy), = stop_step(name)
        assert index == len(jobs[name]['steps']) - 1 and copy == step, (name, index)
    # A run that failed the check skips every job that needs the declaration, and its `tests` legs never start.
    for name in ['test-build', 'test-prepare', 'tests', 'doctests', 'targets']:
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

THIS_RUN, WORKFLOW = 900, 375050718

def queue_run(run_id, number, base, head, status='in_progress', workflow=WORKFLOW):
    return {'id': run_id, 'workflow_id': workflow, 'status': status, 'head_sha': head,
            'head_branch': queue_ref(number, base).removeprefix('refs/heads/')}

def answer(value):
    return [0, json.dumps(value), '']

def queue_replies(queued, running, refs, cancels=None):
    """The API as the canceller sees it: this run, the unfinished merge-group runs and the queue's refs.

    `refs` maps a group's branch to the commit it holds; a branch it omits no longer exists.
    """
    replies = {f'api repos/{REPOSITORY}/actions/runs/{THIS_RUN}': answer({'id': THIS_RUN, 'workflow_id': WORKFLOW}),
               f'api repos/{REPOSITORY}/actions/runs?event=merge_group&status=queued&per_page=100': answer({'workflow_runs': queued}),
               f'api repos/{REPOSITORY}/actions/runs?event=merge_group&status=in_progress&per_page=100': answer({'workflow_runs': running})}
    for run in queued + running:
        held = refs.get(run['head_branch'])
        replies[f"api repos/{REPOSITORY}/git/ref/heads/{run['head_branch']}"] = (
            answer({'object': {'sha': held}}) if held else [1, '', 'gh: Not Found (HTTP 404)'])
        replies[f"api --method POST repos/{REPOSITORY}/actions/runs/{run['id']}/cancel"] = (cancels or {}).get(run['id'], answer({}))
    return replies

def run_canceller(replies):
    job = jobs[CANCELLER]
    step = next(step for step in job['steps'] if step['name'] == CANCEL_STEP)
    result, calls = run_step(step['run'], replies, {'GITHUB_RUN_ID': str(THIS_RUN)})
    cancelled = [int(call[3].split('/')[-2]) for call in calls if call[:3] == ['api', '--method', 'POST']]
    return result, cancelled

def a_rebuilt_or_closed_group_loses_its_run_and_live_groups_keep_theirs():
    # The queue at 06:15 on 2026-11-09: pull request 4396 was closed, then main moved and the queue rebuilt
    # 4404 and 4407 on the new base, leaving 4398 without a group. Run 900 is the rebuilt 4404 group.
    old, new = 'c' * 40, 'd' * 40
    closed = queue_run(101, 4396, old, '1' * 40)
    dropped = queue_run(102, 4398, old, '2' * 40)
    replaced = queue_run(103, 4407, 'e' * 40, '3' * 40)
    this = queue_run(THIS_RUN, 4404, new, '4' * 40)
    rebuilt = queue_run(105, 4407, '4' * 40, '5' * 40, status='queued')
    # The same pull request queued again on the same base gets the old name back on a new commit.
    reused = queue_run(106, 4362, new, '6' * 40, status='queued')
    elsewhere = queue_run(107, 4396, old, '7' * 40, workflow=1)
    refs = {run['head_branch']: run['head_sha'] for run in [this, rebuilt]} | {reused['head_branch']: '8' * 40}
    result, cancelled = run_canceller(queue_replies([rebuilt, reused], [closed, dropped, replaced, this, elsewhere], refs))
    assert result.returncode == 0, result.stdout + result.stderr
    assert sorted(cancelled) == [101, 102, 103, 106], cancelled
    for run in [closed, dropped, replaced, reused]:
        assert f"{run['id']}" in result.stdout and run['head_branch'] in result.stdout, result.stdout

def the_canceller_cancels_only_what_the_api_shows_replaced():
    gone = queue_run(101, 4396, 'c' * 40, '1' * 40)
    unread = queue_run(102, 4398, 'c' * 40, '2' * 40)
    finished = queue_run(103, 4399, 'c' * 40, '3' * 40)
    replies = queue_replies([], [gone, unread, finished], {})
    # A ref read that fails for another reason says nothing about the group; the other runs are still judged.
    replies[f"api repos/{REPOSITORY}/git/ref/heads/{unread['head_branch']}"] = [1, '', 'gh: Bad Gateway (HTTP 502)']
    # A run that finished since the listing can no longer be cancelled, and that is the outcome sought.
    replies[f"api --method POST repos/{REPOSITORY}/actions/runs/{finished['id']}/cancel"] = [
        1, '', 'gh: Cannot cancel a workflow run that is completed. (HTTP 409)']
    result, cancelled = run_canceller(replies)
    assert sorted(cancelled) == [101, 103], cancelled
    assert result.returncode != 0 and unread['head_branch'] in result.stdout and '::warning' in result.stdout, result.stdout
    assert result.stdout.count('::warning') == 1, result.stdout
    # A listing that fails cancels nothing.
    replies = queue_replies([], [gone], {})
    replies[f'api repos/{REPOSITORY}/actions/runs?event=merge_group&status=in_progress&per_page=100'] = [1, '', 'gh: Bad Gateway (HTTP 502)']
    result, cancelled = run_canceller(replies)
    assert result.returncode != 0 and cancelled == [] and '::warning' in result.stdout, (result.stdout, result.stderr)

def the_canceller_starts_at_once_in_merge_groups_only():
    job = jobs[CANCELLER]
    condition = expression(job['if'])
    starts = {event for event in ['pull_request', 'merge_group', 'push', 'workflow_dispatch', 'schedule', 'workflow_call']
              if evaluate(condition, {'github': {'event_name': event}})}
    assert starts == {'merge_group'}, starts
    # A scan, warm or browser-only call runs only its own jobs, whatever the event.
    for mode in ['browser-only', 'warm-cache', 'advisory-scan', 'fuzz-scan']:
        assert not evaluate(condition, {'github': {'event_name': 'merge_group'}, 'inputs': {mode: True}}), mode
    # No `needs`: it runs beside the declaration, before any job of this run takes a macOS runner.
    assert 'needs' not in job and job['runs-on'] == 'ubuntu-latest', job
    # The only job of a merge-group run with a write grant runs no caller code: no checkout, no action.
    assert job['permissions'] == {'contents': 'read', 'actions': 'write'}, job['permissions']
    assert all('uses' not in step for step in job['steps']), job['steps']
    step = next(step for step in job['steps'] if step['name'] == CANCEL_STEP)
    # A failed cancellation never fails the live group that ran it.
    assert step['continue-on-error'] is True and step['env'] == {'GH_TOKEN': '${{ github.token }}'}, step

for check in [a_replaced_group_stops_and_a_live_group_continues, a_replaced_run_never_starts_its_test_shards,
              a_rebuilt_or_closed_group_loses_its_run_and_live_groups_keep_theirs,
              the_canceller_cancels_only_what_the_api_shows_replaced, the_canceller_starts_at_once_in_merge_groups_only]:
    check()
    print('PASS ' + check.__name__, flush=True)
