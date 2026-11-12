import ast
import json
import os
import pathlib
import re
import subprocess
import tempfile

workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
steps = {step['name']: step for step in document['jobs']['test-build']['steps']}
READ = 'Earlier test seconds'

# The seam's GitHub expression reader, so every job name below is the one the workflow's own `name:` produces.
reader = ast.parse(pathlib.Path('.github/ci-internal/superseded-merge-groups-at-workflow-seam.py').read_text(encoding='utf-8'))
reader.body = [node for node in reader.body if isinstance(node, ast.FunctionDef) and node.name in {'parse', 'evaluate', 'expression'}]
expressions = {'re': re}
exec(compile(reader, 'GitHub expression reader', 'exec'), expressions)
expression, evaluate = expressions['expression'], expressions['evaluate']

def named(job, runner='ubuntu-latest', shard=1, count=2, caller='suite'):
    """The name GitHub shows for `job` of the `caller` job's call on `runner`."""
    matrix = {'runner': runner, 'shard': shard, 'count': count}
    return f"{caller} / {evaluate(expression(document['jobs'][job]['name']), {'inputs': {'warm-cache': False}, 'matrix': matrix})}"

def run(identifier, created, workflow=55, repository='o/r', event='merge_group'):
    return {'id': identifier, 'workflow_id': workflow, 'created_at': created, 'event': event, 'head_branch': 'gh-readonly-queue/main/pr-1',
            'head_sha': f'sha{identifier}', 'head_repository': {'full_name': repository}}

def job(identifier, name, conclusion: str | None = 'success', status='completed', runner='GitHub Actions 7'):
    return {'id': identifier, 'name': name, 'conclusion': conclusion, 'status': status, 'runner_name': runner}

def jobs(run_id, *found):
    return {f'actions/runs/{run_id}/jobs?per_page=100': [{'jobs': list(found)}]}

def listed(event, *runs):
    return {f'actions/runs?event={event}&status=success&per_page=100': {'workflow_runs': list(runs)}}

def current(runner='ubuntu-latest'):
    """The current run 100 of workflow 55, whose `suite / Build test archive` job for `runner` runs on runner `GitHub Actions 7`."""
    return {'actions/runs/100': run(100, '2026-11-05T10:00:00Z'),
            'actions/runs/100/attempts/1/jobs?per_page=100': [{'jobs': [
                job(1, 'suite / Caller declaration', runner='GitHub Actions 3'),
                job(3, named('test-build', 'macos-latest' if runner == 'ubuntu-latest' else 'ubuntu-latest'), None, 'in_progress', 'GitHub Actions 5'),
                job(2, named('test-build', runner), None, 'in_progress')]}]}

CURRENT = current()

def shard_log(*lines):
    return ''.join(f'2026-11-05T09:00:00.0000000Z {line}\n' for line in lines)

def read(routes, checkout=None, runner_os='Linux'):
    """Run the step in a checkout holding the given files, against a gh that answers only the given routes, a None route as GitHub's 404;
    return the seconds it wrote and its log."""
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        for name, text in (checkout or {}).items():
            (directory / name).write_text(text, encoding='utf-8')
        (directory / 'routes.json').write_text(json.dumps(routes), encoding='utf-8')
        (directory / 'bin').mkdir()
        gh = directory / 'bin/gh'
        # A route {'fail': n, 'then': payload} answers HTTP 502 to its first n requests (all of them without 'then'), then the payload.
        gh.write_text('#!/usr/bin/env python3\nimport json, pathlib, sys\n'
                      f'routes = json.load(open({str(directory / "routes.json")!r}))\n'
                      'path = sys.argv[-1].removeprefix("repos/o/r/")\n'
                      'route = routes.get(path)\n'
                      'if route is None:\n    sys.exit("gh: Not Found (HTTP 404)")\n'
                      'if isinstance(route, dict) and "fail" in route:\n'
                      f'    count = pathlib.Path({str(directory / "requests")!r} + path.replace("/", "_"))\n'
                      '    seen = int(count.read_text()) if count.exists() else 0\n'
                      '    count.write_text(str(seen + 1))\n'
                      '    if "then" not in route or seen < route["fail"]:\n        sys.exit("gh: Bad Gateway (HTTP 502)")\n'
                      '    route = route["then"]\n'
                      'print(route if isinstance(route, str) else json.dumps(route))\n', encoding='utf-8')
        gh.chmod(0o700)
        env = {key: value for key, value in os.environ.items() if not key.startswith('GITHUB_')} | {
            'PATH': f'{directory / "bin"}:' + os.environ['PATH'], 'GITHUB_REPOSITORY': 'o/r', 'GITHUB_RUN_ID': '100',
            'GITHUB_RUN_ATTEMPT': '1', 'RUNNER_NAME': 'GitHub Actions 7', 'RUNNER_OS': runner_os}
        result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', steps[READ]['run']],
                                cwd=directory, env=env, text=True, capture_output=True)
        assert result.returncode == 0, result.stdout + result.stderr
        return json.loads((directory / 'target/test-seconds.json').read_text(encoding='utf-8')), result.stdout

def the_newest_earlier_run_of_this_caller_supplies_seconds_per_test():
    routes = CURRENT | listed('merge_group',
        run(90, '2026-11-05T11:00:00Z'),  # Created after the current run.
        run(80, '2026-11-05T09:30:00Z', workflow=56),  # Another workflow.
        run(70, '2026-11-05T09:20:00Z', repository='fork/r'),
        run(60, '2026-11-05T09:10:00Z'),  # Its shards skipped: the tests were reused.
        run(50, '2026-11-05T09:00:00Z'),
        run(40, '2026-11-01T09:00:00Z')) | listed('push', run(45, '2026-11-04T09:00:00Z', event='push')) | jobs(
        60, job(61, named('tests', shard=1), 'skipped'), job(62, named('tests', shard=2), 'skipped')) | jobs(
        50, job(51, named('tests', shard=1)), job(52, named('tests', shard=2)), job(53, named('tests', count=1, caller='other')),
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
    assert seconds == {'heavy::integration': {'slow_one': 1.25, 'slow_two': 60.5, 'slow_three': 2.0}, 'light::bin/light': {'quick': 0.01}}, seconds
    assert 'Test seconds from run 50 (merge_group on gh-readonly-queue/main/pr-1 at sha50, 2026-11-05T09:00:00Z): 2 test binaries' in log, log

def a_failed_shard_keeps_its_run_out():
    routes = CURRENT | listed('merge_group', run(50, '2026-11-05T09:00:00Z')) | listed('push') | jobs(
        50, job(51, named('tests', shard=1)), job(52, named('tests', shard=2), 'cancelled')) | {
        'actions/jobs/51/logs': shard_log('        PASS [   1.000s] (1/1) heavy::integration slow_one'),
        'actions/jobs/52/logs': shard_log('        PASS [   1.000s] (1/1) light::bin/light quick')}
    seconds, log = read(routes)
    assert seconds == {}, seconds
    assert 'Test seconds unavailable' in log and 'test binaries are grouped by test count' in log, log

def each_runner_takes_seconds_from_its_own_shards():
    # Run 60 ran the Linux shards only; run 50 ran both runners' shards, each runner with its own shard count.
    linux = [job(61, named('tests', shard=1)), job(62, named('tests', shard=2))]
    both = [job(51, named('tests', shard=1)), job(52, named('tests', shard=2)),
            job(55, named('tests', 'macos-latest', 1, 3)), job(56, named('tests', 'macos-latest', 2, 3)), job(57, named('tests', 'macos-latest', 3, 3))]
    logs = {'actions/jobs/61/logs': shard_log('        PASS [   2.000s] (1/1) heavy::integration slow_one'),
            'actions/jobs/62/logs': shard_log('        PASS [   1.000s] (1/1) light::bin/light quick'),
            'actions/jobs/51/logs': shard_log('        PASS [   4.000s] (1/1) heavy::integration slow_one'),
            'actions/jobs/52/logs': shard_log('        PASS [   3.000s] (1/1) light::bin/light quick'),
            'actions/jobs/55/logs': shard_log('        PASS [  20.000s] (1/1) heavy::integration slow_one'),
            'actions/jobs/56/logs': shard_log('        PASS [  10.000s] (1/1) light::bin/light quick'),
            'actions/jobs/57/logs': shard_log('        PASS [   5.000s] (1/1) heavy::integration slow_two')}
    earlier = listed('merge_group', run(60, '2026-11-05T09:30:00Z'), run(50, '2026-11-05T09:00:00Z')) | listed('push') | jobs(60, *linux) | jobs(50, *both)
    for runner, expected, source in [('ubuntu-latest', {'heavy::integration': {'slow_one': 2.0}, 'light::bin/light': {'quick': 1.0}}, 60),
                                     ('macos-latest', {'heavy::integration': {'slow_one': 20.0, 'slow_two': 5.0}, 'light::bin/light': {'quick': 10.0}}, 50)]:
        seconds, log = read(current(runner) | earlier | logs)
        assert seconds == expected, (runner, seconds)
        assert f'Test seconds from run {source} ' in log, (runner, log)

def without_earlier_timing_the_binaries_go_by_test_count():
    for routes in [CURRENT | listed('merge_group') | listed('push'), {}]:
        seconds, log = read(routes)
        assert seconds == {}, seconds
        assert 'Test seconds unavailable' in log and 'test binaries are grouped by test count' in log, log

def an_earlier_run_that_cannot_be_read_is_skipped():
    # Run 50's jobs listing keeps answering HTTP 502; run 40 is the next earlier run whose shards ran.
    routes = CURRENT | listed('merge_group', run(50, '2026-11-05T09:00:00Z'), run(40, '2026-11-01T09:00:00Z')) | listed('push') | {
        'actions/runs/50/jobs?per_page=100': {'fail': 0}} | jobs(
        40, job(41, named('tests', shard=1)), job(42, named('tests', shard=2))) | {
        # A request that fails once is retried and its run still serves.
        'actions/jobs/41/logs': {'fail': 1, 'then': shard_log('        PASS [   3.000s] (1/1) heavy::integration slow_one')},
        'actions/jobs/42/logs': shard_log('        PASS [   1.000s] (1/1) light::bin/light quick')}
    seconds, log = read(routes)
    assert seconds == {'heavy::integration': {'slow_one': 3.0}, 'light::bin/light': {'quick': 1.0}}, seconds
    assert 'Skipped earlier run 50: gh: Bad Gateway (HTTP 502)' in log and 'Test seconds from run 40 ' in log, log

def counting_by_tests_waits_until_no_earlier_run_can_be_read():
    routes = CURRENT | listed('merge_group', run(50, '2026-11-05T09:00:00Z')) | listed('push') | {
        'actions/runs/50/jobs?per_page=100': {'fail': 0}}
    seconds, log = read(routes)
    assert seconds == {}, seconds
    assert 'Skipped earlier run 50: gh: Bad Gateway (HTTP 502)' in log and 'test binaries are grouped by test count' in log, log

def a_linux_walk_passes_over_main_pushes():
    # Main pushes run no Linux shards, so a Linux run never lists them; another host's shards run on pushes.
    both = [job(51, named('tests', shard=1)), job(52, named('tests', shard=2))]
    logs = {'actions/jobs/51/logs': shard_log('        PASS [   2.000s] (1/1) heavy::integration slow_one'),
            'actions/jobs/52/logs': shard_log('        PASS [   1.000s] (1/1) light::bin/light quick')}
    merge = listed('merge_group', run(50, '2026-11-05T09:00:00Z'))
    push = listed('push', run(50, '2026-11-05T09:00:00Z', event='push'))
    seconds, _ = read(CURRENT | merge | jobs(50, *both) | logs)
    assert seconds == {'heavy::integration': {'slow_one': 2.0}, 'light::bin/light': {'quick': 1.0}}, seconds
    seconds, _ = read(CURRENT | push | jobs(50, *both) | logs)
    assert seconds == {}, seconds
    seconds, _ = read(CURRENT | merge | push | jobs(50, *both) | logs, runner_os='macOS')
    assert seconds == {'heavy::integration': {'slow_one': 2.0}, 'light::bin/light': {'quick': 1.0}}, seconds

def the_caller_checkout_never_runs_with_the_token():
    # The step runs in the caller's checkout with the token, so a module there must never shadow the one the step imports.
    seconds, _ = read({}, {'json.py': "raise SystemExit('the caller checkout ran with the token')\n"})
    assert seconds == {}, seconds

for check in [the_newest_earlier_run_of_this_caller_supplies_seconds_per_test, a_failed_shard_keeps_its_run_out,
              each_runner_takes_seconds_from_its_own_shards, without_earlier_timing_the_binaries_go_by_test_count,
              an_earlier_run_that_cannot_be_read_is_skipped, counting_by_tests_waits_until_no_earlier_run_can_be_read,
              a_linux_walk_passes_over_main_pushes, the_caller_checkout_never_runs_with_the_token]:
    check()
    print('PASS ' + check.__name__, flush=True)
