import json
import os
import pathlib
import subprocess
import tempfile

workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
steps = {step['name']: step for step in document['jobs']['test-prepare']['steps']}
SELECT = 'Test setup candidates'
ARTIFACT = 'test-setup-root'

def execute(name, cwd, extra):
    env = {key: value for key, value in os.environ.items() if not key.startswith('GITHUB_')} | extra
    return subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', steps[name]['run']],
                          cwd=cwd, env=env, text=True, capture_output=True)

def run(identifier, head, repository: str | None = 'o/r', workflow=55):
    return {'id': identifier, 'head_sha': head, 'workflow_id': workflow,
            'head_repository': {'full_name': repository} if repository else None}

def artifacts(*expired):
    return {'artifacts': [{'name': ARTIFACT, 'expired': value} for value in expired]}

def bundle(identifier, expired=False):
    return {f'actions/runs/{identifier}/artifacts?name={ARTIFACT}&per_page=100': artifacts(expired)}

def selected(routes, **extra):
    """Run the selection step against a gh that answers only the given routes, a None route as GitHub's 404; return its output, the routes asked and its log."""
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        log = directory / 'asked'
        (directory / 'routes.json').write_text(json.dumps(routes), encoding='utf-8')
        (directory / 'bin').mkdir()
        gh = directory / 'bin/gh'
        gh.write_text('#!/usr/bin/env python3\nimport json, os, sys\n'
                      f'routes = json.load(open({str(directory / "routes.json")!r}))\n'
                      f'open({str(log)!r}, "a").write(sys.argv[2] + "\\n")\n'
                      'path = sys.argv[2].removeprefix("repos/o/r/")\n'
                      'if path not in routes:\n    sys.exit("unexpected request: " + path)\n'
                      'if routes[path] is None:\n    sys.exit("gh: Not Found (HTTP 404)")\n'
                      'print(json.dumps(routes[path]))\n', encoding='utf-8')
        gh.chmod(0o700)
        output = directory / 'output'
        env = {'PATH': f'{directory / "bin"}:' + os.environ['PATH'], 'GITHUB_OUTPUT': str(output), 'GITHUB_REPOSITORY': 'o/r',
               'GITHUB_RUN_ID': '100', 'CACHE_WRITER': 'false', 'CACHE_DEFAULT_BRANCH': 'main',
               'ARTIFACT_NAME': ARTIFACT, 'GITHUB_SHA': 'merged'} | extra
        result = execute(SELECT, directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        return output.read_text(encoding='utf-8'), log.read_text(encoding='utf-8') if log.exists() else '', result.stdout

def at(sha, event='pull_request'):
    return f'actions/runs?event={event}&head_sha={sha}&status=success&per_page=100'

def group(*subjects):
    # The queue branch is cut from the previous group, so only a comparison with the default branch lists every pull request still ahead of it.
    return {'compare/main...merged?per_page=100': {'commits': [{'commit': {'message': subject}} for subject in subjects]}}

# A workflow an organization ruleset requires (55) runs the pull requests and merge groups; GitHub has no actions/workflows route for it.
REQUIRED = {'actions/runs/100': {'workflow_id': 55},
            **{f'actions/workflows/55/runs?event=pull_request&head_sha={sha}&status=success&per_page=100': None for sha in 'bdg'}}

def a_merge_group_gets_the_bundles_its_pull_request_runs_uploaded_under_a_required_workflow():
    routes = REQUIRED | group('feat: first (#7)\n\nbody (#99)', 'fix: second (#8)', 'chore: no number') | {
        'pulls/7': {'head': {'sha': 'b'}}, 'pulls/8': {'head': {'sha': 'd'}},
        at('b'): {'workflow_runs': [run(31, 'b'), run(32, 'b')]},
        at('d'): {'workflow_runs': [run(40, 'd'), run(41, 'd')]}} | bundle(31) | bundle(40, expired=True) | bundle(41)
    output, _, _ = selected(routes, GITHUB_EVENT_NAME='merge_group')
    # The pull request names the head being merged; the newest successful run at that head with a live bundle wins.
    assert output == 'runs=31 41\n', output

def a_fork_run_is_never_a_candidate():
    # A bundle holds executables the repository's token then runs, so only this repository's branches may supply one.
    pushed = {'actions/runs/100': {'workflow_id': 55},
              'actions/workflows/55/runs?event=merge_group&head_sha=merged&status=success&per_page=100': {'workflow_runs': [run(60, 'merged', repository='fork/r')]},
              at('merged', 'merge_group'): {'workflow_runs': [run(60, 'merged', repository='fork/r')]}} | bundle(60)
    output, _, _ = selected(pushed, GITHUB_EVENT_NAME='push', CACHE_WRITER='true')
    assert output == 'runs=\n', output
    routes = REQUIRED | group('fix: fork (#9)') | {
        'pulls/9': {'head': {'sha': 'g'}},
        at('g'): {'workflow_runs': [run(51, 'g', repository='fork/r'), run(52, 'g', repository=None), run(53, 'g')]}} | bundle(51) | bundle(52) | bundle(53)
    output, _, _ = selected(routes, GITHUB_EVENT_NAME='merge_group')
    assert output == 'runs=53\n', output

def a_push_gets_the_bundle_its_merge_group_run_uploaded_under_another_workflow():
    # The push runs the caller's own workflow (66), which never runs a merge group.
    routes = {'actions/runs/100': {'workflow_id': 66},
              'actions/workflows/66/runs?event=merge_group&head_sha=merged&status=success&per_page=100': {'workflow_runs': []},
              at('merged', 'merge_group'): {'workflow_runs': [run(70, 'merged'), run(71, 'merged')]}} | bundle(70, expired=True) | bundle(71)
    output, _, _ = selected(routes, GITHUB_EVENT_NAME='push', CACHE_WRITER='true')
    assert output == 'runs=71\n', output

def a_failed_lookup_warns_and_keeps_the_bundles_found():
    routes = group('feat: first (#7)', 'fix: second (#8)') | {
        'actions/runs/100': {'workflow_id': 55},
        'actions/workflows/55/runs?event=pull_request&head_sha=b&status=success&per_page=100': {'workflow_runs': [run(31, 'b')]},
        'actions/workflows/55/runs?event=pull_request&head_sha=d&status=success&per_page=100': None,
        'pulls/7': {'head': {'sha': 'b'}}, 'pulls/8': {'head': {'sha': 'd'}},
        at('b'): {'workflow_runs': [run(31, 'b')]}, at('d'): None} | bundle(31)
    output, _, log = selected(routes, GITHUB_EVENT_NAME='merge_group')
    assert output == 'runs=31\n', output
    assert '::warning::The test setup candidates could not be listed' in log, log

def other_events_get_no_candidates():
    for extra in [{'GITHUB_EVENT_NAME': 'pull_request'}, {'GITHUB_EVENT_NAME': 'push'}, {'GITHUB_EVENT_NAME': 'workflow_dispatch'}]:
        output, asked, _ = selected({}, **extra)
        assert output == 'runs=\n' and asked == '', (extra, output, asked)

for check in [a_merge_group_gets_the_bundles_its_pull_request_runs_uploaded_under_a_required_workflow, a_fork_run_is_never_a_candidate,
              a_push_gets_the_bundle_its_merge_group_run_uploaded_under_another_workflow, a_failed_lookup_warns_and_keeps_the_bundles_found,
              other_events_get_no_candidates]:
    check()
    print('PASS ' + check.__name__, flush=True)
