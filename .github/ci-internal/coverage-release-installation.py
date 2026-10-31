import pathlib
import re
import subprocess
import textwrap
workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
step = next(s for s in workflow.split('      - name: ')[1:] if s.splitlines()[0] == 'Coverage tools')
match = re.search(r'^        run: (?:&[\w-]+ )?\|\n((?:          .*\n|\n)+)', step, re.MULTILINE)
subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', textwrap.dedent(match[1])], check=True)
