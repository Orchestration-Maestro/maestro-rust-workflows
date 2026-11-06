import json
import os
import pathlib
import subprocess
import tempfile

workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
job = document['jobs']['browser-tests']
steps = {step['name']: step for step in job['steps']}
PACKAGES = 'Browser runtime packages'

def execute(name, cwd, extra):
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(('GITHUB_', 'FAKE_'))} | extra
    return subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', steps[name]['run']],
                          cwd=cwd, env=env, text=True, capture_output=True)

DRIVER = '''#!/usr/bin/env python3
# Stands in for the Playwright driver: it checks the apt settings apt reads and records the install.
import os
import pathlib
import sys

trace = pathlib.Path(os.environ['FAKE_TRACE'])
mode = sys.argv[2] if len(sys.argv) > 2 else ''
if mode == '--version':
    print('Version 1.64.1')
    sys.exit(0)
assert mode == 'install-deps', 'unexpected driver command: ' + mode
settings = pathlib.Path(os.environ['FAKE_APT_SETTINGS'])
assert settings.is_file(), 'install-deps ran before the apt settings were written'
text = settings.read_text(encoding='utf-8')
for setting in ['Acquire::http::Timeout "15";', 'Acquire::https::Timeout "15";', 'Acquire::Retries "3";']:
    assert setting in text, (setting, text)
with trace.open('a', encoding='utf-8') as log:
    log.write(' '.join(sys.argv[2:]) + '\\n')
'''

SUDO = '''#!/usr/bin/env python3
# The sandbox keeps the drop-in where the test can read it instead of writing it as root.
import os
import shutil
import sys

with open(os.environ['FAKE_TRACE'], 'a', encoding='utf-8') as log:
    log.write(sys.argv[1] + '\\n')
shutil.copyfile(sys.argv[4], os.environ['FAKE_APT_SETTINGS'])
'''


def sandbox(directory):
    bin_dir = directory / 'bin'
    bin_dir.mkdir()
    for tool, source in [('node', DRIVER), ('sudo', SUDO)]:
        (bin_dir / tool).write_text(source, encoding='utf-8')
        (bin_dir / tool).chmod(0o700)
    return {
        'PATH': str(bin_dir) + os.pathsep + os.environ['PATH'],
        'PLAYWRIGHT_MODULE': str(directory / 'playwright'),
        'FAKE_TRACE': str(directory / 'trace'),
        'FAKE_APT_SETTINGS': str(directory / 'apt-settings'),
    }


def installed():
    directory = pathlib.Path(tempfile.mkdtemp())
    try:
        result = execute(PACKAGES, directory, sandbox(directory))
        trace = directory / 'trace'
        return result, trace.read_text(encoding='utf-8') if trace.exists() else ''
    finally:
        subprocess.run(['rm', '-rf', str(directory)], check=True)

def install_deps_runs_only_after_apt_reads_its_settings():
    result, trace = installed()
    # The driver fails unless every setting is in place, so the order proves the drop-in reached apt first.
    assert result.returncode == 0, result.stdout + result.stderr
    assert trace == 'install\ninstall-deps chromium\n', trace

def the_step_fails_when_the_apt_settings_go_missing_or_late():
    body = steps[PACKAGES]['run']
    writing = next(line for line in body.splitlines() if 'apt.conf.d' in line)
    # yq returns the run body dedented, so the install line is found the same way as the writing line.
    installing = next(line for line in body.splitlines() if 'install-deps chromium' in line)
    dropped = body.replace(writing + '\n', '')
    late = dropped.replace(installing + '\n', installing + '\n' + writing + '\n')
    assert late != dropped, 'the late mutation must move the settings after the install'
    assert late.index(installing) < late.index(writing), late
    for mutation in [dropped, late]:
        assert mutation != body
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', mutation], cwd=directory,
                                    env=sandbox(directory), text=True, capture_output=True)
            assert result.returncode != 0, result.stdout + result.stderr
            assert 'the apt settings were written' in result.stderr, result.stderr

def the_step_is_bounded_and_sits_after_the_tools_step():
    for bound in [step.get('timeout-minutes') for step in job['steps'] if step['name'] == PACKAGES]:
        assert bound == 5, bound
    # The packages follow the tools that name their driver, and the step owns the only bound.
    order = [step['name'] for step in job['steps']]
    assert order.index('Browser runtime tools') < order.index(PACKAGES) < order.index('Browser test recipe'), order
    assert 'if' not in steps[PACKAGES] and 'continue-on-error' not in steps[PACKAGES], steps[PACKAGES]

for check in [install_deps_runs_only_after_apt_reads_its_settings, the_step_fails_when_the_apt_settings_go_missing_or_late,
              the_step_is_bounded_and_sits_after_the_tools_step]:
    check()
    print('PASS ' + check.__name__, flush=True)
