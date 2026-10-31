import ast
import json
import os
import pathlib
import re
import shlex
import subprocess
import tempfile
import textwrap

workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding="utf-8")
root = pathlib.Path.cwd()
default_ref = "format('refs/heads/{0}', github.event.repository.default_branch)"

def evaluate(condition, values):
    expression = condition.removeprefix('$' + '{{').removesuffix('}}').strip()
    for key, value in values.items():
        expression = expression.replace(key, repr(value))
    expression = expression.replace('&&', ' and ').replace('||', ' or ')
    expression = re.sub(r'!(?!=)', ' not ', expression).strip()
    return eval(expression, {'__builtins__': {}})

def body(name, source=workflow):
    import json
    import re
    import textwrap
    if source.lstrip().startswith('{'):
        document = json.loads(source)
        jobs = document['jobs'].values() if 'jobs' in document else [document]
        for job in jobs:
            for step in job.get('steps', []):
                if step.get('name') == name and 'run' in step:
                    assert isinstance(step['run'], str) and step['run'].strip(), f'Missing executable body: {name}'
                    return step['run']
    else:
        # Authored workflow bodies use named steps and literal block scalars.
        layout = re.search(r'^( *)steps:\n', source, re.MULTILINE)
        assert layout, f'Missing executable body: {name}'
        indent = layout[1] + '  '
        steps = re.split(r'^' + indent + r'- name: ', source, flags=re.MULTILINE)[1:]
        for step in steps:
            if step.splitlines()[0] != name:
                continue
            run = re.search(r'^' + indent + r'  run: (.+)$', step, re.MULTILINE)
            if not run:
                continue
            value = run[1]
            if value.startswith('*'):
                anchor = re.escape(value[1:])
                step = next(s for s in steps if re.search(
                    r'^' + indent + r'  run: &' + anchor + r' \|$', s, re.MULTILINE))
                value = '|'
            if value == '|' or re.fullmatch(r'&[\w-]+ \|', value):
                match = re.search(r'^' + indent + r'  run: (?:&[\w-]+ )?\|\n((?:' +
                                  indent + r'    .*\n|\n)+)', step, re.MULTILINE)
                assert match, f'Missing executable body: {name}'
                return textwrap.dedent(match[1])
            if value.startswith('"'):
                value = json.loads(value)
            elif value.startswith("'"):
                value = value[1:-1].replace("''", "'")
            assert value.strip() and not value.startswith(('[', '{')), f'Missing executable body: {name}'
            return value
    raise AssertionError(f'Missing executable body: {name}')

def workflow_body_resolves_parsed_steps():
    source = '''jobs:
  fixture:
    env:
      name: wanted
      run: unrelated
    steps:
      - name: wanted
        uses: example/action
      - name: block
        run: &command |
          printf 'block\\n'
      - name: wanted
        run: "printf inline"
      - name: wanted
        run: wrong
      - name: alias
        run: *command
'''
    assert body('wanted', source) == "printf inline"
    assert body('block', source) == body('alias', source) == "printf 'block\\n'\n"
    for invalid in ['missing', 'empty', 'invalid']:
        sample = source + '      - {name: empty, run: ""}\n      - {name: invalid, run: [wrong]}\n'
        try:
            body(invalid, sample)
        except AssertionError as error:
            assert str(error) == f'Missing executable body: {invalid}'
        else:
            raise AssertionError(f'Accepted invalid body: {invalid}')
    # The proof workflow extracts only this definition.
    namespace = {'workflow': source}
    code = (root / '.github/ci-internal/caller-checks-at-workflow-seam.py').read_text(encoding='utf-8')
    resolver = next(node for node in ast.parse(code).body
                    if isinstance(node, ast.FunctionDef) and node.name == 'body')
    definition = ast.get_source_segment(code, resolver)
    exec(definition, namespace)
    assert namespace['body']('alias') == "printf 'block\\n'\n"
    # Standalone native proofs have Python, not a provisioned YAML tool.
    import sys
    probe = 'workflow = ' + repr(source) + '\n' + definition + '\n'
    probe += "assert body('alias') == " + repr("printf 'block\\n'\n") + '\n'
    probe += "assert body('inline', '{\"steps\":[{\"name\":\"inline\",\"run\":\"ok\"}]}') == 'ok'\n"
    result = subprocess.run([sys.executable, '-I', '-S', '-c', probe],
                            env=os.environ | {'PATH': ''}, capture_output=True, text=True)
    assert result.returncode == 0, result.stdout + result.stderr

workflow_body_resolves_parsed_steps()
print('PASS workflow_body_resolves_parsed_steps')

def job_run_defaults_preserve_pipeline_failures():
    jobs = dict(re.findall(r'^  ([\w-]+):\n(.*?)(?=^  [\w-]+:\n|\Z)',
                           workflow.split('\njobs:\n', 1)[1], re.MULTILINE | re.DOTALL))
    expected = {'declaration', 'quality', 'docs', 'test-build', 'test-prepare', 'tests', 'checks', 'fuzz', 'advisories', 'targets', 'browser', 'browser-tests'}
    defaults = {name: re.search(r'^    defaults:(?: &[\w-]+)?\n(.*?)^    steps:', job, re.MULTILINE | re.DOTALL)
                for name, job in jobs.items()}
    assert {name for name, match in defaults.items() if match} == expected
    for name in expected:
        block = defaults[name][1]
        assert '        shell: bash\n' in block, name
        assert "        working-directory: $" + "{{ inputs.working-directory || '.' }}\n" in block, name
    for step in workflow.split('      - name: ')[1:]:
        if step.startswith('All test shards passed\n'):
            assert '        shell: python\n' in step

job_run_defaults_preserve_pipeline_failures()
print('PASS job_run_defaults_preserve_pipeline_failures')

def workflow_text_io_uses_utf8():
    for filename in ['ci.yml', 'ci-internal.yml']:
        source = (root / '.github/workflows' / filename).read_text(encoding='utf-8')
        for step in re.split(r'^      - name: ', source, flags=re.MULTILINE)[1:]:
            script = re.search(r'^        run: python3 (\.github/ci-internal/\S+\.py)$', step, re.MULTILINE)
            if '        shell: python\n' not in step and "<<'PYTHON'\n" not in step and not script:
                continue
            name = step.splitlines()[0]
            command = body(name, source)
            scripts = [command] if '        shell: python\n' in step else [(root / script[1]).read_text(encoding='utf-8')] if script else re.findall(
                r"python3 [^\n]*<<'PYTHON'\n(.*?)\nPYTHON", command, re.DOTALL)
            for call in (node for script in scripts for node in ast.walk(ast.parse(script))):
                if not isinstance(call, ast.Call):
                    continue
                function = call.func
                method = function.attr if isinstance(function, ast.Attribute) else getattr(function, 'id', '')
                if method not in ['open', 'read_text', 'write_text']:
                    continue
                if method == 'open':
                    mode_index = 0 if isinstance(function, ast.Attribute) else 1
                    mode = next((keyword.value for keyword in call.keywords if keyword.arg == 'mode'),
                                call.args[mode_index] if len(call.args) > mode_index else ast.Constant('r'))
                    if isinstance(mode, ast.Constant) and 'b' in mode.value:
                        continue
                assert any(keyword.arg == 'encoding' and isinstance(keyword.value, ast.Constant)
                           and keyword.value.value == 'utf-8' for keyword in call.keywords), \
                    f'{filename}: {name}:{call.lineno}: {method} requires encoding="utf-8"'

workflow_text_io_uses_utf8()
print('PASS workflow_text_io_uses_utf8')

def execute(name, cwd, extra=None):
    env = {k: v for k, v in os.environ.items()
           if k not in credential_names and not k.endswith('_API_KEY')}
    env['TEST_SHARD_COUNT'] = '16'
    env.update(extra or {})
    if name in ['Tests', 'Isolated tests']:
        env.update(toolchain_environment(cwd, env))
    env = {key: value for key, value in env.items() if value is not None}
    command = body(name)
    if name in ['Browser tools', 'Test tools']:
        command = body('Mise bootstrap') + (body('Browser tool pin') if name == 'Browser tools' else '') + command
    bash = 'bash'
    if os.name == 'nt':
        import shutil
        # Match the runner's Bash shell, not the Windows WSL launcher.
        bash = str(pathlib.Path(shutil.which('git')).parent.parent / 'bin/bash.exe')
        assert pathlib.Path(bash).is_file(), f'Git Bash not found: {bash}'
        # Git Bash prepends its own utilities during startup; restore the caller's order.
        env['MAESTRO_SEAM_PATH'] = env['PATH']
        command = 'export PATH="$(cygpath -u -p "$MAESTRO_SEAM_PATH")"\n' + command
    argv = [bash, '-e', '-o', 'pipefail', '-c', command]
    if os.environ.get('MAESTRO_LOCAL_CAPPED'):
        command = command.replace('env -i PATH=', 'env -i CARGO_BUILD_JOBS=3 PATH=')
        argv = [os.path.expanduser('~/.local/bin/capped'), 'bash', '-e', '-o', 'pipefail', '-c', command]
        env['CARGO_BUILD_JOBS'] = '3'
    return subprocess.run(argv, cwd=cwd, env=env, text=True, capture_output=True)

def select_packages(cwd, extra):
    # Merge-group selection reads a baseline manifest from RUNNER_TEMP; give it one that matches the fixture.
    if extra.get('TEST_EVENT') != 'merge_group' or 'RUNNER_TEMP' in extra:
        return execute('Select test packages', cwd, extra)
    with tempfile.TemporaryDirectory() as selection_temp:
        baseline = pathlib.Path(selection_temp) / 'coverage-baseline'
        baseline.mkdir()
        import tomllib
        package_roots = {}
        for manifest in pathlib.Path(cwd).rglob('Cargo.toml'):
            package = tomllib.loads(manifest.read_text(encoding='utf-8')).get('package', {})
            if 'name' in package:
                package_roots[package['name']] = manifest.parent.relative_to(cwd).as_posix()
        (baseline / 'manifest.json').write_text(json.dumps({'head': extra.get('BASE_REF'), 'package_roots': package_roots}), encoding='utf-8')
        return execute('Select test packages', cwd, extra | {'RUNNER_TEMP': selection_temp})

credential_names = '''ANTHROPIC_API_KEY ANTHROPIC_OAUTH_TOKEN OPENAI_API_KEY GEMINI_API_KEY
GROQ_API_KEY CEREBRAS_API_KEY XAI_API_KEY OPENROUTER_API_KEY ZAI_API_KEY
MISTRAL_API_KEY MINIMAX_API_KEY MINIMAX_CN_API_KEY KIMI_API_KEY HF_TOKEN
AI_GATEWAY_API_KEY OPENCODE_API_KEY COPILOT_GITHUB_TOKEN GH_TOKEN GITHUB_TOKEN
GOOGLE_APPLICATION_CREDENTIALS GOOGLE_CLOUD_PROJECT GCLOUD_PROJECT
GOOGLE_CLOUD_LOCATION AWS_PROFILE AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY
AWS_SESSION_TOKEN AWS_REGION AWS_DEFAULT_REGION AWS_BEARER_TOKEN_BEDROCK
AWS_CONTAINER_CREDENTIALS_RELATIVE_URI AWS_CONTAINER_CREDENTIALS_FULL_URI
AWS_WEB_IDENTITY_TOKEN_FILE BEDROCK_EXTENSIVE_MODEL_TEST FIREWORKS_API_KEY'''.split()

def link_test_toolchains(directory):
    physical = pathlib.Path(subprocess.check_output(['rustup', 'which', 'rustc'], text=True).strip()).parent.parent
    host = subprocess.check_output([str(physical / 'bin/rustc'), '-vV'], text=True).split('host: ', 1)[1].splitlines()[0]
    home = directory / 'rustup'
    (home / 'toolchains').mkdir(parents=True)
    selectors = ['stable-' + host, 'nightly-' + host]
    for selector in selectors:
        (home / 'toolchains' / selector).symlink_to(physical, target_is_directory=True)
    broken = '1.0.0-' + host
    (home / 'toolchains' / broken / 'lib').mkdir(parents=True)
    (home / 'settings.toml').write_text(
        f'version = "12"\ndefault_toolchain = "{broken}"\nprofile = "minimal"\n', encoding='utf-8')
    (directory / 'rust-toolchain.toml').write_text(
        f'[toolchain]\nchannel="{selectors[0]}"\n', encoding='utf-8')
    return {'RUSTUP_HOME': str(home), 'RUSTUP_TOOLCHAIN': None}, selectors

def toolchain_environment(directory, extra):
    with tempfile.TemporaryDirectory() as temporary:
        output = pathlib.Path(temporary) / 'env'
        result = execute('Test toolchain', directory, extra | {'GITHUB_ENV': str(output)})
        assert result.returncode == 0, result.stdout + result.stderr
        text = output.read_text(encoding='utf-8')
        values = dict(line.split('=', 1) for line in text.splitlines())
        assert text == 'RUSTUP_TOOLCHAIN=' + values['RUSTUP_TOOLCHAIN'] + '\n'
        return values

def toolchain_setup_exports_active_selector():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        env, selectors = link_test_toolchains(directory)
        for inherited, expected in [(None, selectors[0]), (selectors[0], selectors[0]), (selectors[1], selectors[1])]:
            extra = env | {'RUSTUP_TOOLCHAIN': inherited}
            observed = toolchain_environment(directory, extra)
            assert observed == {'RUSTUP_TOOLCHAIN': expected}, inherited
        proof = (root / '.github/workflows/ci-proof.yml').read_text(encoding='utf-8')
        assert 'run: bash -e -o pipefail "$PROOF_STEPS/Test toolchain"' in proof

toolchain_setup_exports_active_selector()
print('PASS toolchain_setup_exports_active_selector')

def child_cargo_keeps_selected_toolchain_offline():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary) / 'caller'
        directory.mkdir()
        env, _ = link_test_toolchains(directory)
        child = directory / 'child'
        external = directory.parent / 'external'
        for crate, name in [(directory, 'launcher-parent'), (child, 'launcher-child'), (external, 'launcher-external')]:
            (crate / 'src').mkdir(parents=True)
            manifest = f'[package]\nname="{name}"\nversion="0.1.0"\nedition="2021"\n[workspace]\n'
            if crate == child:
                manifest += '[dependencies]\nlauncher-external={path="../../external"}\n'
            (crate / 'Cargo.toml').write_text(manifest, encoding='utf-8')
        child_command = textwrap.dedent('''
                let output = std::process::Command::new(env!("CARGO"))
                    .args(["check", "--offline", "--locked", "--manifest-path", "child/Cargo.toml",
                           "--target-dir", "child-output"])
                    .output().unwrap();
                assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        ''')
        (child / 'src/lib.rs').write_text('pub fn child() { launcher_external::external(); }\n', encoding='utf-8')
        (external / 'src/lib.rs').write_text('pub fn external() {}\n', encoding='utf-8')
        (directory / 'src/lib.rs').write_text('', encoding='utf-8')
        selected = toolchain_environment(directory, env)
        build_env = os.environ | env | selected
        for crate in [directory, child]:
            subprocess.run(['cargo', 'generate-lockfile', '--offline', '--manifest-path', str(crate / 'Cargo.toml')],
                           env=build_env, check=True, capture_output=True)
        for step, role in [(step, role) for step in ['Tests', 'Isolated tests'] for role in ['1', 'doctests']]:
            source = ('#[test]\nfn external_child_build_keeps_selected_compiler() {\n' + child_command + '}\n'
                      if role == '1' else '//! ```\n' + ''.join('//! ' + line + '\n' for line in child_command.splitlines()) + '//! ```\n')
            (directory / 'src/lib.rs').write_text(source, encoding='utf-8')
            shutil.rmtree(directory / 'child-output', ignore_errors=True)
            result = execute(step, directory, env | {'RUNNER_TEMP': temporary, 'SHARD': role,
                                                    'COLLECT_COVERAGE': 'false', 'CARGO_NET_OFFLINE': 'true', 'RUSTC_WRAPPER': ''})
            output = result.stdout + result.stderr
            assert result.returncode == 0, output
            assert ('external_child_build_keeps_selected_compiler' in output and 'PASS' in output) if role == '1' else 'test result: ok. 1 passed' in output, output
            assert 'syncing channel updates' not in output, output
            assert (directory / 'child-output/debug/deps').is_dir()

child_cargo_keeps_selected_toolchain_offline()
print('PASS child_cargo_keeps_selected_toolchain_offline')

def declaration(directory):
    with tempfile.NamedTemporaryFile() as output:
        result = execute('Read caller declaration', directory, {'GITHUB_OUTPUT': output.name})
        values = dict(line.split('=', 1) for line in pathlib.Path(output.name).read_text(encoding="utf-8").splitlines())
        return result, values


def browser_test_declaration_checks_exact_packages():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        (directory / '.github').mkdir()
        (directory / 'src').mkdir()
        (directory / 'src/lib.rs').write_text('', encoding='utf-8')
        (directory / 'Cargo.toml').write_text('[package]\nname="browser-case"\nversion="0.1.0"\n', encoding='utf-8')
        subprocess.run(['cargo', 'generate-lockfile', '--offline'], cwd=directory, check=True)
        path = directory / '.github/ci.toml'
        for value in ['[]', '["browser-case"]', '["browser-case", "browser-case"]']:
            path.write_text('wasm_crates=[]\ncheck_targets=[]\nbrowser_test_crates=' + value, encoding='utf-8')
            result, values = declaration(directory)
            assert result.returncode == 0, result.stderr
            assert json.loads(values['browser_test_crates']) == sorted(set(json.loads(value)))
        for value in ['"browser-case"', '1', '1.5', 'true', '{}', '[1]']:
            path.write_text('wasm_crates=[]\ncheck_targets=[]\nbrowser_test_crates=' + value, encoding='utf-8')
            result, _ = declaration(directory)
            assert result.returncode != 0, result.stdout + result.stderr
            assert result.stderr.strip() == '.github/ci.toml: browser_test_crates must be an array of strings', result.stderr
        for name in ['', 'unknown', ' browser-case', 'Browser-case', '$(touch injected)']:
            path.write_text('wasm_crates=[]\ncheck_targets=[]\nbrowser_test_crates=' + json.dumps([name]), encoding='utf-8')
            result, _ = declaration(directory)
            assert result.returncode != 0, result.stdout + result.stderr
            assert result.stderr.strip() == f'.github/ci.toml: browser_test_crates contains unknown package {name!r}', result.stderr
        assert not (directory / 'injected').exists()
        path.write_text('wasm_crates=[]\ncheck_targets=[]\nbrowser_test_crates=["browser-case"]', encoding='utf-8')
        (directory / 'Cargo.toml').write_text('invalid manifest', encoding='utf-8')
        result, _ = declaration(directory)
        assert result.returncode != 0 and '.github/ci.toml: caller metadata/toolchain failed' in result.stderr, result.stdout + result.stderr
        schema = json.loads((root / '.github/ci.schema.json').read_text(encoding='utf-8'))
        assert schema['properties']['browser_test_crates']['type'] == 'array'
        assert schema['properties']['browser_test_crates']['items'] == {'type': 'string'}
        assert schema['required'] == ['wasm_crates', 'check_targets']


browser_test_declaration_checks_exact_packages()

def test_inputs_declaration_checks_paths_and_packages():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        (directory / '.github').mkdir()
        (directory / 'src').mkdir()
        (directory / 'src/lib.rs').write_text('', encoding='utf-8')
        (directory / 'Cargo.toml').write_text('[package]\nname="reader"\nversion="0.1.0"\n', encoding='utf-8')
        subprocess.run(['cargo', 'generate-lockfile', '--offline'], cwd=directory, check=True)
        path = directory / '.github/ci.toml'
        for value in ['{}', '{"shared/"=["reader"], "list.json"=["reader", "reader"], "fuzz"=[]}', '{"a/./b"=[]}']:
            path.write_text('wasm_crates=[]\ncheck_targets=[]\ntest_inputs=' + value, encoding='utf-8')
            result, _ = declaration(directory)
            assert result.returncode == 0, (value, result.stderr)
        for value, diagnostic in [('[]', 'must map repository paths to arrays of package names'),
                                  ('{"shared/"="reader"}', 'must map repository paths to arrays of package names'),
                                  ('{"shared/"=[1]}', 'must map repository paths to arrays of package names'),
                                  ('{""=[]}', 'keys must be repository-relative paths'),
                                  ('{"./"=[]}', 'keys must be repository-relative paths'),
                                  ('{"/abs"=[]}', 'keys must be repository-relative paths'),
                                  ('{"a/../b"=[]}', 'keys must be repository-relative paths'),
                                  ('{"a\\\\b"=[]}', 'keys must be repository-relative paths'),
                                  ('{"\\u0007"=[]}', 'cannot contain control characters or surrogates'),
                                  ('{"shared/"=["unknown"]}', "contains unknown package 'unknown'")]:
            path.write_text('wasm_crates=[]\ncheck_targets=[]\ntest_inputs=' + value, encoding='utf-8')
            result, _ = declaration(directory)
            assert result.returncode != 0, value + '\n' + result.stdout + result.stderr
            assert result.stderr.strip() == '.github/ci.toml: test_inputs ' + diagnostic, (value, result.stderr)
        schema = json.loads((root / '.github/ci.schema.json').read_text(encoding='utf-8'))
        entry = schema['properties']['test_inputs']
        assert entry['type'] == 'object' and entry['default'] == {}
        assert entry['additionalProperties'] == {'type': 'array', 'items': {'type': 'string'}}
        assert entry['propertyNames'] == {'minLength': 1}
        assert 'test_inputs' not in schema['required']

test_inputs_declaration_checks_paths_and_packages()
print('PASS test_inputs_declaration_checks_paths_and_packages')
print('PASS browser_test_declaration_checks_exact_packages')

def browser_test_job_is_single_and_optional():
    jobs = dict(re.findall(r'^  ([\w-]+):\n(.*?)(?=^  [\w-]+:\n|\Z)', workflow.split('\njobs:\n', 1)[1], re.MULTILINE | re.DOTALL))
    job = jobs['browser-tests']
    assert "if: $" + "{{ needs.declaration.outputs.coverage == 'true' && !inputs.advisory-scan && !inputs.fuzz-scan && (!inputs.warm-cache || (github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch))) && needs.declaration.outputs.browser_tests == 'true' }}" in job
    assert 'runs-on: $' + '{{ fromJSON(needs.declaration.outputs.linux_runner) }}' in job and 'matrix:' not in job
    assert body('Browser test recipe').strip() == 'just browser-test'
    assert 'mise exec' not in body('Browser test recipe')
    assert 'if:' not in job.split('- name: Browser test caller tools', 1)[1].split('- name:', 1)[0]
    assert job.index('run: *caller-tools') < job.index('name: Browser runtime tools') < job.index('name: Browser test recipe')
    assert 'browser_test_crates = ["declared"]' in (root / 'fixtures/declared/.github/ci.toml').read_text(encoding='utf-8')

browser_test_job_is_single_and_optional()
print('PASS browser_test_job_is_single_and_optional')

def browser_qualification_reuses_production_bodies():
    source = (root / '.github/workflows/ci-internal.yml').read_text(encoding='utf-8')
    job = source.split('  browser-qualification:\n', 1)[1].split('\n  consumers:', 1)[0]
    names = ['Toolchain', 'Compile cache identity', 'Compile cache', 'Browser test tool pin',
             'Browser test mise bootstrap', 'Browser test caller tools', 'Browser runtime tools',
             'Compile cache statistics', 'Seal compile cache']
    def require_extracted(candidate):
        for name in names:
            assert body(name, candidate).strip() == 'bash -e -o pipefail "$PROOF_STEPS/' + name + '"', name
    require_extracted(job)
    for name in names:
        copied = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=job, text=True))
        step = next(step for step in copied['steps'] if step.get('name') == name)
        step['run'] = body(name).strip()
        try:
            require_extracted(json.dumps(copied))
        except AssertionError as error:
            assert str(error) == name
        else:
            raise AssertionError('copied setup body accepted: ' + name)
    assert 'python3 workflow/.github/extract-workflow-steps.py' in body('Extract workflow steps', job)
    proof = (root / '.github/workflows/ci-proof.yml').read_text(encoding='utf-8')
    assert body('Extract workflow steps', proof) == body('Extract workflow steps', job)
    qualification = ast.parse(body('Immutable shared recipe qualification', job))
    command = next(node.value for node in qualification.body if isinstance(node, ast.Assign)
                   and any(isinstance(target, ast.Name) and target.id == 'command' for target in node.targets))
    assert ast.unparse(command) == "(pathlib.Path(os.environ['PROOF_STEPS']) / 'Browser test recipe').read_text(encoding='utf-8')"
    assert 'browser-qualification' in source.split('  consumers:\n', 1)[1].split('    if:', 1)[0]
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        env = os.environ | {'RUNNER_TEMP': temporary, 'GITHUB_ENV': str(directory / 'env')}
        subprocess.run(['python3', str(root / '.github/extract-workflow-steps.py')], env=env, check=True)
        for name in names + ['Browser test recipe']:
            assert (directory / 'proof-steps' / name).read_text(encoding='utf-8') == body(name), name

browser_qualification_reuses_production_bodies()
print('PASS browser_qualification_reuses_production_bodies')

def test_prepare_command_is_validated_and_forwarded():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        (directory / '.github').mkdir()
        path = directory / '.github/ci.toml'
        for command in ['', 'sh .config/prepare.sh', 'printf "ready\\n"', 'echo λ']:
            path.write_text('wasm_crates=[]\ncheck_targets=[]\ntest_prepare=' + json.dumps(command), encoding='utf-8')
            result, values = declaration(directory)
            assert result.returncode == 0, result.stderr
            assert 'test_prepare' not in values, 'the selection step forwards the command'
        for value in ['true', '1', '[]', '{}']:
            path.write_text('wasm_crates=[]\ncheck_targets=[]\ntest_prepare=' + value, encoding='utf-8')
            result, _ = declaration(directory)
            assert result.returncode != 0
            assert result.stderr.strip() == '.github/ci.toml: test_prepare must be a string', result.stderr
        path.unlink()
        result, values = declaration(directory)
        assert result.returncode == 0 and 'test_prepare' not in values
    schema = json.loads((root / '.github/ci.schema.json').read_text(encoding='utf-8'))
    assert schema['properties']['test_prepare'] == {
        'description': 'Standalone shell command run beside the test build. Write reusable outputs to TEST_PREPARE_OUTPUT.',
        'type': 'string', 'default': ''}

test_prepare_command_is_validated_and_forwarded()
print('PASS test_prepare_command_is_validated_and_forwarded')

def archived_shards_reuse_build_and_prepared_outputs():
    import shutil
    for coverage, route in [('false', 'Tests'), ('true', 'Isolated tests')]:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary) / 'caller'
            shutil.copytree(root / 'fixtures/sample', directory, ignore=shutil.ignore_patterns('target'))
            (directory / '.config').mkdir()
            (directory / '.config/nextest.toml').write_text(
                'experimental=["setup-scripts"]\n'
                                          '[scripts.setup.prepare]\ncommand="sh .config/prepare.sh"\n'
                '[[profile.default.scripts]]\nfilter="all()"\nsetup="prepare"\n', encoding='utf-8')
            (directory / '.config/prepare.sh').write_text(
                'test -x target/test-setup/prepared || exit 41\n'
                'echo WORKFLOW_SETUP=ready >> "$NEXTEST_ENV"\n', encoding='utf-8')
            (directory / 'src/lib.rs').write_text(
                '#[test] fn archive_requires_setup() {\n'
                'assert_eq!(std::env::var("WORKFLOW_SETUP").unwrap(), "ready");\n}\n', encoding='utf-8')
            (directory / 'src/main.rs').write_text('fn main() {}\n', encoding='utf-8')
            (directory / 'tests').mkdir()
            (directory / 'tests/archived_binary.rs').write_text(
                '#[test] fn archived_binary_still_runs() {\n'
                'assert!(std::process::Command::new(env!("CARGO_BIN_EXE_sample")).status().unwrap().success());\n}\n', encoding='utf-8')
            command = 'printf "#!/bin/sh\\nexit 0\\n" > "$TEST_PREPARE_OUTPUT/prepared"; chmod +x "$TEST_PREPARE_OUTPUT/prepared"; echo once >> prepare-count'
            env = {'RUNNER_TEMP': temporary, 'COLLECT_COVERAGE': coverage,
                   'TEST_PREPARE': command, 'TEST_PACKAGES': '["sample"]',
                   'RUSTUP_TOOLCHAIN': subprocess.check_output(['rustup', 'show', 'active-toolchain'], text=True).split()[0]}
            result = execute('Prepare test setup', directory, env)
            assert result.returncode == 0, result.stdout + result.stderr
            result = execute('Build test archive', directory, env)
            assert result.returncode == 0, result.stdout + result.stderr
            assert (result.stdout + result.stderr).count('Compiling sample ') == 1, result.stdout + result.stderr
            assert (directory / 'test-binaries.tar.zst').is_file()
            assert (directory / 'prepare-count').read_text(encoding='utf-8') == 'once\n'
            shutil.rmtree(directory / 'target')
            downloaded = pathlib.Path(temporary) / 'maestro-test-setup'
            downloaded.mkdir()
            shutil.copyfile(pathlib.Path(temporary) / 'test-setup.tar', downloaded / 'test-setup.tar')
            result = execute('Restore test setup', directory, env)
            assert result.returncode == 0, result.stdout + result.stderr
            # A compile attempt must fail; only the archived binary can pass.
            (directory / 'src/lib.rs').write_text('compile_error!("shards must not compile");\n', encoding='utf-8')
            for shard in ['1', '2']:
                result = execute(route, directory, env | {'SHARD': shard, 'TEST_SHARD_COUNT': '2',
                                 'TEST_ARCHIVE': str(directory / 'test-binaries.tar.zst'),
                                 'CARGO_COVERAGE_MIN_LINES': ''})
                assert result.returncode == 0, result.stdout + result.stderr
                assert 'Compiling' not in result.stdout + result.stderr
                assert 'error: cannot clean' not in result.stdout + result.stderr, result.stdout + result.stderr
                if coverage == 'true':
                    assert (directory / f'shard-{shard}.lcov').is_file()
            assert (directory / 'prepare-count').read_text(encoding='utf-8') == 'once\n'
            result = execute('Prepare test setup', directory, env | {'TEST_PREPARE': 'exit 43'})
            assert result.returncode == 43, result.stdout + result.stderr
    jobs = dict(re.findall(r'^  ([\w-]+):\n(.*?)(?=^  [\w-]+:\n|\Z)',
                           workflow.split('\njobs:\n', 1)[1], re.MULTILINE | re.DOTALL))
    assert 'needs: [declaration, test-build, test-prepare]' in jobs['tests']
    assert 'name: Download test archive' in jobs['tests']
    assert 'name: Upload test archive' in jobs['test-build']
    assert 'matrix:' not in jobs['test-build']
    assert 'TEST_PREPARE:' not in jobs['tests']
    assert 'TEST_PREPARE:' not in jobs['test-build']
    assert 'needs: declaration' in jobs['test-prepare']
    assert "matrix.shard != 'doctests'" in jobs['tests']

def archived_shards_provision_child_build_dependencies():
    for route, coverage in [('Tests', 'false'), ('Isolated tests', 'false'), ('Isolated tests', 'true')]:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            tools, log = install_recording_runner(directory)
            env = {'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'TOOLS': str(tools),
                   'RUNNER_TEMP': temporary, 'TEST_ARCHIVE': str(directory / 'test-binaries.tar.zst'),
                   'TEST_PREPARE_OUTPUT': str(directory / 'untrusted-inherited-output'),
                   'TEST_PACKAGES': '["sample"]', 'SHARD': '1', 'COLLECT_COVERAGE': coverage,
                   'CARGO_COVERAGE_MIN_LINES': '', 'ADDITIONAL_LOCKED_MANIFESTS': '["fixture/Cargo.toml"]'}
            for step in ['Fetch additional locked manifests', route]:
                result = execute(step, directory, env)
                assert result.returncode == 0, result.stdout + result.stderr
            commands = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
            assert [command['args'] for command in commands[:2]] == [
                ['fetch', '--locked', '--manifest-path', 'fixture/Cargo.toml'], ['fetch', '--locked']]
            assert len(commands) == 3, commands
            assert '--archive-file' in commands[2]['args']
            assert '--workspace' not in commands[2]['args'] and '--locked' not in commands[2]['args']
            if route == 'Isolated tests':
                assert commands[2]['test_prepare_output'] == str(directory / 'target/test-setup'), commands[2]
    tests = workflow.split('  tests:\n', 1)[1].split('\n  tests-result:', 1)[0]
    fetch = next(step for step in tests.split('      - name: ')[1:] if step.startswith('Fetch additional locked manifests\n'))
    assert "if: needs.declaration.outputs.additional_locked_manifests != '[]'" in fetch

def cargo_git_cache_restores_before_every_fetch():
    build = workflow.split('  test-build:\n', 1)[1].split('\n  test-prepare:', 1)[0]
    prepare = workflow.split('  test-prepare:\n', 1)[1].split('\n  tests:', 1)[0]
    tests = workflow.split('  tests:\n', 1)[1].split('\n  tests-result:', 1)[0]
    for job, fetching in [(build, ['Fetch additional locked manifests', 'Build test archive']),
                          (prepare, ['Fetch additional locked manifests', 'Prepare test setup']),
                          (tests, ['Fetch additional locked manifests', 'Tests', 'Isolated tests'])]:
        names = [step.splitlines()[0] for step in job.split('      - name: ')[1:]]
        order = ['Cargo git cache identity', 'Restore Cargo git cache'] + fetching
        assert [names.index(name) for name in order] == sorted(names.index(name) for name in order), names
        restore = next(step for step in job.split('      - name: ')[1:] if step.startswith('Restore Cargo git cache\n'))
        assert 'uses: actions/cache/restore@' in restore and 'continue-on-error: true' in restore
        if job is not build:
            assert 'with: *git-cache-restore' in restore
            continue
        assert 'with: &git-cache-restore' in restore
        assert 'steps.git-cache-identity.outputs.home }}/git/db' in restore
        assert 'steps.git-cache-identity.outputs.home }}/git/checkouts' in restore
        assert 'key: $' + '{{ steps.git-cache-identity.outputs.key }}' in restore
        assert 'restore-keys: $' + '{{ steps.git-cache-identity.outputs.prefix }}' in restore
    save = next(step for step in build.split('      - name: ')[1:] if step.startswith('Save Cargo git cache\n'))
    assert 'uses: actions/cache/save@' in save and 'continue-on-error: true' in save
    assert "if: steps.cache-identity.outputs.writer == 'true' && steps.git-cache-restore.outputs.cache-hit != 'true'" in save
    assert 'key: $' + '{{ steps.git-cache-identity.outputs.key }}' in save
    assert build.index('name: Build test archive') < build.index('name: Save Cargo git cache')
    assert 'Save Cargo git cache' not in tests and 'Save Cargo git cache' not in prepare

    def identity(directory, manifests='[]', **extra):
        output = directory / 'github-output'
        output.write_text('', encoding='utf-8')
        env = {'GITHUB_OUTPUT': str(output), 'ADDITIONAL_LOCKED_MANIFESTS': manifests, 'GITHUB_EVENT_NAME': 'push',
               'GITHUB_REF': 'refs/heads/main', 'CACHE_DEFAULT_BRANCH': 'main', 'CALLER_DIRECTORY': '.',
               'CACHE_CALL': '', 'CALLER_REPOSITORY': 'owner/repo', 'HOME': '/home/runner', 'CARGO_HOME': None} | extra
        result = execute('Cargo git cache identity', directory, env)
        assert result.returncode == 0, result.stderr
        return dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())

    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        (directory / 'fixture').mkdir()
        (directory / 'Cargo.lock').write_text('version = 4\n', encoding='utf-8')
        (directory / 'fixture/Cargo.lock').write_text('version = 4\n', encoding='utf-8')
        declared = '["fixture/Cargo.toml"]'
        before, with_fixture = identity(directory), identity(directory, declared)
        assert before == identity(directory) and before['key'].startswith(before['prefix'])
        assert before['home'] == '/home/runner/.cargo' and identity(directory, CARGO_HOME='/cargo')['home'] == '/cargo'
        assert with_fixture['key'] != before['key'] and with_fixture['prefix'] == before['prefix']
        (directory / 'fixture/Cargo.lock').write_text('version = 4\n# changed\n', encoding='utf-8')
        assert identity(directory) == before and identity(directory, declared)['key'] != with_fixture['key']
        (directory / 'Cargo.lock').write_text('version = 4\n# changed\n', encoding='utf-8')
        assert identity(directory)['key'] != before['key']
        assert identity(directory, CACHE_CALL='other')['prefix'] != before['prefix']
        assert 'writer' not in before

def archive_signal_reaches_only_archived_shards():
    for shard, warm, coverage, archive, expected in [
        ('1', 'false', 'false', 'test-binaries.tar.zst', '1'),
        ('2', 'false', 'true', 'test-binaries.tar.zst', '1'),
        ('warm', 'true', 'false', '', None),
        ('2', 'false', 'false', '', None),
        ('2', 'false', 'true', '', None),
        ('doctests', 'false', 'false', '', None),
    ]:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            tools, log = install_recording_runner(directory)
            result = execute('Isolated tests', directory, {
                'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'TOOLS': str(tools),
                'RUNNER_TEMP': temporary, 'SHARD': shard, 'WARM_CACHE': warm,
                'COLLECT_COVERAGE': coverage, 'TEST_ARCHIVE': archive,
                'TEST_PACKAGES': '["sample"]', 'CARGO_COVERAGE_MIN_LINES': ''})
            assert result.returncode == 0, result.stdout + result.stderr
            commands = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
            assert len(commands) == 2, commands
            assert commands[1]['test_archive'] == expected, (shard, warm, coverage, commands)
            assert commands[1]['test_prepare_output'] == str(directory / 'target/test-setup')

def archived_shards_skip_compile_cache_lifecycle():
    document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
    steps = {step['name']: step for step in document['jobs']['tests']['steps']}
    for name in ['Compile cache identity', 'Restore compile cache', 'Compile cache',
                 'Compile cache statistics', 'Seal compile cache']:
        assert "(inputs.warm-cache || matrix.shard == 'doctests')" in steps[name]['if'], name
    assert steps['Compile cache statistics']['if'].startswith('always() && ')

archived_shards_skip_compile_cache_lifecycle()
print('PASS archived_shards_skip_compile_cache_lifecycle')

def doctests_run_independently_and_remain_required():
    document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
    jobs = document['jobs']
    assert jobs['doctests']['needs'] == 'declaration'
    assert jobs['doctests']['name'] == '$' + "{{ matrix.shard == 'doctests' && 'Doctests' }}"
    assert jobs['doctests']['strategy']['matrix']['shard'] == ['doctests']
    for key in ['steps', 'defaults', 'env']:
        assert jobs['doctests'][key] == jobs['tests'][key], key
    assert set(jobs['tests-result']['needs']) == {'tests', 'doctests', 'declaration', 'test-build'}
    for tests, docs in [('success', 'success'), ('success', 'failure'), ('failure', 'success')]:
        result = subprocess.run(['python3', '-c', body('All test shards passed')],
                                env=os.environ | {'RESULT': tests, 'DOCTEST_RESULT': docs,
                                'COLLECT_COVERAGE': 'true', 'DECLARATION_RESULT': 'success', 'INCLUDED_TESTS': 'false'},
                                capture_output=True, text=True)
        assert (result.returncode == 0) == (tests == docs == 'success')

doctests_run_independently_and_remain_required()
print('PASS doctests_run_independently_and_remain_required')

def preparation_runs_beside_host_compilation():
    document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
    jobs = document['jobs']
    assert jobs['test-build']['needs'] == jobs['test-prepare']['needs'] == 'declaration'
    assert set(jobs['tests']['needs']) == {'declaration', 'test-build', 'test-prepare'}
    assert 'TEST_PREPARE' not in jobs['test-build']['env']
    assert 'needs.declaration.outputs.test_prepare' in jobs['test-prepare']['if']
    assert 'TEST_PREPARE_OUTPUT' in body('Prepare test setup')
    assert 'target/test-setup' in body('Prepare test setup')
    assert 'tar -C target' in body('Restore test setup')
    assert jobs['doctests']['needs'] == 'declaration'

preparation_runs_beside_host_compilation()
print('PASS preparation_runs_beside_host_compilation')

def absent_keeps_base_checks():
    result, values = declaration(root / 'fixtures/sample')
    assert result.returncode == 0, result.stdout + result.stderr
    assert values == {'test_prepare_inputs': '[]', 'warm_test_filter': json.dumps('none()'), 'test_shard_count': '16', 'additional_locked_manifests': '[]', 'quality_config': '{}', 'enabled': 'false', 'wasm_crates': '[]', 'browser_test_crates': '[]', 'targets': '[]', 'browser_build': 'false', 'test_tools': 'false', 'reserved_ports': '', 'fuzz_manifest': '', 'fuzz_targets': '[]'}
    assert 'if: needs.declaration.outputs.enabled != \'true\'' in workflow
    assert 'if: needs.declaration.outputs.enabled == \'true\'' in workflow
    assert "inputs.working-directory != '.'" in workflow

absent_keeps_base_checks()
print('PASS absent_keeps_base_checks')

def warm_filter_declaration_keeps_defaults():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        (directory / '.github').mkdir()
        (directory / '.github/ci.toml').write_text('wasm_crates=[]\ncheck_targets=[]\n', encoding='utf-8')
        result, values = declaration(directory)
        assert result.returncode == 0, result.stderr
        assert values['enabled'] == 'true'
        assert json.loads(values['warm_test_filter']) == 'none()'
    schema = json.loads((root / '.github/ci.schema.json').read_text(encoding='utf-8'))
    prop = schema['properties']['warm_test_filter']
    assert prop['type'] == 'string' and prop['default'] == 'none()'
    assert schema['required'] == ['wasm_crates', 'check_targets']
    assert schema['additionalProperties'] is False

warm_filter_declaration_keeps_defaults()
print('PASS warm_filter_declaration_keeps_defaults')

warm_filters = ['test(child_build)', 'none()', '', ' test("quoted words") ',
                'test(λ)', r'test(a\b)', 'test(child_build)\n & !test(unrelated)',
                '$(touch injected); `touch injected`; $HOME']

def warm_filter_declaration_preserves_strings():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        (directory / '.github').mkdir()
        for value in warm_filters:
            (directory / '.github/ci.toml').write_text(
                'wasm_crates=[]\ncheck_targets=[]\nwarm_test_filter=' + json.dumps(value, ensure_ascii=False), encoding='utf-8')
            result, values = declaration(directory)
            assert result.returncode == 0, result.stderr
            assert json.loads(values['warm_test_filter']) == value
            assert values['quality_config'] == '{}'
            assert values['enabled'] == 'true'
        assert not (directory / 'injected').exists()

warm_filter_declaration_preserves_strings()
print('PASS warm_filter_declaration_preserves_strings')

def warm_filter_declaration_rejects_non_strings():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        (directory / '.github').mkdir()
        for value in ['true', '1', '1.5', '[]', '{}', '1979-05-27', '07:32:00', '1979-05-27T07:32:00Z']:
            (directory / '.github/ci.toml').write_text(
                'wasm_crates=[]\ncheck_targets=[]\nwarm_test_filter=' + value, encoding='utf-8')
            result, values = declaration(directory)
            assert result.returncode != 0, result.stdout + result.stderr
            assert result.stderr.strip() == '.github/ci.toml: warm_test_filter must be a string', result.stderr
            assert values == {}

warm_filter_declaration_rejects_non_strings()
print('PASS warm_filter_declaration_rejects_non_strings')

def install_recording_runner(directory, real_cargo=None, manifest=None, status=0):
    tools = directory / 'runner-tools'
    tools.mkdir()
    log = directory / 'commands.jsonl'
    source = directory / 'runner-source'
    source.write_text('#!/usr/bin/env python3\n' + textwrap.dedent(f'''
        import json, os, pathlib, subprocess, sys
        args = sys.argv[1:]
        with open({str(log)!r}, 'a', encoding='utf-8') as output:
            output.write(json.dumps(dict(tool=pathlib.Path(sys.argv[0]).name, args=args,
                                         home=os.environ.get('HOME'), cargo_home=os.environ.get('CARGO_HOME'),
                                         ambient=os.environ.get('UNRELATED_AMBIENT_CANARY'),
                                         test_prepare_output=os.environ.get('TEST_PREPARE_OUTPUT'),
                                         test_archive=os.environ.get('TEST_ARCHIVE'),
                                         diagnostics=os.environ.get('MAESTRO_TEST_DIAGNOSTICS_DIR'))) + '\\n')
        if args[:1] != ['fetch'] and os.environ.get('MAESTRO_TEST_DIAGNOSTICS_DIR'):
            (pathlib.Path(os.environ['MAESTRO_TEST_DIAGNOSTICS_DIR']) / 'runner.txt').write_text(
                'child marker', encoding='utf-8')
        if args[:1] == ['fetch'] and {real_cargo!r}:
            sys.exit(subprocess.run([{real_cargo!r}] + args).returncode)
        if args[:1] != ['fetch'] and {manifest!r}:
            subprocess.run([{real_cargo!r}, 'metadata', '--offline', '--locked', '--format-version', '1',
                            '--manifest-path', {manifest!r}], check=True, stdout=subprocess.DEVNULL)
        if pathlib.Path(sys.argv[0]).name != 'cargo':
            if '--output-path' in args:
                pathlib.Path(args[args.index('--output-path') + 1]).write_text(
                    'SF:/src/lib.rs\\nDA:1,1\\nend_of_record\\n', encoding='utf-8')
            print('runner stdout', flush=True)
            print('runner stderr', file=sys.stderr, flush=True)
            sys.exit({status})
    '''), encoding='utf-8')
    for name in ['cargo', 'cargo-nextest', 'cargo-llvm-cov']:
        subprocess.run(['install', '-m', '0700', str(source), str(tools / name)], check=True)
    return tools, log

def archived_coverage_excludes_unselected_report_packages():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        tools, log = install_recording_runner(directory)
        result = execute('Isolated tests', directory, {
            'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'TOOLS': str(tools),
            'RUNNER_TEMP': temporary, 'SHARD': '2', 'COLLECT_COVERAGE': 'true',
            'CARGO_COVERAGE_MIN_LINES': '', 'TEST_PACKAGES': '["sample"]',
            'TEST_ARCHIVE': str(directory / 'archive.tar.zst'), 'WARM_CACHE': 'false',
            'COVERAGE_EXCLUDE': '["unrelated"]', 'COVERAGE_SCOPE': 'selected packages: sample'})
        assert result.returncode == 0, result.stdout + result.stderr
        commands = [json.loads(line)['args'] for line in log.read_text(encoding='utf-8').splitlines()]
        args = next(args for args in commands if args[:2] == ['llvm-cov', 'nextest'])
        assert '--archive-file' in args and '--no-clean' in args, args
        assert args[args.index('--exclude-from-report') + 1] == 'unrelated', args
        assert '-p' not in args, args

archived_coverage_excludes_unselected_report_packages()
print('PASS archived_coverage_excludes_unselected_report_packages')

def selected_archive_build_compiles_only_selected_packages():
    for coverage in ['true', 'false']:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            tools, log = install_recording_runner(directory)
            env = {'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'TOOLS': str(tools),
                   'COLLECT_COVERAGE': coverage, 'TEST_PACKAGES': '["sample"]',
                   'COVERAGE_SCOPE': 'selected packages: sample'}
            result = execute('Build test archive', directory, env)
            assert result.returncode == 0, result.stdout + result.stderr
            args = json.loads(log.read_text(encoding='utf-8').splitlines()[-1])['args']
            assert '-p' in args and args[args.index('-p') + 1] == 'sample', args
            assert '--workspace' not in args, args
            log.unlink()
            result = execute('Build test archive', directory, env | {
                'TEST_PACKAGES': '["sample", "unrelated"]', 'COVERAGE_SCOPE': 'full workspace'})
            assert result.returncode == 0, result.stdout + result.stderr
            args = json.loads(log.read_text(encoding='utf-8').splitlines()[-1])['args']
            assert '--workspace' in args and '-p' not in args, args

selected_archive_build_compiles_only_selected_packages()
print('PASS selected_archive_build_compiles_only_selected_packages')

def isolated_coverage_runs_selected_packages_and_merge_names_scope():
    for step in ['Isolated tests']:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            tools, log = install_recording_runner(directory)
            env = {'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'TOOLS': str(tools),
                   'RUNNER_TEMP': temporary, 'SHARD': '2', 'COLLECT_COVERAGE': 'true',
                   'CARGO_COVERAGE_MIN_LINES': '', 'TEST_PACKAGES': '["sample"]',
                   'COVERAGE_EXCLUDE': '["unrelated"]', 'COVERAGE_SCOPE': 'selected packages: sample',
                   'WARM_CACHE': 'false'}
            result = execute(step, directory, env)
            assert result.returncode == 0, result.stdout + result.stderr
            commands = [json.loads(line)['args'] for line in log.read_text(encoding='utf-8').splitlines()]
            args = next(args for args in commands if args[:2] == ['llvm-cov', 'nextest'])
            assert '-p' in args and args[args.index('-p') + 1] == 'sample', args
            assert '--workspace' not in args, args
            assert args[args.index('--exclude-from-report') + 1] == 'unrelated', args
            log.unlink()
            result = execute(step, directory, env | {
                'TEST_PACKAGES': '["sample", "unrelated"]', 'COVERAGE_EXCLUDE': '[]',
                'COVERAGE_SCOPE': 'full workspace'})
            assert result.returncode == 0, result.stdout + result.stderr
            commands = [json.loads(line)['args'] for line in log.read_text(encoding='utf-8').splitlines()]
            args = next(args for args in commands if args[:2] == ['llvm-cov', 'nextest'])
            assert '--workspace' in args and '--exclude-from-report' not in args, args
    # The shared merger names the scope and retains the same floor.
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        reports = directory / 'coverage'
        reports.mkdir()
        (reports / 'shard-1.lcov').write_text('SF:/sample/lib.rs\nDA:1,1\nDA:2,0\nend_of_record\n', encoding='utf-8')
        result = execute('Merge coverage', directory, {
            'COVERAGE_REPORT_COUNT': '1', 'COVERAGE_SCOPE': 'full workspace'})
        assert result.returncode != 0 and 'minimum 80%' in result.stdout, result.stdout + result.stderr
        assert 'Coverage scope: full workspace' in result.stdout, result.stdout

isolated_coverage_runs_selected_packages_and_merge_names_scope()
print('PASS isolated_coverage_runs_selected_packages_and_merge_names_scope')

def undeclared_coverage_measures_the_full_workspace():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        tools, log = install_recording_runner(directory)
        result = execute('Coverage', directory, {
            'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'TOOLS': str(tools),
            'TEST_PACKAGES': '["sample"]', 'COVERAGE_EXCLUDE': '["unrelated"]'})
        assert result.returncode == 0, result.stdout + result.stderr
        commands = [json.loads(line)['args'] for line in log.read_text(encoding='utf-8').splitlines()]
        args = next(args for args in commands if args[:2] == ['llvm-cov', 'nextest'])
        assert '--workspace' in args and '-p' not in args and '--exclude-from-report' not in args, args
    step = workflow.split('      - name: Coverage\n', 1)[1].split('      - name: ', 1)[0]
    for name in ['TEST_PACKAGES', 'COVERAGE_EXCLUDE', 'COVERAGE_SCOPE']:
        assert name not in step, name + ' reaches the undeclared coverage step'

undeclared_coverage_measures_the_full_workspace()
print('PASS undeclared_coverage_measures_the_full_workspace')
archived_shards_provision_child_build_dependencies()
print('PASS archived_shards_provision_child_build_dependencies')
cargo_git_cache_restores_before_every_fetch()
print('PASS cargo_git_cache_restores_before_every_fetch')
archive_signal_reaches_only_archived_shards()
print('PASS archive_signal_reaches_only_archived_shards')

def warm_filter_runs_only_in_warm_mode():
    for value in warm_filters:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            tools, log = install_recording_runner(directory)
            result = execute('Isolated tests', directory, {
                'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'TOOLS': str(tools),
                'RUNNER_TEMP': temporary, 'SHARD': 'warm', 'COLLECT_COVERAGE': 'false',
                'WARM_CACHE': 'true', 'WARM_TEST_FILTER': value, 'TEST_PACKAGES': '[]',
                'CARGO_COVERAGE_MIN_LINES': ''})
            assert result.returncode == 0, result.stdout + result.stderr
            commands = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
            assert len(commands) == 2 and commands[0]['args'] == ['fetch', '--locked']
            args = commands[1]['args']
            assert args.count('-E') == 1 and args[args.index('-E') + 1] == value, args
            assert '--workspace' in args and '--no-report' in args
            assert '--partition' not in args and '--doc' not in args and '--lcov' not in args
            assert commands[1]['home'] != os.environ.get('HOME')
            assert not (directory / 'injected').exists() and not list(directory.glob('*.lcov'))
    for step, shard, coverage, floor in [('Tests', '2', 'false', ''),
                                         ('Isolated tests', '2', 'false', ''),
                                         ('Isolated tests', '2', 'true', ''),
                                         ('Isolated tests', 'doctests', 'true', ''),
                                         ('Isolated tests', '', 'true', '80')]:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            tools, log = install_recording_runner(directory)
            env = {'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'TOOLS': str(tools),
                   'RUNNER_TEMP': temporary, 'SHARD': shard, 'COLLECT_COVERAGE': coverage,
                   'CARGO_COVERAGE_MIN_LINES': floor, 'TEST_PACKAGES': '["sample"]', 'WARM_CACHE': 'false'}
            observed = []
            for value in [None, 'test(']:
                result = execute(step, directory, env | {'WARM_TEST_FILTER': value})
                assert result.returncode == 0, result.stdout + result.stderr
                commands = [json.loads(line)['args'] for line in log.read_text(encoding='utf-8').splitlines()]
                assert commands and all('-E' not in args and 'test(' not in args for args in commands)
                observed.append(commands)
                log.unlink()
            assert observed[0] == observed[1]

warm_filter_runs_only_in_warm_mode()
print('PASS warm_filter_runs_only_in_warm_mode')

def warm_filter_workspace(directory):
    import shutil
    shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
    (directory / '.config').mkdir()
    (directory / '.config/nextest.toml').write_text('[profile.default]\ndefault-filter="none()"\n', encoding='utf-8')
    child = directory / 'child'
    (child / 'src').mkdir(parents=True)
    (child / 'Cargo.toml').write_text(
        '[workspace]\n[package]\nname="warm-child"\nversion="0.1.0"\nedition="2021"\n', encoding='utf-8')
    (child / 'src/lib.rs').write_text('pub fn child() {}\n', encoding='utf-8')
    subprocess.run(['cargo', 'generate-lockfile', '--offline', '--manifest-path', str(child / 'Cargo.toml')], check=True)
    (directory / 'src/lib.rs').write_text(textwrap.dedent('''
        #[test]
        fn child_build() {
            assert!(std::env::var_os("WARM_TEST_FILTER").is_none());
            let started = std::time::Instant::now();
            let output = std::process::Command::new(env!("CARGO"))
                .args(["build", "--offline", "--locked", "--manifest-path", "child/Cargo.toml",
                       "--target-dir", "child-output"])
                .output().unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr);
            std::fs::write("child-build.log", format!("child build elapsed: {:?}; lock wait reported: {}; compiler output:\\n{}",
                                                   started.elapsed(), stderr.contains("Blocking waiting for file lock"), stderr)).unwrap();
            assert!(output.status.success(), "child build failed");
            assert!(stderr.contains("Compiling warm-child"));
            std::fs::write("child-built", "compiled").unwrap();
        }
        #[test]
        fn unrelated() { std::fs::write("unrelated-ran", "wrong selection").unwrap(); }
        #[test]
        fn permanently_failing() { assert_eq!(1, 2, "intentional warm failure"); }
    '''), encoding='utf-8')
    return {'RUNNER_TEMP': str(directory), 'SHARD': 'warm', 'COLLECT_COVERAGE': 'false',
            'WARM_CACHE': 'true', 'TEST_PACKAGES': '[]', 'CARGO_COVERAGE_MIN_LINES': '', 'RUSTC_WRAPPER': ''}

def warm_filter_runs_selected_child_builds():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        env = warm_filter_workspace(directory)
        assert not (directory / 'target').exists() and not (directory / 'child-output').exists()
        result = execute('Isolated tests', directory, env | {
            'WARM_TEST_FILTER': 'test(child_build)\n & !test(unrelated)'})
        output = result.stdout + result.stderr
        assert result.returncode == 0, output
        assert '1 test run: 1 passed' in output and 'child_build' in output, output
        assert (directory / 'child-built').read_text(encoding='utf-8') == 'compiled'
        assert (directory / 'child-output/debug/deps').is_dir()
        assert not (directory / 'unrelated-ran').exists() and not list(directory.glob('*.lcov'))
        assert 'Doc-tests' not in output
        print(output + (directory / 'child-build.log').read_text(encoding='utf-8'), flush=True)

warm_filter_runs_selected_child_builds()
print('PASS warm_filter_runs_selected_child_builds')

def warm_filter_failures_keep_job_red():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        env = warm_filter_workspace(directory)
        for value, status in [('test(', 94), ('', 94), ('test(permanently_failing)', 100),
                              ('test(missing)', 0), ('test([)', 0)]:
            result = execute('Isolated tests', directory, env | {'WARM_TEST_FILTER': value})
            output = result.stdout + result.stderr
            assert result.returncode == status, output
            if status == 94:
                assert 'failed to parse filterset' in output, output
            elif status == 100:
                assert 'intentional warm failure' in output and 'assertion `left == right` failed' in output, output
                assert 'TRY 3 FAIL' in output, output
            else:
                assert '0 tests run' in output, output
            assert not (directory / 'child-built').exists() and not (directory / 'unrelated-ran').exists()
            assert not list(directory.glob('*.lcov')) and 'Doc-tests' not in output
            print(f'PASS warm filter {value!r}: exit {result.returncode}\n{output}', flush=True)
    document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
    save = next(step for step in document['jobs']['tests']['steps'] if step.get('name') == 'Save compile cache')
    assert save['if'] == "steps.cache-identity.outputs.writer == 'true' && steps.cache-seal.outputs.eligible == 'true'"

warm_filter_failures_keep_job_red()
print('PASS warm_filter_failures_keep_job_red')

def warm_push_seeds_non_test_shapes():
    document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))

    for name in ('quality', 'docs', 'browser', 'browser-tests', 'doctests'):
        condition = document['jobs'][name]['if']
        for warm in (False, True):
            for event in ('push', 'pull_request', 'merge_group', 'workflow_dispatch'):
                for ref in ('refs/heads/main', 'refs/heads/feature', 'refs/tags/main'):
                    for excluded in ('none', 'advisory', 'fuzz'):
                        observed = evaluate(condition, {
                            default_ref: 'refs/heads/main',
                            'inputs.warm-cache': warm, 'github.event_name': event, 'github.ref': ref,
                            'inputs.advisory-scan': excluded == 'advisory',
                            'inputs.fuzz-scan': excluded == 'fuzz',
                            'github.repository_id': 'caller', 'inputs.working-directory': '.',
                            'needs.declaration.outputs.coverage': 'true',
                            'needs.declaration.outputs.included_tests': 'true',
                            'needs.declaration.outputs.wasm_crates': '["web"]',
                            'needs.declaration.outputs.browser_build': 'true',
                            'needs.declaration.outputs.browser_tests': 'true'})
                        assert observed == (excluded == 'none' and (not warm or
                            (event == 'push' and ref == 'refs/heads/main'))), (name, warm, event, ref, excluded)

    doctests = document['jobs']['doctests']
    assert doctests['env']['WARM_CACHE'] == '$' + "{{ inputs.warm-cache && matrix.shard != 'doctests' }}"

    def test_steps_run(job, warm, enabled):
        shard = 'warm' if job == 'tests' else document['jobs'][job]['strategy']['matrix']['shard'][0]
        values = {'inputs.warm-cache': warm, 'matrix.shard': shard,
                  'needs.declaration.outputs.enabled': enabled, 'runner.environment': 'github-hosted'}
        values['env.WARM_CACHE'] = str(evaluate(document['jobs'][job]['env']['WARM_CACHE'], values)).lower()
        steps = {step['name']: step for step in document['jobs'][job]['steps']}
        return {name: evaluate(steps[name]['if'], values) for name in
                ('Free instrumented build space', 'Tests', 'Search tools', 'Isolated tests')}

    for enabled in ('true', 'false'):
        # The warm doctests writer runs the same test steps as its readers.
        assert test_steps_run('doctests', True, enabled) == test_steps_run('doctests', False, enabled), enabled
        assert test_steps_run('tests', True, enabled) == {
            'Free instrumented build space': True, 'Tests': False, 'Search tools': True, 'Isolated tests': True}, enabled
    assert doctests['steps'] == document['jobs']['tests']['steps']
    assert doctests['env']['SHARD'] == document['jobs']['tests']['env']['SHARD']
    assert next(step for step in document['jobs']['browser-tests']['steps'] if step.get('name') == 'Browser test recipe')['run'].strip() == 'just browser-test'

warm_push_seeds_non_test_shapes()
print('PASS warm_push_seeds_non_test_shapes')

def warm_cache_keeps_one_writer():
    document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
    declaration_job = document['jobs']['declaration']
    assert declaration_job['outputs']['warm_test_filter'] == '$' + '{{ steps.config.outputs.warm_test_filter }}'
    assert next(step for step in declaration_job['steps'] if step.get('name') == 'Read caller declaration')['id'] == 'config'
    assert document['jobs']['tests']['env']['WARM_TEST_FILTER'] == '$' + '{{ fromJSON(needs.declaration.outputs.warm_test_filter) }}'
    assert all('needs.declaration.outputs.warm_test_filter' not in step.get('run', '')
               for job in document['jobs'].values() for step in job.get('steps', []))
    assert "TEST_EVENT: $" + "{{ inputs.warm-cache && 'push' ||" in workflow
    with tempfile.TemporaryDirectory() as temporary:
        output = pathlib.Path(temporary) / 'selection'
        result = select_packages(root / 'fixtures/sample', {
            'TEST_EVENT': 'push', 'BASE_REF': 'unrelated-workflow-base', 'GITHUB_OUTPUT': str(output)})
        assert result.returncode == 0, result.stderr
        values = dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())
        assert json.loads(values['packages']) == ['sample'] and values['coverage'] == 'true'
    assert "warm-cache:\n        description:" in workflow
    jobs = dict(re.findall(r'^  ([\w-]+):\n(.*?)(?=^  [\w-]+:\n|\Z)',
                           workflow.split('\njobs:\n', 1)[1], re.MULTILINE | re.DOTALL))
    for name, job in jobs.items():
        if name not in ('declaration', 'tests', 'targets', 'quality', 'docs', 'browser', 'browser-tests', 'doctests'):
            assert '!inputs.warm-cache' in re.search(r'^    if: (.*)', job, re.MULTILINE)[1], name
    condition = document['jobs']['targets']['if']
    for warm in (False, True):
        for event in ('push', 'pull_request', 'merge_group', 'workflow_dispatch'):
            for advisory, fuzz, targets in [(False, False, '[{}]'), (True, False, '[{}]'),
                                             (False, True, '[{}]'), (False, False, '[]')]:
                observed = evaluate(condition, {
                    'inputs.warm-cache': warm, 'github.event_name': event,
                    'inputs.advisory-scan': advisory, 'inputs.fuzz-scan': fuzz,
                    'needs.declaration.outputs.targets': targets,
                    'needs.declaration.outputs.coverage': str(warm or event != 'pull_request').lower()})
                assert observed == (not advisory and not fuzz and targets != '[]' and
                                    (not warm or event == 'push')), (warm, event, advisory, fuzz, targets)
    tests = jobs['tests']
    assert 'WARM_CACHE: $' + "{{ inputs.warm-cache && matrix.shard != 'doctests' }}" in tests
    assert "inputs.warm-cache && '[\"warm\"]'" in tests
    assert "if: env.WARM_CACHE == 'true' || needs.declaration.outputs.enabled == 'true'" in tests
    proof = (root / '.github/workflows/ci-proof.yml').read_text(encoding='utf-8')
    assert '  maestro:\n    needs: warm\n' in proof
    assert '      warm-cache: true\n' in proof

warm_cache_keeps_one_writer()
print('PASS warm_cache_keeps_one_writer')

coverage_report_args = ['--no-default-ignore-filename-regex', '--ignore-filename-regex',
                        r'(^|/)(tests|benches)/([^.]|\.[^./]|\.\.[^/])|(^|/)(target|vendor|registry|toolchains)/|(^|/)git/checkouts/|(^|/)rustc/([0-9a-f]+|[0-9]+\.[0-9]+\.[0-9]+)/|(^|/)(tests\.rs|[0-9a-zA-Z_-]+[_-]tests\.rs)$']

def coverage_exclusions_have_one_workflow_owner():
    document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
    policy = document['env']['COVERAGE_IGNORE_FILENAME_REGEX']
    assert policy == coverage_report_args[2]
    assert workflow.count(policy) == 1
    for name, count in [('Coverage', 1), ('Isolated tests', 1)]:
        assert body(name).count('--ignore-filename-regex "$COVERAGE_IGNORE_FILENAME_REGEX"') == count, name
    internal = (root / '.github/workflows/ci-internal.yml').read_text(encoding='utf-8')
    with tempfile.TemporaryDirectory() as temporary:
        exported = pathlib.Path(temporary) / 'env'
        result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', body('Shared coverage policy', internal)],
                                env=os.environ | {'GITHUB_ENV': str(exported), 'RUNNER_TEMP': temporary},
                                capture_output=True, text=True)
        assert result.returncode == 0, result.stdout + result.stderr
        exported_text = exported.read_text(encoding='utf-8')
        assert exported_text.startswith(f'PROOF_STEPS={temporary}/proof-steps\nCOVERAGE_IGNORE_FILENAME_REGEX={policy}\n')
        assert 'COVERAGE_MERGE<<COVERAGE_POLICY\n' + document['env']['COVERAGE_MERGE'] + 'COVERAGE_POLICY\n' in exported_text
        assert workflow.count('source = os.path.normpath(record[3:])') == 1
        for name in ['Coverage', 'Isolated tests', 'Merge coverage']:
            assert body(name).count('python3 -c "$COVERAGE_MERGE"') == 1, name

coverage_exclusions_have_one_workflow_owner()
print('PASS coverage_exclusions_have_one_workflow_owner')

def package_ownership_has_one_workflow_owner():
    document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
    assert 'PACKAGE_OWNER' in document['env'], 'missing workflow-level package ownership snippet'
    helper = document['env']['PACKAGE_OWNER']
    assert workflow.count('def package_owner(') == 1
    assert len(re.findall(r'key=lambda [^:]+: len\([^)]*\.parts\)', workflow)) == 1
    for consumer in [document['env']['COVERAGE_MERGE'], body('Select test packages')]:
        assert "exec(os.environ['PACKAGE_OWNER'])" in consumer and 'package_owner(' in consumer
    namespace = {}
    exec(helper, namespace)
    roots = {'outer': pathlib.PurePosixPath('crates/a'), 'inner': pathlib.PurePosixPath('crates/a/nested')}
    for source, owner in [('crates/a/nested/src/lib.rs', 'inner'), ('crates/a/src/lib.rs', 'outer'),
                          ('crates/b/src/lib.rs', None)]:
        assert namespace['package_owner'](pathlib.PurePosixPath(source), roots) == owner, source
    internal = (root / '.github/workflows/ci-internal.yml').read_text(encoding='utf-8')
    with tempfile.TemporaryDirectory() as temporary:
        exported = pathlib.Path(temporary) / 'env'
        result = subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', body('Shared coverage policy', internal)],
                                env=os.environ | {'GITHUB_ENV': str(exported), 'RUNNER_TEMP': temporary},
                                capture_output=True, text=True)
        assert result.returncode == 0, result.stdout + result.stderr
        assert 'PACKAGE_OWNER<<COVERAGE_POLICY\n' + helper + 'COVERAGE_POLICY\n' in exported.read_text(encoding='utf-8')

package_ownership_has_one_workflow_owner()
print('PASS package_ownership_has_one_workflow_owner')

def warm_cache_builds_without_tests_or_reports():
    for status in (0, 17):
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            tools, log = install_recording_runner(directory, status=status)
            result = execute('Isolated tests', directory, {
                'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'TOOLS': str(tools),
                'RUNNER_TEMP': temporary, 'SHARD': 'warm', 'COLLECT_COVERAGE': 'false',
                'WARM_CACHE': 'true', 'TEST_PACKAGES': '[]', 'CARGO_COVERAGE_MIN_LINES': ''})
            assert result.returncode == status, result.stdout + result.stderr
            commands = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
            assert len(commands) == 2, commands
            assert commands[0]['args'] == ['fetch', '--locked']
            assert commands[1]['tool'] == 'cargo-llvm-cov'
            assert commands[1]['args'] == [
                'llvm-cov', 'nextest', '--user-config-file', 'none', '--ignore-default-filter',
                '--retries', '2', '--flaky-result', 'pass', '--final-status-level', 'flaky',
                '-E', 'none()', '--no-tests', 'pass', '--workspace', '--locked'] + coverage_report_args + ['--no-report']
            assert commands[1]['home'] != os.environ.get('HOME')
            assert not list(directory.glob('*.lcov'))

warm_cache_builds_without_tests_or_reports()
print('PASS warm_cache_builds_without_tests_or_reports')

def fixture_git(directory, *args):
    return subprocess.check_output(['git', '-c', 'user.name=Fixture', '-c',
        'user.email=fixture@example.com', '-c', 'commit.gpgsign=false', *args],
        cwd=directory, text=True).strip()

def coverage_matrix_splits_doctests_from_partitions():
    tests = workflow.split('  tests:\n', 1)[1].split('\n  tests-result:', 1)[0]
    matrix = re.search(r'        shard: (.*)', tests)[1]
    assert 'test_shard_count: $' + '{{ steps.config.outputs.test_shard_count }}' in workflow
    assert 'test_shards: $' + '{{ steps.selection.outputs.test_shards }}' in workflow
    assert matrix == "$" + "{{ fromJSON(inputs.warm-cache && '[\"warm\"]' || needs.declaration.outputs.test_shards) }}"
    assert "name: $" + "{{ inputs.warm-cache && 'Warm coverage cache' || matrix.shard == 'doctests' && 'Doctests' || format('Test shard ({0}/{1})', matrix.shard, needs.declaration.outputs.test_shard_count) }}" in tests
    assert '    needs: [declaration, test-build, test-prepare]\n' in tests and '      fail-fast: false\n' in tests
    assert 'advisory-scan' in tests and "inputs.working-directory != '.'" in tests
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        import shutil
        shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        fixture_git(directory, 'init', '--quiet')
        fixture_git(directory, 'add', '.')
        fixture_git(directory, 'commit', '-qm', 'coverage fixture')
        for declared in [False, True]:
            if declared:
                (directory / '.github').mkdir()
                (directory / '.github/ci.toml').write_text('wasm_crates=[]\ncheck_targets=[]\n', encoding='utf-8')
            result, values = declaration(directory)
            assert result.returncode == 0 and values['enabled'] == str(declared).lower(), result.stderr
            for event in ['pull_request', 'merge_group', 'push']:
                output = directory / 'output'
                result = select_packages(directory, {'TEST_EVENT': event, 'BASE_REF': '', 'GITHUB_OUTPUT': str(output)})
                assert result.returncode == 0, result.stderr
                selected = dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())
                legs = json.loads(selected['test_shards'])
                assert legs == list(range(1, 17))

coverage_matrix_splits_doctests_from_partitions()
print('PASS coverage_matrix_splits_doctests_from_partitions')

def doctest_leg_runs_only_selected_docs():
    for step in ['Tests', 'Isolated tests']:
        for selected, expected in [(None, ['--workspace']), ('["alpha","beta"]', ['-p', 'alpha', '-p', 'beta']), ('[]', [])]:
            with tempfile.TemporaryDirectory() as temporary:
                directory = pathlib.Path(temporary)
                tools, log = install_recording_runner(directory)
                result = execute(step, directory, {
                    'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'TOOLS': str(tools),
                    'RUNNER_TEMP': temporary, 'SHARD': 'doctests', 'COLLECT_COVERAGE': 'true',
                    'TEST_PACKAGES': selected, 'MAESTRO_TEST_DIAGNOSTICS_DIR': None})
                assert result.returncode == 0, result.stdout + result.stderr
                commands = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
                assert [c['args'] for c in commands] == [['fetch', '--locked']] + (
                    [['test', '--locked', '--doc'] + expected] if expected else [])
                assert not (directory / 'nextest.log').exists()
                assert not list(directory.glob('*.lcov'))

doctest_leg_runs_only_selected_docs()
print('PASS doctest_leg_runs_only_selected_docs')

def doctest_failure_keeps_required_result_red():
    for step in ['Tests', 'Isolated tests']:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            (directory / 'src').mkdir()
            (directory / 'Cargo.toml').write_text('[package]\nname="doc-control"\nversion="0.1.0"\nedition="2024"\n', encoding='utf-8')
            (directory / 'Cargo.lock').write_text('version = 4\n\n[[package]]\nname = "doc-control"\nversion = "0.1.0"\n', encoding='utf-8')
            for value, status in [(2, 101), (1, 0)]:
                (directory / 'src/lib.rs').write_text(f'//! ```\n//! assert_eq!(1, {value});\n//! ```\n', encoding='utf-8')
                result = execute(step, directory, {'RUNNER_TEMP': temporary, 'SHARD': 'doctests',
                    'COLLECT_COVERAGE': 'true', 'TEST_PACKAGES': None, 'CARGO_TARGET_DIR': str(directory / 'target')})
                output = result.stdout + result.stderr
                assert result.returncode == status, output
                assert ('test result: FAILED' if status else 'test result: ok. 1 passed') in output, output
                assert not (directory / 'nextest.log').exists()
                print(f'PASS real doctest {step}: exit {status}', flush=True)
    aggregate = workflow.split('  tests-result:\n', 1)[1].split('  checks:', 1)[0]
    assert 'continue-on-error' not in aggregate

doctest_failure_keeps_required_result_red()
print('PASS doctest_failure_keeps_required_result_red')

def diagnostic_directory_reaches_isolated_runner():
    for caller in ['.', 'nested space/雪']:
        for coverage in ['false', 'true']:
            for shard in [*map(str, range(1, 17)), 'doctests']:
                with tempfile.TemporaryDirectory() as temporary:
                    directory = pathlib.Path(temporary) / caller
                    directory.mkdir(parents=True, exist_ok=True)
                    tools, log = install_recording_runner(directory)
                    result = execute('Isolated tests', directory, {
                        'PATH': str(tools) + os.pathsep + os.environ['PATH'],
                        'TOOLS': str(tools), 'RUNNER_TEMP': temporary, 'SHARD': shard,
                        'COLLECT_COVERAGE': coverage, 'UNRELATED_AMBIENT_CANARY': 'absent',
                        'MAESTRO_TEST_DIAGNOSTICS_DIR': temporary + '/decoy'})
                    assert result.returncode == 0, result.stdout + result.stderr
                    commands = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
                    children = [c for c in commands if c['args'][:1] != ['fetch']]
                    expected = str(directory.resolve() / 'target/browser-author-diagnostics')
                    assert children and all(c['diagnostics'] == expected for c in children), children
                    assert all(c['ambient'] is None for c in children)
                    assert len(children) == (2 if shard == '1' and coverage == 'false' else 1), children
                    if shard == 'doctests':
                        assert children[0]['args'] == ['test', '--locked', '--doc', '--workspace']
                    else:
                        assert children[0]['tool'] == ('cargo-llvm-cov' if coverage == 'true' else 'cargo-nextest')
                        assert children[0]['args'][children[0]['args'].index('--partition') + 1] == f'count:{shard}/16'
                        if shard == '1' and coverage == 'false':
                            assert children[1]['args'] == ['test', '--locked', '--doc', '--workspace']
                    assert (pathlib.Path(expected) / 'runner.txt').read_text(encoding='utf-8') == 'child marker'
                    assert all(not pathlib.Path(c['home']).exists() for c in children)
                    assert not (pathlib.Path(temporary) / 'decoy').exists()

diagnostic_directory_reaches_isolated_runner()
print('PASS diagnostic_directory_reaches_isolated_runner')

def isolated_cleanup_stops_leaked_processes_and_keeps_test_status():
    import signal
    import shutil
    def alive(pid):
        try:
            return pathlib.Path(f'/proc/{pid}/stat').read_text(encoding='utf-8').rsplit(')', 1)[1].split()[0] != 'Z'
        except FileNotFoundError:
            return False
    for status, leak, removal_fails in [(0, 'writer', False), (3, 'writer', False), (0, 'compile cache', False),
                                        (3, 'compile cache', False), (0, None, True), (3, None, True),
                                        (0, 'environment', False), (0, 'working directory', False),
                                        (0, 'open file', False), (0, 'executable', False)]:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            tools = directory / 'runner-tools'
            tools.mkdir()
            writer = directory / 'leaking-writer.py'
            # A test runner may exit while a descendant in its own session keeps writing to TMPDIR.
            writer.write_text(textwrap.dedent('''
                import os, pathlib, time
                temporary = pathlib.Path(os.environ['TMPDIR'])
                os.chdir(temporary)
                for index in range(100000):
                    (temporary / f'leaked-{index}').touch()
                while True:
                    time.sleep(60)
            '''), encoding='utf-8')
            # The compile cache server is a real executable named sccache; its owner stops it after the tests.
            shutil.copy(shutil.which('sleep'), tools / 'sccache')
            runner = tools / 'cargo-nextest'
            runner.write_text('#!/usr/bin/env python3\n' + textwrap.dedent(f'''
                import os, pathlib, shutil, subprocess, sys, time
                leak = {leak!r}
                if leak:
                    isolated_tmp = os.environ['TMPDIR']
                    # Each sleeper is visible to one check only: it has no isolated environment, directory, file or executable except the one named.
                    command, cwd, env, stdout = [shutil.which('sleep'), '600'], '/', {{}}, subprocess.DEVNULL
                    if leak == 'writer':
                        command, cwd, env = [sys.executable, {str(writer)!r}], None, None
                    elif leak == 'compile cache':
                        command, cwd, env = [{str(tools / 'sccache')!r}, '600'], None, None
                    elif leak == 'environment':
                        env = None
                    elif leak == 'working directory':
                        cwd = isolated_tmp
                    elif leak == 'open file':
                        stdout = open(isolated_tmp + '/held', 'w')
                    elif leak == 'executable':
                        command[0] = shutil.copy(command[0], isolated_tmp + '/sleeper')
                    process = subprocess.Popen(command, cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=stdout,
                                               stderr=subprocess.DEVNULL, start_new_session=True)
                    pathlib.Path({str(directory / 'leaked.pid')!r}).write_text(str(process.pid), encoding='utf-8')
                    for _ in range(1000):
                        if leak != 'writer' or len(os.listdir(isolated_tmp)) > 2000:
                            break
                        time.sleep(0.01)
                    else:
                        sys.exit('leaking writer never started')
                print('runner done', flush=True)
                sys.exit({status})
            '''), encoding='utf-8')
            (tools / 'cargo').write_text('#!/bin/sh\nexit 0\n', encoding='utf-8')
            (tools / 'rm').write_text('#!/bin/sh\nexit 1\n' if removal_fails else '#!/bin/sh\nexec /bin/rm "$@"\n', encoding='utf-8')
            for tool in tools.iterdir():
                tool.chmod(0o700)
            pid_file = directory / 'leaked.pid'
            try:
                result = execute('Isolated tests', directory, {
                    'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'TOOLS': str(tools),
                    'RUNNER_TEMP': temporary, 'SHARD': '2', 'COLLECT_COVERAGE': 'false'})
                output = result.stdout + result.stderr
                assert result.returncode == status, (status, leak, removal_fails, output)
                stopped = [line for line in output.splitlines() if line.startswith('Stopped test process ')]
                pid = int(pid_file.read_text(encoding='utf-8')) if leak else None
                if leak not in (None, 'compile cache'):
                    name = 'leaking-writer.py' if leak == 'writer' else 'sleep'
                    assert len(stopped) == 1 and f' {pid}:' in stopped[0] and name in stopped[0], (leak, output)
                    assert not alive(pid) and not list(directory.glob('maestro-tests.*')), (leak, output)
                else:
                    assert not stopped, output
                if leak == 'compile cache':
                    assert alive(pid), output
                if removal_fails:
                    assert 'Could not remove ' in output, output
            finally:
                if pid_file.exists() and alive(int(pid_file.read_text(encoding='utf-8'))):
                    os.kill(int(pid_file.read_text(encoding='utf-8')), signal.SIGKILL)

isolated_cleanup_stops_leaked_processes_and_keeps_test_status()
print('PASS isolated_cleanup_stops_leaked_processes_and_keeps_test_status')

def diagnostic_post_step_keeps_partial_transcripts():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        env = {'GITHUB_OUTPUT': temporary + '/outputs', 'CALLER_REPOSITORY': 'org/repo',
               'CALLER_DIRECTORY': '.', 'CALL_KEY': '', 'GITHUB_REPOSITORY_ID': '123',
               'SHARD': '1', 'GITHUB_RUN_ID': '456', 'GITHUB_RUN_ATTEMPT': '1'}
        diagnostics = directory / 'target/browser-author-diagnostics'
        for state in ['missing', 'empty', 'partial']:
            if state == 'empty':
                diagnostics.mkdir(parents=True)
            if state == 'partial':
                (diagnostics / 'one.txt').write_bytes(b'CHILD-READY-MARKER\n::error::literal payload\n')
                (diagnostics / 'two.txt').write_bytes(b'phase: waiting-for-child-exit')
            result = execute('Print test diagnostics', directory, env)
            assert result.returncode == 0, result.stderr
            token = re.search(r'^::stop-commands::([^\n]+)\n', result.stdout)
            assert token, result.stdout
            assert result.stdout.endswith('\n::' + token[1] + '::\n'), result.stdout
            if state == 'partial':
                for path in diagnostics.iterdir():
                    assert path.read_bytes() in result.stdout.encode('utf-8')
                    assert result.stdout.index(path.read_text(encoding='utf-8')) > token.end() - 1
            repeated = execute('Print test diagnostics', directory, env)
            assert repeated.returncode == 0, repeated.stderr
            assert token[1] not in repeated.stdout
            assert pathlib.Path(env['GITHUB_OUTPUT']).read_text(encoding='utf-8').startswith('artifact_name=')

diagnostic_post_step_keeps_partial_transcripts()
print('PASS diagnostic_post_step_keeps_partial_transcripts')

def diagnostic_upload_is_unconditional_and_scoped():
    steps = dict((s.splitlines()[0], s) for s in workflow.split('      - name: ')[1:])
    for name in ['Print test diagnostics', 'Upload test diagnostics']:
        step = steps[name]
        assert "        if: always() && needs.declaration.outputs.enabled == 'true'\n" in step
        assert 'continue-on-error' not in step and 'coverage' not in step
        assert list(steps).index(name) > list(steps).index('Isolated tests')
    upload = steps['Upload test diagnostics']
    assert 'uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02' in upload
    assert 'if-no-files-found: ignore\n' in upload
    paths = re.search(r'          path: \|\n((?:            .*\n)+)', upload)[1].splitlines()
    assert [p.strip() for p in paths] == [
        "$" + "{{ inputs.working-directory || '.' }}/target/browser-author-diagnostics",
        "$" + "{{ inputs.working-directory || '.' }}/nextest.log"]
    assert 'name: $' + '{{ steps.diagnostics.outputs.artifact_name }}\n' in upload
    assert 'timeout-minutes: 45\n' in steps['Isolated tests']
    assert "if: $" + "{{ env.WARM_CACHE != 'true' && needs.declaration.outputs.enabled != 'true' }}\n" in steps['Tests']
    for coverage in ['false', 'true']:
        for included in ['false', 'true']:
            for status in ['success', 'failure', 'cancelled', 'skipped']:
                result = subprocess.run(['python3', '-c', body('All test shards passed')],
                                        env=os.environ | {'RESULT': status, 'DOCTEST_RESULT': status, 'COLLECT_COVERAGE': coverage,
                                                          'INCLUDED_TESTS': included, 'DECLARATION_RESULT': 'success'},
                                        capture_output=True, text=True)
                expected = 'success' if 'true' in (coverage, included) else 'skipped'
                assert (result.returncode == 0) == (status == expected), (coverage, included, status, result.stderr)

diagnostic_upload_is_unconditional_and_scoped()
print('PASS diagnostic_upload_is_unconditional_and_scoped')

def diagnostic_artifact_names_distinguish_invocations():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        base = {'CALLER_REPOSITORY': 'org/repo', 'CALLER_DIRECTORY': '.', 'CALL_KEY': '',
                'GITHUB_REPOSITORY_ID': '123', 'SHARD': '1', 'GITHUB_RUN_ID': '456', 'GITHUB_RUN_ATTEMPT': '1'}
        def name(changes):
            output = directory / 'output'
            output.write_text('', encoding='utf-8')
            result = execute('Print test diagnostics', directory, base | changes | {'GITHUB_OUTPUT': str(output)})
            assert result.returncode == 0, result.stderr
            value = output.read_text(encoding='utf-8').removeprefix('artifact_name=').strip()
            assert re.fullmatch(r'test-diagnostics-123-[0-9a-f]{64}-shard-(?:[12]|doctests)-run-45[67]-attempt-[12]', value), value
            return value
        original = name({})
        assert original == 'test-diagnostics-123-f6d6a7cff3ca148497eb84888c4fd22a9de927c2e9fda6aa267078943c10ea14-shard-1-run-456-attempt-1'
        for field, values in [('CALLER_REPOSITORY', ['other/repo']),
                              ('CALLER_DIRECTORY', ['fixtures/a b', 'fixtures/a/b']),
                              ('CALL_KEY', ['a/b', 'a-b'])]:
            names = [original] + [name({field: value}) for value in values]
            assert len(set(names)) == len(names), (field, names)
        for field, value, suffix in [('SHARD', '2', '-shard-2-run-456-attempt-1'),
                                     ('SHARD', 'doctests', '-shard-doctests-run-456-attempt-1'),
                                     ('GITHUB_RUN_ID', '457', '-shard-1-run-457-attempt-1'),
                                     ('GITHUB_RUN_ATTEMPT', '2', '-shard-1-run-456-attempt-2')]:
            changed = name({field: value})
            assert changed.split('-shard-')[0] == original.split('-shard-')[0]
            assert changed.endswith(suffix)
        unicode_name = name({'CALLER_DIRECTORY': 'fixtures/雪 space', 'CALL_KEY': 'a/b:!?'})
        assert unicode_name == 'test-diagnostics-123-062b74de152feb2104328f6103b56364b374d5af055bba4cce98a5c548f90089-shard-1-run-456-attempt-1'
    step = next(s for s in workflow.split('      - name: ')[1:] if s.startswith('Print test diagnostics\n'))
    for expression in ["inputs.checkout-repository || github.repository", "inputs.working-directory || '.'", 'inputs.concurrency-key']:
        assert '$' + '{{ ' + expression + ' }}' in step.split('        run:', 1)[0]
    assert '$' + '{{' not in body('Print test diagnostics')

diagnostic_artifact_names_distinguish_invocations()
print('PASS diagnostic_artifact_names_distinguish_invocations')

def shard_runner_status_survives_log_capture():
    job_run_defaults_preserve_pipeline_failures()
    for step, coverage in [(step, coverage) for step in ['Tests', 'Isolated tests'] for coverage in ['false', 'true']]:
        for shard in range(1, 17):
            for status in [0, 42]:
                with tempfile.TemporaryDirectory() as temporary:
                    directory = pathlib.Path(temporary)
                    tools, log = install_recording_runner(directory, status=status)
                    result = execute(step, directory, {'PATH': str(tools) + os.pathsep + os.environ['PATH'],
                                     'TOOLS': str(tools), 'RUNNER_TEMP': temporary,
                                     'SHARD': str(shard), 'COLLECT_COVERAGE': coverage})
                    assert result.returncode == status, (step, shard, status, result.stdout, result.stderr)
                    assert (directory / 'nextest.log').read_text(encoding='utf-8') == 'runner stdout\nrunner stderr\n'
                    commands = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
                    runners = [command for command in commands if '--partition' in command['args']]
                    assert len(runners) == 1
                    args = runners[0]['args']
                    assert args[args.index('--partition') + 1] == f'count:{shard}/16'
                    doctests = [command for command in commands if command['args'][:1] == ['test']]
                    assert bool(doctests) == (shard == 1 and status == 0 and coverage == 'false'), commands
                    report = directory / f'shard-{shard}.lcov'
                    assert report.exists() == (step == 'Isolated tests' and coverage == 'true'), (step, shard, status)
                    if report.exists():
                        assert report.read_text(encoding='utf-8') == 'SF:/src/lib.rs\nDA:1,1\nend_of_record\n'

shard_runner_status_survives_log_capture()
print('PASS shard_runner_status_survives_log_capture')

def failed_shard_and_missing_coverage_stay_distinct():
    step = next(s for s in workflow.split('      - name: ')[1:] if s.startswith('Upload shard coverage\n')).split('\n  tests-result:', 1)[0]
    assert "if: $" + "{{ !inputs.warm-cache && needs.declaration.outputs.enabled == 'true' && needs.declaration.outputs.coverage == 'true' && matrix.shard != 'doctests' }}\n" in step
    assert 'always()' not in step and 'if-no-files-found: error\n' in step
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        (directory / 'coverage').mkdir()
        result = execute('Merge coverage', directory)
        assert result.returncode != 0, result.stdout + result.stderr
        assert result.stderr.strip() == 'Missing or unexpected shard coverage reports'

failed_shard_and_missing_coverage_stay_distinct()
print('PASS failed_shard_and_missing_coverage_stay_distinct')

def locked_manifests_default_to_no_extra_fetch():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        (directory / '.github').mkdir()
        tools, log = install_recording_runner(directory)
        for suffix in ['', 'additional_locked_manifests=[]\n']:
            (directory / '.github/ci.toml').write_text('wasm_crates=[]\ncheck_targets=[]\n' + suffix, encoding='utf-8')
            result, values = declaration(directory)
            assert result.returncode == 0, result.stderr
            assert values['additional_locked_manifests'] == '[]'
            assert values['quality_config'] == '{}' and values['enabled'] == 'true'
            result = execute('Fetch additional locked manifests', directory, {
                'PATH': str(tools) + os.pathsep + os.environ['PATH'],
                'ADDITIONAL_LOCKED_MANIFESTS': values['additional_locked_manifests']})
            assert result.returncode == 0 and not log.exists(), result.stderr
        step = next(s for s in workflow.split('      - name: ')[1:] if s.startswith('Fetch additional locked manifests\n'))
        assert "if: needs.declaration.outputs.additional_locked_manifests != '[]'" in step
        assert 'always()' not in step

locked_manifests_default_to_no_extra_fetch()

def locked_manifests_reject_malformed_values():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        (directory / '.github').mkdir()
        base = 'wasm_crates=[]\ncheck_targets=[]\nadditional_locked_manifests='
        cases = [(value, 'must be an array of nonempty strings') for value in
                 ['"Cargo.toml"', '1', '1.5', 'true', '{}', '[1]', '[true]', '[{}]', '[[]]', '[""]']]
        cases += [(json.dumps(['fixture' + chr(code) + '/Cargo.toml']),
                   'cannot contain control characters or surrogates') for code in [0, 31, 127, 159]]
        cases += [('broken [', None), ('[]\nadditional_locked_manifests=[]', None), ('["\\uD800"]', None)]
        for value, diagnostic in cases:
            (directory / '.github/ci.toml').write_text(base + value, encoding='utf-8')
            result, values = declaration(directory)
            assert result.returncode != 0 and not values, value + '\n' + result.stdout + result.stderr
            assert '.github/ci.toml' in result.stderr, result.stderr
            if diagnostic:
                assert result.stderr.strip() == '.github/ci.toml: additional_locked_manifests ' + diagnostic, result.stderr

locked_manifests_reject_malformed_values()

def locked_manifests_reject_missing_or_outside_paths():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        parent = pathlib.Path(temporary)
        directory = parent / 'caller'
        shutil.copytree(root / 'fixtures/sample', directory, ignore=shutil.ignore_patterns('target'))
        (directory / '.github').mkdir()
        outside = parent / 'caller-sibling'
        outside.mkdir()
        (outside / 'Cargo.toml').write_text('[workspace]\n', encoding='utf-8')
        (directory / 'directory/Cargo.toml').mkdir(parents=True)
        (directory / 'Other.toml').write_text('[workspace]\n', encoding='utf-8')
        (directory / 'dangling').symlink_to(parent / 'missing', target_is_directory=True)
        (directory / 'escape').symlink_to(outside, target_is_directory=True)
        (directory / 'Cargo-link').mkdir()
        (directory / 'Cargo-link/Cargo.toml').symlink_to(outside / 'Cargo.toml')
        for value in ['missing/Cargo.toml', 'directory/Cargo.toml', 'Other.toml', 'dangling/Cargo.toml',
                      str(directory / 'Cargo.toml'), str(outside / 'Cargo.toml'), '../caller-sibling/Cargo.toml',
                      'escape/Cargo.toml', 'Cargo-link/Cargo.toml']:
            (directory / '.github/ci.toml').write_text(
                'wasm_crates=[]\ncheck_targets=[]\nadditional_locked_manifests=' + json.dumps([value]), encoding='utf-8')
            result, values = declaration(directory)
            assert result.returncode != 0 and not values, value + '\n' + result.stdout + result.stderr
            assert result.stderr.strip() == ('.github/ci.toml: additional_locked_manifests requires existing '
                                             'repository-contained relative Cargo.toml files'), result.stderr

locked_manifests_reject_missing_or_outside_paths()

def locked_manifests_accept_literal_contained_paths():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        (directory / '.github').mkdir()
        paths = ['z author build/Cargo.toml', 'a café/Cargo.toml', ' /Cargo.toml', '\u00a0/Cargo.toml']
        for value in paths:
            path = directory / value
            path.parent.mkdir()
            path.write_text('[workspace]\n', encoding='utf-8')
        (directory / 'link').symlink_to(directory / 'a café', target_is_directory=True)
        paths += ['link/Cargo.toml', './a café/Cargo.toml', 'z author build/../a café/Cargo.toml', paths[0]]
        for entries in [[paths[0]], paths]:
            (directory / '.github/ci.toml').write_text(
                'wasm_crates=[]\ncheck_targets=[]\nadditional_locked_manifests=' + json.dumps(entries, ensure_ascii=False), encoding='utf-8')
            result, values = declaration(directory)
            assert result.returncode == 0, result.stderr
            assert json.loads(values['additional_locked_manifests']) == entries
            assert values['quality_config'] == '{}'

locked_manifests_accept_literal_contained_paths()

def locked_manifests_schema_matches_contract():
    schema = json.loads((root / '.github/ci.schema.json').read_text(encoding='utf-8'))
    entry = schema['properties']['additional_locked_manifests']
    assert entry['type'] == 'array' and entry['default'] == []
    assert entry['items'] == {'type': 'string', 'minLength': 1}
    assert not entry.get('uniqueItems', False)
    assert 'additional_locked_manifests' not in schema['required']
    assert 'runtime' in entry['description'] and 'containment' in entry['description']

locked_manifests_schema_matches_contract()

def fuzz_caller(temporary):
    import shutil
    directory = pathlib.Path(temporary) / 'caller'
    shutil.copytree(root / 'fixtures/sample', directory, ignore=shutil.ignore_patterns('target'))
    (directory / '.github').mkdir()
    (directory / 'fuzz').mkdir()
    (directory / 'fuzz/Cargo.toml').write_text('[workspace]\n', encoding='utf-8')
    return directory

fuzz_entry = '{name="planted", seconds=60, max_len=64, timeout=5, rss_limit_mb=512}'

def fuzz_declaration_accepts_exact_entries():
    with tempfile.TemporaryDirectory() as temporary:
        directory = fuzz_caller(temporary)
        base = 'wasm_crates=[]\ncheck_targets=[]\n'
        two = (f'fuzz_targets=[{fuzz_entry}, {{name="clean", seconds=10, max_len=8, timeout=5, rss_limit_mb=512}}]\n')
        for text, manifest, targets in [
                (base, '', '[]'),
                (base + 'fuzz_manifest="fuzz/Cargo.toml"\n' + two, 'fuzz/Cargo.toml',
                 '[{"name": "planted", "seconds": 60, "max_len": 64, "timeout": 5, "rss_limit_mb": 512}, '
                 '{"name": "clean", "seconds": 10, "max_len": 8, "timeout": 5, "rss_limit_mb": 512}]'),
                (base + 'fuzz_manifest="fuzz/Cargo.toml"\nfuzz_targets=[]\n', 'fuzz/Cargo.toml', '[]')]:
            (directory / '.github/ci.toml').write_text(text, encoding='utf-8')
            result, values = declaration(directory)
            assert result.returncode == 0, result.stderr
            assert (values['fuzz_manifest'], values['fuzz_targets'], values['quality_config']) == (manifest, targets, '{}'), values

fuzz_declaration_accepts_exact_entries()
print('PASS fuzz_declaration_accepts_exact_entries')

def fuzz_declaration_rejects_malformed_entries():
    def targets(**fields):
        entry = dict(name='"planted"', seconds=60, max_len=64, timeout=5, rss_limit_mb=512) | fields
        return 'fuzz_manifest="fuzz/Cargo.toml"\nfuzz_targets=[{' + ', '.join(f'{key}={value}' for key, value in entry.items()) + '}]'
    shape = 'fuzz_targets requires name, seconds, max_len, timeout and rss_limit_mb'
    path = 'fuzz_manifest requires an existing repository-contained relative Cargo.toml file'
    number = 'fuzz_targets seconds, max_len, timeout and rss_limit_mb must be positive integers'
    cases = [('fuzz_manifest="fuzz/Cargo.toml"', 'fuzz_manifest and fuzz_targets must be declared together'),
             (f'fuzz_targets=[{fuzz_entry}]', 'fuzz_manifest and fuzz_targets must be declared together'),
             (f'fuzz_manifest=1\nfuzz_targets=[{fuzz_entry}]', 'fuzz_manifest must be a nonempty string'),
             (f'fuzz_manifest=""\nfuzz_targets=[{fuzz_entry}]', 'fuzz_manifest must be a nonempty string'),
             (f'fuzz_manifest="fuzz\\u0001/Cargo.toml"\nfuzz_targets=[{fuzz_entry}]',
              'fuzz_manifest cannot contain control characters or surrogates'),
             (f'fuzz_manifest="missing/Cargo.toml"\nfuzz_targets=[{fuzz_entry}]', path),
             (f'fuzz_manifest="../outside/Cargo.toml"\nfuzz_targets=[{fuzz_entry}]', path),
             (f'fuzz_manifest="fuzz/Other.toml"\nfuzz_targets=[{fuzz_entry}]', path),
             ('fuzz_manifest="fuzz/Cargo.toml"\nfuzz_targets={}', 'fuzz_targets must be an array'),
             ('fuzz_manifest="fuzz/Cargo.toml"\nfuzz_targets=["planted"]', shape),
             ('fuzz_manifest="fuzz/Cargo.toml"\nfuzz_targets=[{name="planted", seconds=60, max_len=64, timeout=5}]', shape),
             (targets(jobs=2), shape),
             (targets(name='"a b"'), 'fuzz_targets name must be a Cargo target name'),
             (targets(name=1), 'fuzz_targets name must be a Cargo target name'),
             (targets(seconds=0), number), (targets(max_len='true'), number),
             (targets(timeout=1.5), number), (targets(rss_limit_mb=-1), number),
             (f'fuzz_manifest="fuzz/Cargo.toml"\nfuzz_targets=[{fuzz_entry}, {fuzz_entry}]', 'fuzz_targets names must be unique')]
    with tempfile.TemporaryDirectory() as temporary:
        directory = fuzz_caller(temporary)
        (directory / 'fuzz/Other.toml').write_text('[workspace]\n', encoding='utf-8')
        (directory.parent / 'outside').mkdir()
        (directory.parent / 'outside/Cargo.toml').write_text('[workspace]\n', encoding='utf-8')
        for text, diagnostic in cases:
            (directory / '.github/ci.toml').write_text('wasm_crates=[]\ncheck_targets=[]\n' + text + '\n', encoding='utf-8')
            result, values = declaration(directory)
            assert result.returncode != 0 and not values, text + '\n' + result.stdout + result.stderr
            assert result.stderr.strip() == '.github/ci.toml: ' + diagnostic, (text, result.stderr)
    return len(cases)

print(f'PASS fuzz_declaration_rejects_malformed_entries: {fuzz_declaration_rejects_malformed_entries()} cases')

def fuzz_schema_matches_contract():
    schema = json.loads((root / '.github/ci.schema.json').read_text(encoding='utf-8'))
    manifest, targets = schema['properties']['fuzz_manifest'], schema['properties']['fuzz_targets']
    assert (manifest['type'], manifest['minLength']) == ('string', 1)
    assert targets['type'] == 'array'
    items = targets['items']
    assert items['type'] == 'object' and items['additionalProperties'] is False
    assert sorted(items['required']) == ['max_len', 'name', 'rss_limit_mb', 'seconds', 'timeout']
    assert items['properties']['name'] == {'type': 'string', 'pattern': '^[A-Za-z0-9_-]+$'}
    for key in ['seconds', 'max_len', 'timeout', 'rss_limit_mb']:
        assert {k: v for k, v in items['properties'][key].items() if k != 'description'} == {'type': 'integer', 'minimum': 1}, key
    assert schema['dependentRequired'] == {'fuzz_manifest': ['fuzz_targets'], 'fuzz_targets': ['fuzz_manifest']}
    assert schema['required'] == ['wasm_crates', 'check_targets']

fuzz_schema_matches_contract()
print('PASS fuzz_schema_matches_contract')

def fuzz_scan_runs_only_declaration_and_fuzz():
    jobs = dict(re.findall(r'^  ([\w-]+):\n(.*?)(?=^  [\w-]+:\n|\Z)', workflow.split('\njobs:\n', 1)[1], re.MULTILINE | re.DOTALL))
    job = jobs['fuzz']
    assert "    if: $" + "{{ !inputs.warm-cache && inputs.fuzz-scan && needs.declaration.outputs.fuzz_targets != '[]' }}\n" in job
    assert 'fuzz-scan' not in jobs['declaration'] and 'fuzz-scan' not in jobs['advisories']
    assert 'fail-fast: false' in job and '      RUSTUP_TOOLCHAIN: nightly-2026-09-15\n' in job
    assert workflow.count('nightly-') == 1 and workflow.count('/cargo-fuzz/') == 1
    assert '        env:\n          FUZZ_SCAN: true\n        run: *check-tools\n' in job
    steps = re.findall(r'^      - name: (.+)$', job, re.MULTILINE)
    assert steps.index('Fuzz licences') < steps.index('Fuzz target'), steps
    assert "if: steps.fuzz.outputs.found == 'true'" in job.split('- name: Upload fuzz finding\n', 1)[1].split('- name:', 1)[0]
    nightly = (root / '.github/workflows/fuzz.yml').read_text(encoding='utf-8')
    triggers = nightly.split('\non:\n', 1)[1].split('\n\n', 1)[0]
    assert re.findall(r'^  (\w+):', triggers, re.MULTILINE) == ['schedule', 'workflow_dispatch'], triggers
    assert '      fuzz-scan: true\n' in nightly and 'test-event' not in nightly

fuzz_scan_runs_only_declaration_and_fuzz()
print('PASS fuzz_scan_runs_only_declaration_and_fuzz')

def locked_manifests_forward_ordered_literal_arguments():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        tools, log = install_recording_runner(directory)
        entries = ['z space \'" $(touch marker) `touch marker`; café/Cargo.toml', 'a/Cargo.toml']
        for entry in entries:
            path = directory / entry
            path.parent.mkdir()
            path.write_text('[workspace]\n', encoding='utf-8')
        entries.append(entries[0])
        result = execute('Fetch additional locked manifests', directory, {
            'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'ADDITIONAL_LOCKED_MANIFESTS': json.dumps(entries)})
        assert result.returncode == 0, result.stderr
        commands = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
        assert [c['args'] for c in commands] == [['fetch', '--locked', '--manifest-path', entry] for entry in entries]
        assert result.stderr.splitlines() == ['Fetching locked dependencies: ' + entry for entry in entries]
        assert not (directory / 'marker').exists()

locked_manifests_forward_ordered_literal_arguments()

def locked_manifests_fail_without_rewriting_inputs():
    import hashlib
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        entries = ['first missing/Cargo.toml', 'second matching/Cargo.toml']
        for entry in entries:
            package = directory / entry
            package.parent.mkdir()
            (package.parent / 'src').mkdir()
            (package.parent / 'src/lib.rs').write_text('pub fn value() {}\n', encoding='utf-8')
            package.write_text('[workspace]\n[package]\nname="proof"\nversion="0.1.0"\nedition="2021"\n', encoding='utf-8')
        real_cargo = shutil.which('cargo')
        tools, log = install_recording_runner(directory, real_cargo)
        env = {'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'ADDITIONAL_LOCKED_MANIFESTS': json.dumps(entries)}
        def cargo(*args):
            result = subprocess.run([real_cargo, *args], cwd=directory, capture_output=True, text=True)
            assert result.returncode == 0, result.stderr
        cargo('generate-lockfile', '--manifest-path', entries[1])
        def hashes():
            return {str(p.relative_to(directory)): hashlib.sha256(p.read_bytes()).hexdigest()
                    for p in directory.rglob('Cargo.*')}
        for case in ['missing', 'stale', 'matching']:
            if case == 'stale':
                cargo('generate-lockfile', '--manifest-path', entries[0])
                path = directory / entries[0]
                path.write_text(path.read_text(encoding='utf-8').replace('0.1.0', '0.2.0'), encoding='utf-8')
            elif case == 'matching':
                cargo('generate-lockfile', '--manifest-path', entries[0])
            before = hashes()
            if log.exists():
                log.unlink()
            result = execute('Fetch additional locked manifests', directory, env)
            assert result.returncode == (0 if case == 'matching' else 101), result.stderr
            commands = [json.loads(line)['args'] for line in log.read_text(encoding='utf-8').splitlines()]
            reached = entries if case == 'matching' else entries[:1]
            assert commands == [['fetch', '--locked', '--manifest-path', entry] for entry in reached]
            assert [line for line in result.stderr.splitlines() if line.startswith('Fetching locked dependencies: ')] == [
                'Fetching locked dependencies: ' + entry for entry in reached]
            assert hashes() == before
            if case != 'matching':
                assert '--locked' in result.stderr and 'lock' in result.stderr.lower(), result.stderr
            if case == 'missing':
                assert not (directory / 'first missing/Cargo.lock').exists()
            print(f'PASS locked manifests {case}: exit {result.returncode}; source/lock SHA-256 {before}', flush=True)

locked_manifests_fail_without_rewriting_inputs()

def independent_manifest_fixture(directory):
    import shutil
    shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
    source = directory / 'acquisition-source'
    (source / 'src').mkdir(parents=True)
    (source / 'Cargo.toml').write_text('[package]\nname="locked-dependency"\nversion="0.1.0"\nedition="2021"\n', encoding='utf-8')
    (source / 'src/lib.rs').write_text('pub fn value() -> u32 { 42 }\n', encoding='utf-8')
    subprocess.run(['git', 'init', '--quiet', str(source)], check=True)
    subprocess.run(['git', '-C', str(source), 'add', '.'], check=True)
    subprocess.run(['git', '-C', str(source), '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.com',
                    '-c', 'commit.gpgsign=false', 'commit', '-qm', 'dependency'], check=True)
    revision = subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
    manifest = 'fixtures/author build/Cargo.toml'
    nested = directory / manifest
    (nested.parent / 'src').mkdir(parents=True)
    nested.write_text('[workspace]\n[package]\nname="offline-consumer"\nversion="0.1.0"\nedition="2021"\n'
                      '[dependencies]\nlocked-dependency={git=' + json.dumps(source.as_uri()) + ',rev=' + json.dumps(revision) + '}\n', encoding='utf-8')
    (nested.parent / 'src/lib.rs').write_text('pub fn value() -> u32 { locked_dependency::value() }\n', encoding='utf-8')
    prepare = directory / 'prepare-cache'
    prepare.mkdir()
    result = subprocess.run(['cargo', 'generate-lockfile', '--manifest-path', manifest], cwd=directory,
                            env=os.environ | {'CARGO_HOME': str(prepare)}, text=True, capture_output=True)
    assert result.returncode == 0, result.stderr
    shutil.rmtree(prepare)
    return manifest, source

def locked_manifests_enable_cold_offline_build():
    import hashlib
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        manifest, source = independent_manifest_fixture(directory)
        inputs = [directory / name for name in ['Cargo.toml', 'Cargo.lock', manifest,
                                               'fixtures/author build/Cargo.lock']]
        def hashes():
            return {str(path.relative_to(directory)): hashlib.sha256(path.read_bytes()).hexdigest() for path in inputs}
        before = hashes()
        acquisition = directory / 'acquired-cache'
        acquisition.mkdir()
        env = os.environ | {'CARGO_HOME': str(acquisition), 'CARGO_TARGET_DIR': str(directory / 'acquisition-target')}
        def cargo(*args, environment=env):
            return subprocess.run(['cargo', *args], cwd=directory, env=environment, capture_output=True, text=True)
        result = cargo('fetch', '--locked')
        assert result.returncode == 0, result.stderr
        result = cargo('build', '--offline', '--locked', '--manifest-path', manifest)
        assert result.returncode == 101 and 'locked-dependency' in result.stderr, result.stderr
        print('PASS root-only provisioning: offline build exit 101', flush=True)
        result = execute('Fetch additional locked manifests', directory, env | {'ADDITIONAL_LOCKED_MANIFESTS': json.dumps([manifest])})
        assert result.returncode == 0, result.stderr
        vendor = directory / 'vendor'
        result = cargo('vendor', '--offline', '--locked', '--manifest-path', manifest, str(vendor))
        assert result.returncode == 0, result.stderr
        configuration = result.stdout
        assert 'directory =' in configuration
        source.rename(directory / 'unavailable-source')
        cold = directory / 'cold-cargo-home'
        cold.mkdir()
        assert not list(cold.iterdir()) and not (directory / 'cold-target').exists()
        # Cargo's emitted source replacement is configuration, not a cached package.
        (cold / 'config.toml').write_text(configuration, encoding='utf-8')
        result = cargo('build', '--offline', '--locked', '--manifest-path', manifest,
                       environment=env | {'CARGO_HOME': str(cold), 'CARGO_TARGET_DIR': str(directory / 'cold-target')})
        assert result.returncode == 0 and 'offline-consumer' in result.stderr, result.stderr
        assert not (cold / 'git').exists() and not (cold / 'registry').exists()
        assert hashes() == before
        print(f'PASS cold vendored offline build: exit {result.returncode}; source/lock SHA-256 {before}', flush=True)

locked_manifests_enable_cold_offline_build()

def locked_manifests_precede_all_test_routes():
    import shutil
    tests = workflow.split('  tests:\n', 1)[1].split('\n  tests-result:', 1)[0]
    fetch = next(s for s in tests.split('      - name: ')[1:] if s.startswith('Fetch additional locked manifests\n'))
    assert tests.index('name: Fetch additional locked manifests') < tests.index('name: Tests\n') < tests.index('name: Isolated tests\n')
    assert fetch.split('        run:', 1)[0].count('if:') == 1
    assert "if: needs.declaration.outputs.additional_locked_manifests != '[]'" in fetch
    assert 'ADDITIONAL_LOCKED_MANIFESTS: $' + '{{ needs.declaration.outputs.additional_locked_manifests }}' in tests
    declaration_job = workflow.split('  declaration:\n', 1)[1].split('\n  quality:', 1)[0]
    assert 'additional_locked_manifests: $' + '{{ steps.config.outputs.additional_locked_manifests }}' in declaration_job
    assert "fromJSON(inputs.warm-cache && '[\"warm\"]' || needs.declaration.outputs.test_shards" in tests
    for name in ['Tests', 'Isolated tests']:
        assert 'cargo fetch --locked\n' in body(name)
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        manifest, _ = independent_manifest_fixture(directory)
        tools, log = install_recording_runner(directory, shutil.which('cargo'), manifest)
        original_home = directory / 'ambient-home'
        original_home.mkdir()
        routes = [('Tests', shard, 'false', None, ['sample']) for shard in range(1, 17)]
        routes += [('Isolated tests', 1, 'false', None, ['sample']),
                   ('Isolated tests', 1, 'true', None, ['sample']),
                   ('Isolated tests', 2, 'true', None, ['sample']),
                   ('Isolated tests', '', 'true', '80', ['sample']),
                   ('Tests', 4, 'false', None, []), ('Isolated tests', 4, 'true', None, []),
                   ('Tests', 'doctests', 'true', None, ['sample']),
                   ('Isolated tests', 'doctests', 'true', None, ['sample'])]
        for index, (route, shard, coverage, threshold, packages) in enumerate(routes):
            acquisition = directory / f'cache-{index}'
            acquisition.mkdir()
            env = {'PATH': str(tools) + os.pathsep + os.environ['PATH'], 'TOOLS': str(tools),
                   'ADDITIONAL_LOCKED_MANIFESTS': json.dumps([manifest]), 'HOME': str(original_home),
                   'CARGO_HOME': str(acquisition), 'RUSTUP_HOME': os.environ.get('RUSTUP_HOME', str(pathlib.Path.home() / '.rustup')),
                   'RUNNER_TEMP': temporary, 'SHARD': str(shard), 'COLLECT_COVERAGE': coverage,
                   'TEST_PACKAGES': json.dumps(packages), 'CARGO_COVERAGE_MIN_LINES': threshold or '',
                   'UNRELATED_AMBIENT_CANARY': 'must-not-inherit'}
            if log.exists():
                log.unlink()
            result = execute('Fetch additional locked manifests', directory, env)
            assert result.returncode == 0, result.stderr
            result = execute(route, directory, env)
            assert result.returncode == 0, result.stdout + result.stderr
            commands = [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()]
            assert [c['args'] for c in commands[:2]] == [['fetch', '--locked', '--manifest-path', manifest], ['fetch', '--locked']]
            runners = commands[2:]
            if not packages:
                assert not runners
                continue
            assert runners and all(c['cargo_home'] == str(acquisition) for c in runners)
            if route == 'Isolated tests':
                assert all(c['home'] != str(original_home) and c['ambient'] is None for c in runners)
            else:
                assert all(c['home'] == str(original_home) for c in runners)
            if shard == 'doctests':
                expected = ['test', '--locked', '--doc', '-p', 'sample']
            elif threshold:
                expected = ['llvm-cov', 'nextest', '--user-config-file', 'none', '--ignore-default-filter',
                            '--retries', '2', '--flaky-result', 'pass', '--final-status-level', 'flaky',
                            '--partition', 'count:1/1', '--no-tests', 'pass', '-p', 'sample', '--locked'] + coverage_report_args + ['--lcov', '--output-path', str(directory / 'target/single-coverage/shard-1.lcov')]
            elif route == 'Isolated tests' and coverage == 'true':
                expected = ['llvm-cov', 'nextest', '--user-config-file', 'none', '--ignore-default-filter',
                            '--retries', '2', '--flaky-result', 'pass', '--final-status-level', 'flaky',
                            '--partition', f'count:{shard}/16', '--no-tests', 'pass', '-p', 'sample', '--locked'] + coverage_report_args + ['--lcov', '--output-path', str(directory / f'shard-{shard}.lcov')]
            else:
                expected = ['nextest', 'run', '--user-config-file', 'none', '--ignore-default-filter', '--locked',
                            '-p', 'sample', '--retries', '2', '--flaky-result', 'pass', '--final-status-level', 'flaky']
                if route == 'Tests':
                    expected += ['--partition', f'count:{shard}/16', '--no-tests', 'pass']
                else:
                    expected += ['--no-tests', 'pass', '--partition', f'count:{shard}/16']
            assert runners[0]['args'] == expected, runners[0]['args']
            assert [c['args'] for c in runners[1:]] == ([['test', '--locked', '--doc', '-p', 'sample']] if shard == 1 and not threshold and coverage == 'false' else [])
            print(f'PASS provisioning route {index}: {route}, shard {shard}, coverage {coverage}, threshold {threshold}', flush=True)

locked_manifests_precede_all_test_routes()

def caller_tools_precede_compiling_operations():
    jobs = dict(re.findall(r'^  ([\w-]+):\n(.*?)(?=^  [\w-]+:\n|\Z)',
                           workflow.split('\njobs:\n', 1)[1], re.MULTILINE | re.DOTALL))
    for job, consumer in [('quality', 'Lint'), ('docs', 'Docs'), ('test-build', 'Build test archive'), ('test-prepare', 'Prepare test setup'), ('tests', 'Isolated tests'),
                          ('targets', 'Check target'), ('browser', 'Build wasm crates')]:
        section = jobs[job]
        assert ('needs: [declaration, test-build, test-prepare]' if job == 'tests' else 'needs: declaration') in section, job
        bootstrap = next(s for s in section.split('      - name: ')[1:] if 'run: *mise-bootstrap' in s or 'run: &mise-bootstrap' in s)
        install = next(s for s in section.split('      - name: ')[1:] if 'run: *caller-tools' in s or 'run: &caller-tools' in s)
        assert section.index(bootstrap) < section.index(install) < section.index('- name: ' + consumer), job
        condition = "needs.declaration.outputs.test_tools == 'true'"
        if job == 'browser':
            condition += " || needs.declaration.outputs.browser_build == 'true'"
        for step in [bootstrap, install]:
            assert 'if: ' + condition + '\n' in step, job
            assert 'timeout-minutes: 10' in step, job
        assert section.count('mise-bootstrap') == 1 and section.count('caller-tools') == 1, job
    for job in ['declaration', 'checks', 'advisories', 'tests-result']:
        assert 'mise-bootstrap' not in jobs[job] and 'caller-tools' not in jobs[job], job
    assert workflow.count('mise install') == 1
    assert body('Test tools') == body('Browser tools')
    assert 'tools.just' not in body('Test tools')
    assert "needs.declaration.outputs.browser_build == 'true'" in jobs['browser'].split('- name: Browser tool pin', 1)[1].split('- name:', 1)[0]

caller_tools_precede_compiling_operations()
print('PASS caller_tools_precede_compiling_operations')

def caller_build_tools_reach_compilers():
    import shutil
    import time
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/test-tools', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        assert (directory / 'build.rs').is_file(), 'compiler fixture needs a build script'
        paths = directory / 'paths'
        env = {'RUNNER_TEMP': temporary, 'GITHUB_PATH': str(paths)}
        result = execute('Test tools', directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        bins = paths.read_text(encoding='utf-8').splitlines()
        env['PATH'] = os.pathsep.join(bins + [os.environ['PATH']])
        routes = [('Check target', {'TARGET': os.environ['TARGET']})] if 'TARGET' in os.environ else [
            ('Lint', {}), ('Docs', {'RUSTDOCFLAGS': '-D warnings -D missing_docs'}),
            ('Build wasm crates', {'WASM_CRATES': '["test-tools"]'}), ('Isolated tests', {}), ('Isolated tests', {'SHARD': 'doctests', 'COLLECT_COVERAGE': 'true'})]
        for index, (route, extra) in enumerate(routes):
            child = env | extra | {'CARGO_TARGET_DIR': str(directory / f'cold-{index}')}
            if extra.get('SHARD') == 'doctests':
                # Isolation excludes CARGO_TARGET_DIR, so remove the earlier route's artifacts.
                shutil.rmtree(directory / 'target', ignore_errors=True)
            start = time.monotonic()
            result = execute(route, directory, child)
            assert result.returncode == 0, result.stdout + result.stderr
            assert 'Compiling test-tools' in result.stdout + result.stderr or 'Checking test-tools' in result.stdout + result.stderr, result.stdout + result.stderr
            if route == 'Isolated tests' and not extra:
                assert 'caller_pinned_tools_are_available' in result.stdout + result.stderr
            print(f'PASS caller compiler {route}: {time.monotonic() - start:.2f}s; jobs={os.environ.get("CARGO_BUILD_JOBS", "runner default")}; '
                  f'lock wait reported={"Blocking waiting" in result.stderr}', flush=True)

caller_build_tools_reach_compilers()
print('PASS caller_build_tools_reach_compilers')

def caller_build_tools_require_pins():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/test-tools', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        paths = directory / 'paths'
        env = {'RUNNER_TEMP': temporary, 'GITHUB_PATH': str(paths)}
        (directory / 'mise.toml').unlink()
        result = execute('Test tools', directory, env)
        assert result.returncode != 0, result.stdout + result.stderr
        assert 'mise.toml: caller tools require a pinned tool configuration' in result.stderr, result.stderr
        (directory / 'mise.toml').write_text('[tools]\n', encoding='utf-8')
        result = execute('Test tools', directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        # Compile without any ambient developer tools, retaining only Rust proxies and host utilities.
        proxies = directory / 'rust-bin'
        proxies.mkdir()
        (proxies / 'rustup').symlink_to(shutil.which('rustup'))
        for tool in ['cargo', 'rustc', 'rustdoc']:
            physical = subprocess.check_output(['rustup', 'which', tool], text=True).strip()
            (proxies / tool).symlink_to(physical)
        child = env | {'PATH': f'{proxies}:/usr/bin:/bin', 'CARGO_TARGET_DIR': str(directory / 'cold-unavailable')}
        result = execute('Docs', directory, child)
        assert result.returncode != 0 and 'caller build tool just is unavailable:' in result.stderr, result.stderr
        result = execute('Build wasm crates', directory, child | {'WASM_CRATES': '["test-tools"]',
                         'CARGO_TARGET_DIR': str(directory / 'cold-unavailable-wasm')})
        assert result.returncode != 0 and 'caller build tool just is unavailable:' in result.stderr, result.stderr
        # This finite child exits once; it never re-executes a shell.
        missing = proxies / 'just'
        subprocess.run(['install', '-m', '0700', '/dev/stdin', str(missing)],
                       input='#!/bin/sh\nexit 127\n', text=True, check=True)
        result = execute('Docs', directory, child | {'CARGO_TARGET_DIR': str(directory / 'cold-failed')})
        assert result.returncode != 0 and 'caller build tool just --version failed' in result.stderr, result.stderr
        (directory / 'mise.toml').write_text('[tools]\njust="not-a-real-version"\n', encoding='utf-8')
        result = execute('Test tools', directory, env)
        assert result.returncode != 0 and 'not-a-real-version' in result.stderr, result.stderr
        (directory / 'mise.toml').write_text('[tools]\njust="1.58.0"\n', encoding='utf-8')
        result = execute('Test tools', directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        bins = paths.read_text(encoding='utf-8').splitlines()
        # GitHub prepends published paths even if a caller path was already present farther back.
        for inherited in [child['PATH'], child['PATH'] + ':' + ':'.join(bins)]:
            result = execute('Docs', directory, child | {'PATH': ':'.join(bins) + ':' + inherited,
                             'CARGO_TARGET_DIR': str(directory / ('cold-restored-' + str(len(inherited))))})
            assert result.returncode == 0, result.stdout + result.stderr
        outputs = list(directory.glob('cold-restored-*/debug/build/test-tools-*/output'))
        assert outputs and all('cargo::rerun-if-changed=mise.toml' in output.read_text(encoding='utf-8') for output in outputs)

caller_build_tools_require_pins()
print('PASS caller_build_tools_require_pins')

def mise_bootstrap_uses_runner_platform():
    import hashlib
    import platform
    import shutil
    host = (platform.system(), platform.machine().lower())
    expected = {
        ('Linux', 'x86_64'): 'linux-x64', ('Linux', 'aarch64'): 'linux-arm64',
        ('Darwin', 'arm64'): 'macos-arm64', ('Darwin', 'x86_64'): 'macos-x64',
        ('Windows', 'amd64'): 'windows-x64.exe',
    }[host]
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        paths = directory / 'paths'
        env = {'RUNNER_TEMP': temporary, 'GITHUB_PATH': str(paths)}
        # Keep the host utilities, but not an ambient mise installation.
        env['PATH'] = os.pathsep.join(p for p in os.environ['PATH'].split(os.pathsep)
                                    if not any((pathlib.Path(p) / name).is_file() for name in ['mise', 'mise.exe']))
        result = execute('Mise bootstrap', directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        published = paths.read_text(encoding='utf-8').splitlines()
        assert len(published) == 1
        binary = pathlib.Path(published[0]) / ('mise.exe' if os.name == 'nt' else 'mise')
        assert binary.is_file(), published
        digest = hashlib.sha256(binary.read_bytes()).hexdigest()
        assert f'platform={expected} ' in body('Mise bootstrap')
        assert f'digest={digest} ;;' in body('Mise bootstrap')
        version = subprocess.check_output([str(binary), '--version'], text=True).split()[0]
        assert version == '2026.10.2'
        clean_path = env['PATH']
        env['PATH'] = os.pathsep.join(published + [clean_path])
        before = paths.read_text(encoding='utf-8')
        result = execute('Mise bootstrap', directory, env)
        assert result.returncode == 0 and paths.read_text(encoding='utf-8') == before, result.stderr
        (directory / 'mise.toml').write_text('[tools]\njust="1.58.0"\n', encoding='utf-8')
        result = execute('Test tools', directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        bins = paths.read_text(encoding='utf-8').splitlines()
        tool = shutil.which('just', path=os.pathsep.join(bins + [env['PATH']]))
        assert tool is not None
        command = [tool, '--version']
        result = subprocess.run(command, env=os.environ | env | {'PATH': os.pathsep.join(bins + [env['PATH']])},
                                capture_output=True, text=True, check=True)
        assert result.stdout.strip() == 'just 1.58.0', result.stderr
        fake = directory / 'fake'
        fake.mkdir()
        # A child closes the executable's write descriptor before Bash can launch it.
        import sys
        def executable(name, contents):
            subprocess.run([sys.executable, '-c',
                            'import pathlib,sys; p=pathlib.Path(sys.argv[1]); p.write_text(sys.argv[2], encoding="utf-8", newline="\\n"); p.chmod(0o755)',
                            str(fake / name), contents], check=True)
        executable('mise', '#!/bin/sh\necho 0.0.0\n')
        wrong = env | {'PATH': os.pathsep.join([str(fake), clean_path])}
        result = execute('Mise bootstrap', directory, wrong)
        assert result.returncode == 0, result.stdout + result.stderr
        before = paths.read_text(encoding='utf-8')
        executable('curl', '#!/bin/sh\ntouch corrupt-download-marker\nwhile [ "$1" != --output ]; do shift; done\n'
                   'printf "#!/bin/sh\\ntouch checksum-bypass-marker\\n" > "$2"\n')
        previous = set(directory.glob('maestro-mise.*/*'))
        result = execute('Mise bootstrap', directory, wrong)
        assert (directory / 'corrupt-download-marker').is_file(), result.stdout + result.stderr
        assert result.returncode != 0, result.stdout + result.stderr
        downloads = set(directory.glob('maestro-mise.*/*')) - previous
        assert len(downloads) == 1 and next(iter(downloads)).read_bytes() == b'#!/bin/sh\ntouch checksum-bypass-marker\n'
        assert not (directory / 'checksum-bypass-marker').exists()
        assert paths.read_text(encoding='utf-8') == before
        print(f'PASS native bootstrap {expected}: {version}; installed just 1.58.0', flush=True)

mise_bootstrap_uses_runner_platform()
print('PASS mise_bootstrap_uses_runner_platform')

def test_tools_defaults_preserve_behavior():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        result, values = declaration(directory)
        assert result.returncode == 0, result.stderr
        assert values['test_tools'] == 'false'
        (directory / '.github').mkdir()
        path = directory / '.github/ci.toml'
        for flag in ['', '\ntest_tools=false']:
            path.write_text('wasm_crates=[]\ncheck_targets=[]' + flag, encoding="utf-8")
            result, values = declaration(directory)
            assert result.returncode == 0 and values['test_tools'] == 'false', result.stderr
        for flag in ['1', '"true"', '[]']:
            path.write_text('wasm_crates=[]\ncheck_targets=[]\ntest_tools=' + flag, encoding="utf-8")
            result, values = declaration(directory)
            assert result.returncode != 0 and 'test_tools must be a boolean' in result.stderr, result.stdout + result.stderr
        assert workflow.count('digest=8f5f6660') == 1
        browser = workflow.split('  browser:\n', 1)[1]
        assert "if: needs.declaration.outputs.test_tools == 'true' || needs.declaration.outputs.browser_build == 'true'\n        run: *mise-bootstrap" in browser
        tests = workflow.split('  tests:\n', 1)[1].split('  targets:', 1)[0]
        for name in ['Mise bootstrap', 'Test tools']:
            step = next(s for s in tests.split('      - name: ')[1:] if s.splitlines()[0] == name)
            assert "if: needs.declaration.outputs.test_tools == 'true'" in step

def reserved_ports_are_validated_and_applied_only_when_set():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        (directory / '.github').mkdir()
        path = directory / '.github/ci.toml'
        path.write_text('wasm_crates=[]\ncheck_targets=[]', encoding="utf-8")
        result, values = declaration(directory)
        assert result.returncode == 0 and values['reserved_ports'] == '', result.stderr
        path.write_text('wasm_crates=[]\ncheck_targets=[]\nreserved_ports=[53692, 1455]', encoding="utf-8")
        result, values = declaration(directory)
        assert result.returncode == 0 and values['reserved_ports'] == '53692,1455', result.stderr
        for bad in ['[0]', '[65536]', '[-1]', '["80"]', '[true]', '[1.5]', '80', '[1455, 1455]']:
            path.write_text('wasm_crates=[]\ncheck_targets=[]\nreserved_ports=' + bad, encoding="utf-8")
            result, values = declaration(directory)
            assert result.returncode != 0 and 'reserved_ports' in result.stderr, bad + result.stderr
    tests = workflow.split('  tests:\n', 1)[1].split('  targets:', 1)[0]
    step = next(s for s in tests.split('      - name: ')[1:] if s.splitlines()[0] == 'Reserved ports')
    assert "if: runner.environment == 'github-hosted' && needs.declaration.outputs.reserved_ports != ''" in step
    assert 'sudo sysctl -w net.ipv4.ip_local_reserved_ports="$RESERVED_PORTS"' in step
    assert tests.index('- name: Reserved ports') < tests.index('- name: Isolated tests')

reserved_ports_are_validated_and_applied_only_when_set()
print('PASS reserved_ports_are_validated_and_applied_only_when_set')

test_tools_defaults_preserve_behavior()
print('PASS test_tools_defaults_preserve_behavior')

def test_tools_are_opt_in():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/test-tools', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        # Keep rustup's bin directory present so its proxy does not prepend it.
        proxies = directory / 'rust-bin'
        proxies.mkdir()
        for tool in ['cargo', 'rustc', 'rustdoc']:
            (proxies / tool).symlink_to(shutil.which(tool))
        for tool in ['just', 'prek']:
            missing = proxies / tool
            missing.write_text('#!/bin/sh\nexit 127\n', encoding="utf-8")
            missing.chmod(0o755)
        env = {'RUNNER_TEMP': temporary, 'GITHUB_PATH': str(directory / 'paths'),
               'PATH': f'{proxies}:/usr/bin:/bin:' + str(pathlib.Path.home() / '.cargo/bin')}
        path = directory / '.github/ci.toml'
        for flag in ['', '\ntest_tools=false']:
            path.write_text('wasm_crates=[]\ncheck_targets=[]' + flag, encoding="utf-8")
            result, values = declaration(directory)
            assert result.returncode == 0 and values['test_tools'] == 'false', result.stderr
            result = execute('Isolated tests', directory, env)
            assert result.returncode != 0 and 'caller build tool just --version failed' in result.stdout + result.stderr, result.stderr
        path.write_text('wasm_crates=[]\ncheck_targets=[]\ntest_tools=true', encoding="utf-8")
        result, values = declaration(directory)
        assert result.returncode == 0 and values['test_tools'] == 'true', result.stderr
        result = execute('Test tools', directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        env['PATH'] = ':'.join((directory / 'paths').read_text(encoding="utf-8").splitlines()) + ':' + env['PATH']
        result = execute('Isolated tests', directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        assert 'caller_pinned_tools_are_available' in result.stdout + result.stderr

test_tools_are_opt_in()
print('PASS test_tools_are_opt_in')

def test_tools_follow_only_caller_pins():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/test-tools', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        conflict = directory / 'conflict'
        conflict.mkdir()
        for tool in ['just', 'prek']:
            binary = conflict / tool
            binary.write_text('#!/bin/sh\necho conflicting-tool\nexit 29\n', encoding="utf-8")
            binary.chmod(0o755)
        env = {'RUNNER_TEMP': temporary, 'GITHUB_PATH': str(directory / 'paths'),
               'PATH': f'{conflict}:' + os.environ['PATH']}
        for just, prek in [('1.58.0', '0.5.3'), ('1.40.0', '0.5.0')]:
            (directory / 'paths').write_text('', encoding="utf-8")
            (directory / 'mise.toml').write_text(f'[tools]\njust="{just}"\nprek="{prek}"\n', encoding="utf-8")
            source = (root / 'fixtures/test-tools/src/lib.rs').read_text(encoding="utf-8")
            (directory / 'src/lib.rs').write_text(source.replace('1.58.0', just).replace('0.5.3', prek), encoding="utf-8")
            result = execute('Test tools', directory, env)
            assert result.returncode == 0, result.stdout + result.stderr
            child = env | {'PATH': ':'.join((directory / 'paths').read_text(encoding="utf-8").splitlines()) + ':' + env['PATH']}
            result = execute('Isolated tests', directory, child)
            assert result.returncode == 0, result.stdout + result.stderr
        (directory / 'mise.toml').unlink()
        result = execute('Test tools', directory, env)
        assert result.returncode != 0, result.stdout + result.stderr
        (directory / 'mise.toml').write_text('[tools]\njust="not-a-real-version"\n', encoding="utf-8")
        result = execute('Test tools', directory, env)
        assert result.returncode != 0, result.stdout + result.stderr

test_tools_follow_only_caller_pins()
print('PASS test_tools_follow_only_caller_pins')

def event_policy_keeps_pull_requests_fast():
    document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
    slow = {'test-build', 'test-prepare', 'tests', 'doctests', 'browser', 'browser-tests'}
    for event in ['pull_request', 'merge_group', 'push']:
        for warm, advisory, fuzz in [(False, False, False), (True, False, False),
                                      (False, True, False), (False, False, True)]:
            with tempfile.TemporaryDirectory() as temporary:
                output = pathlib.Path(temporary) / 'selection'
                result = select_packages(root / 'fixtures/sample', {
                    'TEST_EVENT': 'push' if warm else event, 'BASE_REF': '', 'GITHUB_OUTPUT': str(output)})
                assert result.returncode == 0, result.stderr
                values = dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())
            for name in slow | {'targets', 'quality', 'docs', 'checks'}:
                context = {'cancelled()': False, 'needs.test-build.result': 'success', 'needs.test-prepare.result': 'success',
                           'needs.declaration.outputs.test_prepare_inputs': '[]',
                           'needs.declaration.outputs.test_prepare': json.dumps('echo ready'), 'inputs.warm-cache': warm, 'inputs.advisory-scan': advisory,
                           'inputs.fuzz-scan': fuzz, 'github.event_name': event,
                           default_ref: 'refs/heads/main', 'github.ref': 'refs/heads/main',
                           'github.repository_id': 'caller', 'inputs.working-directory': '.',
                           'needs.declaration.outputs.coverage': values['coverage'],
                           'needs.declaration.outputs.included_tests': values['included_tests'],
                           'needs.declaration.outputs.targets': '[{}]',
                           'needs.declaration.outputs.wasm_crates': '["sample"]',
                           'needs.declaration.outputs.browser_build': 'true',
                           'needs.declaration.outputs.browser_tests': 'true'}
                expected = not advisory and not fuzz
                if name in slow:
                    expected &= event != 'pull_request' or warm
                    expected &= not warm or name == 'tests' or (event == 'push' and name in ('doctests', 'browser', 'browser-tests'))
                elif name == 'targets':
                    expected &= not warm or event == 'push'
                else:
                    expected &= not warm or (event == 'push' and name in ('quality', 'docs'))
                assert bool(evaluate(document['jobs'][name]['if'], context)) == expected, (name, event, warm, advisory, fuzz)

event_policy_keeps_pull_requests_fast()
print('PASS event_policy_keeps_pull_requests_fast')

def windows_checks_run_on_every_pull_request_push():
    import shutil
    document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
    declaration_steps = document['jobs']['declaration']['steps']
    config = next(step for step in declaration_steps if step.get('id') == 'config')
    selection = next(step for step in declaration_steps if step.get('id') == 'selection')
    assert document['jobs']['targets']['needs'] == 'declaration', 'Windows must not wait for other PR jobs'
    declared = [
        {'target': 'aarch64-unknown-linux-gnu', 'runner': 'ubuntu-24.04-arm'},
        {'target': 'aarch64-apple-darwin', 'runner': 'macos-latest'},
        {'target': 'x86_64-apple-darwin', 'runner': 'macos-latest'},
        {'target': 'x86_64-pc-windows-msvc', 'runner': 'windows-latest'},
    ]
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        (directory / '.github').mkdir()
        path = directory / '.github/ci.toml'
        for event in ['pull_request', 'merge_group', 'push']:
            for targets, pr_targets in [([], []), (declared, [declared[-1]]),
                                        (declared[:-1], []), (declared[-1:], [declared[-1]])]:
                path.write_text('wasm_crates=[]\ncheck_targets=' + json.dumps([target['target'] for target in targets]), encoding='utf-8')
                output = directory / 'output'
                output.write_text('', encoding='utf-8')
                result = execute('Read caller declaration', directory, {'TEST_EVENT': event, 'GITHUB_OUTPUT': str(output)})
                assert result.returncode == 0, result.stderr
                values = dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())
                expected = pr_targets if event == 'pull_request' else targets
                assert json.loads(values['targets']) == expected, (event, targets, values['targets'])
        path.write_text('wasm_crates=[]\ncheck_targets=["unsupported-target"]', encoding='utf-8')
        result = execute('Read caller declaration', directory, {'TEST_EVENT': 'pull_request', 'GITHUB_OUTPUT': str(output)})
        assert result.returncode != 0 and 'unknown target' in result.stderr, result.stderr
    assert config['env']['TEST_EVENT'] == selection['env']['TEST_EVENT']

windows_checks_run_on_every_pull_request_push()
print('PASS windows_checks_run_on_every_pull_request_push')

def removed_shard_lists_fail_clearly():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        (directory / '.github').mkdir()
        (directory / '.github/ci.toml').write_text(
            'wasm_crates=[]\ncheck_targets=[]\ntest_shards=[["sample"],[],[],[]]\n', encoding='utf-8')
        result, values = declaration(directory)
        assert result.returncode != 0 and not values and 'test_shards was removed' in result.stderr, result.stdout + result.stderr
    aggregate = workflow.split('  tests-result:\n', 1)[1].split('  checks:', 1)[0]
    assert 'name: Tests (ubuntu-latest)' in aggregate
    assert 'needs: [tests, doctests, declaration, test-build]' in aggregate and 'if: $' + '{{ always()' in aggregate
    for coverage in ['false', 'true', '']:
        for declaration_status in ['success', 'failure', 'cancelled', 'skipped']:
            for status in ['success', 'failure', 'cancelled', 'skipped']:
                result = subprocess.run(['python3', '-c', body('All test shards passed')],
                                        env=os.environ | {'RESULT': status, 'DOCTEST_RESULT': status, 'COLLECT_COVERAGE': coverage,
                                                          'DECLARATION_RESULT': declaration_status},
                                        capture_output=True, text=True)
                expected = declaration_status == 'success' and (
                    (coverage == 'false' and status == 'skipped') or
                    (coverage == 'true' and status == 'success'))
                assert (result.returncode == 0) == expected, f'{coverage} {declaration_status} {status}\n' + result.stdout + result.stderr

removed_shard_lists_fail_clearly()
print('PASS removed_shard_lists_fail_clearly')

def partitions_run_every_test_once_and_doctests():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        (directory / 'Cargo.toml').write_text(
            '[workspace]\nresolver="3"\nmembers=["a","b","c","d","e"]\n', encoding='utf-8')
        for name in 'abcde':
            crate = directory / name
            (crate / 'src').mkdir(parents=True)
            (crate / 'tests').mkdir()
            (crate / 'Cargo.toml').write_text(
                f'[package]\nname="{name}"\nversion="0.1.0"\nedition="2024"\n', encoding='utf-8')
            (crate / 'src/lib.rs').write_text(
                f'//! ```\n//! assert_eq!({name}::value(), 1);\n//! ```\n'
                'pub fn value() -> u8 { 1 }\n' + ''.join(
                    f'#[test] fn unit_{name}_{index}() {{ assert_eq!(value(), 1); }}\n'
                    for index in range(16)), encoding='utf-8')
            (crate / 'tests/proof.rs').write_text(''.join(
                f'#[test] fn integration_{name}_{index}() {{ assert_eq!({name}::value(), 1); }}\n'
                for index in range(16)), encoding='utf-8')
        subprocess.run(['cargo', 'generate-lockfile'], cwd=directory, check=True)
        (directory / '.config').mkdir()
        (directory / '.config/nextest.toml').write_text('[profile.default]\ndefault-filter="none()"\n', encoding='utf-8')
        for coverage in ['false', 'true']:
            outputs = []
            for shard in range(1, 17):
                result = execute('Tests', directory, {'SHARD': str(shard), 'COLLECT_COVERAGE': coverage})
                assert result.returncode == 0, result.stdout + result.stderr
                assert 'PASS' in result.stdout + result.stderr, shard
                outputs.append(result.stdout + result.stderr)
            combined = ''.join(outputs)
            for name in 'abcde':
                for kind in ['unit', 'integration']:
                    for index in range(16):
                        identity = f'{kind}_{name}_{index}'
                        assert len(re.findall(r'PASS[^\n]*\b' + identity + r'\b', combined)) == 1, combined
            assert combined.count('src/lib.rs - (line 1) ... ok') == (5 if coverage == 'false' else 0)
            if coverage == 'true':
                result = execute('Tests', directory, {'SHARD': 'doctests', 'COLLECT_COVERAGE': coverage})
                assert result.returncode == 0, result.stdout + result.stderr
                assert (result.stdout + result.stderr).count('src/lib.rs - (line 1) ... ok') == 5
        # A failed test must fail exactly its partition, not disappear.
        (directory / 'c/tests/proof.rs').write_text(
            '#[test] fn failed() { panic!("shard failure"); }\n', encoding='utf-8')
        failures = []
        for shard in range(1, 17):
            result = execute('Tests', directory, {'SHARD': str(shard)})
            if result.returncode:
                failures.append(result.stdout + result.stderr)
        assert len(failures) == 1 and 'shard failure' in failures[0], failures

partitions_run_every_test_once_and_doctests()
print('PASS partitions_run_every_test_once_and_doctests')

def local_callers_grant_baseline_artifact_reads():
    for path in sorted((root / '.github/workflows').glob('*.yml')):
        document = json.loads(subprocess.check_output(
            ['yq', '-o=json', '-'], input=path.read_text(encoding='utf-8'), text=True))
        for name, job in document.get('jobs', {}).items():
            if job.get('uses') == './.github/workflows/ci.yml':
                assert job.get('permissions', {}).get('actions') == 'read', (path.name, name)
        assert 'actions' not in document.get('permissions', {}), path.name

local_callers_grant_baseline_artifact_reads()
print('PASS local_callers_grant_baseline_artifact_reads')

def archive_checkout_root_is_the_coverage_source_root():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        output = directory / 'output'
        result = execute('Record archive checkout root', directory, {'GITHUB_OUTPUT': str(output)})
        assert result.returncode == 0, result.stdout + result.stderr
        assert output.read_text(encoding='utf-8').strip() == 'path=' + str(directory)
    build = workflow.split('\n  test-build:\n', 1)[1].split('\n  test-prepare:', 1)[0]
    assert 'coverage_checkout_root: $' + '{{ steps.archive-root.outputs.path }}' in build
    aggregate = workflow.split('\n  tests-result:\n', 1)[1].split('\n  checks:', 1)[0]
    assert 'needs: [tests, doctests, declaration, test-build]' in aggregate
    assert 'COVERAGE_CHECKOUT_ROOT: $' + '{{ needs.test-build.outputs.coverage_checkout_root }}' in aggregate

archive_checkout_root_is_the_coverage_source_root()
print('PASS archive_checkout_root_is_the_coverage_source_root')

def coverage_baseline_transfer_requires_coverage():
    for name, owner in [('Upload coverage baseline', 'steps.selection'),
                        ('Download coverage baseline', 'needs.declaration')]:
        step = next(part for part in workflow.split('      - name: ')[1:] if part.startswith(name + '\n'))
        condition = re.search(r'^        if: (.*)$', step, re.MULTILINE)[1]
        assert owner + ".outputs.coverage == 'true'" in condition, (name, condition)

coverage_baseline_transfer_requires_coverage()
print('PASS coverage_baseline_transfer_requires_coverage')

def coverage_baseline_download_requires_complete_manifest():
    import zipfile
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        fixture_git(directory, 'init', '--quiet')
        fixture_git(directory, 'commit', '--allow-empty', '-qm', 'baseline')
        head = fixture_git(directory, 'rev-parse', 'HEAD')
        manifest = {'head': head, 'checkout_root': '/old/caller', 'package_roots': {'a': 'a'},
                    'shard_count': 1, 'artifact_prefix': 'coverage-root-'}
        for name, records in [('manifest', {'manifest.json': json.dumps(manifest)}),
                              ('shard', {'shard-1.lcov': 'SF:/old/caller/a/src/lib.rs\nDA:1,1\nend_of_record\n'})]:
            with zipfile.ZipFile(directory / (name + '.zip'), 'w') as archive:
                for filename, text in records.items():
                    archive.writestr(filename, text)
        artifacts = [{'id': 1, 'name': 'coverage-root-full', 'expired': False, 'workflow_run': {'id': 10}}]
        (directory / 'artifacts.json').write_text(json.dumps({'artifacts': artifacts}), encoding='utf-8')
        gh = directory / 'gh'
        gh.write_text('#!/usr/bin/env python3\nimport json,pathlib,sys\n'
            'root=pathlib.Path(__file__).parent\nurl=sys.argv[2]\n'
            'if (root/"deny").exists(): sys.exit(1)\n'
            'if "artifacts/1/zip" in url: sys.stdout.buffer.write((root/"manifest.zip").read_bytes())\n'
            'elif "artifacts/2/zip" in url: sys.stdout.buffer.write((root/"shard.zip").read_bytes())\n'
            'elif "/runs/10/artifacts" in url: print((root/"shards.json").read_text())\n'
            'elif "/runs/10" in url: print((root/"run.json").read_text())\n'
            'else: print((root/"artifacts.json").read_text())\n', encoding='utf-8')
        gh.chmod(0o755)
        shards = {'artifacts': [{'id': 2, 'name': 'coverage-root-1', 'expired': False}]}
        runs = {'conclusion': 'success', 'event': 'merge_group'}
        (directory / 'shards.json').write_text(json.dumps(shards), encoding='utf-8')
        (directory / 'run.json').write_text(json.dumps(runs), encoding='utf-8')
        env = {'PATH': str(directory) + os.pathsep + os.environ['PATH'], 'RUNNER_TEMP': temporary,
               'BASE_REF': head, 'GITHUB_REPOSITORY': 'fixture/caller', 'COVERAGE_KEY': 'root'}
        result = execute('Find coverage baseline', directory, env)
        baseline = directory / 'coverage-baseline'
        assert result.returncode == 0 and (baseline / 'manifest.json').is_file(), result.stdout + result.stderr
        assert (baseline / 'shard-1.lcov').read_text(encoding='utf-8').startswith('SF:')
        fixture_git(directory, 'commit', '--allow-empty', '-qm', 'newer than the group base')
        with zipfile.ZipFile(directory / 'manifest.zip', 'w') as archive:
            archive.writestr('manifest.json', json.dumps(manifest | {'head': fixture_git(directory, 'rev-parse', 'HEAD')}))
        result = execute('Find coverage baseline', directory, env)
        assert result.returncode == 0 and not (baseline / 'manifest.json').exists(), result.stdout + result.stderr
        assert (directory / 'coverage-baseline-reason').read_text(encoding='utf-8') == 'coverage baseline head is not an ancestor'
        def write_manifest(**changes):
            with zipfile.ZipFile(directory / 'manifest.zip', 'w') as archive:
                archive.writestr('manifest.json', json.dumps(manifest | changes))
        for label, reason, changes in [
                ('unsuccessful run', 'no baseline manifest yet', {'run': {'conclusion': 'failure', 'event': 'merge_group'}}),
                ('empty package layout', 'coverage baseline manifest or reports invalid', {'manifest': {'package_roots': {}}}),
                ('relative checkout root', 'coverage baseline manifest or reports invalid', {'manifest': {'checkout_root': 'relative'}}),
                ('missing shard', 'coverage baseline reports missing or expired', {'shards': {'artifacts': []}}),
                ('expired shard', 'coverage baseline reports missing or expired',
                 {'shards': {'artifacts': [{'id': 2, 'name': 'coverage-root-1', 'expired': True}]}})]:
            write_manifest(**changes.get('manifest', {}))
            (directory / 'run.json').write_text(json.dumps(changes.get('run', runs)), encoding='utf-8')
            (directory / 'shards.json').write_text(json.dumps(changes.get('shards', shards)), encoding='utf-8')
            result = execute('Find coverage baseline', directory, env)
            assert result.returncode == 0 and not (baseline / 'manifest.json').exists(), (label, result.stdout + result.stderr)
            assert (directory / 'coverage-baseline-reason').read_text(encoding='utf-8') == reason, label
        write_manifest()
        (directory / 'run.json').write_text(json.dumps(runs), encoding='utf-8')
        (directory / 'shards.json').write_text(json.dumps(shards), encoding='utf-8')
        for failure in ['expired', 'missing', 'denied']:
            if failure == 'expired':
                artifacts[0]['expired'] = True
            elif failure == 'missing':
                artifacts.clear()
            else:
                (directory / 'deny').touch()
            (directory / 'artifacts.json').write_text(json.dumps({'artifacts': artifacts}), encoding='utf-8')
            result = execute('Find coverage baseline', directory, env)
            assert result.returncode == 0 and not (baseline / 'manifest.json').exists(), result.stdout + result.stderr
            assert 'baseline' in (directory / 'coverage-baseline-reason').read_text(encoding='utf-8')

coverage_baseline_download_requires_complete_manifest()
print('PASS coverage_baseline_download_requires_complete_manifest')

def selected_coverage_merges_only_unchanged_baseline_packages():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        for folder in ['coverage', 'baseline']:
            (directory / folder).mkdir()
        layout = {'head': 'fixture',
                  'package_roots': {'selected': 'crates/a', 'unchanged': 'crates/b', 'nested': 'crates/a/nested'},
                  'shard_count': 1}
        baseline = layout | {'checkout_root': '/old/runner/caller'}
        (directory / 'baseline/manifest.json').write_text(json.dumps(baseline), encoding='utf-8')
        def records(root, selected_hits, other_hits):
            return ''.join(f'SF:{root}/{path}/src/lib.rs\nDA:1,{hits}\nend_of_record\n'
                           for path, hits in [('crates/a', selected_hits), ('crates/b', other_hits),
                                              ('crates/a/nested', other_hits), ('external/dependency', 0)])
        (directory / 'coverage/shard-1.lcov').write_text(records('/current/caller', 0, 0), encoding='utf-8')
        (directory / 'baseline/shard-1.lcov').write_text(records('/old/runner/caller', 1, 1), encoding='utf-8')
        env = {'TEST_SHARD_COUNT': '1', 'COVERAGE_REPORT_COUNT': '1',
               'COVERAGE_SCOPE': 'selected packages: selected', 'TEST_PACKAGES': '["selected"]',
               'COVERAGE_LAYOUT': json.dumps(layout), 'COVERAGE_CHECKOUT_ROOT': '/current/caller',
               'COVERAGE_BASELINE_DIR': str(directory / 'baseline')}
        result = execute('Merge coverage', directory, env)
        assert result.returncode != 0 and '66.67% (2/3)' in result.stdout, result.stdout + result.stderr
        assert 'Workspace line coverage (gate): 66.67% (2/3)' in result.stdout, result.stdout
        assert 'Selected-package line coverage: 0.00% (0/1)' in result.stdout, result.stdout
        (directory / 'baseline/shard-1.lcov').write_text(
            records('/old/runner/caller', 1, 1) +
            'SF:/old/runner/caller/crates/b/src/lib.rs\nDA:2,1\nDA:3,1\nend_of_record\n', encoding='utf-8')
        result = execute('Merge coverage', directory, env)
        assert result.returncode == 0 and '80.00% (4/5)' in result.stdout, result.stdout + result.stderr
        assert 'Selected-package line coverage: 0.00% (0/1)' in result.stdout
        (directory / 'baseline/shard-1.lcov').write_text(records('/old/runner/caller', 1, 1), encoding='utf-8')
        (directory / 'coverage/shard-1.lcov').write_text(records('/current/caller', 1, 0), encoding='utf-8')
        result = execute('Merge coverage', directory, env)
        assert result.returncode == 0 and '100.00% (3/3)' in result.stdout, result.stdout + result.stderr
        assert 'Selected-package line coverage: 100.00% (1/1)' in result.stdout
        (directory / 'coverage/shard-1.lcov').write_text(records('/build/caller', 1, 0), encoding='utf-8')
        result = execute('Merge coverage', directory, env | {'COVERAGE_CHECKOUT_ROOT': '/build/caller'})
        assert result.returncode == 0 and 'Workspace line coverage (gate): 100.00% (3/3)' in result.stdout, result.stdout + result.stderr
        assert 'Selected-package line coverage: 100.00% (1/1)' in result.stdout
        (directory / 'coverage/shard-1.lcov').write_text(
            'SF:/current/caller/crates/b/src/lib.rs\nDA:1,0\nend_of_record\n', encoding='utf-8')
        result = execute('Merge coverage', directory, env)
        assert result.returncode == 0 and 'Selected-package line coverage: N/A (0/0)' in result.stdout, result.stdout + result.stderr
        assert 'Workspace line coverage (gate): 100.00% (2/2)' in result.stdout
        (directory / 'baseline/shard-1.lcov').unlink()
        result = execute('Merge coverage', directory, env)
        assert result.returncode != 0 and 'baseline' in result.stderr.lower(), result.stdout + result.stderr

selected_coverage_merges_only_unchanged_baseline_packages()
print('PASS selected_coverage_merges_only_unchanged_baseline_packages')

def coverage_merge_looks_up_each_source_path_owner_once():
    document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        for folder in ['coverage', 'baseline']:
            (directory / folder).mkdir()
        layout = {'head': 'fixture',
                  'package_roots': {'selected': 'crates/a', 'unchanged': 'crates/b', 'nested': 'crates/a/nested'},
                  'shard_count': 2}
        (directory / 'baseline/manifest.json').write_text(
            json.dumps(layout | {'checkout_root': '/old/runner/caller'}), encoding='utf-8')
        def records(root, selected_hits, other_hits):
            return ''.join(f'SF:{root}/{path}/src/lib.rs\nDA:1,{hits}\nend_of_record\n'
                           for path, hits in [('crates/a', selected_hits), ('crates/b', other_hits),
                                              ('crates/a/nested', other_hits), ('external/dependency', 0)]
                           ) + 'SF:/elsewhere/lib.rs\nDA:1,1\nend_of_record\n'
        # Only the second shard of each directory covers a line, so a repeated path must keep its later records.
        for shard, hits in [(1, 0), (2, 1)]:
            (directory / f'coverage/shard-{shard}.lcov').write_text(
                records('/current/caller', hits, 0), encoding='utf-8')
            (directory / f'baseline/shard-{shard}.lcov').write_text(
                records('/old/runner/caller', hits, hits), encoding='utf-8')
        lookups = directory / 'owner-lookups'
        counted = (document['env']['PACKAGE_OWNER'] + '\n_owner = package_owner\n'
                   'def package_owner(path, roots):\n'
                   f"    open({str(lookups)!r}, 'a', encoding='utf-8').write(path.as_posix() + '\\n')\n"
                   '    return _owner(path, roots)\n')
        env = {'TEST_SHARD_COUNT': '2', 'COVERAGE_REPORT_COUNT': '2',
               'COVERAGE_SCOPE': 'selected packages: selected', 'TEST_PACKAGES': '["selected"]',
               'COVERAGE_LAYOUT': json.dumps(layout), 'COVERAGE_CHECKOUT_ROOT': '/current/caller',
               'COVERAGE_BASELINE_DIR': str(directory / 'baseline'), 'PACKAGE_OWNER': counted}
        result = execute('Merge coverage', directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        assert 'Selected-package line coverage: 100.00% (1/1)' in result.stdout, result.stdout
        assert 'Workspace line coverage (gate): 100.00% (3/3)' in result.stdout, result.stdout
        # Four source paths in each of the two report directories, however many shards repeat them.
        assert len(lookups.read_text(encoding='utf-8').splitlines()) == 8, lookups.read_text(encoding='utf-8')

coverage_merge_looks_up_each_source_path_owner_once()
print('PASS coverage_merge_looks_up_each_source_path_owner_once')

def baseline_selection_includes_changes_before_group_base():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        (directory / '.github').mkdir()
        (directory / '.github/ci.toml').write_text('wasm_crates=[]\ncheck_targets=[]\n', encoding='utf-8')
        (directory / 'Cargo.toml').write_text('[workspace]\nresolver="3"\nmembers=["a","b","c"]\n', encoding='utf-8')
        for name in ['a', 'b', 'c']:
            (directory / name / 'src').mkdir(parents=True)
            (directory / name / 'Cargo.toml').write_text(
                f'[package]\nname="{name}"\nversion="0.1.0"\nedition="2024"\n', encoding='utf-8')
            (directory / name / 'src/lib.rs').write_text('pub fn initial() {}\n', encoding='utf-8')
        subprocess.run(['cargo', 'generate-lockfile'], cwd=directory, check=True)
        fixture_git(directory, 'init', '--quiet')
        fixture_git(directory, 'add', '.')
        fixture_git(directory, 'commit', '-qm', 'baseline')
        baseline_head = fixture_git(directory, 'rev-parse', 'HEAD')
        (directory / 'b/src/lib.rs').write_text('pub fn between() {}\n', encoding='utf-8')
        fixture_git(directory, 'add', '.')
        fixture_git(directory, 'commit', '-qm', 'between baseline and base')
        base = fixture_git(directory, 'rev-parse', 'HEAD')
        (directory / 'a/src/lib.rs').write_text('pub fn selected() {}\n', encoding='utf-8')
        fixture_git(directory, 'add', '.')
        fixture_git(directory, 'commit', '-qm', 'group head')
        baseline = directory / '.git/coverage-baseline'
        baseline.mkdir()
        manifest = baseline / 'manifest.json'
        manifest.write_text(json.dumps({'head': baseline_head, 'package_roots': {'a': 'a', 'b': 'b', 'c': 'c'}}), encoding='utf-8')
        output = directory / '.git/selection'
        env = {'TEST_EVENT': 'merge_group', 'BASE_REF': base, 'GITHUB_OUTPUT': str(output),
               'RUNNER_TEMP': str(directory / '.git')}
        result = select_packages(directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        values = dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())
        assert json.loads(values['packages']) == ['a', 'b'], values
        assert json.loads(values['coverage_layout'])['package_roots'] == {'a': 'a', 'b': 'b', 'c': 'c'}
        assert 'checkout_root' not in json.loads(values['coverage_layout']), 'the selection layout names a checkout root'
        manifest.write_text(json.dumps({'head': baseline_head, 'package_roots': {'a': 'old'}}), encoding='utf-8')
        result = select_packages(directory, env)
        assert result.returncode == 0 and 'baseline package layout changed' in result.stdout, result.stdout + result.stderr
        manifest.unlink()
        output.write_text('', encoding='utf-8')
        result = select_packages(directory, env)
        values = dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())
        assert result.returncode == 0 and 'no baseline manifest yet' in result.stdout, result.stdout + result.stderr
        assert json.loads(values['packages']) == ['a', 'b', 'c'], values
        manifest.write_text(json.dumps({'head': baseline_head, 'package_roots': {'a': 'a', 'b': 'b', 'c': 'c'}}), encoding='utf-8')
        (directory / '.github/ci.toml').unlink()
        result = select_packages(directory, env)
        assert result.returncode == 0 and 'undeclared caller: full suite' in result.stdout, result.stdout + result.stderr

baseline_selection_includes_changes_before_group_base()
print('PASS baseline_selection_includes_changes_before_group_base')

def coverage_merge_uses_union_and_requires_all_reports():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        reports = directory / 'coverage'
        reports.mkdir()
        def report(shard, lines, source='/src/lib.rs'):
            (reports / f'shard-{shard}.lcov').write_text(
                f'SF:{source}\n' + ''.join(f'DA:{line},{hits}\n' for line, hits in lines) + 'end_of_record\n',
                encoding='utf-8')
        for shard in range(1, 17):
            report(shard, [])
        report(1, [(line, 1) for line in range(1, 7)])
        report(2, [(line, 1) for line in range(1, 4)], '/caller/examples/coverage.rs')
        report(3, [(line, 0) for line in range(1, 4)], '/caller/src/tests/../../examples/coverage.rs')
        result = execute('Merge coverage', directory)
        assert result.returncode == 0 and '100.00% (9/9)' in result.stdout, result.stdout + result.stderr
        report(1, [(1, 1), (2, 0)])
        report(2, [(1, 0), (2, 1)])
        for shard in range(3, 16):
            report(shard, [(1, 0)])
        report(16, [(3, 1)])
        result = execute('Merge coverage', directory)
        assert result.returncode == 0 and '100.00% (3/3)' in result.stdout, result.stdout + result.stderr
        report(16, [(3, 0)])
        result = execute('Merge coverage', directory)
        assert result.returncode != 0 and '66.67% (2/3)' in result.stdout and 'below threshold' in result.stderr, result.stdout + result.stderr
        report(16, [(3, 1), (4, 0)])
        result = execute('Merge coverage', directory, {'QUALITY_CONFIG': '{"coverage_min_lines": 90}'})
        assert result.returncode != 0 and 'minimum 90%' in result.stdout, result.stdout + result.stderr
        for shard in range(1, 17):
            path = reports / f'shard-{shard}.lcov'
            content = path.read_text(encoding='utf-8')
            path.unlink()
            result = execute('Merge coverage', directory)
            assert result.returncode != 0 and 'Missing' in result.stderr, f'shard {shard}\n' + result.stdout + result.stderr
            path.write_text(content, encoding='utf-8')
        report(17, [(1, 1)])
        result = execute('Merge coverage', directory)
        assert result.returncode != 0 and 'unexpected' in result.stderr, result.stdout + result.stderr

coverage_merge_uses_union_and_requires_all_reports()
print('PASS coverage_merge_uses_union_and_requires_all_reports')

def advisory_selection_is_caller_relative():
    from functools import partial
    import shutil
    for caller in ['.', 'fixtures/declared']:
        with tempfile.TemporaryDirectory() as temporary:
            repository = pathlib.Path(temporary)
            directory = repository / caller
            shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True,
                            ignore=shutil.ignore_patterns('target', '.git'))
            (directory / '.github').mkdir(exist_ok=True)
            declaration = directory / '.github/ci.toml'
            declaration.write_text('advisory_exceptions=[]\nspelling_words=["base"]\n', encoding='utf-8')
            subprocess.run(['git', 'init', '--quiet'], cwd=repository, check=True)
            git = partial(fixture_git, repository)
            git('add', '.')
            git('commit', '-qm', 'base')
            base = git('rev-parse', 'HEAD')
            for change in ['Cargo.lock', 'advisory_exceptions', 'yanked_crate_exceptions', 'spelling_words', 'additional_locked_manifests']:
                git('reset', '--hard', base)
                if change == 'Cargo.lock':
                    with (directory / 'Cargo.lock').open('a', encoding='utf-8') as lock:
                        lock.write('\n# dependency fixture\n')
                else:
                    value = {'advisory_exceptions': '[{id="RUSTSEC-2000-0001", package="a", version="0.1.0", reason="fixture"}]',
                             'yanked_crate_exceptions': '[{package="a", version="0.1.0"}]',
                             'spelling_words': '["changed"]',
                             'additional_locked_manifests': '["extra/Cargo.toml"]'}[change]
                    unchanged = 'advisory_exceptions=[]\n' if change != 'advisory_exceptions' else ''
                    declaration.write_text(unchanged + f'{change}={value}\n', encoding='utf-8')
                git('add', '.')
                git('commit', '-qm', change)
                directions = ['add', 'remove'] if change in ['advisory_exceptions', 'yanked_crate_exceptions'] else ['add']
                comparison_base = base
                for direction in directions:
                    if direction == 'remove':
                        comparison_base = git('rev-parse', 'HEAD')
                        git('checkout', base, '--', str(declaration.relative_to(repository)))
                        git('commit', '-qm', 'remove ' + change)
                    for event in ['pull_request', 'merge_group']:
                        output = repository / '.git/selection'
                        output.write_text('', encoding='utf-8')
                        result = select_packages(directory, {'TEST_EVENT': event,
                            'BASE_REF': comparison_base, 'GITHUB_OUTPUT': str(output)})
                        assert result.returncode == 0, result.stdout + result.stderr
                        values = dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())
                        expected = 'false' if change in ['spelling_words', 'additional_locked_manifests'] else 'true'
                        assert values['check_advisories'] == expected, (caller, change, direction, event, values)

advisory_selection_is_caller_relative()
print('PASS advisory_selection_is_caller_relative')

def browser_merge_groups_follow_shared_package_selection():
    from functools import partial
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        (directory / 'Cargo.toml').write_text(
            '[workspace]\nresolver="3"\nmembers=["core", "web", "other"]\n', encoding='utf-8')
        for name in ['core', 'web', 'other']:
            crate = directory / name
            (crate / 'src').mkdir(parents=True)
            manifest = f'[package]\nname="{name}"\nversion="0.1.0"\nedition="2024"\n'
            if name == 'web':
                manifest += '[dependencies]\ncore={path="../core"}\n'
            (crate / 'Cargo.toml').write_text(manifest, encoding='utf-8')
            (crate / 'src/lib.rs').write_text('pub fn value() {}\n', encoding='utf-8')
        (directory / '.github').mkdir()
        (directory / '.github/ci.toml').write_text(
            'wasm_crates=[]\ncheck_targets=[]\nbrowser_test_crates=["web"]\n', encoding='utf-8')
        subprocess.run(['cargo', 'generate-lockfile'], cwd=directory, check=True)
        subprocess.run(['git', 'init', '--quiet'], cwd=directory, check=True)
        git = partial(fixture_git, directory)
        git('add', '.')
        git('commit', '-qm', 'base')
        base = git('rev-parse', 'HEAD')
        job = workflow.split('  browser-tests:\n', 1)[1]
        document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
        gate = document['jobs']['browser-tests']['if']
        for path, expected in [('other/src/lib.rs', False), ('web/src/lib.rs', True),
                               ('core/src/lib.rs', True), ('Cargo.lock', True)]:
            git('reset', '--hard', base)
            target = directory / path
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open('a', encoding='utf-8') as changed:
                changed.write('\n# changed shared input\n' if target.suffix == '.lock' else '\n')
            git('add', '.')
            git('commit', '-qm', path)
            for event in ['merge_group', 'push']:
                output = directory / '.git/selection'
                output.write_text('', encoding='utf-8')
                result = select_packages(directory, {
                    'TEST_EVENT': event, 'BASE_REF': base, 'GITHUB_OUTPUT': str(output)})
                assert result.returncode == 0, result.stdout + result.stderr
                values = dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())
                should_run = expected or event == 'push'
                assert values['browser_tests'] == str(should_run).lower(), (path, event, values)
                context = {'needs.declaration.outputs.browser_tests': values['browser_tests'],
                           'needs.declaration.outputs.coverage': values['coverage'],
                           'inputs.advisory-scan': False, 'inputs.fuzz-scan': False,
                           'inputs.warm-cache': False}
                assert bool(evaluate(gate, context)) == should_run, (path, event, gate)

browser_merge_groups_follow_shared_package_selection()
print('PASS browser_merge_groups_follow_shared_package_selection')

def merge_group_nested_members_have_one_owner():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        (directory / '.github').mkdir()
        (directory / '.github/ci.toml').write_text('wasm_crates=[]\ncheck_targets=[]\n', encoding='utf-8')
        (directory / 'src').mkdir()
        (directory / 'src/lib.rs').write_text('pub fn root() {}\n', encoding='utf-8')
        (directory / 'nested/src').mkdir(parents=True)
        (directory / 'nested/src/lib.rs').write_text('pub fn nested() {}\n', encoding='utf-8')
        (directory / 'Cargo.toml').write_text(
            '[workspace]\nresolver="3"\nmembers=["nested"]\n'
            '[package]\nname="root"\nversion="0.1.0"\nedition="2024"\n', encoding='utf-8')
        (directory / 'nested/Cargo.toml').write_text(
            '[package]\nname="nested"\nversion="0.1.0"\nedition="2024"\n', encoding='utf-8')
        subprocess.run(['cargo', 'generate-lockfile'], cwd=directory, check=True)
        fixture_git(directory, 'init', '--quiet')
        fixture_git(directory, 'add', '.')
        fixture_git(directory, 'commit', '-qm', 'base')
        base = fixture_git(directory, 'rev-parse', 'HEAD')
        (directory / 'nested/src/lib.rs').write_text('pub fn changed() {}\n', encoding='utf-8')
        fixture_git(directory, 'add', '.')
        fixture_git(directory, 'commit', '-qm', 'source')
        output = directory / '.git/selection'
        result = select_packages(directory, {
            'TEST_EVENT': 'merge_group', 'BASE_REF': base, 'GITHUB_OUTPUT': str(output)})
        assert result.returncode == 0, result.stdout + result.stderr
        values = dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())
        assert json.loads(values['packages']) == ['nested'], values
        assert json.loads(values['coverage_exclude']) == ['root'], values
        assert values['coverage_scope'] == 'selected packages: nested', values

merge_group_nested_members_have_one_owner()
print('PASS merge_group_nested_members_have_one_owner')

def merge_groups_select_graph_and_fail_closed():
    from functools import partial
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        (directory / '.github').mkdir()
        (directory / '.github/ci.toml').write_text(
            'wasm_crates=[]\ncheck_targets=[]\nbrowser_test_crates=["b"]\n', encoding='utf-8')
        names = ['a', 'b', 'c', 'build', 'dev', 'target', 'unrelated']
        (directory / 'Cargo.toml').write_text(
            '[workspace]\nresolver="3"\nmembers=' + json.dumps(names) + '\n', encoding='utf-8')
        edges = {'b': ('dependencies', 'a'), 'c': ('dependencies', 'b'),
                 'build': ('build-dependencies', 'a'), 'dev': ('dev-dependencies', 'a'),
                 'target': ('target.\'cfg(windows)\'.dependencies', 'a')}
        for name in names:
            crate = directory / name
            (crate / 'src').mkdir(parents=True)
            manifest = f'[package]\nname="{name}"\nversion="0.1.0"\nedition="2024"\n'
            if name in edges:
                section, dependency = edges[name]
                manifest += f'[{section}]\n{dependency}={{path="../{dependency}"}}\n'
            (crate / 'Cargo.toml').write_text(manifest, encoding='utf-8')
            (crate / 'src/lib.rs').write_text('pub fn value() {}\n', encoding='utf-8')
        subprocess.run(['cargo', 'generate-lockfile'], cwd=directory, check=True)
        git = partial(fixture_git, directory)
        git('init', '--quiet')
        git('add', '.')
        git('commit', '-qm', 'base')
        base = git('rev-parse', 'HEAD')
        def select(base_ref=base, event='merge_group', browser='true'):
            output = directory / '.git/selection'
            output.write_text('', encoding='utf-8')
            result = select_packages(directory, {
                'TEST_EVENT': event, 'BASE_REF': base_ref, 'GITHUB_OUTPUT': str(output)})
            assert result.returncode == 0, result.stdout + result.stderr
            values = dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())
            assert values['coverage'] == 'true'
            assert values['browser_tests'] == browser, values
            return json.loads(values['packages']), result.stdout
        source = directory / 'a/src/lib.rs'
        source.write_text('pub fn changed() {}\n', encoding='utf-8')
        git('add', '.')
        git('commit', '-qm', 'source')
        selected, log = select()
        assert selected == ['a', 'b', 'build', 'c', 'dev', 'target'], (selected, log)
        assert 'Coverage scope: selected packages' in log
        assert select(event='push')[0] == sorted(names)
        git('reset', '--hard', base)
        (directory / 'unrelated/src/lib.rs').write_text('pub fn changed() {}\n', encoding='utf-8')
        git('add', '.')
        git('commit', '-qm', 'unrelated source')
        assert select(browser='false')[0] == ['unrelated']
        for filename in ['Cargo.lock', '.config/nextest.toml', 'rust-toolchain.toml',
                         '.github/workflows/check.yml', '.github/ci.toml', 'README.md',
                         'Cargo.toml', 'unknown/file', 'docs/guide.md']:
            git('reset', '--hard', base)
            path = directory / filename
            existed = path.exists()
            previous = path.read_text(encoding='utf-8') if existed else None
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text((previous or '') + '\n# shared input\n', encoding='utf-8')
            git('add', '.')
            git('commit', '-qm', 'shared input')
            # Restore the runtime manifest/toolchain: selection uses the committed diff.
            if previous is not None:
                path.write_text(previous, encoding='utf-8')
            elif filename == 'rust-toolchain.toml':
                path.unlink()
            selected, log = select()
            assert selected == sorted(names), (filename, selected, log)
            assert 'Full-suite fallback:' in log and 'Coverage scope: full workspace' in log
        git('reset', '--hard', base)
        for invalid in ['', '0' * 40]:
            selected, log = select(base_ref=invalid)
            assert selected == sorted(names) and 'Full-suite fallback:' in log, log
        # Broken resolution must still enumerate and run every workspace member.
        manifest = directory / 'a/Cargo.toml'
        manifest.write_text(manifest.read_text(encoding='utf-8').replace('0.1.0', '0.2.0'), encoding='utf-8')
        selected, log = select()
        assert selected == sorted(names) and 'Full-suite fallback:' in log, log

merge_groups_select_graph_and_fail_closed()
print('PASS merge_groups_select_graph_and_fail_closed')

def changed_packages_include_reverse_dependencies():
    from functools import partial
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        (directory / 'Cargo.toml').write_text('[workspace]\nresolver="3"\nmembers=["a","b","c","d"]\n', encoding='utf-8')
        for name, dependency in [('a', None), ('b', 'a'), ('c', 'b'), ('d', None)]:
            crate = directory / name
            (crate / 'src').mkdir(parents=True)
            manifest = f'[package]\nname="{name}"\nversion="0.1.0"\nedition="2024"\n'
            if dependency:
                manifest += f'[dependencies]\n{dependency}={{path="../{dependency}"}}\n'
            (crate / 'Cargo.toml').write_text(manifest, encoding='utf-8')
            (crate / 'src/lib.rs').write_text('pub fn value() {}\n', encoding='utf-8')
        subprocess.run(['cargo', 'generate-lockfile'], cwd=directory, check=True)
        subprocess.run(['git', 'init', '--quiet'], cwd=directory, check=True)
        git = partial(fixture_git, directory)
        (directory / '.github').mkdir()
        (directory / '.github/ci.toml').write_text('wasm_crates=[]\ncheck_targets=[]\n', encoding='utf-8')
        git('add', '.')
        git('commit', '-qm', 'base')
        base = git('rev-parse', 'HEAD')
        def select(event='pull_request'):
            output = directory / '.git/selection'
            output.write_text('', encoding='utf-8')
            result = select_packages(directory, {'TEST_EVENT': event, 'BASE_REF': base, 'GITHUB_OUTPUT': str(output)})
            assert result.returncode == 0, result.stdout + result.stderr
            return dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())
        declaration_path = directory / '.github/ci.toml'
        for value in ['test(child_build)', 'none()', None]:
            declaration_path.write_text('wasm_crates=[]\ncheck_targets=[]\n' + (
                'warm_test_filter=' + json.dumps(value) + '\n' if value is not None else ''), encoding='utf-8')
            git('add', '.')
            git('commit', '-qm', 'warm filter only')
            values = select()
            assert json.loads(values['packages']) == [] and values['check_advisories'] == 'false'
            base = git('rev-parse', 'HEAD')
        declaration_path.write_text('wasm_crates=[]\ncheck_targets=[]\nbrowser_test_crates=["b", "b"]\n', encoding='utf-8')
        git('add', '.')
        git('commit', '-qm', 'browser opt-in')
        assert select()['browser_tests'] == 'true'
        assert json.loads(select()['packages']) == []
        base = git('rev-parse', 'HEAD')
        (directory / 'docs.md').write_text('Documentation', encoding='utf-8')
        git('add', '.')
        git('commit', '-qm', 'docs')
        assert select()['browser_tests'] == 'false'
        (directory / 'd/src/lib.rs').write_text('pub fn unrelated() {}\n', encoding='utf-8')
        git('add', '.')
        git('commit', '-qm', 'unrelated')
        assert select()['browser_tests'] == 'false'
        assert select('merge_group')['browser_tests'] == select('push')['browser_tests'] == 'true'
        git('reset', '--hard', base)
        (directory / 'a/src/lib.rs').write_text('pub fn changed() {}\n', encoding='utf-8')
        git('add', '.')
        git('commit', '-qm', 'source')
        values = select()
        assert json.loads(values['packages']) == ['a', 'b', 'c'] and values['coverage'] == 'false'
        assert values['browser_tests'] == 'true'
        declaration_path.write_text('wasm_crates=[]\ncheck_targets=[]\nbrowser_test_crates=[]\n', encoding='utf-8')
        git('add', '.')
        git('commit', '-qm', 'remove browser opt-in')
        assert select()['browser_tests'] == 'false'
        git('reset', '--hard', 'HEAD~1')
        assert json.loads(select('merge_group')['packages']) == ['a', 'b', 'c']
        assert select('push')['coverage'] == 'true'
        # Advisories follow dependency changes only on pull requests and merge groups.
        assert [select(event)['check_advisories'] for event in ['pull_request', 'merge_group', 'push', 'schedule']] == ['false', 'false', 'true', 'true']
        output = directory / '.git/no-base'
        result = select_packages(directory, {'TEST_EVENT': 'pull_request', 'BASE_REF': '', 'GITHUB_OUTPUT': str(output)})
        assert result.returncode == 0 and 'check_advisories=true' in output.read_text(encoding='utf-8'), result.stdout + result.stderr
        # A base the checkout cannot read keeps the check.
        output.write_text('', encoding='utf-8')
        result = select_packages(directory, {'TEST_EVENT': 'merge_group', 'BASE_REF': '0' * 40, 'GITHUB_OUTPUT': str(output)})
        assert result.returncode == 0 and 'check_advisories=true' in output.read_text(encoding='utf-8'), result.stdout + result.stderr
        git('reset', '--hard', base)
        (directory / 'a/Cargo.toml').write_text((directory / 'a/Cargo.toml').read_text(encoding='utf-8').replace('0.1.0', '0.2.0'), encoding='utf-8')
        subprocess.run(['cargo', 'generate-lockfile'], cwd=directory, check=True)
        git('add', '.')
        git('commit', '-qm', 'dependency identity')
        assert json.loads(select()['packages']) == ['a', 'b', 'c']
        assert select()['check_advisories'] == select('merge_group')['check_advisories'] == 'true'
        git('reset', '--hard', base)
        (directory / '.github/ci.toml').write_text('wasm_crates=[]\ncheck_targets=[]\nspelling_words=["word"]\n', encoding='utf-8')
        git('add', '.')
        git('commit', '-qm', 'declaration')
        base = git('rev-parse', 'HEAD')
        (directory / '.github/ci.toml').write_text('wasm_crates=["a"]\ncheck_targets=[]\nspelling_words=["another"]\n', encoding='utf-8')
        git('add', '.')
        git('commit', '-qm', 'non-test settings')
        assert json.loads(select()['packages']) == []
        # Settings that cannot change dependencies leave advisories to main's pushes and scan.
        assert select()['check_advisories'] == select('merge_group')['check_advisories'] == 'false'
        assert select('push')['check_advisories'] == 'true'
        settings = git('rev-parse', 'HEAD')
        for key, value in [('advisory_exceptions', '[{id="RUSTSEC-2000-0001", package="a", version="0.1.0", reason="fixture"}]'),
                           ('yanked_crate_exceptions', '[{package="a", version="0.1.0"}]')]:
            (directory / '.github/ci.toml').write_text(
                'wasm_crates=["a"]\ncheck_targets=[]\nspelling_words=["another"]\n' + f'{key}={value}\n', encoding='utf-8')
            git('add', '.')
            git('commit', '-qm', key)
            assert select()['check_advisories'] == select('merge_group')['check_advisories'] == 'true', key
            git('reset', '--hard', settings)
        for entries in [['b/Cargo.toml'], ['c/Cargo.toml'], ['b/Cargo.toml', 'a/Cargo.toml'],
                        ['a/Cargo.toml', 'b/Cargo.toml'], ['a/Cargo.toml', 'b/Cargo.toml', 'a/Cargo.toml'], []]:
            base = git('rev-parse', 'HEAD')
            (directory / '.github/ci.toml').write_text(
                'wasm_crates=[]\ncheck_targets=[]\nadditional_locked_manifests=' + json.dumps(entries), encoding='utf-8')
            git('add', '.')
            git('commit', '-qm', 'additional manifests')
            assert json.loads(select()['packages']) == ['a', 'b', 'c', 'd']
            assert select()['check_advisories'] == select('merge_group')['check_advisories'] == 'false'
        base = git('rev-parse', 'HEAD')
        (directory / '.github/ci.toml').write_text('wasm_crates=[]\ncheck_targets=[]\n', encoding='utf-8')
        git('add', '.')
        git('commit', '-qm', 'remove additional manifests')
        assert json.loads(select()['packages']) == ['a', 'b', 'c', 'd']
        (directory / '.github/workflows').mkdir()
        (directory / '.github/workflows/ci.yml').write_text('name: changed\n', encoding='utf-8')
        git('add', '.')
        git('commit', '-qm', 'workflow')
        assert json.loads(select()['packages']) == ['a', 'b', 'c', 'd']

changed_packages_include_reverse_dependencies()
print('PASS changed_packages_include_reverse_dependencies')

def included_docs_run_only_readers_and_reverse_dependencies():
    from functools import partial
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        (directory / 'Cargo.toml').write_text('[workspace]\nresolver="3"\nmembers=["crates/a","crates/b","crates/c"]\n', encoding='utf-8')
        for name, dependency in [('a', None), ('b', 'a'), ('c', None)]:
            crate = directory / 'crates' / name
            (crate / 'src').mkdir(parents=True)
            manifest = f'[package]\nname="{name}"\nversion="0.1.0"\nedition="2024"\n'
            if dependency:
                manifest += f'[dependencies]\n{dependency}={{path="../{dependency}"}}\n'
            (crate / 'Cargo.toml').write_text(manifest, encoding='utf-8')
            (crate / 'src/lib.rs').write_text('', encoding='utf-8')
        (directory / 'docs').mkdir()
        for name in ['policy.md', 'bytes.md', 'escaped.md', 'line\nbreak.md', 'commented.md', 'unrelated.md']:
            (directory / 'docs' / name).write_text('base\n', encoding='utf-8')
        (directory / 'crates/a/src/lib.rs').write_text(
            'const _: &str = include_str!("../../../docs/policy.md");\n'
            'const _: &[u8] = include_bytes! (\n r#"../../../docs/bytes.md"#\n);\n'
            'const _: &str = include_str!("../../../docs/escap\\u{65}d.md");\n'
            'const _: &str = include_str!("../../../docs/line\nbreak.md");\n'
            'const _: &str = include_str /* macro */ ! // bang\n'
            ' (/* outer /* nested */ comment */ "../../../docs/commented.md" /* end */);\n', encoding='utf-8')
        command = ['cargo', 'generate-lockfile']
        if os.environ.get('MAESTRO_LOCAL_CAPPED'):
            command.insert(0, os.path.expanduser('~/.local/bin/capped'))
        subprocess.run(command, cwd=directory, check=True)
        subprocess.run(['git', 'init', '--quiet'], cwd=directory, check=True)
        git = partial(fixture_git, directory)
        git('add', '.')
        git('commit', '-qm', 'base')
        base = git('rev-parse', 'HEAD')
        for changed, expected, runs in [('docs/policy.md', ['a', 'b'], 'true'),
                                         ('docs/bytes.md', ['a', 'b'], 'true'),
                                         ('docs/escaped.md', ['a', 'b'], 'true'),
                                         ('docs/line\nbreak.md', ['a', 'b'], 'true'),
                                         ('docs/commented.md', ['a', 'b'], 'true'),
                                         ('docs/unrelated.md', [], 'false'),
                                         ('crates/c/src/lib.rs', ['c'], 'false')]:
            (directory / changed).write_text('// changed\n', encoding='utf-8')
            git('add', '.')
            git('commit', '-qm', 'change')
            output = directory / '.git/selection'
            output.write_text('', encoding='utf-8')
            result = select_packages(directory, {'TEST_EVENT': 'pull_request', 'BASE_REF': base, 'GITHUB_OUTPUT': str(output)})
            assert result.returncode == 0, result.stdout + result.stderr
            values = dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())
            assert json.loads(values['packages']) == expected, (changed, values)
            assert values.get('included_tests') == runs, (changed, values)
            assert values['coverage'] == 'false', values
            git('reset', '--hard', base)
        tests = workflow.split('  tests:\n', 1)[1].split('\n  checks:', 1)[0]
        condition = next(line for line in tests.splitlines() if line.startswith('    if:'))
        assert "(needs.declaration.outputs.coverage == 'true' || needs.declaration.outputs.included_tests == 'true')" in condition
        assert 'included_tests: $' + '{{ steps.selection.outputs.included_tests }}' in workflow

included_docs_run_only_readers_and_reverse_dependencies()
print('PASS included_docs_run_only_readers_and_reverse_dependencies')

def included_readers_follow_member_roots():
    from functools import partial
    for nested in [False, True]:
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary)
            manifest = '[package]\nname="root-reader"\nversion="0.1.0"\nedition="2024"\n'
            if nested:
                manifest += '[workspace]\nresolver="3"\nmembers=["crates/child"]\n'
            (directory / 'Cargo.toml').write_text(manifest, encoding='utf-8')
            (directory / 'src').mkdir()
            (directory / 'src/lib.rs').write_text('' if nested else 'const _: &str = include_str!("../docs/policy.md");\n', encoding='utf-8')
            if nested:
                child = directory / 'crates/child'
                (child / 'src').mkdir(parents=True)
                (child / 'Cargo.toml').write_text('[package]\nname="child"\nversion="0.1.0"\nedition="2024"\n', encoding='utf-8')
                (child / 'src/lib.rs').write_text('const _: &str = include_str!("../../../docs/policy.md");\n', encoding='utf-8')
            (directory / 'docs').mkdir()
            (directory / 'docs/policy.md').write_text('base\n', encoding='utf-8')
            command = ['cargo', 'generate-lockfile']
            if os.environ.get('MAESTRO_LOCAL_CAPPED'):
                command.insert(0, os.path.expanduser('~/.local/bin/capped'))
            subprocess.run(command, cwd=directory, check=True)
            subprocess.run(['git', 'init', '--quiet'], cwd=directory, check=True)
            git = partial(fixture_git, directory)
            git('add', '.')
            git('commit', '-qm', 'base')
            base = git('rev-parse', 'HEAD')
            (directory / 'docs/policy.md').write_text('changed\n', encoding='utf-8')
            git('add', '.')
            git('commit', '-qm', 'change')
            output = directory / '.git/selection'
            result = select_packages(directory, {'TEST_EVENT': 'pull_request', 'BASE_REF': base, 'GITHUB_OUTPUT': str(output)})
            assert result.returncode == 0, result.stdout + result.stderr
            values = dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())
            assert json.loads(values['packages']) == (['child'] if nested else ['root-reader']), (nested, values)
            assert values['included_tests'] == 'true', (nested, values)
            assert values['coverage'] == 'false', values

included_readers_follow_member_roots()
print('PASS included_readers_follow_member_roots')

def included_test_failures_remain_required():
    for included in ['false', 'true']:
        for status in ['success', 'failure', 'cancelled', 'skipped']:
            result = subprocess.run(['python3', '-c', body('All test shards passed')],
                                    env=os.environ | {'RESULT': status, 'DOCTEST_RESULT': status, 'COLLECT_COVERAGE': 'false',
                                                      'INCLUDED_TESTS': included, 'DECLARATION_RESULT': 'success'},
                                    capture_output=True, text=True)
            expected = status == ('success' if included == 'true' else 'skipped')
            assert (result.returncode == 0) == expected, (included, status, result.stderr)
    assert 'INCLUDED_TESTS: $' + '{{ needs.declaration.outputs.included_tests }}' in workflow

included_test_failures_remain_required()
print('PASS included_test_failures_remain_required')

def declared_test_inputs_select_their_readers():
    from functools import partial
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        (directory / 'Cargo.toml').write_text('[workspace]\nresolver="3"\nmembers=["a","b","c","d"]\n', encoding='utf-8')
        for name, dependency in [('a', None), ('b', 'a'), ('c', 'b'), ('d', None)]:
            crate = directory / name
            (crate / 'src').mkdir(parents=True)
            manifest = f'[package]\nname="{name}"\nversion="0.1.0"\nedition="2024"\n'
            if dependency:
                manifest += f'[dependencies]\n{dependency}={{path="../{dependency}"}}\n'
            (crate / 'Cargo.toml').write_text(manifest, encoding='utf-8')
            (crate / 'src/lib.rs').write_text('pub fn value() {}\n', encoding='utf-8')
        subprocess.run(['cargo', 'generate-lockfile'], cwd=directory, check=True)
        subprocess.run(['git', 'init', '--quiet'], cwd=directory, check=True)
        git = partial(fixture_git, directory)
        for name in ['shared/data.txt', 'shared/notes.md', 'list.json', 'fuzz/src/main.rs', 'other/data.txt']:
            (directory / name).parent.mkdir(parents=True, exist_ok=True)
            (directory / name).write_text('base\n', encoding='utf-8')
        declared = 'wasm_crates=[]\ncheck_targets=[]\ntest_inputs={"shared/"=["d"], "list.json"=["a"], "fuzz"=[]}\n'
        (directory / '.github').mkdir()
        (directory / '.github/ci.toml').write_text(declared, encoding='utf-8')
        git('add', '.')
        git('commit', '-qm', 'base')
        base = git('rev-parse', 'HEAD')
        def select(event='pull_request', fallback=None):
            output = directory / '.git/selection'
            output.write_text('', encoding='utf-8')
            result = select_packages(directory, {'TEST_EVENT': event, 'BASE_REF': base, 'GITHUB_OUTPUT': str(output)})
            assert result.returncode == 0, result.stdout + result.stderr
            if fallback:
                assert 'Full-suite fallback: ' + fallback in result.stdout, result.stdout
            return json.loads(dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())['packages'])
        for changed, expected in [(['shared/data.txt'], ['d']), (['shared/notes.md'], ['d']), (['list.json'], ['a', 'b', 'c']),
                                  (['fuzz/src/main.rs'], []), (['fuzz/src/main.rs', 'shared/data.txt'], ['d']),
                                  (['other/data.txt'], ['a', 'b', 'c', 'd']), (['shared/data.txt', 'other/data.txt'], ['a', 'b', 'c', 'd'])]:
            for name in changed:
                (directory / name).write_text('changed\n', encoding='utf-8')
            git('add', '.')
            git('commit', '-qm', 'change')
            assert select() == expected, (changed, select())
            merge_expected = ['a', 'b', 'c', 'd'] if not expected or any('/' not in name for name in changed) else expected
            assert select('merge_group', 'no workspace test packages selected' if not expected else None) == merge_expected, changed
            git('reset', '--hard', base)
        for source, destination, expected in [('shared/data.txt', 'fuzz/data.txt', ['d']),
                                              ('other/data.txt', 'shared/moved.txt', ['a', 'b', 'c', 'd']),
                                              ('list.json', 'fuzz/list.json', ['a', 'b', 'c'])]:
            git('mv', source, destination)
            git('commit', '-qm', 'move input')
            assert select() == expected, (source, destination, select())
            git('reset', '--hard', base)
        # A changed route cannot be checked against the old one, so it selects everything.
        (directory / '.github/ci.toml').write_text(declared.replace('["d"]', '["c"]'), encoding='utf-8')
        git('add', '.')
        git('commit', '-qm', 'route')
        assert select() == ['a', 'b', 'c', 'd']

declared_test_inputs_select_their_readers()
print('PASS declared_test_inputs_select_their_readers')
print('PASS browser_test_selection_reuses_package_scope')

def retries_report_flaky_tests_and_keep_failures():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        source = directory / 'src/lib.rs'
        source.write_text('#[test] fn flaky_control() { assert_eq!(std::env::var("NEXTEST_ATTEMPT").unwrap(), "3"); }\n', encoding='utf-8')
        result = execute('Isolated tests', directory, {'RUNNER_TEMP': temporary, 'COLLECT_COVERAGE': 'false'})
        assert result.returncode == 0 and 'FLAKY' in result.stdout, result.stdout + result.stderr
        summary = directory / 'summary'
        result = execute('Flaky test summary', directory, {'GITHUB_STEP_SUMMARY': str(summary)})
        assert result.returncode == 0 and 'flaky_control' in summary.read_text(encoding='utf-8'), result.stderr
        source.write_text('#[test] fn permanent_failure() { panic!("permanent"); }\n', encoding='utf-8')
        result = execute('Isolated tests', directory, {'RUNNER_TEMP': temporary, 'COLLECT_COVERAGE': 'false'})
        assert result.returncode != 0 and 'permanent_failure' in result.stdout, result.stdout + result.stderr

retries_report_flaky_tests_and_keep_failures()
print('PASS retries_report_flaky_tests_and_keep_failures')

def declaration_types_are_checked():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        (directory / '.github').mkdir()
        for filename in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml']:
            (directory / filename).write_bytes((root / 'fixtures/sample' / filename).read_bytes())
        (directory / 'src').mkdir()
        (directory / 'src/lib.rs').write_text('//! Fixture.\n', encoding="utf-8")
        path = directory / '.github/ci.toml'
        for text in ['wasm_crates=[]\ncheck_targets=[]',
                     'wasm_crates=[]\ncheck_targets=[]\nbrowser_build=false',
                     'wasm_crates=[]\ncheck_targets=[]\nbrowser_build=true']:
            path.write_text(text, encoding="utf-8")
            result, values = declaration(directory)
            assert result.returncode == 0, result.stdout + result.stderr
            assert values['enabled'] == 'true'
            assert values['browser_build'] == str('browser_build=true' in text).lower()
        for text in ['', 'broken [', 'wasm_crates=[]', 'check_targets=[]',
                     'wasm_crates=[]\ncheck_targets=[]\nextra=true',
                     'wasm_crates="sample"\ncheck_targets=[]',
                     'wasm_crates=[]\ncheck_targets="target"',
                     'wasm_crates=[1]\ncheck_targets=[]',
                     'wasm_crates=[]\ncheck_targets=[false]',
                     'wasm_crates=[]\ncheck_targets=[]\nbrowser_build=1',
                     'wasm_crates=[]\ncheck_targets=[]\nbrowser_build="true"',
                     'wasm_crates=[]\nwasm_crates=[]\ncheck_targets=[]']:
            path.write_text(text, encoding="utf-8")
            result, values = declaration(directory)
            assert result.returncode != 0, text + '\n' + result.stdout + result.stderr
            assert '.github/ci.toml' in result.stderr
            if 'extra=true' in text:
                assert 'extra' in result.stderr
            assert not values
        path.write_bytes(b'\xff')  # Invalid UTF-8 still names the declaration.
        result, values = declaration(directory)
        assert result.returncode != 0 and '.github/ci.toml' in result.stderr and not values, result.stderr
        path.unlink()
        path.mkdir()  # A present path that cannot be read as a file is not absence.
        result, values = declaration(directory)
        assert result.returncode != 0 and not values, result.stdout + result.stderr
        path.rmdir()
        path.symlink_to(directory / 'missing')
        result, values = declaration(directory)
        assert result.returncode != 0 and not values, result.stdout + result.stderr

declaration_types_are_checked()
print('PASS declaration_types_are_checked')

def declared_members_are_literal_arguments():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        import shutil
        shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        (directory / '.github').mkdir()
        path = directory / '.github/ci.toml'
        path.write_text('wasm_crates=["sample", "sample"]\ncheck_targets=["x86_64-pc-windows-msvc", "x86_64-pc-windows-msvc"]', encoding="utf-8")
        result, values = declaration(directory)
        assert result.returncode == 0, result.stdout + result.stderr
        assert json.loads(values['wasm_crates']) == ['sample', 'sample']
        assert json.loads(values['targets']) == [
            {'target': 'x86_64-pc-windows-msvc', 'runner': 'windows-latest'},
            {'target': 'x86_64-pc-windows-msvc', 'runner': 'windows-latest'}]
        for key, valid in [('wasm_crates', 'sample'), ('check_targets', 'x86_64-pc-windows-msvc')]:
            for value in ['missing', ' '+valid, valid+' ', valid+'\n', '/'+valid,
                          '--help', '$(touch marker)', '\ufeff'+valid, valid+'\ufeff',
                          '\u0085'+valid, valid+'\u0085']:
                path.write_text(f'wasm_crates=[]\ncheck_targets=[]\n'.replace(f'{key}=[]', f'{key}=[{json.dumps(value)}]'), encoding="utf-8")
                result, values = declaration(directory)
                assert result.returncode != 0 and not values, value + '\n' + result.stdout + result.stderr
                assert key in result.stderr and '.github/ci.toml' in result.stderr
                assert not (directory / 'marker').exists()
        path.write_text('wasm_crates=[]\ncheck_targets=["x86_64-unknown-linux-gnu"]', encoding="utf-8")
        result, values = declaration(directory)
        assert result.returncode != 0 and 'runner' in result.stderr and not values, result.stdout + result.stderr

declared_members_are_literal_arguments()
print('PASS declared_members_are_literal_arguments')

def privileged_trigger_workflows_are_validated():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        workflows = directory / '.github/workflows'
        workflows.mkdir(parents=True)
        (workflows / 'gate.yml').write_text('name: Gate\n', encoding="utf-8")
        (workflows / 'directory.yml').mkdir()
        path = directory / '.github/ci.toml'
        base = 'wasm_crates=[]\ncheck_targets=[]\nprivileged_trigger_workflows='
        failures = []
        for case, entries in [
            ('missing', ['.github/workflows/missing.yml']),
            ('glob', ['.github/workflows/*.yml']),
            ('outside', ['gate.yml']),
            ('duplicate', ['.github/workflows/gate.yml', '.github/workflows/gate.yml']),
            ('empty', ['']), ('parent', ['.github/workflows/../gate.yml']),
            ('nested', ['.github/workflows/nested/gate.yml']),
            ('colon', ['.github/workflows/gate:2.yml']),
            ('backslash', ['.github/workflows/gate\\name.yml']),
            ('control', ['.github/workflows/gate\n.yml']),
            ('directory', ['.github/workflows/directory.yml']),
            ('extension', ['.github/workflows/gate.txt']),
            ('scalar', 'gate.yml'), ('non-string', [1]),
        ]:
            path.write_text(base + json.dumps(entries), encoding="utf-8")
            result, values = declaration(directory)
            valid = result.returncode != 0 and not values and 'privileged_trigger_workflows' in result.stderr and 'unknown keys' not in result.stderr
            print(f'{"PASS" if valid else "FAIL"} privileged-trigger validation {case}: exit {result.returncode}', flush=True)
            if not valid:
                failures.append(case + ': ' + result.stderr)
        for entries in [[], ['.github/workflows/gate.yml']]:
            path.write_text(base + json.dumps(entries), encoding="utf-8")
            result, values = declaration(directory)
            assert result.returncode == 0, result.stdout + result.stderr
            assert json.loads(values['quality_config']) == {'privileged_trigger_workflows': entries}
        print('PASS privileged-trigger declaration round-trip', flush=True)
        assert not failures, '\n'.join(failures)

privileged_trigger_workflows_are_validated()

def quality_extensions_are_validated():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        (directory / '.github').mkdir()
        path = directory / '.github/ci.toml'
        base = 'wasm_crates=[]\ncheck_targets=[]\n'
        for value in ['79', '101', 'true', '"80"', 'nan', 'inf']:
            path.write_text(base + 'coverage_min_lines=' + value, encoding="utf-8")
            result, values = declaration(directory)
            assert result.returncode != 0 and not values, value + '\n' + result.stdout + result.stderr
            assert 'coverage_min_lines' in result.stderr
        for text in ['spelling_words="word"', 'spelling_words=[1]', 'spelling_words=[""]',
                     'git_sources=["*"]', 'git_sources=["https://example.com/repo?prefix=true"]',
                     'git_sources=["https://example.com/repo#branch"]',
                     'license_exceptions=[{package="sample", version="*", licenses=["MIT"]}]',
                     'license_exceptions=[{package="sample", version="0.1.0", licenses=[]}]',
                     'license_exceptions=[{package="sample", version="0.1.0", licenses=["MIT"], extra=true}]']:
            path.write_text(base + text, encoding="utf-8")
            result, values = declaration(directory)
            assert result.returncode != 0 and not values, text + '\n' + result.stdout + result.stderr
        path.write_text(base + 'coverage_min_lines=90\nspelling_words=["Maestro"]\n'
                        'git_sources=["https://github.com/example/repo"]\n'
                        'license_exceptions=[{package="sample", version="0.1.0", licenses=["GPL-3.0-only"]}]', encoding="utf-8")
        result, values = declaration(directory)
        assert result.returncode == 0, result.stderr
        config = json.loads(values['quality_config'])
        assert config['coverage_min_lines'] == 90
        assert config['spelling_words'] == ['Maestro']
        assert config['license_exceptions'][0]['version'] == '0.1.0'
        assert config['git_sources'] == ['https://github.com/example/repo']
        # Each consumer call includes the job: its failure propagates to both required results.
        checks = workflow.split('  checks:\n', 1)[1].split('  targets:\n', 1)[0]
        assert 'needs: declaration' in checks and not re.search(r'^    continue-on-error:', checks, re.MULTILINE)
        assert 'QUALITY_CONFIG: $' + '{{ needs.declaration.outputs.quality_config }}' in checks
        assert 'Isolated coverage' not in checks
        aggregate = workflow.split('  tests-result:\n', 1)[1].split('  checks:\n', 1)[0]
        assert 'QUALITY_CONFIG: $' + '{{ needs.declaration.outputs.quality_config }}' in aggregate

quality_extensions_are_validated()
print('PASS quality_extensions_are_validated')

def quality_strings_reject_controls_and_surrogates():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/sample', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        (directory / '.github').mkdir()
        path = directory / '.github/ci.toml'
        base = 'wasm_crates=[]\ncheck_targets=[]\n'
        for key, text in [
            ('spelling_words', 'spelling_words=["word\\u0001"]'),
            ('git_sources', 'git_sources=["https://example.com/repo\\u0001"]'),
            ('license_exceptions.package', 'license_exceptions=[{package="sample\\u0001", version="0.1.0", licenses=["MIT"]}]'),
            ('license_exceptions.version', 'license_exceptions=[{package="sample", version="0.1.0\\u0001", licenses=["MIT"]}]'),
            ('license_exceptions.licenses', 'license_exceptions=[{package="sample", version="0.1.0", licenses=["MIT\\u0001"]}]'),
        ]:
            path.write_text(base + text, encoding="utf-8")
            result, values = declaration(directory)
            assert result.returncode != 0 and not values, key + ': control accepted\n' + result.stdout + result.stderr
            assert key in result.stderr, result.stderr
            print(f'PASS quality_strings_reject_controls: {key}', flush=True)
        path.write_text(base + 'spelling_words=["\\uD800"]', encoding="utf-8")
        result, values = declaration(directory)
        assert result.returncode != 0 and not values, result.stdout + result.stderr
        assert '.github/ci.toml' in result.stderr and 'Unicode scalar' in result.stderr, result.stderr
        print('PASS quality_strings_reject_surrogates', flush=True)

quality_strings_reject_controls_and_surrogates()

def credential_names_fail_closed():
    with tempfile.TemporaryDirectory() as temporary:
        for role in ['1', 'doctests']:
            for name in credential_names + ['DEEPSEEK_API_KEY']:
                for value in ['', 'secret-canary-do-not-print']:
                    result = execute('Isolated tests', root / 'fixtures/sample',
                                     {name: value, 'RUNNER_TEMP': temporary, 'SHARD': role, 'COLLECT_COVERAGE': 'true'})
                    assert result.returncode != 0, result.stdout + result.stderr
                    assert name in result.stderr, result.stderr
                    assert 'secret-canary-do-not-print' not in result.stdout + result.stderr
                    assert 'Compiling' not in result.stderr
        result = execute('Isolated tests', root / 'fixtures/sample',
                         {'AZURE_EXTENSION_DIR': 'allowed', 'RUNNER_TEMP': temporary})
        assert result.returncode == 0, result.stdout + result.stderr

credential_names_fail_closed()
print('PASS credential_names_fail_closed')

def isolated_tests_ignore_ambient_state():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        home = directory / 'original-home'
        home.mkdir()
        (home / '.maestro').mkdir()
        sentinel = home / '.maestro/auth.json'
        sentinel.write_text('original-auth-canary', encoding="utf-8")
        (home / 'original-home-marker').write_text('controlled-home', encoding="utf-8")
        env = {'HOME': str(home), 'CARGO_HOME': os.environ.get('CARGO_HOME', str(pathlib.Path.home() / '.cargo')),
               'RUSTUP_HOME': os.environ.get('RUSTUP_HOME', str(pathlib.Path.home() / '.rustup')),
               'RUNNER_TEMP': temporary, 'UNRELATED_AMBIENT_CANARY': 'must-not-inherit'}
        result = execute('Isolated tests', root / 'fixtures/declared', env)
        assert result.returncode == 0, result.stdout + result.stderr
        assert sentinel.read_text(encoding="utf-8") == 'original-auth-canary'
        for fixture in ['credential-dependent', 'home-dependent']:
            contaminated = env | {'OPENAI_API_KEY': 'controlled-provider'}
            command = ['cargo', 'test', '--workspace', '--locked']
            if os.environ.get('MAESTRO_LOCAL_CAPPED'):
                command.insert(0, os.path.expanduser('~/.local/bin/capped'))
                contaminated['CARGO_BUILD_JOBS'] = '3'
            positive = subprocess.run(command, cwd=root / 'fixtures' / fixture,
                                      env=os.environ | contaminated, capture_output=True, text=True)
            assert positive.returncode == 0, positive.stderr
            negative = execute('Isolated tests', root / 'fixtures' / fixture, env)
            assert negative.returncode != 0 and 'FAIL' in negative.stdout + negative.stderr, negative.stderr
            assert sentinel.read_text(encoding="utf-8") == 'original-auth-canary'
        sentinel.unlink()
        result = execute('Isolated tests', root / 'fixtures/declared', env)
        assert result.returncode == 0, result.stdout + result.stderr
        assert not sentinel.exists()

isolated_tests_ignore_ambient_state()
print('PASS isolated_tests_ignore_ambient_state')

def real_search_tools_are_exercised():
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        bin_dir = directory / 'bin'
        bin_dir.mkdir()
        import shutil
        for binary in ['rg', 'fd']:
            (bin_dir / binary).symlink_to(shutil.which(binary))
        result = execute('Isolated tests', root / 'fixtures/declared',
                         {'RUNNER_TEMP': temporary, 'PATH': f'{bin_dir}:/usr/bin:' + os.environ['PATH']})
        assert result.returncode == 0, result.stdout + result.stderr
        assert 'search_commands_read_fixture_files' in result.stdout + result.stderr

real_search_tools_are_exercised()
print('PASS real_search_tools_are_exercised')

def supported_targets_use_native_runners():
    targets_job = workflow.split('  targets:\n', 1)[1].split('\n  browser:', 1)[0]
    assert 'defaults:\n      run:\n        shell: bash\n' in targets_job, \
        'Target commands require Bash, including on Windows'
    assert 'strategy:\n      fail-fast: false\n' in targets_job, \
        'Every target must finish instead of cancelling matrix siblings'
    expected = [
        {'target': 'aarch64-unknown-linux-gnu', 'runner': 'ubuntu-24.04-arm'},
        {'target': 'aarch64-apple-darwin', 'runner': 'macos-latest'},
        {'target': 'x86_64-apple-darwin', 'runner': 'macos-latest'},
        {'target': 'x86_64-pc-windows-msvc', 'runner': 'windows-latest'},
    ]
    result, values = declaration(root / 'fixtures/declared')
    assert result.returncode == 0, result.stderr
    assert json.loads(values['targets']) == expected
    assert 'runs-on: $' + "{{ matrix.runner == 'ubuntu-latest' && fromJSON(needs.declaration.outputs.linux_runner) || matrix.runner }}" in workflow
    assert 'rustup target add "$TARGET"' in body('Check target')
    assert 'rustup component add clippy' in body('Check target')
    assert 'cargo clippy --workspace --all-targets --locked --target "$TARGET" -- -D warnings' in body('Check target')
    if 'GITHUB_OUTPUT' in os.environ:
        with open(os.environ['GITHUB_OUTPUT'], 'a', encoding="utf-8") as output:
            output.write('targets=' + json.dumps(expected + [{'target': 'x86_64-apple-darwin', 'runner': 'macos-15-intel'}]) + '\n')

supported_targets_use_native_runners()
print('PASS supported_targets_use_native_runners')

def wasm_target_failure_is_detected():
    for fixture in ['declared', 'wasm-rejected']:
        result = execute('Build wasm crates', root / 'fixtures' / fixture,
                         {'WASM_CRATES': json.dumps([fixture])})
        if fixture == 'declared':
            assert result.returncode == 0, result.stderr
        else:
            assert result.returncode != 0 and 'fixture rejects wasm32' in result.stderr, result.stderr
            command = ['cargo', 'check', '--workspace', '--all-targets', '--locked']
            if os.environ.get('MAESTRO_LOCAL_CAPPED'):
                command.insert(0, os.path.expanduser('~/.local/bin/capped'))
            host = subprocess.run(command, cwd=root / 'fixtures' / fixture, capture_output=True, text=True)
            assert host.returncode == 0, host.stderr
    # Repeated entries are executed separately rather than silently deduplicated.
    result = execute('Build wasm crates', root / 'fixtures/declared',
                     {'WASM_CRATES': '["declared", "declared"]'})
    assert result.returncode == 0, result.stderr
    assert result.stderr.count('Finished') == 2, result.stderr

wasm_target_failure_is_detected()
print('PASS wasm_target_failure_is_detected')

def foreign_target_warnings_are_denied():
    target = os.environ.get('TARGET', 'x86_64-unknown-linux-gnu')
    for fixture in ['target-field-unix', 'target-field-portable']:
        result = execute('Check target', root / 'fixtures' / fixture, {'TARGET': target})
        output = result.stdout + result.stderr
        rejected = fixture == 'target-field-unix' and target.endswith('windows-msvc')
        if rejected:
            assert result.returncode != 0 and 'field `value` is never read' in output, output
        else:
            assert result.returncode == 0, output
        print(f'PASS foreign_target_warnings_are_denied: {fixture}, {target}, exit {result.returncode}', flush=True)

foreign_target_warnings_are_denied()

def foreign_target_failures_are_detected():
    target = os.environ.get('TARGET')
    if target and ('apple-darwin' in target or target.endswith('windows-msvc')):
        fixture = 'windows-rejected' if target.endswith('windows-msvc') else 'macos-rejected'
        result = execute('Check target', root / 'fixtures' / fixture, {'TARGET': target})
        diagnostic = 'fixture rejects windows' if fixture == 'windows-rejected' else 'fixture rejects macos'
        assert result.returncode != 0 and diagnostic in result.stderr, result.stdout + result.stderr
    else:
        for fixture in ['windows-rejected', 'macos-rejected']:
            command = ['cargo', 'check', '--workspace', '--all-targets', '--locked']
            if os.environ.get('MAESTRO_LOCAL_CAPPED'):
                command.insert(0, os.path.expanduser('~/.local/bin/capped'))
            result = subprocess.run(command, cwd=root / 'fixtures' / fixture, capture_output=True, text=True)
            assert result.returncode == 0, result.stderr

foreign_target_failures_are_detected()
print('PASS foreign_target_failures_are_detected')

def browser_recipe_runs_after_wasm():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/declared', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        env = {'RUNNER_TEMP': temporary, 'GITHUB_PATH': str(directory / 'github-path')}
        bootstrap = execute('Browser tools', directory, env)
        assert bootstrap.returncode == 0, bootstrap.stderr
        paths = directory / 'github-path'
        if paths.exists():
            env['PATH'] = ':'.join(paths.read_text(encoding="utf-8").splitlines()) + ':' + os.environ['PATH']
        result = execute('Build wasm crates', directory, env | {'WASM_CRATES': '["declared"]'})
        assert result.returncode == 0, result.stderr
        result = execute('Browser recipe', directory, env)
        assert result.returncode == 0, result.stderr
        assert (directory / 'browser-built').read_text(encoding="utf-8") == 'built-after-wasm'
        (directory / 'browser-built').unlink()
        command = body('Build wasm crates') + body('Browser recipe')
        argv = ['bash', '-e', '-o', 'pipefail', '-c', command]
        if os.environ.get('MAESTRO_LOCAL_CAPPED'):
            argv.insert(0, os.path.expanduser('~/.local/bin/capped'))
        result = subprocess.run(argv, cwd=directory, env=os.environ | env | {'WASM_CRATES': '["missing-package"]'},
                                capture_output=True, text=True)
        assert result.returncode != 0 and 'missing-package' in result.stderr, result.stderr
        assert not (directory / 'browser-built').exists()
        # A browser-only declaration must still invoke the recipe.
        (directory / 'justfile').write_text('browser-build:\n    @printf browser-only > browser-built\n', encoding="utf-8")
        result = execute('Build wasm crates', directory, {'WASM_CRATES': '[]'})
        assert result.returncode == 0, result.stderr
        result = execute('Browser recipe', directory, env)
        assert result.returncode == 0 and (directory / 'browser-built').read_text(encoding="utf-8") == 'browser-only', result.stderr
    steps = workflow[workflow.index('  browser:'):]
    assert steps.index('name: Build wasm crates') < steps.index('name: Browser recipe')
    for name in ['Browser tool pin', 'Browser recipe']:
        step = next(s for s in steps.split('      - name: ')[1:] if s.splitlines()[0] == name)
        assert "if: needs.declaration.outputs.browser_build == 'true'" in step
        assert 'always()' not in step

browser_recipe_runs_after_wasm()
print('PASS browser_recipe_runs_after_wasm')

def browser_recipe_errors_fail_check():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/declared', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        env = {'RUNNER_TEMP': temporary, 'GITHUB_PATH': str(directory / 'paths')}
        result = execute('Browser tools', directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        paths = directory / 'paths'
        if paths.exists():
            env['PATH'] = ':'.join(paths.read_text(encoding="utf-8").splitlines()) + ':' + os.environ['PATH']
        for text in ['another-recipe:\n    @true\n', 'browser-build:\n    @exit 17\n']:
            (directory / 'justfile').write_text(text, encoding="utf-8")
            result = execute('Browser recipe', directory, env)
            assert result.returncode != 0, result.stdout + result.stderr
        (directory / 'mise.toml').unlink()
        result = execute('Browser tools', directory, {'RUNNER_TEMP': temporary, 'GITHUB_PATH': str(directory / 'paths')})
        assert result.returncode != 0, result.stdout + result.stderr
        (directory / 'mise.toml').write_text('[tools]\n', encoding="utf-8")
        result = execute('Browser tools', directory, env)
        assert result.returncode != 0, 'Missing caller just pin must fail\n' + result.stdout + result.stderr
        (directory / 'mise.toml').write_text('[tools]\njust="not-a-real-version"\n', encoding="utf-8")
        result = execute('Browser tools', directory, {'RUNNER_TEMP': temporary, 'GITHUB_PATH': str(directory / 'paths')})
        assert result.returncode != 0, result.stdout + result.stderr

browser_recipe_errors_fail_check()
print('PASS browser_recipe_errors_fail_check')

def browser_tools_follow_caller_pins():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/declared', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        (directory / 'justfile').write_text('browser-build:\n    @just --version > browser-version\n', encoding="utf-8")
        bin_dir = directory / 'conflict'
        bin_dir.mkdir()
        fake = bin_dir / 'just'
        fake.write_text('#!/bin/sh\nprintf "conflicting just\\n"\nexit 29\n', encoding="utf-8")
        fake.chmod(0o755)
        env = {'PATH': f'{bin_dir}:' + os.environ['PATH'], 'RUNNER_TEMP': temporary,
               'GITHUB_PATH': str(directory / 'github-path')}
        result = execute('Browser tools', directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        paths = pathlib.Path(env['GITHUB_PATH'])
        if paths.exists():
            env['PATH'] = ':'.join(paths.read_text(encoding="utf-8").splitlines()) + ':' + env['PATH']
        result = execute('Browser recipe', directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        assert (directory / 'browser-version').read_text(encoding="utf-8") == 'just 1.58.0\n'

browser_tools_follow_caller_pins()
print('PASS browser_tools_follow_caller_pins')

def browser_test_downloads_verify_every_archive():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        paths, exported = directory / 'paths', directory / 'env'
        env = {'RUNNER_TEMP': temporary, 'GITHUB_PATH': str(paths), 'GITHUB_ENV': str(exported)}
        caller = directory / 'caller with spaces'
        caller.mkdir()
        (caller / 'mise.toml').write_text('[tools]\njust="1.58.0"\n', encoding='utf-8')
        for step in ['Browser test tool pin', 'Browser test mise bootstrap', 'Browser test caller tools']:
            result = execute(step, caller, env)
            assert result.returncode == 0, result.stdout + result.stderr
            if step == 'Browser test mise bootstrap' and paths.exists():
                env['PATH'] = os.pathsep.join(reversed(paths.read_text(encoding='utf-8').splitlines())) + os.pathsep + os.environ['PATH']
        result = execute('Browser runtime tools', caller, env)
        assert result.returncode == 0, result.stdout + result.stderr
        values = dict(line.split('=', 1) for line in exported.read_text(encoding='utf-8').splitlines())
        installed = pathlib.Path(values['PLAYWRIGHT_MODULE']).parent.parent
        cli = paths.read_text(encoding='utf-8').splitlines()[-1]
        assert 'Version 1.64.1' in result.stdout and '156.0.8078.4' in result.stdout
        assert 'wasm-bindgen 0.2.129' in result.stdout
        assert pathlib.Path(values['CHROMIUM_PATH']).is_absolute()
        assert pathlib.Path(values['PLAYWRIGHT_MODULE']).is_absolute()
        shim = directory / 'shim'
        shim.mkdir()
        source = directory / 'shim-source'
        trace = directory / 'unpacked'
        for extractor in ['tar', 'unzip']:
            source.write_text('#!/bin/sh\necho "$*" >> ' + shlex.quote(str(trace)) + '\nexec ' + shlex.quote(shutil.which(extractor)) + ' "$@"\n', encoding='utf-8')
            subprocess.run(['install', '-m', '0700', str(source), str(shim / extractor)], check=True)
        # Only the downloader is replaced: production verification and unpacking remain real.
        def downloader(corrupt):
            source.write_text('#!/usr/bin/env python3\n' + textwrap.dedent(f'''
                import pathlib, shutil, sys
                name = sys.argv[sys.argv.index('-o') + 1]
                if name == {corrupt!r}:
                    pathlib.Path(name).write_bytes(b'corrupted archive')
                else:
                    shutil.copyfile(pathlib.Path({str(installed)!r}) / name, name)
            '''), encoding='utf-8')
            subprocess.run(['install', '-m', '0700', str(source), str(shim / 'curl')], check=True)
        for archive in ['chromium.zip', 'bindgen.tar.gz', 'playwright.tgz', 'playwright-core.tgz']:
            trace.unlink(missing_ok=True)
            downloader(archive)
            result = execute('Browser runtime tools', directory, env | {'PATH': f'{shim}:' + os.environ['PATH']})
            assert result.returncode != 0, archive + '\n' + result.stdout + result.stderr
            unpacked = trace.read_text(encoding='utf-8') if trace.exists() else ''
            if archive == 'chromium.zip':
                assert not unpacked
            elif archive == 'bindgen.tar.gz':
                assert 'bindgen.tar.gz' not in unpacked
            else:
                assert 'playwright' not in unpacked
        source.write_text('#!/bin/sh\nexit 73\n', encoding='utf-8')
        subprocess.run(['install', '-m', '0700', str(source), str(shim / 'curl')], check=True)
        result = execute('Browser runtime tools', directory, env | {'PATH': f'{shim}:' + os.environ['PATH']})
        assert result.returncode == 73, f'exit {result.returncode}\n' + result.stdout + result.stderr
        subprocess.run(['install', '-m', '0700', str(source), str(shim / 'unzip')], check=True)
        downloader(None)
        result = execute('Browser runtime tools', directory, env | {'PATH': f'{shim}:' + os.environ['PATH']})
        assert result.returncode == 73, f'exit {result.returncode}\n' + result.stdout + result.stderr
        assert 'npm install' not in body('Browser runtime tools')
        assert 'cli.js install chromium' not in body('Browser runtime tools')
        print('PASS browser_test_downloads_verify_every_archive', flush=True)
        browser_test_recipe_receives_pinned_tools(directory, env, values, cli)
        browser_test_setup_and_recipe_fail_closed(directory, env, values, cli)

def browser_test_recipe_receives_pinned_tools(directory, env, values, cli):
    caller = directory / 'caller with spaces'
    (caller / 'justfile').write_text('browser-test:\n    wasm-bindgen --version\n    node -e \'console.log(require(process.env.PLAYWRIGHT_MODULE + "/package.json").version)\'\n    "$CHROMIUM_PATH" --version\n', encoding='utf-8')
    decoy = directory / 'decoy'
    decoy.mkdir()
    source = directory / 'decoy-source'
    source.write_text('#!/bin/sh\nexit 69\n', encoding='utf-8')
    subprocess.run(['install', '-m', '0700', str(source), str(decoy / 'wasm-bindgen')], check=True)
    for inherited in [str(decoy) + os.pathsep + os.environ['PATH'], os.environ['PATH'] + os.pathsep + str(decoy)]:
        caller_paths = pathlib.Path(env['GITHUB_PATH']).read_text(encoding='utf-8').splitlines()
        selected = values | env | {'PATH': os.pathsep.join(reversed(caller_paths)) + os.pathsep + inherited}
        assert selected['PATH'].split(os.pathsep)[0] == cli
        result = execute('Browser test recipe', caller, selected)
        assert result.returncode == 0, result.stdout + result.stderr
        assert 'wasm-bindgen 0.2.129' in result.stdout and '1.64.1' in result.stdout and '156.0.8078.4' in result.stdout
    (caller / 'justfile').write_text((root / 'fixtures/declared/justfile').read_text(encoding='utf-8'), encoding='utf-8')
    result = execute('Browser test recipe', caller, selected)
    assert result.returncode == 0 and 'PASS browser runtime fixture' in result.stdout, result.stdout + result.stderr
    print('PASS browser_test_recipe_receives_pinned_tools', flush=True)

def browser_test_setup_and_recipe_fail_closed(directory, env, values, cli):
    caller = directory / 'failing caller'
    caller.mkdir()
    result = execute('Browser test tool pin', caller, env)
    assert result.returncode != 0, result.stdout + result.stderr
    (caller / 'mise.toml').write_text('[tools]\n', encoding='utf-8')
    result = execute('Browser test tool pin', caller, env)
    assert result.returncode != 0, result.stdout + result.stderr
    selected = env | values | {'PATH': cli + os.pathsep + os.environ['PATH']}
    for recipe in ['other:\n    @true\n', 'browser-test:\n    @exit 17\n', 'browser-test:\n    @node -e "throw Error(\'binding mismatch\')"\n']:
        (caller / 'justfile').write_text(recipe, encoding='utf-8')
        result = execute('Browser test recipe', caller, selected)
        assert result.returncode != 0, result.stdout + result.stderr
    shim = caller / 'bin'
    shim.mkdir()
    source = caller / 'tool-failure'
    source.write_text('#!/bin/sh\nexit 73\n', encoding='utf-8')
    subprocess.run(['install', '-m', '0700', str(source), str(shim / 'mise')], check=True)
    result = execute('Browser test caller tools', caller, env | {'PATH': str(shim) + os.pathsep + os.environ['PATH']})
    assert result.returncode == 73, f'exit {result.returncode}\n' + result.stdout + result.stderr
    print('PASS browser_test_setup_and_recipe_fail_closed', flush=True)

def workflow_failures_remain_required():
    internal = (root / '.github/workflows/ci-internal.yml').read_text(encoding="utf-8")
    assert '  declared:\n' in internal and 'working-directory: fixtures/declared' in internal
    assert 'needs: [sample, declared, test-tools]' in internal
    assert 'working-directory: fixtures/test-tools' in internal
    assert 'needs: [seams, native, fuzz-issues, browser-qualification, shard-seams]' in internal
    assert "RESULTS: $" + "{{ toJSON(needs) }}" in internal
    assert 'matrix:\n        include: $' + '{{ fromJSON(needs.seams.outputs.targets) }}' in internal
    for name in ['targets', 'browser', 'tests']:
        section = workflow.split(f'  {name}:\n')[1]
        assert ('needs: [declaration, test-build, test-prepare]' if name == 'tests' else 'needs: declaration') in section
    assert 'pull_request:' in workflow and 'merge_group:' in workflow
    assert "if: $" + "{{ needs.declaration.outputs.coverage == 'true' && !inputs.advisory-scan && !inputs.fuzz-scan && (!inputs.warm-cache || (github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch))) && (needs.declaration.outputs.wasm_crates != '[]' || needs.declaration.outputs.browser_build == 'true') }}" in workflow
    # Each scan mode runs only the declaration and its own job.
    jobs = dict(re.findall(r'^  ([\w-]+):\n(.*?)(?=^  [\w-]+:\n|\Z)', workflow.split('\njobs:\n', 1)[1], re.MULTILINE | re.DOTALL))
    assert "    if: $" + "{{ inputs.advisory-scan && !inputs.warm-cache }}\n" in jobs['advisories'] and 'advisory-scan' not in jobs['declaration']
    assert all('!inputs.advisory-scan && !inputs.fuzz-scan' in job.split('\n    steps:', 1)[0]
               for name, job in jobs.items() if name not in ('declaration', 'advisories', 'fuzz')), sorted(jobs)

def browser_test_result_stays_required():
    internal = (root / '.github/workflows/ci-internal.yml').read_text(encoding='utf-8')
    assert 'needs: [sample, declared, test-tools]' in internal
    assert "all(job['result'] == 'success'" in internal
    assert not re.search(r'^    continue-on-error:', workflow, re.MULTILINE)
    job = workflow.split('  browser-tests:\n', 1)[1]
    assert "needs.declaration.outputs.browser_tests == 'true'" in job
    assert 'persist-credentials: false' in job
    workflow_failures_remain_required()
    print('PASS browser_test_result_stays_required')

browser_test_downloads_verify_every_archive()
browser_test_result_stays_required()

def mise_bootstrap_checks_release_digest():
    import shutil
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        shutil.copytree(root / 'fixtures/declared', directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('target'))
        paths = directory / 'paths'
        env = {'RUNNER_TEMP': temporary, 'GITHUB_PATH': str(paths)}
        # Start without mise: production must fetch and verify the official release.
        env['PATH'] = '/usr/bin:/bin'
        result = execute('Mise bootstrap', directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        installed_bin = paths.read_text(encoding="utf-8").splitlines()[-1]
        installed = pathlib.Path(installed_bin) / 'mise'
        assert subprocess.check_output([str(installed), '--version'], text=True).split()[0] == '2026.10.2'
        bin_dir = directory / 'bin'
        bin_dir.mkdir()
        (bin_dir / 'mise').symlink_to(installed)
        curl = bin_dir / 'curl'
        curl.write_text('#!/bin/sh\nexit 77\n', encoding="utf-8")
        curl.chmod(0o755)
        env['PATH'] = f'{bin_dir}:/usr/bin:/bin'
        before = paths.read_text(encoding="utf-8")
        result = execute('Mise bootstrap', directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        assert paths.read_text(encoding="utf-8") == before  # Matching version is reused, no download.
        (bin_dir / 'mise').unlink()
        (bin_dir / 'mise').write_text('#!/bin/sh\necho 0.0.0\n', encoding="utf-8")
        (bin_dir / 'mise').chmod(0o755)
        curl.write_text('#!/bin/sh\nexec /usr/bin/curl "$@"\n', encoding="utf-8")
        result = execute('Mise bootstrap', directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
        assert paths.read_text(encoding="utf-8") != before  # Wrong version selects verified release.
        before = paths.read_text(encoding="utf-8")
        # A hostile download is data until the production checksum has passed.
        curl.write_text('#!/bin/sh\nwhile [ "$1" != --output ]; do shift; done\n'
                        'printf "#!/bin/sh\\ntouch checksum-bypass-marker\\n" > "$2"\n', encoding="utf-8")
        result = execute('Mise bootstrap', directory, env)
        assert result.returncode != 0, result.stdout + result.stderr
        assert not (directory / 'checksum-bypass-marker').exists()
        assert paths.read_text(encoding="utf-8") == before

mise_bootstrap_checks_release_digest()
print('PASS mise_bootstrap_checks_release_digest')

workflow_failures_remain_required()
print('PASS workflow_failures_remain_required')

def reusable_calls_complete_independently():
    internal = (root / '.github/workflows/ci-internal.yml').read_text(encoding="utf-8")
    group = re.search(r'^  group: (.+)$', workflow, re.MULTILINE).group(1)
    assert group == 'rust-ci-$' + "{{ github.event_name == 'push' && format('{0}-{1}', github.workflow, github.sha) || github.ref }}$" + "{{ inputs.concurrency-key && format('-{0}', inputs.concurrency-key) || '' }}", \
        'The push group must name the caller and the commit, other runs the ref, and keyed calls the key'
    keys = []
    for job in ['sample', 'declared', 'test-tools']:
        section = re.split(r'\n  \S', internal.split(f'  {job}:\n', 1)[1], maxsplit=1)[0]
        key = re.search(r'^      concurrency-key: (.+)$', section, re.MULTILINE)
        assert key, f'{job} must select its own stable concurrency key'
        keys.append(key.group(1).casefold())
    assert len(set(keys)) == 3, 'Concurrency groups are case-insensitive'
    assert 'needs: [sample, declared, test-tools]' in internal
    assert "all(job['result'] == 'success'" in internal
    assert "cancel-in-progress: $" + "{{ github.event_name == 'pull_request' }}" in workflow

reusable_calls_complete_independently()
print('PASS reusable_calls_complete_independently')

def caller_name_stays_in_the_concurrency_group():
    named = [line for line in workflow.splitlines() if re.search(r'github\.workflow\b', line)]
    assert named == [re.search(r'^  group: .+$', workflow, re.MULTILINE).group(0)], \
        'The caller name must not reach artifact names, coverage keys or cache keys'
    assert "name: coverage-$" + "{{ inputs.concurrency-key || 'root' }}-full" in workflow
    assert "COVERAGE_KEY: $" + "{{ inputs.concurrency-key || 'root' }}" in workflow

caller_name_stays_in_the_concurrency_group()
print('PASS caller_name_stays_in_the_concurrency_group')

def workflow_safety_is_unchanged():
    assert re.findall(r'^        run: (cargo .+)$', workflow, re.MULTILINE) + ['cargo test --workspace --locked'] == [
        'cargo fmt --all --check',
        'cargo clippy --workspace --all-targets --locked -- -D warnings',
        'cargo doc --workspace --no-deps --locked',
        'cargo test --workspace --locked',
    ]
    assert not re.search(r'^    continue-on-error:', workflow, re.MULTILINE) and '|| true' not in workflow
    assert 'permissions:\n  contents: read' in workflow
    assert "cancel-in-progress: $" + "{{ github.event_name == 'pull_request' }}" in workflow
    for step in workflow.split('      - name: ')[1:]:
        if 'uses:' in step:
            assert re.search(r'uses: [^\n]+@[0-9a-f]{40}', step)
        if step.startswith('Checkout\n'):
            assert 'persist-credentials: false' in step
    assert 'RUSTDOCFLAGS: -D warnings -D missing_docs' in workflow
    assert 'run_isolated cargo test --locked --doc' in body('Isolated tests')
    for job in ['quality', 'docs', 'tests', 'targets', 'browser', 'browser-tests', 'declaration']:
        section = workflow.split(f'  {job}:\n', 1)[1]
        assert 'run: rustup show' in section or 'rustup show\n' in section
    internal = (root / '.github/ci-internal/each-broken-fixture-fails-only-its-own-check.py').read_text(encoding="utf-8")
    assert "('unformatted', 0), ('clippy-warning', 1)" in internal
    assert "('failing-test', 3), ('undocumented', 2), ('broken-doc-link', 2)" in internal
    # Only the base-vs-isolated tests branch may condition an original command.
    for step in workflow.split('      - name: ')[1:]:
        if any(f'run: {command}\n' in step for command in [
                'cargo fmt --all --check',
                'cargo clippy --workspace --all-targets --locked -- -D warnings',
                'cargo doc --workspace --no-deps --locked']):
            assert not re.search(r'^        if:', step, re.MULTILINE)

workflow_safety_is_unchanged()
print('PASS workflow_safety_is_unchanged')
