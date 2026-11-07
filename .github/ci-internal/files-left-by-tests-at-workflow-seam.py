import os
import pathlib
import re
import subprocess
import tempfile
import textwrap

workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
BASELINE = 'Test checkout baseline'
AFTER = 'Tests leave no files behind'

def body(name):
    for step in workflow.split('      - name: ')[1:]:
        if step.splitlines()[0] != name:
            continue
        match = re.search(r'^        run: (?:&[\w-]+ )?\|\n((?:          .*\n|\n)+)', step, re.MULTILINE)
        assert match, f'Missing executable body: {name}'
        return textwrap.dedent(match[1])
    raise AssertionError(f'Missing step: {name}')

def job(name, following):
    return workflow.split(f'  {name}:\n', 1)[1].split(f'\n  {following}:', 1)[0]

def exclusions():
    # Declared once for both steps: the files the workflow itself writes into the checkout.
    match = re.search(r"^      TEST_CHECKOUT_EXCLUSIONS: '([^']+)'$", job('tests', 'doctests'), re.MULTILINE)
    assert match, 'Missing the checkout exclusions the state steps share'
    return match[1]

def git(directory, *args):
    return subprocess.check_output(['git', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.com',
                                    '-c', 'commit.gpgsign=false', *args], cwd=directory, text=True).strip()

def repository(directory):
    (directory / 'crates/maestro-rpc').mkdir(parents=True)
    (directory / 'crates/maestro-rpc/lib.rs').write_text('pub fn run() {}\n', encoding='utf-8')
    (directory / 'README.md').write_text('fixture\n', encoding='utf-8')
    git(directory, 'init', '--quiet')
    git(directory, 'add', '.')
    git(directory, 'commit', '-qm', 'fixture')

def state_steps(*steps):
    """Run the checkout state steps in a fresh checkout; return each result in order."""
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary) / 'checkout'
        directory.mkdir()
        repository(directory)
        runner_temp = pathlib.Path(temporary) / 'runner-temp'
        runner_temp.mkdir()
        env = {key: value for key, value in os.environ.items() if not key.startswith('GITHUB_')} | {
            'RUNNER_TEMP': str(runner_temp), 'TEST_CHECKOUT_EXCLUSIONS': exclusions()}
        results = []
        for write, name in steps:
            write(directory)
            results.append(subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', body(name)], cwd=directory,
                                          env=env, text=True, capture_output=True))
        return results

def files_left_by_the_tests_fail_the_shard():
    def leftover(directory):
        (directory / 'crates/maestro-rpc/agent').mkdir()
        (directory / 'crates/maestro-rpc/agent/auth.json').write_text('{}\n', encoding='utf-8')
        (directory / 'README.md').write_text('fixture, edited by a test\n', encoding='utf-8')
        (directory / 'crates/maestro-rpc/lib.rs').unlink()
    baseline, after = state_steps((lambda directory: None, BASELINE), (leftover, AFTER))
    assert baseline.returncode == 0, baseline.stdout + baseline.stderr
    assert after.returncode != 0, after.stdout + after.stderr
    # Every file the tests added, edited or deleted is named, and nothing else is.
    assert '::error file=crates/maestro-rpc/agent/auth.json::' in after.stdout, after.stdout
    assert '::error file=README.md::' in after.stdout, after.stdout
    assert '::error file=crates/maestro-rpc/lib.rs::' in after.stdout, after.stdout
    assert after.stdout.count('::error ') == 3, after.stdout

def the_workflow_own_files_never_fail_the_shard():
    def written_by_the_workflow(directory):
        # The workflow itself fills the build directory, the test log, the shard report and the tool lock.
        (directory / 'target/llvm-cov-target/bin').mkdir(parents=True)
        (directory / 'target/llvm-cov-target/bin/sample').write_text('', encoding='utf-8')
        (directory / 'nextest.log').write_text('PASS [ 0.001s] sample\n', encoding='utf-8')
        (directory / 'shard-1.lcov').write_text('SF:/src/lib.rs\nDA:1,1\nend_of_record\n', encoding='utf-8')
        (directory / 'mise.lock').write_text('just 1.58.0\n', encoding='utf-8')
    _, after = state_steps((lambda directory: None, BASELINE), (written_by_the_workflow, AFTER))
    assert after.returncode == 0, after.stdout + after.stderr
    assert '::error' not in after.stdout, after.stdout

def a_checkout_changed_before_the_tests_is_not_the_tests_doing():
    def already_changed(directory):
        (directory / 'NOTES.md').write_text('a local note\n', encoding='utf-8')
        (directory / 'README.md').write_text('fixture, edited before the tests\n', encoding='utf-8')
    baseline, after = state_steps((already_changed, BASELINE), (lambda directory: None, AFTER))
    assert baseline.returncode == 0, baseline.stdout + baseline.stderr
    assert after.returncode == 0, after.stdout + after.stderr
    assert '::error' not in after.stdout, after.stdout

def a_missing_baseline_fails_the_check():
    after, = state_steps((lambda directory: None, AFTER))
    assert after.returncode != 0, after.stdout + after.stderr
    assert 'baseline' in after.stderr, after.stderr

def every_test_route_runs_between_the_state_and_its_check():
    tests = job('tests', 'doctests')
    names = [step.splitlines()[0] for step in tests.split('      - name: ')[1:]]
    assert names.index(BASELINE) < names.index('Tests') < names.index('Isolated tests') < names.index(AFTER), names
    # The doctests job runs the same steps, so its route is between them too.
    assert '    steps: &test-steps\n' in tests and '    steps: *test-steps\n' in job('doctests', 'tests-result')
    # A failed test still names the files it left.
    assert re.search(r'^      - name: ' + AFTER + r'\n        if: always\(\)\n', tests, re.MULTILINE), tests

for check in [files_left_by_the_tests_fail_the_shard, the_workflow_own_files_never_fail_the_shard,
              a_checkout_changed_before_the_tests_is_not_the_tests_doing, a_missing_baseline_fails_the_check,
              every_test_route_runs_between_the_state_and_its_check]:
    check()
    print('PASS ' + check.__name__, flush=True)
