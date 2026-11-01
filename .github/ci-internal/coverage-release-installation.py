import pathlib
import re
import subprocess
import textwrap
workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
step = next(s for s in workflow.split('      - name: ')[1:] if s.splitlines()[0] == 'Coverage tools')
match = re.search(r'^        run: (?:&[\w-]+ )?\|\n((?:          .*\n|\n)+)', step, re.MULTILINE)
command = textwrap.dedent(match[1])
def coverage_tools_use_host_archives():
    rows = re.findall(r'^https://github.com/\S+ \w{64} (cargo-llvm-cov|cargo-nextest(?:\.exe)?)$', command, re.MULTILINE)
    assert rows == ['cargo-llvm-cov', 'cargo-nextest', 'cargo-nextest', 'cargo-nextest.exe'], rows
    for host in ['Linux:x86_64', 'Darwin:arm64', 'MINGW*:x86_64']:
        assert host in command
    assert 'hashlib.sha256(data).hexdigest() != digest' in command

coverage_tools_use_host_archives()
print('PASS coverage_tools_use_host_archives')
subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', textwrap.dedent(match[1])], check=True)
