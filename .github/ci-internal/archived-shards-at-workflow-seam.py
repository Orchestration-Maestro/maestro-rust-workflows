import ast
import pathlib
import textwrap
code = pathlib.Path('.github/ci-internal/caller-checks-at-workflow-seam.py').read_text(encoding='utf-8')
tree = ast.parse(textwrap.dedent(code))
globals_needed = {'workflow', 'root', 'credential_names', 'coverage_report_args'}
tree.body = [node for node in tree.body if isinstance(node, (ast.Import, ast.ImportFrom, ast.FunctionDef))
             or (isinstance(node, ast.Assign) and any(isinstance(target, ast.Name) and target.id in globals_needed
                                                     for target in node.targets))]
exec(compile(tree, 'archive workflow seam', 'exec'))
archived_shards_reuse_build_and_prepared_outputs()
print('PASS archived_shards_reuse_build_and_prepared_outputs', flush=True)
