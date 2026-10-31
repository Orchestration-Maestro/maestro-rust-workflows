import pathlib
import re
import subprocess
import textwrap
workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
step = workflow.split('      - name: Search tools\n', 1)[1]
command = re.search(r'        run: (?:&[\w-]+ )?\|\n((?:          .*\n|\n)+)', step)[1]
subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', textwrap.dedent(command)], check=True)
