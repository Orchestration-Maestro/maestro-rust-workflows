import os
import pathlib
import re
import shlex
import subprocess

workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding="utf-8")
commands = re.findall(r'^        run: (cargo .+)$', workflow, re.MULTILINE) + ['cargo test --workspace --locked']
expected = [
    'cargo fmt --all --check',
    'cargo clippy --workspace --all-targets --locked -- -D warnings',
    'cargo doc --workspace --no-deps --locked',
    'cargo test --workspace --locked',
]
assert commands == expected, f'Unexpected check commands: {commands}'
def snapshot_only_transfers_fail_open():
    count = 0
    for step in re.split(r'^      - name: ', workflow, flags=re.M)[1:]:
        if re.search(r'^        continue-on-error:', step, re.M):
            assert step.splitlines()[0] in ['Restore compile cache', 'Save compile cache', 'Restore test setup cache', 'Save test setup cache']
            assert re.search(r'^        uses: actions/cache/(restore|save)@0400d5f644dc74513175e3cd8d07132dd4860809 ', step, re.M)
            assert '        continue-on-error: true\n' in step
            count += 1
    assert count == 20
    assert not re.search(r'^    continue-on-error:', workflow, re.M)

snapshot_only_transfers_fail_open()
print('PASS snapshot_only_transfers_fail_open')
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
for fixture, broken in [('unformatted', 0), ('clippy-warning', 1),
                        ('failing-test', 3), ('undocumented', 2), ('broken-doc-link', 2)]:
    for check, command in enumerate(commands):
        env = os.environ.copy()
        if check == 2:
            env['RUSTDOCFLAGS'] = flags.group(1)
        argv = shlex.split(command)
        if os.environ.get('MAESTRO_LOCAL_CAPPED'):
            argv.insert(0, os.path.expanduser('~/.local/bin/capped'))
            env['CARGO_BUILD_JOBS'] = '3'
        result = subprocess.run(argv, cwd=f'fixtures/{fixture}', env=env)
        expected_failure = check == broken
        print(f'{fixture}: {command}: exit {result.returncode}; '
              f'expected {"failure" if expected_failure else "success"}', flush=True)
        if (result.returncode != 0) != expected_failure:
            failures.append(f'{fixture}: {command}: exit {result.returncode}')
assert not failures, '\n'.join(failures)
