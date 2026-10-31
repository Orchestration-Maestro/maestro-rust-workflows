import json
import os
import pathlib
import re
import subprocess
import tempfile
import textwrap
import time
from unittest.mock import patch

source = pathlib.Path('.github/workflows/post-landing.yml').read_text(encoding='utf-8')
assert '          permission-workflows: write\n' in source, 'Reverts must handle workflow-file landings'
assert '  report:\n    permissions:\n      contents: read\n      actions: read\n' in source, 'The reporter job must hold actions read'
assert re.findall(r'^      ([\w-]+):$', source.split('    inputs:\n', 1)[1].split('    secrets:\n', 1)[0], re.M) == ['run-id'], 'The reusable reporter must take only the run id'
step = source.split('      - name: Report and propose revert\n', 1)[1]
code = textwrap.dedent(step.split('        run: |\n', 1)[1])
real_run = subprocess.run

def landing_report_prints_run_link_first():
    start = time.monotonic()
    link_step = source.split('      - name: Original full-suite run\n', 1)[1].split('      - name: ', 1)[0]
    assert source.index('name: Original full-suite run') < source.index('name: Mint the report token'), 'A failed token step must still show the run'
    assert 'RUN_URL: $' + '{{ github.server_url }}/$' + '{{ github.repository }}/actions/runs/$' + '{{ inputs.run-id }}\n' in link_step and '        if:' not in link_step
    printed = real_run(['bash', '-e', '-c', textwrap.dedent(link_step.split('        run: |\n', 1)[1])],
                       env={'PATH': os.environ['PATH'], 'RUN_URL': 'https://example.invalid/runs/9'}, capture_output=True, text=True, check=True).stdout
    assert 'https://example.invalid/runs/9' in printed, printed
    print(f'PASS landing_report_prints_run_link_first ({time.monotonic() - start:.3f}s)', flush=True)

landing_report_prints_run_link_first()

def landing_report_handles(scenario):
    with tempfile.TemporaryDirectory() as temporary:
        root = pathlib.Path(temporary)
        remote, checkout = root / 'remote', root / 'checkout'
        def git(*args, cwd=checkout):
            return real_run(['git', *args], cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()
        git('init', '--bare', '--initial-branch=main', str(remote), cwd=root)
        git('clone', str(remote), str(checkout), cwd=root)
        git('config', 'user.name', 'Test'); git('config', 'user.email', 'test@example.invalid')
        git('config', 'commit.gpgsign', 'false')
        (checkout / 'base').write_text('good', encoding='utf-8')
        git('add', '.'); git('commit', '-m', 'base')
        base, earlier = git('rev-parse', 'HEAD'), None
        if scenario == 'batched':
            (checkout / 'other').write_text('also pushed', encoding='utf-8')
            git('add', '.'); git('commit', '-m', 'earlier landing'); earlier = git('rev-parse', 'HEAD')
        (checkout / 'defect').write_text('bad', encoding='utf-8')
        git('add', '.'); git('commit', '-m', 'landing')
        sha = git('rev-parse', 'HEAD'); git('push', 'origin', 'main')
        if scenario == 'conflict':
            (checkout / 'defect').write_text('newer change', encoding='utf-8')
            git('add', '.'); git('commit', '-m', 'later landing'); git('push', 'origin', 'main')
        if scenario == 'foreign':
            git('commit', '--allow-empty', '-m', 'revert(ci): other landing', '-m', 'This reverts commit ' + '1' * 40 + '.')
            git('push', 'origin', 'HEAD:refs/heads/ci/revert-' + sha); git('reset', '--hard', sha)
        calls = []
        issues, prs = [], []
        run = {'id': 7, 'workflow_id': 5, 'event': 'push', 'head_branch': 'main', 'head_sha': sha,
               'status': 'completed', 'conclusion': 'failure', 'run_attempt': 1,
               'repository': {'full_name': 'owner/repo'}, 'html_url': 'https://github.com/owner/repo/actions/runs/7'}
        if scenario == 'pass': run['conclusion'] = 'success'
        landing = {'number': 42, 'merge_commit_sha': sha, 'merged_at': 'today',
                   'head': {'sha': sha}, 'base': {'ref': 'main', 'repo': {'full_name': 'owner/repo'}}}
        def command(argv, **kwargs):
            if argv[0] != 'gh':
                return real_run(argv, **kwargs)
            expected = 'token' if argv[1] == 'api' else 'report-token'
            assert kwargs['env']['GH_TOKEN'] == expected
            calls.append(argv)
            if argv[1] == 'api':
                endpoint = argv[-1]
                if scenario == 'permission':
                    return subprocess.CompletedProcess(argv, 1, '', 'HTTP 403: Resource not accessible')
                if endpoint.endswith('/runs/7'): value = run
                elif endpoint.endswith('/attempts/1/jobs?per_page=100'):
                    jobs = [] if scenario == 'pass' else [{'id': 8, 'name': {'doctest': 'Doctests', 'non-suite': 'Format'}.get(scenario, 'Tests / Test shard (1)'), 'conclusion': 'failure'}]
                    if scenario == 'pagination':
                        assert '--paginate' in argv and '--slurp' in argv
                        value = [{'jobs': []}, {'jobs': jobs}]
                    else:
                        if scenario == 'large': jobs = [dict(jobs[0], name=f'Test shard ({n}/16)') for n in range(20)]
                        value = [{'jobs': jobs}] if '--slurp' in argv else {'jobs': jobs}
                elif endpoint.startswith('repos/owner/repo/actions/workflows/5/runs?event=push&branch=main&head_sha='):
                    value = {'total_count': int(endpoint.rsplit('=', 1)[1] in ((sha,) if scenario == 'unbounded' else (base, sha)))}
                elif endpoint.endswith('/commits/' + str(earlier) + '/pulls'): value = [dict(landing, number=41, merge_commit_sha=earlier, head={'sha': earlier})]
                elif endpoint.endswith('/commits/' + sha + '/pulls'): value = [] if scenario == 'unknown' else [dict(landing, number=99, merge_commit_sha='0' * 40, head={'sha': '0' * 40}), landing]
                elif endpoint.endswith('/jobs/8/logs'):
                    assert '--allow-escape-sequences' in argv
                    return subprocess.CompletedProcess(argv, 0, '\x1b[31mFAIL [0.01s] sample::broken\x1b[0m\n' + ('FAIL ' + 'x' * 7990 if scenario == 'large' else ''), '')
                else: raise AssertionError(endpoint)
            elif argv[1:3] == ['issue', 'list']: value = issues
            elif argv[1:3] == ['pr', 'list']: value = prs
            elif argv[1:3] == ['issue', 'create']:
                issues.append({'title': argv[argv.index('--title') + 1], 'url': 'https://github.com/owner/repo/issues/43'})
                assert len(kwargs['input']) < 65536
                assert '#42' in kwargs['input'] and sha in kwargs['input'] and 'sample::broken' in kwargs['input'] and run['html_url'] in kwargs['input']
                assert ('#41' in kwargs['input']) == (scenario == 'batched')
                return subprocess.CompletedProcess(argv, 0, issues[-1]['url'], '')
            elif argv[1:3] == ['pr', 'create']:
                if scenario == 'resume' and not any(call[1:3] == ['pr', 'create'] for call in calls[:-1]):
                    return subprocess.CompletedProcess(argv, 1, '', 'temporary API failure')
                assert argv[argv.index('--title') + 1].startswith('revert(ci):')
                assert '#42' in kwargs['input'] and issues[0]['url'] in kwargs['input']
                prs.append({'url': 'https://github.com/owner/repo/pull/44'})
                return subprocess.CompletedProcess(argv, 0, prs[-1]['url'], '')
            else: raise AssertionError(argv)
            return subprocess.CompletedProcess(argv, 0, json.dumps(value), '')
        env = {'REPOSITORY': 'owner/repo', 'RUN_ID': '7', 'GH_TOKEN': 'token', 'REPORT_TOKEN': 'report-token', 'GITHUB_SERVER_URL': 'https://github.com'}
        with patch.dict(os.environ, env), patch('subprocess.run', side_effect=command):
            previous = pathlib.Path.cwd()
            os.chdir(checkout)
            try:
                if scenario in ('unknown', 'permission', 'conflict', 'resume', 'foreign', 'non-suite', 'batched', 'unbounded'):
                    try:
                        exec(compile(code, 'post-landing workflow', 'exec'), {})
                        raise AssertionError('Expected a clear failure')
                    except SystemExit as error:
                        assert run['html_url'] in str(error), str(error)
                        diagnostic = {'unknown': 'attribute', 'permission': '403', 'conflict': 'revert', 'resume': 'temporary API failure', 'foreign': 'does not target', 'non-suite': 'no failed suite job', 'batched': 'landed together', 'unbounded': 'No earlier full-suite run'}[scenario]
                        assert diagnostic in str(error), str(error)
                    if scenario != 'resume':
                        assert prs == []
                        assert len(issues) == (scenario in ('conflict', 'foreign', 'batched'))
                        return
                exec(compile(code, 'post-landing workflow', 'exec'), {})
                if scenario == 'pass':
                    assert issues == prs == []
                    assert all(call[1] == 'api' for call in calls)
                    return
                assert git('show', '-s', '--format=%s', 'origin/ci/revert-' + sha).startswith('revert(ci):')
                assert git('show', 'origin/ci/revert-' + sha + ':base') == 'good'
                assert real_run(['git', 'cat-file', '-e', 'origin/ci/revert-' + sha + ':defect'], cwd=checkout, capture_output=True).returncode != 0
                exec(compile(code, 'post-landing workflow', 'exec'), {})
                assert len(issues) == len(prs) == 1
                assert sum(call[1:3] == ['issue', 'create'] for call in calls) == 1
                assert sum(call[1:3] == ['pr', 'create'] for call in calls) == (2 if scenario == 'resume' else 1)
            finally:
                os.chdir(previous)

for scenario in ['large', 'failure', 'pagination', 'doctest', 'resume', 'pass', 'unknown', 'permission', 'conflict', 'foreign', 'non-suite', 'batched', 'unbounded']:
    start = time.monotonic()
    landing_report_handles(scenario)
    print(f'PASS landing_report_handles_{scenario} ({time.monotonic() - start:.3f}s)', flush=True)
