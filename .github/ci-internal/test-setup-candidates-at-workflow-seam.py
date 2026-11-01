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

def run(identifier, head, repository='o/r'):
    return {'id': identifier, 'head_sha': head, 'head_repository': {'full_name': repository}}

def artifacts(*expired):
    return {'artifacts': [{'name': ARTIFACT, 'expired': value} for value in expired]}

def selected(routes, **extra):
    """Run the selection step against a gh that answers only the given routes; return its output and the routes asked."""
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
                      'print(json.dumps(routes[path]))\n', encoding='utf-8')
        gh.chmod(0o700)
        output = directory / 'output'
        env = {'PATH': f'{directory / "bin"}:' + os.environ['PATH'], 'GITHUB_OUTPUT': str(output), 'GITHUB_REPOSITORY': 'o/r',
               'GITHUB_RUN_ID': '100', 'CACHE_WRITER': 'false', 'CACHE_DEFAULT_BRANCH': 'main',
               'ARTIFACT_NAME': ARTIFACT, 'GITHUB_SHA': 'merged'} | extra
        result = execute(SELECT, directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        return output.read_text(encoding='utf-8'), log.read_text(encoding='utf-8') if log.exists() else ''

def at(sha):
    return f'actions/workflows/55/runs?event=pull_request&head_sha={sha}&status=success&per_page=100'

# The queue branch is cut from the previous group, so only a comparison with the default branch lists every pull request still ahead of it.
GROUP = {'actions/runs/100': {'workflow_id': 55},
         'compare/main...merged?per_page=100': {'commits': [{'commit': {'message': message}} for message in
                                                            ['feat: first (#7)\n\nbody (#99)', 'fix: second (#8)', 'chore: no number', 'fix: fork (#9)']]}}

def a_merge_group_gets_the_bundles_of_its_pull_request_runs():
    routes = GROUP | {
        'pulls/7': {'head': {'sha': 'b'}}, 'pulls/8': {'head': {'sha': 'd'}}, 'pulls/9': {'head': {'sha': 'g'}},
        at('b'): {'workflow_runs': [run(31, 'b'), run(32, 'b')]},
        at('d'): {'workflow_runs': [run(40, 'd'), run(41, 'd')]},
        at('g'): {'workflow_runs': [run(51, 'g', repository='fork/r')]},
        'actions/runs/31/artifacts?name=test-setup-root&per_page=100': artifacts(False),
        'actions/runs/40/artifacts?name=test-setup-root&per_page=100': artifacts(True),
        'actions/runs/41/artifacts?name=test-setup-root&per_page=100': artifacts(False)}
    output, _ = selected(routes, GITHUB_EVENT_NAME='merge_group')
    # The pull request names the head being merged; the newest successful run at that head with a live bundle wins.
    assert output == 'runs=31 41\n', output

def a_push_to_the_default_branch_gets_the_bundle_of_its_merge_group_run():
    routes = {'actions/runs/100': {'workflow_id': 55},
              'actions/workflows/55/runs?event=merge_group&head_sha=merged&status=success&per_page=100': {'workflow_runs': [run(70, 'merged'), run(71, 'merged')]},
              'actions/runs/70/artifacts?name=test-setup-root&per_page=100': artifacts(True),
              'actions/runs/71/artifacts?name=test-setup-root&per_page=100': artifacts(False)}
    output, _ = selected(routes, GITHUB_EVENT_NAME='push', CACHE_WRITER='true')
    assert output == 'runs=71\n', output

def other_events_get_no_candidates():
    for extra in [{'GITHUB_EVENT_NAME': 'pull_request'}, {'GITHUB_EVENT_NAME': 'push'}, {'GITHUB_EVENT_NAME': 'workflow_dispatch'}]:
        output, asked = selected({}, **extra)
        assert output == 'runs=\n' and asked == '', (extra, output, asked)

for check in [a_merge_group_gets_the_bundles_of_its_pull_request_runs, a_push_to_the_default_branch_gets_the_bundle_of_its_merge_group_run,
              other_events_get_no_candidates]:
    check()
    print('PASS ' + check.__name__, flush=True)

def setup_artifacts_are_host_qualified():
    for job, names in [('test-prepare', ['Test setup candidates', 'Download test setup candidates', 'Upload test setup']),
                       ('tests', ['Download test setup', 'Download test setup in one stream'])]:
        for name in names:
            entry = next(step for step in document['jobs'][job]['steps'] if step['name'] == name)
            artifact = entry.get('env', {}).get('ARTIFACT_NAME') or entry['with']['name']
            assert "format('-{0}', matrix.runner)" in artifact, (job, name)

setup_artifacts_are_host_qualified()
print('PASS setup_artifacts_are_host_qualified')
