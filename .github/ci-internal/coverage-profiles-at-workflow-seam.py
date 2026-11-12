import json
import os
import pathlib
import subprocess
import tempfile

import seam_runner

root = pathlib.Path.cwd()
workflow = (root / '.github/workflows/ci.yml').read_text(encoding='utf-8')
# The step runner and its recording tool stand-ins, so a case runs the workflow's own step.
resolver = seam_runner.load(__name__)
body = resolver['body']
execute = resolver['execute']
install_recording_runner = resolver['install_recording_runner']

document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
CHECK = document['env']['COVERAGE_PROFILES']
OBJECTS = document['env'].get('COVERAGE_OBJECTS')
# The profile runtime appends the merge pool index to the signature, and prints the signature in decimal.
SIGNATURE = '9182736455463728'

def checkout(directory):
    """A cargo checkout with the instrumented build's directory, and the profiles' own directory."""
    (directory / 'target/llvm-cov-target/debug/deps').mkdir(parents=True)
    return directory / 'target/llvm-cov-target'

def program(directory, name, signature):
    """A program that writes the profile its own merge signature names, as the profile runtime does."""
    path = directory / 'target/llvm-cov-target/debug/deps' / name
    path.write_text('#!/bin/bash\n'
                    f'printf PROFILE > "${{LLVM_PROFILE_FILE//%4m/{signature}_0}}"\n', encoding='utf-8')
    path.chmod(0o755)

def toolchain(directory, accepted):
    """A toolchain whose own merge tool is a stand-in, so a complete profile needs no instrumented build."""
    tool = directory / 'toolchain/lib/rustlib/test-host/bin'
    tool.mkdir(parents=True)
    (tool / 'llvm-profdata').write_text(f'#!/bin/sh\nhead -c 7 "$2" | grep -qx {accepted} || exit 1\n', encoding='utf-8')
    (tool / 'llvm-profdata').chmod(0o755)
    compiler = directory / 'stand-in-toolchain'
    compiler.mkdir()
    (compiler / 'rustc').write_text('#!/bin/sh\n'
                                    'case "$1" in\n'
                                    f'  --print) echo {tool.parents[3]} ;;\n'
                                    '  -vV) echo "host: test-host" ;;\n'
                                    'esac\n', encoding='utf-8')
    (compiler / 'rustc').chmod(0o755)
    return str(compiler) + os.pathsep + os.environ['PATH']

def ambient_toolchain(directory):
    """A merge tool earlier on PATH than the toolchain's own, accepting anything it is given."""
    tool = directory / 'path-tools'
    tool.mkdir()
    (tool / 'llvm-profdata').write_text('#!/bin/sh\nexit 0\n', encoding='utf-8')
    (tool / 'llvm-profdata').chmod(0o755)
    return str(tool) + os.pathsep + os.environ['PATH']

def check(directory, path=None):
    environment = os.environ | ({'PATH': path} if path else {})
    return subprocess.run(['python3', '-c', CHECK], cwd=directory, env=environment, capture_output=True, text=True)

def complete_profiles_pass():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        site = checkout(directory)
        program(directory, 'maestro-test-process-1a2b3c4d', SIGNATURE)
        (site / f'maestro-999-{SIGNATURE}_0.profraw').write_text('PROFILE', encoding='utf-8')
        (site / f'maestro-999-{SIGNATURE}_1.profraw').write_text('PROFILE', encoding='utf-8')
        result = check(directory, toolchain(directory, 'PROFILE'))
        assert result.returncode == 0, result.stdout + result.stderr
        assert 'Coverage profiles: 2 complete' in result.stdout, result.stdout
        assert '::error' not in result.stdout

def no_profiles_pass():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        checkout(directory)
        result = check(directory)
        assert result.returncode == 0, result.stdout + result.stderr
        assert 'Coverage profiles: 0 complete' in result.stdout, result.stdout

def damaged_profiles_fail_naming_each_file_and_its_program():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        site = checkout(directory)
        program(directory, 'maestro-test-process-1a2b3c4d', SIGNATURE)
        # A process stopped while it wrote its profile leaves an empty or a cut file; the pool index is not the signature.
        (site / f'maestro-999-{SIGNATURE}_1.profraw').write_text('', encoding='utf-8')
        (site / 'maestro-999-0f1e2d3c4b5a6978_2.profraw').write_text('cut', encoding='utf-8')
        (site / f'maestro-999-{SIGNATURE}_0.profraw').write_text('PROFILE', encoding='utf-8')
        result = check(directory, toolchain(directory, 'PROFILE'))
        assert result.returncode != 0, result.stdout + result.stderr
        output = result.stdout + result.stderr
        assert f'maestro-999-{SIGNATURE}_1.profraw is empty; its binary is maestro-test-process-1a2b3c4d' in output, output
        # A signature no program carries still names the file.
        assert 'maestro-999-0f1e2d3c4b5a6978_2.profraw is cut; its binary is unnamed' in output, output
        assert 'Damaged coverage profiles: 2 of 3' in output, output

def the_check_reads_profiles_with_the_tool_the_report_merges_them():
    # The toolchain's own merge tool reads the profiles, so a lax one earlier on PATH cannot pass a cut file.
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        site = checkout(directory)
        (site / f'maestro-999-{SIGNATURE}_0.profraw').write_bytes(b'not a profile')
        result = check(directory, ambient_toolchain(directory))
        assert result.returncode != 0, result.stdout + result.stderr
        assert 'is cut' in result.stdout + result.stderr, result.stdout + result.stderr

def an_archived_shard_reports_from_its_archive():
    # A report merges one run's profiles, so an archived shard must hand it the archive it ran.
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        tools, log = install_recording_runner(directory)
        archive = str(directory / 'shard-1.tar.zst')
        (directory / 'test-binaries.json').write_text('{"shard-1.tar.zst": {"binaries": 1, "filter": "binary_id(=sample::integration)"}}', encoding='utf-8')
        result = execute('Isolated tests', directory, {
            'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'TOOLS': str(tools),
            'RUNNER_TEMP': temporary, 'TEST_ARCHIVE': archive, 'TEST_PACKAGES': '["sample"]',
            'SHARD': '1', 'COLLECT_COVERAGE': 'true', 'CARGO_COVERAGE_MIN_LINES': ''})
        assert result.returncode == 0, result.stdout + result.stderr
        commands = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
        report = next(command['args'] for command in commands if command['args'][:2] == ['llvm-cov', 'report'])
        assert report[report.index('--nextest-archive-file') + 1] == archive, report
        # The real tool refuses this pair, because `--no-report` already implies `--no-clean`; a stand-in never would.
        run = next(command['args'] for command in commands if command['args'][:2] == ['llvm-cov', 'nextest'])
        assert not {'--no-report', '--no-clean'} <= set(run), run

def prepared_children_become_coverage_objects():
    # The report maps every executable of the instrumented build directory, so a prepared child counts only once it sits there.
    assert OBJECTS, 'the workflow has no prepared coverage object snippet'
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        checkout(directory)
        prepared = directory / 'target/test-setup'
        (prepared / 'bin').mkdir(parents=True)
        child = prepared / 'bin/child'
        child.write_bytes(b'\x7fELF-section-names:__llvm_covfun:__llvm_covmap')
        plain = prepared / 'bin/plain'
        plain.write_bytes(b'\x7fELF-no-counters')
        notes = prepared / 'notes.txt'
        notes.write_bytes(b'mentions __llvm_covfun but is data')
        for path in (child, plain):
            path.chmod(0o755)
        result = subprocess.run(['python3', '-c', OBJECTS], cwd=directory, env=os.environ | {'TEST_PREPARE_OUTPUT': str(prepared)},
                                capture_output=True, text=True)
        assert result.returncode == 0, result.stdout + result.stderr
        objects = directory / 'target/llvm-cov-target/debug/prepared'
        copy = objects / 'bin/child'
        assert copy.read_bytes() == child.read_bytes() and os.access(copy, os.X_OK), sorted(objects.rglob('*'))
        assert sorted(path.relative_to(objects).as_posix() for path in objects.rglob('*') if path.is_file()) == ['bin/child']
        assert 'Prepared coverage objects: 1' in result.stdout, result.stdout

def a_run_without_prepared_files_hands_over_no_objects():
    assert OBJECTS, 'the workflow has no prepared coverage object snippet'
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        checkout(directory)
        result = subprocess.run(['python3', '-c', OBJECTS], cwd=directory, env=os.environ | {'TEST_PREPARE_OUTPUT': str(directory / 'target/test-setup')},
                                capture_output=True, text=True)
        assert result.returncode == 0, result.stdout + result.stderr
        assert 'Prepared coverage objects: 0' in result.stdout and not (directory / 'target/llvm-cov-target/debug/prepared').exists()

def a_damaged_profile_of_a_prepared_child_names_the_child():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        site = checkout(directory)
        (site / 'debug/prepared').mkdir()
        child = site / 'debug/prepared/child'
        child.write_text('#!/bin/bash\n'
                         f'printf PROFILE > "${{LLVM_PROFILE_FILE//%4m/{SIGNATURE}_0}}"\n', encoding='utf-8')
        child.chmod(0o755)
        (site / f'maestro-999-{SIGNATURE}_1.profraw').write_text('', encoding='utf-8')
        result = check(directory, toolchain(directory, 'PROFILE'))
        assert result.returncode != 0, result.stdout + result.stderr
        assert f'maestro-999-{SIGNATURE}_1.profraw is empty; its binary is child' in result.stdout + result.stderr, result.stdout + result.stderr

def prepared_objects_are_handed_over_between_the_run_and_the_profile_check():
    command = body('Isolated tests')
    handover = 'run_isolated python3 -c "$COVERAGE_OBJECTS"'
    assert command.count(handover) == 1
    assert command.index('llvm-cov nextest') < command.index(handover) < command.index('python3 -c "$COVERAGE_PROFILES"') < command.index('llvm-cov report')

def the_check_reads_the_profiles_before_the_report_merges_them():
    for name in ['Coverage', 'Isolated tests']:
        command = body(name)
        assert command.count('python3 -c "$COVERAGE_PROFILES"') == 1, name
        assert command.index('python3 -c "$COVERAGE_PROFILES"') < command.index('llvm-cov report'), name
        assert '--no-report' in command and 'tee -a nextest.log' in command, name
        assert '--no-clean' not in command, name
    # The extracted proof steps carry the check the way they carry the merge.
    internal = (root / '.github/workflows/ci-internal.yml').read_text(encoding='utf-8')
    with tempfile.TemporaryDirectory() as temporary:
        exported = pathlib.Path(temporary) / 'env'
        result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', body('Shared coverage policy', internal)],
                                env=os.environ | {'GITHUB_ENV': str(exported), 'RUNNER_TEMP': temporary},
                                capture_output=True, text=True)
        assert result.returncode == 0, result.stdout + result.stderr
        assert 'COVERAGE_PROFILES<<COVERAGE_POLICY\n' + CHECK + 'COVERAGE_POLICY\n' in exported.read_text(encoding='utf-8')

for case in [complete_profiles_pass, no_profiles_pass, damaged_profiles_fail_naming_each_file_and_its_program,
             the_check_reads_profiles_with_the_tool_the_report_merges_them, an_archived_shard_reports_from_its_archive,
             the_check_reads_the_profiles_before_the_report_merges_them, prepared_children_become_coverage_objects,
             a_run_without_prepared_files_hands_over_no_objects, a_damaged_profile_of_a_prepared_child_names_the_child,
             prepared_objects_are_handed_over_between_the_run_and_the_profile_check]:
    case()
    print('PASS ' + case.__name__, flush=True)
