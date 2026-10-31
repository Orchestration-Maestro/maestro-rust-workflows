import os
import pathlib
import re
import subprocess
import tempfile
import textwrap

root = pathlib.Path.cwd()
content = (root / '.github/workflows/content.yml').read_text(encoding='utf-8')

def content_body(name):
    step = next(s for s in content.split('      - name: ')[1:] if s.splitlines()[0] == name)
    block = re.search(r'^        run: \|\n((?:          .*\n|\n)+)', step, re.MULTILINE)
    return textwrap.dedent(block[1]) if block else re.search(r'^        run: (.+)$', step, re.MULTILINE)[1]

def content_run(name, directory, env=None):
    return subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', content_body(name)], cwd=directory,
                          env=os.environ | (env or {}), capture_output=True, text=True)

def content_workflow_delegates_to_caller():
    assert all(re.search(r'^  ' + event + ':', content, re.MULTILINE)
               for event in ['pull_request', 'merge_group', 'workflow_call'])
    assert re.findall(r'^  ([\w-]+):', content.split('\njobs:\n', 1)[1], re.MULTILINE) == ['content']
    assert '    name: Required content checks\n' in content
    assert 'permissions:\n  contents: read\n' in content and 'persist-credentials: false' in content
    assert re.findall(r'^    if: (.+)$', content, re.MULTILINE) == [
        "$" + "{{ github.repository_id != '1404598722' }}"]
    assert not re.search(r'continue-on-error:', content)
    for action in re.findall(r'uses: (\S+)', content):
        assert re.fullmatch(r'[^@]+@[0-9a-f]{40}', action), action
    assert content_body('Caller tools').strip() == 'mise install'
    assert content_body('Caller check').strip() == 'mise exec -- just check'
    assert not any(name in content for name in ['typos', 'editorconfig-checker', 'rumdl', 'PyYAML'])
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        paths = directory / 'paths'
        env = {'RUNNER_TEMP': temporary, 'GITHUB_PATH': str(paths)}
        result = content_run('Mise bootstrap', directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        installed = pathlib.Path(paths.read_text(encoding='utf-8').strip()) / 'mise'
        assert subprocess.check_output([str(installed), '--version'], text=True).startswith('2026.10.2 ')
        fake = directory / 'mise'
        fake.write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> "$CALLS"\nexit "${STATUS:-0}"\n', encoding='utf-8')
        fake.chmod(0o755)
        calls = directory / 'calls'
        selected = {'PATH': str(directory) + os.pathsep + os.environ['PATH'], 'CALLS': str(calls)}
        for name, command in [('Caller tools', 'install'), ('Caller check', 'exec -- just check')]:
            for status in ['0', '73']:
                result = content_run(name, directory, selected | {'STATUS': status})
                assert result.returncode == int(status), result.stdout + result.stderr
                assert calls.read_text(encoding='utf-8').splitlines()[-1] == command
        # A corrupted bootstrap download must not become executable or enter PATH.
        curl = directory / 'curl'
        curl.write_text('#!/bin/sh\nwhile [ "$1" != --output ]; do shift; done\nprintf corrupt > "$2"\n', encoding='utf-8')
        curl.chmod(0o755)
        before = paths.read_text(encoding='utf-8')
        result = content_run('Mise bootstrap', directory, env | selected)
        assert result.returncode != 0 and paths.read_text(encoding='utf-8') == before, result.stdout + result.stderr

content_workflow_delegates_to_caller()
print('PASS content_workflow_delegates_to_caller', flush=True)

def content_requires_caller_files_and_pins():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        result = content_run('Caller check files', directory)
        assert result.returncode != 0 and 'justfile' in result.stderr, result.stdout + result.stderr
        justfile = directory / 'justfile'
        justfile.touch()
        result = content_run('Caller check files', directory)
        assert result.returncode != 0 and 'mise.toml' in result.stderr, result.stdout + result.stderr
        config = directory / 'mise.toml'
        for text in ['[broken', '[tools]\n', '[tools]\njust="latest"\nprek="0.5.4"\n',
                     '[tools]\njust="1.58.0"\nprek="0.5"\n', '[tools]\njust=3\nprek="0.5.4"\n']:
            config.write_text(text, encoding='utf-8')
            result = content_run('Caller check files', directory)
            assert result.returncode != 0 and 'mise.toml' in result.stderr, result.stdout + result.stderr
        config.write_text('[tools]\njust="1.58.0"\nprek="0.5.4"\n', encoding='utf-8')
        justfile.unlink()
        for name in ['justfile', '.justfile', 'Justfile']:
            path = directory / name
            path.touch()
            result = content_run('Caller check files', directory)
            assert result.returncode == 0, result.stdout + result.stderr
            path.unlink()
        names = re.findall(r'^      - name: (.+)$', content, re.MULTILINE)
        assert names == ['Checkout', 'Caller check files', 'Mise bootstrap', 'Caller tools', 'Caller check']

content_requires_caller_files_and_pins()
print('PASS content_requires_caller_files_and_pins', flush=True)
