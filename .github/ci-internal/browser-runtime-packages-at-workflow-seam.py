import ast
import json
import os
import pathlib
import re
import subprocess
import tempfile

workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
job = document['jobs']['browser-tests']
steps = {step['name']: step for step in job['steps']}
IDENTITY = 'Browser runtime packages identity'
RESTORE = 'Restore browser runtime packages'
PACKAGES = 'Browser runtime packages'
SAVE = 'Save browser runtime packages'

def execute(name, cwd, extra):
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(('GITHUB_', 'FAKE_', 'CACHE_', 'PLAYWRIGHT_', 'RUNNER_TEMP', 'APT_'))} | extra
    return subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', steps[name]['run']],
                          cwd=cwd, env=env, text=True, capture_output=True)

DRY_RUN = '''Installing dependencies...
Missing system dependencies (2):
  fonts-liberation
  libasound2t64
'''

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
archives = pathlib.Path(os.environ['FAKE_ARCHIVES'])
expected = os.environ.get('FAKE_EXPECT_ARCHIVE', '')
assert not expected or (archives / expected).is_file(), expected + ' was not installed from the restored bundle'
with trace.open('a', encoding='utf-8') as log:
    log.write(' '.join(sys.argv[2:]) + '\\n')
if os.environ.get('FAKE_NO_ARCHIVES') != 'true':
    for package in ['fonts-liberation_1%3a2.1.5-3ubuntu0.1_all.deb', 'libasound2t64_1.2.11-1build2_amd64.deb']:
        (archives / package).write_bytes(package.encode('utf-8'))
'''

SUDO = '''#!/usr/bin/env python3
# Root is only needed for the apt configuration and the archive copy; the sandbox records the first and
# runs the second where the test keeps it.
import os
import shutil
import subprocess
import sys

with open(os.environ['FAKE_TRACE'], 'a', encoding='utf-8') as log:
    log.write(sys.argv[1] + '\\n')
if sys.argv[1] == 'install':
    shutil.copyfile(sys.argv[4], os.environ['FAKE_APT_SETTINGS'])
else:
    subprocess.run(sys.argv[1:], check=True)
'''

ARCHIVES = ['fonts-liberation_1%3a2.1.5-3ubuntu0.1_all.deb', 'libasound2t64_1.2.11-1build2_amd64.deb']


def sandbox(directory, **extra):
    bin_dir = directory / 'bin'
    bin_dir.mkdir()
    (bin_dir / 'node').write_text(DRIVER, encoding='utf-8')
    (bin_dir / 'sudo').write_text(SUDO, encoding='utf-8')
    for tool in ['node', 'sudo']:
        (bin_dir / tool).chmod(0o700)
    (directory / 'outputs').touch()
    return {
        'PATH': str(bin_dir) + os.pathsep + os.environ['PATH'],
        'RUNNER_TEMP': str(directory),
        'PLAYWRIGHT_MODULE': str(directory / 'playwright'),
        'GITHUB_OUTPUT': str(directory / 'outputs'),
        'APT_ARCHIVES': str(directory / 'apt-archives'),
        'FAKE_ARCHIVES': str(directory / 'apt-archives'),
        'FAKE_TRACE': str(directory / 'trace'),
        'FAKE_APT_SETTINGS': str(directory / 'apt-settings'),
        'FAKE_EXPECT_ARCHIVE': '',
        'FAKE_NO_ARCHIVES': '',
    } | extra


def installed(**extra):
    directory = pathlib.Path(tempfile.mkdtemp())
    (directory / 'apt-archives').mkdir()
    bundle = directory / 'browser-runtime-packages'
    result = execute(PACKAGES, directory, sandbox(directory, **extra))
    outputs = directory / 'outputs'
    return (result, dict(line.split('=', 1) for line in outputs.read_text(encoding='utf-8').splitlines()),
            (directory / 'trace').read_text(encoding='utf-8') if (directory / 'trace').exists() else '',
            bundle, directory)


def install_deps_runs_only_after_apt_reads_its_settings():
    result, outputs, trace, bundle, _ = installed()
    assert result.returncode == 0, result.stdout + result.stderr
    assert trace == 'install\ninstall-deps chromium\n', trace
    assert sorted(path.name for path in bundle.glob('*.deb')) == ARCHIVES, list(bundle.iterdir())
    assert re.fullmatch(r'[0-9a-f]{64}', outputs.get('digest', '')), outputs

def the_step_fails_when_the_apt_settings_go_missing_or_late():
    body = steps[PACKAGES]['run']
    writing = next(line for line in body.splitlines() if 'apt.conf.d' in line)
    installing = '          node "$PLAYWRIGHT_MODULE/cli.js" install-deps chromium\n'
    for mutation in [body.replace(writing + '\n', ''),
                     body.replace(writing + '\n', '').replace(installing, installing + writing + '\n')]:
        assert mutation != body
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            (directory / 'apt-archives').mkdir()
            result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', mutation], cwd=directory,
                                    env=sandbox(directory), text=True, capture_output=True)
            assert result.returncode != 0, result.stdout + result.stderr
            assert 'the apt settings were written' in result.stderr, result.stderr

def a_restored_bundle_is_installed_before_any_download():
    directory = pathlib.Path(tempfile.mkdtemp())
    (directory / 'apt-archives').mkdir()
    bundle = directory / 'browser-runtime-packages'
    bundle.mkdir()
    (bundle / ARCHIVES[0]).write_bytes(b'restored')
    result = execute(PACKAGES, directory, sandbox(directory, CACHE_MATCHED_KEY='maestro-browser-runtime-v1-0',
                                                 FAKE_EXPECT_ARCHIVE=ARCHIVES[0]))
    assert result.returncode == 0, result.stdout + result.stderr
    trace = (directory / 'trace').read_text(encoding='utf-8')
    # The restored archives reach apt before it looks for them, and the step says where they came from.
    assert trace == 'install\ncp\ninstall-deps chromium\n', trace
    assert 'from the restored bundle' in result.stdout, result.stdout

def a_missing_bundle_leaves_the_download_to_apt():
    result, outputs, trace, bundle, _ = installed()
    assert result.returncode == 0 and trace == 'install\ninstall-deps chromium\n', (trace, result.stdout + result.stderr)
    assert 'from the restored bundle' not in result.stdout and outputs.get('digest'), result.stdout

def a_bundle_without_archives_is_never_saved():
    result, outputs, trace, bundle, _ = installed(FAKE_NO_ARCHIVES='true')
    assert result.returncode == 0, result.stdout + result.stderr
    assert 'digest' not in outputs and '::warning::' in result.stdout, (outputs, result.stdout)

def the_bundle_digest_names_its_contents():
    digests = []
    for content in [b'first', b'first', b'second']:
        directory = pathlib.Path(tempfile.mkdtemp())
        archives = directory / 'apt-archives'
        archives.mkdir()
        (archives / ARCHIVES[0]).write_bytes(content)
        result = execute(PACKAGES, directory, sandbox(directory, FAKE_NO_ARCHIVES='true'))
        assert result.returncode == 0, result.stdout + result.stderr
        outputs = (directory / 'outputs').read_text(encoding='utf-8')
        digests.append(dict(line.split('=', 1) for line in outputs.splitlines())['digest'])
    assert digests[0] == digests[1] != digests[2], digests

def only_a_default_branch_writer_saves_a_changed_bundle():
    save, restore = steps[SAVE], steps[RESTORE]
    bundle = '${{ runner.temp }}/browser-runtime-packages'
    assert save['continue-on-error'] is True and restore['continue-on-error'] is True
    assert save['with']['path'] == restore['with']['path'] == bundle
    assert save['with']['key'] == ('${{ steps.runtime-packages.outputs.prefix }}'
                                   '${{ steps.runtime-apt.outputs.digest }}'), save['with']['key']
    for required in ["steps.cache-identity.outputs.writer == 'true'",
                     "steps.runtime-apt.outputs.digest != ''",
                     '!endsWith(steps.runtime-cache.outputs.cache-matched-key, steps.runtime-apt.outputs.digest)']:
        assert required in save['if'], required
    assert restore['with']['key'] == '${{ steps.runtime-packages.outputs.key }}'
    assert restore['with']['restore-keys'] == '${{ steps.runtime-packages.outputs.prefix }}'
    # One shape, one writer: the installer sits between the restore and the save, and one step owns the bound.
    order = [step['name'] for step in job['steps']]
    assert order.index('Browser runtime tools') < order.index(IDENTITY) < order.index(RESTORE) < order.index(PACKAGES) < order.index(SAVE) < order.index('Browser test recipe'), order
    assert steps[PACKAGES]['timeout-minutes'] == 5, steps[PACKAGES].get('timeout-minutes')
    assert steps[RESTORE]['if'] == "${{ steps.runtime-packages.outputs.prefix != '' }}", steps[RESTORE].get('if')

def the_identity_keys_the_bundle_by_the_packages_playwright_lists():
    def identity(dry_run=DRY_RUN):
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            bin_dir = directory / 'bin'
            bin_dir.mkdir()
            (bin_dir / 'node').write_text('#!/usr/bin/env python3\nimport os, sys\n'
                                          'assert sys.argv[1].startswith(os.environ["PLAYWRIGHT_MODULE"]), sys.argv[1]\n'
                                          f'print({dry_run!r})\n',
                                          encoding='utf-8')
            (bin_dir / 'node').chmod(0o700)
            result = execute(IDENTITY, directory, {'PATH': str(bin_dir) + os.pathsep + os.environ['PATH'],
                                                   'PLAYWRIGHT_MODULE': str(directory / 'playwright'),
                                                   'GITHUB_OUTPUT': str(directory / 'outputs')})
            assert result.returncode == 0, result.stdout + result.stderr
            written = (directory / 'outputs').read_text(encoding='utf-8') if (directory / 'outputs').exists() else ''
            return dict(line.split('=', 1) for line in written.splitlines())
    listed = identity()
    prefix = listed['prefix']
    assert re.fullmatch(r'maestro-browser-runtime-v1-[0-9a-f]{64}-', prefix), prefix
    assert listed['key'] == prefix + 'lookup'
    assert identity() == listed, 'one package list must keep one key'
    assert identity(DRY_RUN.replace('libasound2t64', 'libnss3'))['prefix'] != prefix, 'a changed package list must change the key'
    assert identity('All system dependencies are installed.\n').get('prefix', '') == '', 'nothing missing means no bundle'

def the_pruner_keeps_one_bundle_for_the_browser_packages():
    prune = next(step for step in document['jobs']['quality-prune']['steps'] if step['name'] == 'Prune superseded caches')['run']
    literal = re.search(r"prefix = re\.compile\((r'.*?')\),?\n", prune)
    assert literal, prune
    pattern = re.compile(ast.literal_eval(literal[1]))
    assert 'maestro-browser-runtime-v1' in pattern.pattern, pattern.pattern
    assert pattern.match('maestro-browser-runtime-v1-' + 'a' * 64 + '-' + 'b' * 64)
    assert not pattern.match('maestro-browser-runtime-v1-' + 'a' * 64)

for check in [install_deps_runs_only_after_apt_reads_its_settings, the_step_fails_when_the_apt_settings_go_missing_or_late,
              a_restored_bundle_is_installed_before_any_download, a_missing_bundle_leaves_the_download_to_apt,
              a_bundle_without_archives_is_never_saved, the_bundle_digest_names_its_contents,
              only_a_default_branch_writer_saves_a_changed_bundle,
              the_identity_keys_the_bundle_by_the_packages_playwright_lists,
              the_pruner_keeps_one_bundle_for_the_browser_packages]:
    check()
    print('PASS ' + check.__name__, flush=True)
