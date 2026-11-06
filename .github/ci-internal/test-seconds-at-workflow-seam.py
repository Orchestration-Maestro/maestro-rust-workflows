import json
import os
import pathlib
import subprocess
import tempfile

workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
steps = {step['name']: step for step in document['jobs']['test-build']['steps']}
READ = 'Earlier test seconds'

def run(identifier, created, workflow=55, repository='o/r', event='merge_group'):
    return {'id': identifier, 'workflow_id': workflow, 'created_at': created, 'event': event, 'head_branch': 'gh-readonly-queue/main/pr-1',
            'head_sha': f'sha{identifier}', 'head_repository': {'full_name': repository}}

def job(identifier, name, conclusion: str | None = 'success', status='completed', runner='GitHub Actions 7'):
    return {'id': identifier, 'name': name, 'conclusion': conclusion, 'status': status, 'runner_name': runner}

def jobs(run_id, *found):
    return {f'actions/runs/{run_id}/jobs?per_page=100': [{'jobs': list(found)}]}

def listed(event, *runs):
    return {f'actions/runs?event={event}&status=success&per_page=100': {'workflow_runs': list(runs)}}

# The current run 100 of workflow 55, whose `suite / Build test archive` job runs on runner `GitHub Actions 7`.
CURRENT = {'actions/runs/100': run(100, '2026-11-05T10:00:00Z'),
           'actions/runs/100/attempts/1/jobs?per_page=100': [{'jobs': [
               job(1, 'suite / Caller declaration', runner='GitHub Actions 3'),
               job(2, 'suite / Build test archive', None, 'in_progress')]}]}

def shard_log(*lines):
    return ''.join(f'2026-11-05T09:00:00.0000000Z {line}\n' for line in lines)

def read(routes, checkout=None):
    """Run the step in a checkout holding the given files, against a gh that answers only the given routes, a None route as GitHub's 404;
    return the seconds it wrote and its log."""
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        for name, text in (checkout or {}).items():
            (directory / name).write_text(text, encoding='utf-8')
        (directory / 'routes.json').write_text(json.dumps(routes), encoding='utf-8')
        (directory / 'bin').mkdir()
        gh = directory / 'bin/gh'
        gh.write_text('#!/usr/bin/env python3\nimport json, sys\n'
                      f'routes = json.load(open({str(directory / "routes.json")!r}))\n'
                      'path = sys.argv[-1].removeprefix("repos/o/r/")\n'
                      'if routes.get(path) is None:\n    sys.exit("gh: Not Found (HTTP 404)")\n'
                      'print(routes[path] if isinstance(routes[path], str) else json.dumps(routes[path]))\n', encoding='utf-8')
        gh.chmod(0o700)
        env = {key: value for key, value in os.environ.items() if not key.startswith('GITHUB_')} | {
            'PATH': f'{directory / "bin"}:' + os.environ['PATH'], 'GITHUB_REPOSITORY': 'o/r', 'GITHUB_RUN_ID': '100',
            'GITHUB_RUN_ATTEMPT': '1', 'RUNNER_NAME': 'GitHub Actions 7'}
        result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', steps[READ]['run']],
                                cwd=directory, env=env, text=True, capture_output=True)
        assert result.returncode == 0, result.stdout + result.stderr
        return json.loads((directory / 'target/test-seconds.json').read_text(encoding='utf-8')), result.stdout

def the_newest_earlier_run_of_this_caller_supplies_seconds_per_test_binary():
    routes = CURRENT | listed('merge_group',
        run(90, '2026-11-05T11:00:00Z'),  # Created after the current run.
        run(80, '2026-11-05T09:30:00Z', workflow=56),  # Another workflow.
        run(70, '2026-11-05T09:20:00Z', repository='fork/r'),
        run(60, '2026-11-05T09:10:00Z'),  # Its shards skipped: the tests were reused.
        run(50, '2026-11-05T09:00:00Z'),
        run(40, '2026-11-01T09:00:00Z')) | listed('push', run(45, '2026-11-04T09:00:00Z', event='push')) | jobs(
        60, job(61, 'suite / Test shard (1/2)', 'skipped'), job(62, 'suite / Test shard (2/2)', 'skipped')) | jobs(
        50, job(51, 'suite / Test shard (1/2)'), job(52, 'suite / Test shard (2/2)'), job(53, 'other / Test shard (1/1)'),
        job(54, 'suite / Doctests')) | {
        'actions/jobs/51/logs': shard_log(
            '  SETUP PASS [   9.500s] native-fixture: target/test-setup/development nextest-setup native-fixture',
            '        PASS [   1.250s] (  1/3) heavy::integration slow_one',
            '\x1b[32;1m        PASS\x1b[0m [  60.500s] (  2/3) heavy::integration slow_two',
            '        PASS [   0.010s] (  3/3) light::bin/light quick',
            '     Summary [  61.000s] 3 tests run: 3 passed, 0 skipped'),
        'actions/jobs/52/logs': shard_log('        PASS [   2.000s] (1/1) heavy::integration slow_three'),
        'actions/jobs/53/logs': shard_log('        PASS [ 900.000s] (1/1) heavy::integration elsewhere')}
    seconds, log = read(routes)
    # The newest earlier run of the same workflow and caller whose shards all ran, summed over all its shards; never the setup.
    assert seconds == {'heavy::integration': 63.75, 'light::bin/light': 0.01}, seconds
    assert 'Test seconds from run 50 (merge_group on gh-readonly-queue/main/pr-1 at sha50, 2026-11-05T09:00:00Z): 2 test binaries' in log, log

def a_failed_shard_keeps_its_run_out():
    routes = CURRENT | listed('merge_group', run(50, '2026-11-05T09:00:00Z')) | listed('push') | jobs(
        50, job(51, 'suite / Test shard (1/2)'), job(52, 'suite / Test shard (2/2)', 'cancelled')) | {
        'actions/jobs/51/logs': shard_log('        PASS [   1.000s] (1/1) heavy::integration slow_one'),
        'actions/jobs/52/logs': shard_log('        PASS [   1.000s] (1/1) light::bin/light quick')}
    seconds, log = read(routes)
    assert seconds == {}, seconds
    assert 'Test seconds unavailable' in log and 'test binaries are grouped by test count' in log, log

def without_earlier_timing_the_binaries_go_by_test_count():
    for routes in [CURRENT | listed('merge_group') | listed('push'), {}]:
        seconds, log = read(routes)
        assert seconds == {}, seconds
        assert 'Test seconds unavailable' in log and 'test binaries are grouped by test count' in log, log

def the_caller_checkout_never_runs_with_the_token():
    # The step runs in the caller's checkout with the token, so a module there must never shadow the one the step imports.
    seconds, _ = read({}, {'json.py': "raise SystemExit('the caller checkout ran with the token')\n"})
    assert seconds == {}, seconds

for check in [the_newest_earlier_run_of_this_caller_supplies_seconds_per_test_binary, a_failed_shard_keeps_its_run_out,
              without_earlier_timing_the_binaries_go_by_test_count, the_caller_checkout_never_runs_with_the_token]:
    check()
    print('PASS ' + check.__name__, flush=True)
