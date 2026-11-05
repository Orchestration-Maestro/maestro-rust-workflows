import json
import os
import pathlib
import re
import subprocess
import tempfile

root = pathlib.Path.cwd()
interpreter = re.compile(r'python3((?:\s+-\S+)*)')
# Any interpolation that names a token, whatever its environment key, is treated as one.
token_value = re.compile(r'\$\{\{\s*(?:secrets\.|[^}]*\btoken\b)')

def read_workflows():
    workflows = []
    for path in sorted((root / '.github/workflows').glob('*.yml')):
        parsed = subprocess.run(['yq', '-o=json', '-I=0', '.'], input=path.read_text(encoding='utf-8'),
                                text=True, capture_output=True)
        assert parsed.returncode == 0, f'yq failed to parse {path.name}: {parsed.stderr}'
        document = json.loads(parsed.stdout)
        assert document.get('jobs'), f'{path.name}: no jobs to check'
        workflows.append((path, document))
    return workflows

workflows = read_workflows()

def holds_token(*environments):
    for environment in environments:
        for key, value in (environment or {}).items():
            if key in ('GH_TOKEN', 'GITHUB_TOKEN'):
                return True
            if isinstance(value, str) and token_value.search(value):
                return True
    return False

def token_steps(document):
    # A step holds a token when it or its job or its workflow sets one in the environment, or interpolates one.
    for job, spec in (document.get('jobs') or {}).items():
        if not isinstance(spec, dict) or 'uses' in spec:
            continue
        for step in spec.get('steps') or []:
            run = step.get('run')
            if not isinstance(run, str):
                continue
            if holds_token(document.get('env'), spec.get('env'), step.get('env')) or token_value.search(run):
                yield job, step.get('name', ''), run

def no_token_bearing_step_starts_python_without_isolation():
    offenders, inspected = [], 0
    for path, document in workflows:
        for job, name, run in token_steps(document):
            for match in interpreter.finditer(run):
                inspected += 1
                if '-I' in match[1].split():
                    continue
                line = run[:match.start()].rsplit('\n', 1)[-1] + match[0]
                offenders.append(f'{path.name}: {job} / {name}: {line.strip()}')
    assert inspected >= 31, f'Read {inspected} token-bearing Python invocations; the sweep lost its steps'
    assert not offenders, (f'{len(offenders)} token-bearing steps start Python without isolation:\n'
                           + '\n'.join(offenders))

def floor_body():
    workflow = next(document for path, document in workflows if path.name == 'ci.yml')
    steps = workflow['jobs']['quality']['steps']
    return next(step['run'] for step in steps if step.get('name') == 'Compile cache hit-rate floor')

def step_result(script, directory):
    (directory / 'sccache-stats').write_text('Compile requests executed 0\n', encoding='utf-8')
    (directory / 'json.py').write_text("raise SystemExit('the caller checkout ran with the token')\n", encoding='utf-8')
    environment = {key: value for key, value in os.environ.items() if not key.startswith(('GITHUB_', 'CACHE_'))} | {
        'RUNNER_TEMP': str(directory), 'GH_TOKEN': 'secret-token', 'GITHUB_REPOSITORY': 'o/r', 'CACHE_SHAPE': 'shape',
        'CACHE_MATCHED_KEY': '', 'CACHE_PREFIX': 'prefix-', 'CACHE_NAMESPACE': 'namespace', 'CACHE_SEEDER': '.'}
    return subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', script], cwd=directory, env=environment, text=True,
                          capture_output=True)

def a_module_in_the_caller_checkout_never_reaches_a_token_step():
    # The floor step runs in the caller's checkout with the token, so the modules it imports must come from elsewhere.
    script = floor_body()
    assert 'python3 -I -' in script, 'The compile cache floor step does not start Python in isolated mode'
    with tempfile.TemporaryDirectory() as temporary:
        isolated = step_result(script, pathlib.Path(temporary))
        unisolated = step_result(script.replace('python3 -I -', 'python3 -'), pathlib.Path(temporary))
    assert isolated.returncode == 0 and 'not judged' in isolated.stdout, isolated.stdout + isolated.stderr
    assert unisolated.returncode != 0 and 'the caller checkout ran with the token' in unisolated.stderr, unisolated.stderr

for check in [no_token_bearing_step_starts_python_without_isolation, a_module_in_the_caller_checkout_never_reaches_a_token_step]:
    check()
    print('PASS ' + check.__name__, flush=True)
