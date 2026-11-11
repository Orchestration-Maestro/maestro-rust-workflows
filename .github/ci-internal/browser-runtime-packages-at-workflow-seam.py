import json
import os
import pathlib
import re
import shutil
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
BOUNDED = ['-o', 'Acquire::http::Timeout=15', '-o', 'Acquire::https::Timeout=15', '-o', 'Acquire::Retries=3']

# Every stand-in that would reach the Ubuntu archive writes its command line to FAKE_ARCHIVE; the others write FAKE_TRACE.
SUDO = '''#!/usr/bin/env python3
# Stands in for sudo: it runs the sandbox's own stand-ins as root and only records any other root command.
import os
import pathlib
import sys

tool = pathlib.Path(__file__).parent / sys.argv[1]
if tool.exists():
    os.execve(tool, [str(tool)] + sys.argv[2:], os.environ | {'FAKE_ROOT': '1'})
with open(os.environ['FAKE_TRACE'], 'a', encoding='utf-8') as log:
    log.write('sudo ' + ' '.join(sys.argv[1:]) + '\\n')
'''

APT_GET = '''#!/usr/bin/env python3
# Stands in for apt-get: each call reaches the archive, so it is recorded and must bound its transfers and run as root.
import os
import pathlib
import stat
import sys

arguments = sys.argv[1:]
with open(os.environ['FAKE_ARCHIVE'], 'a', encoding='utf-8') as log:
    log.write(' '.join(arguments) + '\\n')
assert os.environ.get('FAKE_ROOT') == '1', 'apt-get ran without sudo'
bounded = ['-o', 'Acquire::http::Timeout=15', '-o', 'Acquire::https::Timeout=15', '-o', 'Acquire::Retries=3']
if arguments[:len(bounded)] != bounded:
    sys.exit('an unbounded transfer: ' + ' '.join(arguments))
options, words = {}, []
rest = iter(arguments)
for word in rest:
    if word == '-o':
        name, _, value = next(rest).partition('=')
        options[name] = value
    else:
        words.append(word)
if words == ['update']:
    sys.exit(0)
assert words[:4] == ['install', '-y', '--no-install-recommends', '--download-only'], words
archives = pathlib.Path(options['Dir::Cache::Archives'])
with open(os.environ['FAKE_DOWNLOADS'], 'a', encoding='utf-8') as log:
    log.write(str(archives) + '\\n')
# Apt downloads as its own unprivileged user, which enters the directory only when every directory on the way lets anyone in.
for directory in [archives, *archives.parents]:
    if not directory.stat().st_mode & stat.S_IXOTH:
        sys.exit(f'apt cannot reach its partial directory through {directory}')
(archives / 'partial').mkdir(mode=0o700)
(archives / 'lock').touch(mode=0o640)
for package in words[4:]:
    (archives / f'{package}_1.0_amd64.deb').write_bytes(b'archive ' + package.encode())
'''

DPKG = '''#!/usr/bin/env python3
# Stands in for dpkg: it installs only from local files and records their names.
import os
import pathlib
import sys

assert os.environ.get('FAKE_ROOT') == '1', 'dpkg ran without sudo'
assert sys.argv[1] == '--install', sys.argv
files = [pathlib.Path(name) for name in sys.argv[2:]]
assert files and all(file.is_file() for file in files), files
with open(os.environ['FAKE_TRACE'], 'a', encoding='utf-8') as log:
    log.write('dpkg --install ' + ' '.join(sorted(file.name for file in files)) + '\\n')
'''

DRIVER = '''#!/usr/bin/env python3
# Stands in for the Playwright driver: its dry run only simulates, and its real install fetches from the archive.
import os
import sys

arguments = sys.argv[2:]
if arguments == ['install-deps', '--dry-run', 'chromium']:
    with open(os.environ['FAKE_TRACE'], 'a', encoding='utf-8') as log:
        log.write(' '.join(arguments) + '\\n')
    sys.exit(0)
with open(os.environ['FAKE_ARCHIVE'], 'a', encoding='utf-8') as log:
    log.write(' '.join(arguments) + '\\n')
'''


def lines(path):
    return path.read_text(encoding='utf-8').splitlines() if path.exists() else []


def installed(directory, source=None, **changes):
    """Runs the packages step in a sandbox and returns its result, trace, archive contacts and download directories."""
    bin_dir = directory / 'bin'
    bin_dir.mkdir()
    for tool, text in [('sudo', SUDO), ('apt-get', APT_GET), ('dpkg', DPKG), ('node', DRIVER)]:
        (bin_dir / tool).write_text(text, encoding='utf-8')
        (bin_dir / tool).chmod(0o700)
    env = {key: value for key, value in os.environ.items() if not key.startswith(('GITHUB_', 'FAKE_', 'RUNNER_'))} | {
        'PATH': str(bin_dir) + os.pathsep + os.environ['PATH'],
        'RUNNER_TEMP': str(directory),
        'PLAYWRIGHT_MODULE': str(directory / 'playwright'),
        'BROWSER_PACKAGES': job.get('env', {}).get('BROWSER_PACKAGES', ''),
        'FAKE_TRACE': str(directory / 'trace'),
        'FAKE_ARCHIVE': str(directory / 'archive'),
        'FAKE_DOWNLOADS': str(directory / 'downloads'),
    } | changes
    # The step stages downloads outside the sandbox, where apt's own user can reach them, so the sandbox removes what it adds there.
    template = re.search(r'mktemp -d (\S+)\.XXXXXX', steps[PACKAGES]['run'])
    assert template, 'the packages step stages its downloads in a directory of its own'
    stage = pathlib.Path(template[1])
    staged = set(stage.parent.glob(stage.name + '.*'))
    try:
        result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', source or steps[PACKAGES]['run']],
                                cwd=directory, env=env, text=True, capture_output=True)
    finally:
        for download in set(stage.parent.glob(stage.name + '.*')) - staged:
            shutil.rmtree(download)
    return result, lines(directory / 'trace'), lines(directory / 'archive'), lines(directory / 'downloads')


def a_cache_hit_installs_the_packages_from_local_files_without_contacting_the_archive():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        cached = directory / 'browser-packages'
        cached.mkdir()
        for name in ['libnss3_2_amd64.deb', 'xvfb_21_amd64.deb']:
            (cached / name).write_bytes(b'restored')
        result, trace, archive, _ = installed(directory, PACKAGES_HIT='true')
        assert result.returncode == 0, result.stdout + result.stderr
        # No update, no download and no install that fetches: the restored files are the whole install.
        assert archive == [], archive
        assert trace == ['dpkg --install libnss3_2_amd64.deb xvfb_21_amd64.deb', 'install-deps --dry-run chromium'], trace


def a_cache_miss_downloads_the_listed_packages_and_keeps_only_their_files_for_the_cache():
    packages = job['env']['BROWSER_PACKAGES'].split()
    assert packages[0] == 'xvfb' and 'libnss3' in packages and len(packages) == len(set(packages)), packages
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        result, trace, archive, downloads = installed(directory, PACKAGES_HIT='')
        assert result.returncode == 0, result.stdout + result.stderr
        assert len(downloads) == 1, downloads
        assert archive == [' '.join(BOUNDED + ['update']),
                           ' '.join(BOUNDED + ['-o', f'Dir::Cache::Archives={downloads[0]}/', 'install', '-y',
                                               '--no-install-recommends', '--download-only'] + packages)], archive
        # The cache entry holds the package files alone: apt's lock and partial directory belong to root and its own user.
        files = sorted(f'{package}_1.0_amd64.deb' for package in packages)
        assert sorted(path.name for path in (directory / 'browser-packages').iterdir()) == files
        assert trace == ['dpkg --install ' + ' '.join(files), 'install-deps --dry-run chromium'], trace


def a_cache_miss_with_every_package_already_installed_downloads_nothing_and_still_checks():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        result, trace, archive, _ = installed(directory, BROWSER_PACKAGES='')
        assert result.returncode == 0, result.stdout + result.stderr
        assert len(archive) == 2 and list((directory / 'browser-packages').iterdir()) == [], archive
        assert trace == ['install-deps --dry-run chromium'], trace


def the_step_fails_when_a_download_is_unbounded_or_out_of_reach_of_apt():
    body = steps[PACKAGES]['run']
    unbounded = body.replace('"${bounded[@]}" ', '')
    unreachable = re.sub(r'^ *chmod 0755 "\$download"\n', '', body, flags=re.MULTILINE)
    for mutation, message in [(unbounded, 'an unbounded transfer'), (unreachable, 'apt cannot reach its partial directory')]:
        assert mutation != body, message
        with tempfile.TemporaryDirectory() as temporary:
            result, _, _, _ = installed(pathlib.Path(temporary), mutation)
            assert result.returncode != 0 and message in result.stderr, result.stdout + result.stderr


def identity(**changes):
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        (directory / 'package.json').write_text(json.dumps({'version': changes.pop('tool', '1.64.1')}), encoding='utf-8')
        env = {key: value for key, value in os.environ.items() if not key.startswith(('GITHUB_', 'Image', 'RUNNER_'))} | {
            'PLAYWRIGHT_MODULE': str(directory), 'ImageOS': 'ubuntu24', 'ImageVersion': '20251102.1.0', 'RUNNER_ARCH': 'X64',
            'BROWSER_PACKAGES': job['env']['BROWSER_PACKAGES'], 'GITHUB_EVENT_NAME': 'push', 'GITHUB_REF': 'refs/heads/main',
            'CACHE_DEFAULT_BRANCH': 'main', 'GITHUB_OUTPUT': str(directory / 'output')} | changes
        result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', steps[IDENTITY]['run']],
                                cwd=directory, env=env, text=True, capture_output=True)
        assert result.returncode == 0, result.stdout + result.stderr
        return dict(line.split('=', 1) for line in lines(directory / 'output'))


def the_cache_key_names_the_runner_image_the_browser_tool_and_the_package_list():
    base = identity()
    # The pruner that follows the job keeps the newest entry of each prefix, so a new tool or list replaces the old entry.
    prefix = re.fullmatch(r'(maestro-browser-packages-v1-[0-9a-f]{64}-)[0-9a-f]{64}', base['key'])
    assert prefix, base
    for change in [{'tool': '1.65.0'}, {'BROWSER_PACKAGES': job['env']['BROWSER_PACKAGES'] + ' libgtk-3-0t64'}]:
        key = identity(**change)['key']
        assert key != base['key'] and key.startswith(prefix[1]), change
    # A staged rollout serves two images at once, and pruning one image's entry for the other's would send its runners back to the archive.
    for change in [{'ImageVersion': '20251109.1.0'}, {'ImageOS': 'ubuntu26'}, {'RUNNER_ARCH': 'ARM64'}]:
        assert not identity(**change)['key'].startswith(prefix[1]), change
    assert identity(GITHUB_EVENT_NAME='merge_group')['key'] == base['key'], 'a merge group restores what the default branch saved'


def only_a_default_branch_push_saves_the_packages():
    assert identity()['writer'] == 'true'
    for event, ref in [('merge_group', 'refs/heads/gh-readonly-queue/main/pr-1-abc'), ('pull_request', 'refs/pull/1/merge'),
                       ('push', 'refs/heads/topic'), ('workflow_dispatch', 'refs/heads/main')]:
        assert identity(GITHUB_EVENT_NAME=event, GITHUB_REF=ref)['writer'] == 'false', (event, ref)


def the_packages_restore_before_the_bounded_install_and_save_after_it():
    order = [step['name'] for step in job['steps']]
    assert (order.index('Browser runtime tools') < order.index(IDENTITY) < order.index(RESTORE) < order.index(PACKAGES)
            < order.index(SAVE) < order.index('Browser test recipe')), order
    packages = steps[PACKAGES]
    assert packages.get('timeout-minutes') == 5 and 'if' not in packages and 'continue-on-error' not in packages, packages
    assert packages['env'] == {'PACKAGES_HIT': '${{ steps.packages-restore.outputs.cache-hit }}'}, packages
    restore, save = steps[RESTORE], steps[SAVE]
    assert restore['id'] == 'packages-restore' and steps[IDENTITY]['id'] == 'packages-identity'
    assert restore['uses'].startswith('actions/cache/restore@') and save['uses'].startswith('actions/cache/save@')
    for step in (restore, save):
        assert step['with']['path'] == '${{ runner.temp }}/browser-packages', step
        assert step['with']['key'] == '${{ steps.packages-identity.outputs.key }}', step
        # A cache outage falls back to the download instead of failing the job.
        assert step['continue-on-error'] is True, step
    # Only the exact image, tool and list is a hit: another image's files could replace newer packages with older ones.
    assert 'restore-keys' not in restore['with'], restore
    assert save['if'] == "steps.packages-identity.outputs.writer == 'true' && steps.packages-restore.outputs.cache-hit != 'true'", save


CHECKS = [a_cache_hit_installs_the_packages_from_local_files_without_contacting_the_archive,
          a_cache_miss_downloads_the_listed_packages_and_keeps_only_their_files_for_the_cache,
          a_cache_miss_with_every_package_already_installed_downloads_nothing_and_still_checks,
          the_step_fails_when_a_download_is_unbounded_or_out_of_reach_of_apt,
          the_cache_key_names_the_runner_image_the_browser_tool_and_the_package_list,
          only_a_default_branch_push_saves_the_packages,
          the_packages_restore_before_the_bounded_install_and_save_after_it]

if __name__ == '__main__':
    for check in CHECKS:
        check()
        print('PASS ' + check.__name__, flush=True)
