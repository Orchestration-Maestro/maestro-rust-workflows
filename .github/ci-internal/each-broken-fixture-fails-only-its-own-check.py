import ast
import os
import pathlib
import re
import subprocess
import time
from collections.abc import Callable
from typing import cast

workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding="utf-8")
commands = re.findall(r'^        run: (cargo .+)$', workflow, re.MULTILINE) + ['cargo test --workspace --locked']
expected = [
    'cargo fmt --all --check',
    'cargo doc --workspace --no-deps --locked',
    'cargo doc --workspace --no-deps --locked --all-features',
    'cargo test --workspace --locked',
]
assert commands == expected, f'Unexpected check commands: {commands}'
code = pathlib.Path('.github/ci-internal/caller-checks-at-workflow-seam.py').read_text(encoding='utf-8')
tree = ast.parse(code)
tree.body = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == 'body']
# The shared body resolver is the module-level function alone: its runner runs the whole seam.
resolver: dict[str, object] = {'workflow': workflow}
exec(compile(tree, 'workflow body resolver', 'exec'), resolver)
body = cast(Callable[[str], str], resolver['body'])
commands.insert(1, body('Lint'))

def only_cache_housekeeping_fails_open():
    transfers = ['Restore compile cache', 'Save compile cache', 'Restore test setup cache', 'Save test setup cache',
                 'Restore Cargo git cache', 'Save Cargo git cache', 'Restore sandbox runtime', 'Save sandbox runtime',
                 'Restore browser runtime packages', 'Save browser runtime packages',
                 'Test setup candidates', 'Download test setup candidates']
    transferred = pruned = cancelled = 0
    for step in re.split(r'^      - name: ', workflow, flags=re.M)[1:]:
        if not re.search(r'^        continue-on-error:', step, re.M):
            continue
        name = step.splitlines()[0]
        if name == 'Prune superseded caches':
            # Pruning runs after the build, so a failed deletion costs a superseded entry and never a check.
            pruned += 1
        elif name == 'Cancel replaced merge groups':
            # A failed cancellation leaves a dropped group's run going and never fails the live group that tried.
            cancelled += 1
        else:
            assert name in transfers, name
            # The candidate steps are transfers of earlier runs' bundles; a failed lookup or download leaves the restored bundle as the only base.
            assert re.search(r'^        uses: actions/cache/(restore|save)@0400d5f644dc74513175e3cd8d07132dd4860809 ', step, re.M) or name in ('Test setup candidates', 'Download test setup candidates'), name
            transferred += 1
        assert '        continue-on-error: true\n' in step
    assert transferred == 34
    assert pruned == 1
    assert cancelled == 1
    assert not re.search(r'^    continue-on-error:', workflow, re.M)

only_cache_housekeeping_fails_open()
print('PASS only_cache_housekeeping_fails_open')
assert '|| true' not in workflow
for step in workflow.split('      - name:')[1:]:
    if any(f'run: {command}\n' in step for command in commands[:3]):
        assert not re.search(r'^        if:', step, re.MULTILINE)
base_test = next(step for step in workflow.split('      - name:')[1:]
                 if step.startswith(' Tests\n'))
assert "if: $" + "{{ env.WARM_CACHE != 'true' && needs.declaration.outputs.enabled != 'true' }}" in base_test
isolated_test = next(step for step in workflow.split('      - name:')[1:]
                     if step.startswith(' Isolated tests\n'))
assert "if: env.WARM_CACHE == 'true' || needs.declaration.outputs.enabled == 'true'" in isolated_test
assert 'run_isolated cargo test --locked --doc' in isolated_test
assert 'llvm-cov nextest' in isolated_test

docs_step = next(step for step in workflow.split('      - name:')[1:]
                 if 'run: cargo doc --workspace --no-deps --locked\n' in step)
flags = re.search(r'^          RUSTDOCFLAGS: (.+)$', docs_step, re.MULTILINE)
assert flags and flags.group(1) == '-D warnings -D missing_docs'

concurrency = re.search(r'^concurrency:\n((?:[ \t].*\n)+)', workflow, re.MULTILINE)
assert concurrency, 'Missing top-level concurrency'
group = re.search(r'^  group: (.+)$', concurrency.group(1), re.MULTILINE)
assert group and 'github.ref' in group.group(1), 'Concurrency group must contain github.ref'
cancellation = re.search(r'^  cancel-in-progress: (.+)$', concurrency.group(1), re.MULTILINE)
assert cancellation and cancellation.group(1) == "$" + "{{ github.event_name == 'pull_request' }}", \
    'Cancellation must be limited to pull requests'

failures = []
for fixture, broken in [('unformatted', (0,)), ('clippy-warning', (1,)),
                        ('failing-test', (4,)), ('undocumented', (2, 3)), ('broken-doc-link', (2, 3))]:
    for check, command in enumerate(commands):
        env = os.environ.copy()
        if check in (2, 3):
            env['RUSTDOCFLAGS'] = flags.group(1)
        argv = ['bash', '-e', '-o', 'pipefail', '-c', command]
        if os.environ.get('MAESTRO_LOCAL_CAPPED'):
            argv.insert(0, os.path.expanduser('~/.local/bin/capped'))
            env['CARGO_BUILD_JOBS'] = '3'
        started = time.perf_counter()
        result = subprocess.run(argv, cwd=f'fixtures/{fixture}', env=env)
        elapsed = time.perf_counter() - started
        expected_failure = check in broken
        print(f'{fixture}: {command}: exit {result.returncode}; '
              f'expected {"failure" if expected_failure else "success"}; elapsed {elapsed:.2f} s', flush=True)
        if (result.returncode != 0) != expected_failure:
            failures.append(f'{fixture}: {command}: exit {result.returncode}')
assert not failures, '\n'.join(failures)
