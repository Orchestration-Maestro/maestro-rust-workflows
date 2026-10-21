import ast
import os
import pathlib
import re
import textwrap
workflow = (pathlib.Path(__file__).resolve().parent / 'workflows/ci.yml').read_text(encoding='utf-8')
directory = pathlib.Path(os.environ['RUNNER_TEMP']) / 'proof-steps'
directory.mkdir()
source = (pathlib.Path(__file__).resolve().parent / 'workflows/ci-internal.yml').read_text(encoding='utf-8')
code = source.split('      - name: Caller checks at workflow seam\n', 1)[1]
code = code.split('        run: |\n', 1)[1].split('\n  native:', 1)[0]
tree = ast.parse(textwrap.dedent(code))
tree.body = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == 'body']
exec(compile(tree, 'workflow body resolver', 'exec'))
for step in workflow.split('      - name: ')[1:]:
    name = step.splitlines()[0]
    if re.search(r'^        run:', step, re.MULTILINE):
        (directory / name).write_text(body(name), encoding='utf-8')
with open(os.environ['GITHUB_ENV'], 'a', encoding='utf-8') as output:
    output.write(f'PROOF_STEPS={directory}\n')
