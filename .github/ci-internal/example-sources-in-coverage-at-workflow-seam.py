import ast
import os
import pathlib
import shutil
import subprocess
import tempfile
import textwrap

root = pathlib.Path.cwd()
workflow = (root / '.github/workflows/ci.yml').read_text(encoding='utf-8')
caller = (root / '.github/ci-internal/caller-checks-at-workflow-seam.py').read_text(encoding='utf-8')
tree = ast.parse(textwrap.dedent(caller))
tree.body = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == 'body']
exec(compile(tree, 'workflow body resolver', 'exec'))

def example_sources_are_measured():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary) / 'caller'
        shutil.copytree(root / 'fixtures/example-coverage', directory, ignore=shutil.ignore_patterns('target'))
        registry = directory / 'registry/src/fixture'
        registry.mkdir(parents=True)
        (registry / 'lib.rs').write_text('/// A dependency fixture.\npub fn answer() -> u32 { 42 }\n', encoding='utf-8')
        test = directory / 'src/tests/coverage_example.rs'
        test.write_text(test.read_text(encoding='utf-8') +
                        '\n#[path = "../../registry/src/fixture/lib.rs"]\nmod dependency;\n'
                        '#[test]\nfn dependency_runs() { assert_eq!(dependency::answer(), 42); }\n', encoding='utf-8')
        toolchain = subprocess.check_output(['rustup', 'show', 'active-toolchain'], cwd=directory, text=True).split()[0]
        subprocess.run(['cargo', 'clippy', '--all-targets', '--locked', '--', '-D', 'warnings'], cwd=directory, check=True)
        for name, floor, shard in [('Coverage', '80', ''), ('Isolated tests', '80', ''),
                                   ('Isolated tests', '', '1')]:
            env = os.environ | {'RUNNER_TEMP': temporary, 'RUSTUP_TOOLCHAIN': toolchain,
                                'CARGO_COVERAGE_MIN_LINES': floor, 'COLLECT_COVERAGE': 'true',
                                'SHARD': shard, 'TEST_SHARD_COUNT': '1'}
            command = body(name)
            result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', command],
                                    cwd=directory, env=env, capture_output=True, text=True)
            report = directory / (f'shard-{shard}.lcov' if shard else 'target/single-coverage/shard-1.lcov')
            assert result.returncode == 0, result.stdout + result.stderr + (report.read_text(encoding='utf-8') if report.exists() else '')
            records = report.read_text(encoding='utf-8').split('end_of_record')
            sources = [os.path.normpath(line[3:]) for record in records
                       for line in record.splitlines() if line.startswith('SF:')]
            print(f'{name} floor={floor} LCOV sources: {sources}', flush=True)
            example = [record for record in records if any(
                line.startswith('SF:') and os.path.normpath(line[3:]) == str(directory / 'examples/coverage.rs')
                for line in record.splitlines())]
            assert example, 'examples/coverage.rs missing from LCOV'
            assert any(line.startswith('DA:') and int(line.split(',')[1]) > 0
                       for record in example for line in record.splitlines()), example
            assert not any('/tests/' in source or '/registry/' in source for source in sources), sources
            print(f'PASS example_sources_are_measured: {name}, floor={floor}', flush=True)
            if shard:
                (directory / 'coverage').mkdir()
                shutil.copyfile(report, directory / 'coverage/shard-1.lcov')
                merged = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', body('Merge coverage')],
                                        cwd=directory, env=env, capture_output=True, text=True)
                assert merged.returncode == 0, merged.stdout + merged.stderr
                print(merged.stdout, flush=True)

def single_coverage_runs_setup_scripts(name):
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary) / 'caller'
        shutil.copytree(root / 'fixtures/sample', directory, ignore=shutil.ignore_patterns('target'))
        (directory / '.config').mkdir()
        (directory / '.config/nextest.toml').write_text(
            'experimental=["setup-scripts"]\n'
            '[scripts.setup.prepare]\ncommand="sh .config/prepare.sh"\n'
            '[[profile.default.scripts]]\nfilter="all()"\nsetup="prepare"\n'
            '[profile.default]\ndefault-filter="none()"\n', encoding='utf-8')
        (directory / '.config/prepare.sh').write_text(
            'echo WORKFLOW_SETUP=ready >> "$NEXTEST_ENV"\n', encoding='utf-8')
        (directory / 'src/lib.rs').write_text(
            '#[test]\nfn requires_setup() {\n'
            '    assert_eq!(std::env::var("WORKFLOW_SETUP").unwrap(), "ready");\n'
            '}\n', encoding='utf-8')
        toolchain = subprocess.check_output(['rustup', 'show', 'active-toolchain'], cwd=directory, text=True).split()[0]
        env = os.environ | {'RUNNER_TEMP': temporary, 'RUSTUP_TOOLCHAIN': toolchain,
                            'CARGO_COVERAGE_MIN_LINES': '100', 'COLLECT_COVERAGE': 'true',
                            'SHARD': '', 'TEST_SHARD_COUNT': '16'}
        result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', body(name)],
                                cwd=directory, env=env, capture_output=True, text=True)
        assert result.returncode == 0, result.stdout + result.stderr
        assert 'requires_setup' in result.stderr + result.stdout
        assert '1 passed' in result.stderr + result.stdout
        print(f'PASS single_coverage_runs_setup_scripts: {name}', flush=True)

def coverage_retries_report_flaky_tests_and_keep_failures():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary) / 'caller'
        shutil.copytree(root / 'fixtures/sample', directory, ignore=shutil.ignore_patterns('target'))
        source = directory / 'src/lib.rs'
        source.write_text('#[test] fn coverage_only_flaky() { assert_eq!(std::env::var("NEXTEST_ATTEMPT").unwrap(), "3"); }\n', encoding='utf-8')
        toolchain = subprocess.check_output(['rustup', 'show', 'active-toolchain'], cwd=directory, text=True).split()[0]
        env = os.environ | {'RUNNER_TEMP': temporary, 'RUSTUP_TOOLCHAIN': toolchain,
                            'CARGO_COVERAGE_MIN_LINES': '0'}
        result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', body('Coverage')],
                                cwd=directory, env=env, capture_output=True, text=True)
        assert result.returncode == 0, result.stdout + result.stderr
        log = directory / 'nextest.log'
        assert log.exists(), 'Coverage must capture nextest output for its flaky summary'
        assert 'FLAKY' in log.read_text(encoding='utf-8')
        summary = directory / 'summary'
        checks = workflow.split('  checks:\n', 1)[1].split('\n  fuzz:', 1)[0]
        assert "if: always()" in checks.split('      - name: Coverage flaky test summary\n', 1)[1].split('      - name:', 1)[0]
        assert body('Coverage flaky test summary') == body('Flaky test summary')
        result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', body('Coverage flaky test summary')],
                                cwd=directory, env=env | {'GITHUB_STEP_SUMMARY': str(summary)}, capture_output=True, text=True)
        assert result.returncode == 0 and 'coverage_only_flaky' in summary.read_text(encoding='utf-8'), result.stderr
        source.write_text('#[test] fn coverage_permanent_failure() { panic!("permanent"); }\n', encoding='utf-8')
        result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', body('Coverage')],
                                cwd=directory, env=env, capture_output=True, text=True)
        assert result.returncode != 0, result.stdout + result.stderr
        assert 'coverage_permanent_failure' in log.read_text(encoding='utf-8')
        print('PASS coverage_retries_report_flaky_tests_and_keep_failures', flush=True)

coverage_retries_report_flaky_tests_and_keep_failures()

single_coverage_runs_setup_scripts('Isolated tests')
single_coverage_runs_setup_scripts('Coverage')
example_sources_are_measured()
