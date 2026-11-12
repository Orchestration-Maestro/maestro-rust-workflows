"""The step runner of caller-checks-at-workflow-seam.py, loaded for the seam scripts that run a workflow step.

That script runs every one of its cases when it runs, so a script that only needs its runner takes the imports, the
functions and the module-level values those functions read, and nothing else. The values are listed here once; a script
never carries its own list.
"""
import ast
import pathlib
import textwrap

SEAM = '.github/ci-internal/caller-checks-at-workflow-seam.py'
# The module-level values the runner's functions read.
VALUES = {'workflow', 'root', 'default_ref', 'credential_names', 'coverage_report_args'}


def load(name):
    """The namespace of the seam's imports, functions and runner values, compiled under `name`."""
    tree = ast.parse(textwrap.dedent(pathlib.Path(SEAM).read_text(encoding='utf-8')))
    tree.body = [node for node in tree.body if isinstance(node, (ast.Import, ast.ImportFrom, ast.FunctionDef))
                 or (isinstance(node, ast.Assign) and any(isinstance(target, ast.Name) and target.id in VALUES
                                                         for target in node.targets))]
    namespace = {'__name__': name}
    exec(compile(tree, name, 'exec'), namespace)  # noqa: S102
    return namespace
