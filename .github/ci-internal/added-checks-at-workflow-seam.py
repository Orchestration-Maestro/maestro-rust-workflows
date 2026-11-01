import json
import os
import pathlib
import re
import shlex
import shutil
import subprocess
import tempfile
import textwrap
import time
import tomllib

workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding="utf-8")
root = pathlib.Path.cwd()

def body(name):
    step = next(s for s in workflow.split('      - name: ')[1:] if s.splitlines()[0] == name)
    match = re.search(r'^        run: (?:&[\w-]+ )?\|\n((?:          .*\n|\n)+)', step, re.MULTILINE)
    assert match, name
    return textwrap.dedent(match.group(1))

def execute(name, directory, env):
    argv = ['bash', '-e', '-o', 'pipefail', '-c', body(name)]
    if os.environ.get('MAESTRO_LOCAL_CAPPED'):
        argv.insert(0, os.path.expanduser('~/.local/bin/capped'))
        env = env | {'CARGO_BUILD_JOBS': '3'}
    return subprocess.run(argv, cwd=directory, env=env, capture_output=True, text=True)

def fuzz_licences_allow_ncsa_only_for_libfuzzer_sys(env, temporary):
    directory = pathlib.Path(temporary) / 'fuzz-licences'
    def crate(path, name, license, dependencies=()):
        (directory / path / 'src').mkdir(parents=True)
        (directory / path / 'src/lib.rs').write_text('', encoding='utf-8')
        (directory / path / 'Cargo.toml').write_text(
            f'[package]\nname = "{name}"\nversion = "0.1.0"\nedition = "2021"\nlicense = "{license}"\n'
            + ('[workspace]\n' if path == 'fuzz' else '') + '[dependencies]\n'
            + ''.join(f'{dependency} = {{ path = "../{dependency}" }}\n' for dependency in dependencies), encoding='utf-8')
    crate('libfuzzer-sys', 'libfuzzer-sys', '(MIT OR Apache-2.0) AND NCSA')
    crate('other', 'other', 'NCSA')
    for dependencies, allowed in [(['libfuzzer-sys'], True), (['libfuzzer-sys', 'other'], False)]:
        shutil.rmtree(directory / 'fuzz', ignore_errors=True)
        crate('fuzz', 'caller-fuzz', 'MIT', dependencies)
        subprocess.run(['cargo', 'generate-lockfile', '--offline', '--manifest-path', 'fuzz/Cargo.toml'], cwd=directory, check=True)
        result = execute('Fuzz licences', directory, env | {'FUZZ_MANIFEST': 'fuzz/Cargo.toml'})
        output = result.stdout + result.stderr
        if allowed:
            assert result.returncode == 0, output
        else:
            assert result.returncode != 0 and 'other' in output and 'NCSA' in output, output

def boost_license_is_allowed_and_business_source_is_not(env, temporary):
    directory = pathlib.Path(temporary) / 'boost-license'
    shutil.copytree(root / 'fixtures/sample', directory, ignore=shutil.ignore_patterns('target'))
    manifest = (root / 'fixtures/sample/Cargo.toml').read_text(encoding='utf-8')
    assert 'license = "MIT"\n' in manifest
    for license, allowed in [('BSL-1.0', True), ('BUSL-1.1', False)]:
        (directory / 'Cargo.toml').write_text(manifest.replace('license = "MIT"', f'license = "{license}"'), encoding='utf-8')
        result = execute('Licenses and dependencies', directory, env | {'CHECK_ADVISORIES': 'false'})
        output = result.stdout + result.stderr
        if allowed:
            assert result.returncode == 0, output
        else:
            assert result.returncode != 0 and 'error[rejected]' in output and license in output, output

def secrets_scan_before_restored_build_output(env, temporary):
    directory = pathlib.Path(temporary) / 'source-only'
    directory.mkdir()
    subprocess.run(['git', 'init', '--quiet', str(directory)], check=True)
    secret = (root / 'fixtures/fake-secret/secret.txt').read_text(encoding='utf-8')
    source = directory / 'secret.txt'
    target = directory / 'target/llvm-cov-target/compiled-test'
    steps = workflow.split('  checks:\n', 1)[1].split('\n  targets:', 1)[0]
    names = [step.splitlines()[0] for step in steps.split('      - name: ')[1:]]
    assert names.index('Check tools') < names.index('Secrets')
    assert names.index('Embedded check defaults') < names.index('Secrets')
    assert names.index('Secrets') < names.index('Compile cache')
    for tracked in [False, True]:
        if tracked:
            source.write_text(secret, encoding='utf-8')
            subprocess.run(['git', 'add', 'secret.txt'], cwd=directory, check=True)
            target.unlink()
        for name in names:
            if name == 'Secrets':
                result = execute(name, directory, env)
                output = result.stdout + result.stderr
                assert result.returncode == (1 if tracked else 0), output
                assert ('leaks found: 1' in output) == tracked, output
            elif name == 'Compile cache':
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(secret, encoding='utf-8')
        assert target.read_text(encoding='utf-8') == secret
    print('PASS secrets_scan_before_restored_build_output: target ignored; tracked source caught', flush=True)

def yanked_exceptions_accept_exact_pairs(env, temporary):
    import shutil
    directory = pathlib.Path(temporary) / 'yanked-declaration'
    shutil.copytree(root / 'fixtures/yanked-crates', directory, ignore=shutil.ignore_patterns('target'))
    declaration = directory / '.github/ci.toml'
    declaration.parent.mkdir()
    declaration.write_text('wasm_crates=[]\ncheck_targets=[]\nyanked_crate_exceptions=[{package="once_cell", version="0.0.1"}]\n', encoding='utf-8')
    output = pathlib.Path(temporary) / 'yanked-output'
    result = execute('Read caller declaration', directory, env | {'GITHUB_OUTPUT': str(output)})
    assert result.returncode == 0, result.stdout + result.stderr
    values = dict(line.split('=', 1) for line in output.read_text(encoding='utf-8').splitlines())
    assert json.loads(values['quality_config']) == {'yanked_crate_exceptions': [{'package': 'once_cell', 'version': '0.0.1'}]}
    result = execute('Embedded check defaults', directory, env | {'QUALITY_CONFIG': values['quality_config']})
    assert result.returncode == 0, result.stdout + result.stderr
    generated = tomllib.loads((pathlib.Path(env['CHECK_DEFAULTS']) / 'deny.toml').read_text(encoding='utf-8'))
    assert generated['advisories'] == {'yanked': 'deny', 'ignore': [{'crate': 'once_cell@=0.0.1'}]}
    print('PASS yanked_exceptions_accept_exact_pairs', flush=True)

def yanked_exceptions_reject_unlocked_identity(env, temporary):
    import shutil
    directory = pathlib.Path(temporary) / 'yanked-identity'
    shutil.copytree(root / 'fixtures/yanked-crates', directory, ignore=shutil.ignore_patterns('target'))
    for package, version in [('once_cell', '0.0.1+reviewed'), ('once_cell', '0.0.3'), ('missing-package', '0.0.1')]:
        result = execute('Embedded check defaults', directory, env | {
            'QUALITY_CONFIG': json.dumps({'yanked_crate_exceptions': [{'package': package, 'version': version}]})})
        assert result.returncode != 0, result.stdout + result.stderr
        assert f'Cargo.lock: stale yanked-crate exception for {package} {version}' in result.stderr, result.stdout + result.stderr
    print('PASS yanked_exceptions_reject_unlocked_identity: 3 cases', flush=True)

def yanked_exceptions_reject_malformed_entries(env, temporary):
    import shutil
    directory = pathlib.Path(temporary) / 'yanked-invalid'
    shutil.copytree(root / 'fixtures/sample', directory, ignore=shutil.ignore_patterns('target'))
    declaration = directory / '.github/ci.toml'
    declaration.parent.mkdir()
    output = pathlib.Path(temporary) / 'invalid-output'
    invalid = ['"once_cell"', '["once_cell"]', '["once_cell@0.0.1"]', '[1]',
                  '[{package="once_cell"}]', '[{package="once_cell",version="0.0.1",extra=true}]',
                  '[{package="once_cell",version=1}]'] + [
                  '[{package="once_cell",version=' + json.dumps(version) + '}]'
                  for version in ['', '*', '^0.0.1', '>=0.0.1', '0.0', '00.0.1', '0.0.1-01', '0.0.1\n']
                  ] + ['[{package=' + json.dumps(package) + ',version="0.0.1"}]'
                       for package in ['', '*', 'once_cell@0.0.1', 'once_cell:0.0.1', 'once_cell\n']]
    for value in invalid:
        declaration.write_text('wasm_crates=[]\ncheck_targets=[]\nyanked_crate_exceptions=' + value, encoding='utf-8')
        output.write_text('', encoding='utf-8')
        result = execute('Read caller declaration', directory, env | {'GITHUB_OUTPUT': str(output)})
        assert result.returncode != 0 and 'yanked_crate_exceptions' in result.stderr, result.stdout + result.stderr
        assert not output.read_text(encoding='utf-8')
    print(f'PASS yanked_exceptions_reject_malformed_entries: {len(invalid)} cases', flush=True)

def yanked_exceptions_keep_other_findings(env, temporary):
    import shutil
    directory = pathlib.Path(temporary) / 'yanked-crates'
    shutil.copytree(root / 'fixtures/yanked-crates', directory, ignore=shutil.ignore_patterns('target'))
    entries = [{'package': 'once_cell', 'version': version} for version in ['0.0.1', '0.0.2']]
    for case, config, expected in [
        ('exact-versions', {'yanked_crate_exceptions': entries}, []),
        ('different-version', {'yanked_crate_exceptions': entries[:1]}, ['0.0.2']),
        ('no-exception', {}, ['0.0.1', '0.0.2']),
        ('empty-exceptions', {'yanked_crate_exceptions': []}, ['0.0.1', '0.0.2']),
    ]:
        result = execute('Embedded check defaults', directory, env | {'QUALITY_CONFIG': json.dumps(config)})
        assert result.returncode == 0, result.stdout + result.stderr
        result = execute('Licenses and dependencies', directory, env)
        output = result.stdout + result.stderr
        # Each yank diagnostic quotes its own Cargo.lock entry; other diagnostics name both versions.
        yanked = [version for block in re.split(r'(?m)^(?=\w+\[)', output) if block.startswith('error[yanked]')
                  for version in re.findall(r'once_cell (\S+) registry\+', block)]
        assert (result.returncode == 0) == (not expected) and sorted(yanked) == expected, output
        print(f'PASS yanked_exceptions_{case}: exit {result.returncode}', flush=True)
    # The yank exception does not excuse a security advisory or a license.
    for fixture, package, version, finding in [('advisory-exceptions', 'bincode', '1.3.3', 'RUSTSEC-2025-0141'),
                                               ('disallowed-license', 'sample', '0.1.0', 'error[rejected]')]:
        result = execute('Embedded check defaults', root / 'fixtures' / fixture, env | {
            'QUALITY_CONFIG': json.dumps({'yanked_crate_exceptions': [{'package': package, 'version': version}]})})
        assert result.returncode == 0, result.stdout + result.stderr
        result = execute('Licenses and dependencies', root / 'fixtures' / fixture, env)
        assert result.returncode != 0 and finding in result.stdout + result.stderr, result.stdout + result.stderr
        print(f'PASS yanked_exceptions_keep_{fixture}: exit {result.returncode}', flush=True)
    generated = tomllib.loads((pathlib.Path(env['CHECK_DEFAULTS']) / 'deny.toml').read_text(encoding='utf-8'))
    assert generated['sources'] == {'unknown-registry': 'deny', 'unknown-git': 'deny', 'allow-git': []}
    # An undeclared local git source still fails the native source check.
    upstream = pathlib.Path(temporary) / 'upstream'
    shutil.copytree(root / 'fixtures/sample', upstream, ignore=shutil.ignore_patterns('target'))
    subprocess.run(['git', 'init', '--quiet', str(upstream)], check=True)
    subprocess.run(['git', 'add', '.'], cwd=upstream, check=True)
    subprocess.run(['git', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.com',
                    '-c', 'commit.gpgsign=false', 'commit', '-qm', 'fixture'], cwd=upstream, check=True)
    consumer = pathlib.Path(temporary) / 'source-consumer'
    shutil.copytree(root / 'fixtures/sample', consumer, ignore=shutil.ignore_patterns('target'))
    manifest = consumer / 'Cargo.toml'
    manifest.write_text(manifest.read_text(encoding='utf-8').replace('name = "sample"', 'name = "source-consumer"')
                        + '\n[dependencies]\nsample = {git = ' + json.dumps(upstream.as_uri()) + '}\n', encoding='utf-8')
    command = ['cargo', 'generate-lockfile']
    if os.environ.get('MAESTRO_LOCAL_CAPPED'):
        command.insert(0, os.path.expanduser('~/.local/bin/capped'))
    result = subprocess.run(command, cwd=consumer, env=env, capture_output=True, text=True)
    assert result.returncode == 0, result.stdout + result.stderr
    for value in ['true', 'false']:
        result = execute('Licenses and dependencies', consumer, env | {'CHECK_ADVISORIES': value})
        assert result.returncode != 0 and 'error[source-not-allowed]' in result.stdout + result.stderr, result.stdout + result.stderr
    print(f'PASS yanked_exceptions_keep_sources: exit {result.returncode}', flush=True)

def advisory_exception_suppresses_named_advisory(env, temporary):
    import shutil
    directory = pathlib.Path(temporary) / 'advisory-exceptions'
    shutil.copytree(root / 'fixtures/advisory-exceptions', directory)
    declaration = directory / '.github/ci.toml'
    declaration.parent.mkdir()
    entry = {'id': 'RUSTSEC-2025-0141', 'package': 'bincode', 'version': '1.3.3',
             'reason': 'Packaged syntax dumps; reviewed exception with "quotes" and 𐐀'}
    declaration.write_text('wasm_crates=[]\ncheck_targets=[]\nadvisory_exceptions=['
                           + '{' + ', '.join(key + '=' + json.dumps(value, ensure_ascii=False)
                                            for key, value in entry.items()) + '}]\n', encoding='utf-8')
    output_path = pathlib.Path(temporary) / 'advisory-output'
    result = execute('Read caller declaration', directory, env | {'GITHUB_OUTPUT': str(output_path)})
    assert result.returncode == 0, result.stdout + result.stderr
    values = dict(line.split('=', 1) for line in output_path.read_text(encoding='utf-8').splitlines())
    result = execute('Embedded check defaults', directory, env | {'QUALITY_CONFIG': values['quality_config']})
    assert result.returncode == 0, result.stdout + result.stderr
    generated = tomllib.loads((pathlib.Path(env['CHECK_DEFAULTS']) / 'deny.toml').read_text(encoding='utf-8'))
    assert generated['advisories'] == {'yanked': 'deny', 'ignore': [{'id': entry['id'], 'reason': entry['reason']}]}
    result = execute('Licenses and dependencies', directory, env)
    assert result.returncode == 0, result.stdout + result.stderr
    print('PASS advisory_exception_suppresses_named_advisory', flush=True)
    result = execute('Embedded check defaults', directory, env | {'QUALITY_CONFIG': json.dumps({
        'advisory_exceptions': [entry], 'yanked_crate_exceptions': [{'package': 'bincode', 'version': '1.3.3'}]})})
    assert result.returncode == 0, result.stdout + result.stderr
    generated = tomllib.loads((pathlib.Path(env['CHECK_DEFAULTS']) / 'deny.toml').read_text(encoding='utf-8'))
    assert generated['advisories']['ignore'] == [{'id': entry['id'], 'reason': entry['reason']}, {'crate': 'bincode@=1.3.3'}]
    result = execute('Licenses and dependencies', directory, env)
    assert result.returncode == 0, result.stdout + result.stderr
    print('PASS advisory_and_yanked_exceptions_coexist', flush=True)
    for key, value in [('package', 'missing-package'), ('version', '1.3.2')]:
        stale = entry | {key: value}
        for checked in ['true', 'false']:
            result = execute('Embedded check defaults', directory, env | {
                'CHECK_ADVISORIES': checked, 'QUALITY_CONFIG': json.dumps({'advisory_exceptions': [stale]})})
            assert result.returncode != 0 and result.stderr.splitlines() == [
                f"Cargo.lock: stale advisory exception {stale['id']} for {stale['package']} {stale['version']}"], result.stdout + result.stderr
    print('PASS advisory_exception_rejects_stale_package_or_version', flush=True)
    for case, mismatch, expected in [
        ('other_package', entry | {'package': 'serde', 'version': '1.0.152'}, ['bincode', '1.3.3']),
        ('no_finding', entry | {'id': 'RUSTSEC-2022-0004'}, ['no such finding'])]:
        result = execute('Embedded check defaults', directory, env | {
            'QUALITY_CONFIG': json.dumps({'advisory_exceptions': [mismatch]})})
        output = result.stdout + result.stderr
        assert result.returncode != 0 and mismatch['id'] in output and all(text in output for text in expected), output
        print('PASS advisory_exception_rejects_' + case, flush=True)
    # Omitting the key restores the advisory finding and original defaults.
    declaration.unlink()
    output_path.write_text('', encoding='utf-8')
    result = execute('Read caller declaration', directory, env | {'GITHUB_OUTPUT': str(output_path)})
    assert result.returncode == 0, result.stdout + result.stderr
    values = dict(line.split('=', 1) for line in output_path.read_text(encoding='utf-8').splitlines())
    assert values['quality_config'] == '{}' and values['enabled'] == 'false'
    result = execute('Embedded check defaults', directory, env | {'QUALITY_CONFIG': values['quality_config']})
    assert result.returncode == 0, result.stdout + result.stderr
    generated = tomllib.loads((pathlib.Path(env['CHECK_DEFAULTS']) / 'deny.toml').read_text(encoding='utf-8'))
    assert generated['advisories'] == {'yanked': 'deny'}
    result = execute('Licenses and dependencies', directory, env)
    assert result.returncode != 0 and entry['id'] in result.stdout + result.stderr, result.stdout + result.stderr
    print('PASS advisory_exception_absent_key_keeps_advisories', flush=True)
    for case, suffix in [('without-key', ''), ('empty-array', 'advisory_exceptions=[]\n')]:
        declaration.write_text('wasm_crates=[]\ncheck_targets=[]\n' + suffix, encoding='utf-8')
        output_path.write_text('', encoding='utf-8')
        result = execute('Read caller declaration', directory, env | {'GITHUB_OUTPUT': str(output_path)})
        assert result.returncode == 0, result.stdout + result.stderr
        values = dict(line.split('=', 1) for line in output_path.read_text(encoding='utf-8').splitlines())
        result = execute('Embedded check defaults', directory, env | {'QUALITY_CONFIG': values['quality_config']})
        assert result.returncode == 0, result.stdout + result.stderr
        generated = tomllib.loads((pathlib.Path(env['CHECK_DEFAULTS']) / 'deny.toml').read_text(encoding='utf-8'))
        assert generated['advisories'] == {'yanked': 'deny'}
        result = execute('Licenses and dependencies', directory, env)
        assert result.returncode != 0 and entry['id'] in result.stdout + result.stderr, result.stdout + result.stderr
        print('PASS advisory_exception_' + case + '_keeps_advisories', flush=True)
    # A distinct, unexcused package still fails the same full cargo-deny command.
    manifest = directory / 'Cargo.toml'
    manifest.write_text(manifest.read_text(encoding='utf-8') + 'rustc-serialize = "=0.3.25"\n', encoding='utf-8')
    lock = directory / 'Cargo.lock'
    lock.write_text(lock.read_text(encoding='utf-8').replace(' "bincode",', ' "bincode",\n "rustc-serialize",')
                    + '\n[[package]]\nname = "rustc-serialize"\nversion = "0.3.25"\n'
                    + 'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
                    + 'checksum = "fe834bc780604f4674073badbad26d7219cadfb4a2275802db12cbae17498401"\n', encoding='utf-8')
    result = execute('Embedded check defaults', directory, env | {
        'QUALITY_CONFIG': json.dumps({'advisory_exceptions': [entry]})})
    assert result.returncode == 0, result.stdout + result.stderr
    result = execute('Licenses and dependencies', directory, env)
    assert result.returncode != 0 and 'RUSTSEC-2022-0004' in result.stdout + result.stderr, result.stdout + result.stderr
    print('PASS advisory_exception_keeps_unexcused_advisory', flush=True)
    invalid = [
        ('missing-reason', {key: value for key, value in entry.items() if key != 'reason'}, 'requires id, package, version and reason'),
        ('extra-key', entry | {'extra': 'value'}, 'requires id, package, version and reason'),
        ('range-version', entry | {'version': '^1.3.3'}, 'exact package version'),
        ('bad-id', entry | {'id': 'CVE-2025-0141'}, 'RUSTSEC-YYYY-NNNN'),
        ('empty-reason', entry | {'reason': ''}, 'nonempty strings'),
        ('wrong-type', entry | {'version': 133}, 'nonempty strings'),
        ('invalid-semver', entry | {'version': '1.3.3-01'}, 'exact package version'),
    ]
    invalid += [('control-' + key, entry | {key: value + chr(1)}, 'control characters or surrogates')
                for key, value in entry.items()]
    for case, invalid_entry, diagnostic in invalid:
        declaration.write_text('wasm_crates=[]\ncheck_targets=[]\nadvisory_exceptions=['
                               + '{' + ', '.join(key + '=' + json.dumps(value, ensure_ascii=False)
                                                for key, value in invalid_entry.items()) + '}]\n', encoding='utf-8')
        result = execute('Read caller declaration', directory, env | {'GITHUB_OUTPUT': str(output_path)})
        output = result.stdout + result.stderr
        assert result.returncode != 0 and 'advisory_exceptions' in output and diagnostic in output, case + ': ' + output
        print('PASS advisory_exception_rejects_' + case, flush=True)
    for value in ['"scalar"', '["entry"]']:
        declaration.write_text('wasm_crates=[]\ncheck_targets=[]\nadvisory_exceptions=' + value, encoding='utf-8')
        result = execute('Read caller declaration', directory, env | {'GITHUB_OUTPUT': str(output_path)})
        assert result.returncode != 0 and 'advisory_exceptions' in result.stdout + result.stderr, result.stdout + result.stderr
    print('PASS advisory_exception_rejects_non_array_or_record', flush=True)

def advisory_exceptions_cover_all_reported_versions(env, temporary):
    global workflow
    import shutil
    directory = pathlib.Path(temporary) / 'dual-advisory-exceptions'
    shutil.copytree(root / 'fixtures/advisory-exceptions', directory)
    manifest = directory / 'Cargo.toml'
    manifest.write_text(manifest.read_text(encoding='utf-8')
                        + 'bincode_next = {package="bincode", version="=2.0.1"}\n', encoding='utf-8')
    command = ['cargo', 'generate-lockfile']
    if os.environ.get('MAESTRO_LOCAL_CAPPED'):
        command.insert(0, os.path.expanduser('~/.local/bin/capped'))
    result = subprocess.run(command, cwd=directory, env=env, capture_output=True, text=True)
    assert result.returncode == 0, result.stdout + result.stderr
    entries = [{'id': 'RUSTSEC-2025-0141', 'package': 'bincode', 'version': version,
                'reason': 'Reviewed exact version ' + version} for version in ['1.3.3', '2.0.1']]
    declaration = directory / '.github/ci.toml'
    declaration.parent.mkdir()
    output_path = pathlib.Path(temporary) / 'dual-advisory-output'
    for ordered in [entries, entries[::-1]]:
        declaration.write_text('wasm_crates=[]\ncheck_targets=[]\nadvisory_exceptions=[' + ','.join(
            '{' + ','.join(key + '=' + json.dumps(value) for key, value in entry.items()) + '}'
            for entry in ordered) + ']\n', encoding='utf-8')
        output_path.write_text('', encoding='utf-8')
        result = execute('Read caller declaration', directory, env | {'GITHUB_OUTPUT': str(output_path)})
        assert result.returncode == 0, result.stdout + result.stderr
        values = dict(line.split('=', 1) for line in output_path.read_text(encoding='utf-8').splitlines())
        assert json.loads(values['quality_config'])['advisory_exceptions'] == ordered
        result = execute('Embedded check defaults', directory, env | {'QUALITY_CONFIG': values['quality_config']})
        assert result.returncode == 0, result.stdout + result.stderr
        generated = tomllib.loads((pathlib.Path(env['CHECK_DEFAULTS']) / 'deny.toml').read_text(encoding='utf-8'))
        assert generated['advisories']['ignore'] == [{'id': entry['id'], 'reason': entry['reason']} for entry in ordered]
        result = execute('Licenses and dependencies', directory, env)
        assert result.returncode == 0, result.stdout + result.stderr
    print('PASS advisory_exceptions_cover_all_reported_versions', flush=True)

    for present, missing in [(entries[0], entries[1]), (entries[1], entries[0])]:
        partial = env | {'QUALITY_CONFIG': json.dumps({'advisory_exceptions': [present]})}
        for checked in ['true', 'false', 'true']:
            generated_path = pathlib.Path(env['CHECK_DEFAULTS']) / 'deny.toml'
            generated_path.unlink(missing_ok=True)
            (pathlib.Path(env['CHECK_DEFAULTS']) / 'probe-deny.toml').unlink(missing_ok=True)
            result = execute('Embedded check defaults', directory, partial | {'CHECK_ADVISORIES': checked})
            if checked == 'true':
                assert result.returncode != 0, result.stdout + result.stderr
                assert result.stderr.splitlines() == [
                    f"Cargo.lock: advisory exception {missing['id']} is missing for {missing['package']} {missing['version']}"], result.stderr
                assert not generated_path.exists()
            else:
                assert result.returncode == 0, result.stdout + result.stderr
                assert not (pathlib.Path(env['CHECK_DEFAULTS']) / 'probe-deny.toml').exists()
                result = execute('Licenses and dependencies', directory, env | {'CHECK_ADVISORIES': 'false'})
                assert result.returncode == 0 and 'advisories: not checked' in result.stdout, result.stdout + result.stderr
            (pathlib.Path(env['CHECK_DEFAULTS']) / 'probe-deny.toml').unlink(missing_ok=True)
    print('PASS advisory_exceptions_reject_missing_version', flush=True)

    # A private database fixture makes the newer locked version unaffected, without changing the real database.
    database = pathlib.Path(temporary) / 'selective-advisory-db'
    cargo_home = pathlib.Path(env.get('CARGO_HOME', pathlib.Path.home() / '.cargo'))
    shutil.copytree(cargo_home / 'advisory-dbs', database)
    advisory = next(database.glob('*/crates/bincode/RUSTSEC-2025-0141.md'))
    original = advisory.read_text(encoding='utf-8')
    assert 'patched = []' in original
    advisory.write_text(original.replace('patched = []', 'patched = [">=2.0.1"]'), encoding='utf-8')
    original_workflow = workflow
    try:
        # Change only the native producer's database configuration and disable fetching this controlled copy.
        workflow = workflow.replace("probe.write_text('[advisories]\\nyanked = ",
                                    "probe.write_text('[advisories]\\ndb-path = " + json.dumps(str(database)) + "\\nyanked = ")
        workflow = workflow.replace("'/cargo-deny', '--format'", "'/cargo-deny', '--offline', '--format'")
        assert workflow != original_workflow
        result = execute('Embedded check defaults', directory, env | {
            'QUALITY_CONFIG': json.dumps({'advisory_exceptions': [entries[0]]})})
        assert result.returncode == 0, result.stdout + result.stderr
        result = execute('Embedded check defaults', directory, env | {
            'QUALITY_CONFIG': json.dumps({'advisory_exceptions': [entries[1]]})})
        assert result.returncode != 0 and result.stderr.splitlines() == [
            'Cargo.lock: advisory exception RUSTSEC-2025-0141 is not reported for bincode 2.0.1: bincode 1.3.3'], result.stdout + result.stderr
    finally:
        workflow = original_workflow
        shutil.rmtree(database)
    print('PASS advisory_exceptions_require_only_reported_versions', flush=True)

def advisories_follow_dependency_changes(env, temporary):
    import shutil
    advisory = pathlib.Path(temporary) / 'gated-advisory'
    shutil.copytree(root / 'fixtures/advisory-exceptions', advisory)
    result = execute('Embedded check defaults', advisory, env | {'QUALITY_CONFIG': '{}'})
    assert result.returncode == 0, result.stdout + result.stderr
    result = execute('Licenses and dependencies', advisory, env | {'CHECK_ADVISORIES': 'false'})
    assert result.returncode == 0 and 'advisories: not checked' in result.stdout, result.stdout + result.stderr
    for value in ['true', '']:
        result = execute('Licenses and dependencies', advisory, env | {'CHECK_ADVISORIES': value})
        assert result.returncode != 0 and 'RUSTSEC-2025-0141' in result.stdout + result.stderr, result.stdout + result.stderr
    result = execute('Licenses and dependencies', root / 'fixtures/disallowed-license', env | {'CHECK_ADVISORIES': 'false'})
    assert result.returncode != 0 and 'error[rejected]' in result.stdout + result.stderr, result.stdout + result.stderr
    # A declared exception the database no longer reports fails only where advisories are checked.
    withdrawn = {'id': 'RUSTSEC-2025-9999', 'package': 'bincode', 'version': '1.3.3', 'reason': 'reviewed'}
    for value, finding in [('false', None), ('true', 'is not reported'), ('false', 'stale advisory exception')]:
        declared = withdrawn | ({'version': '9.9.9'} if finding == 'stale advisory exception' else {})
        result = execute('Embedded check defaults', advisory,
                         env | {'CHECK_ADVISORIES': value, 'QUALITY_CONFIG': json.dumps({'advisory_exceptions': [declared]})})
        output = result.stdout + result.stderr
        assert (result.returncode == 0) if finding is None else (result.returncode != 0 and finding in output), output
    print('PASS advisories_follow_dependency_changes: unchanged lockfile skips only advisories', flush=True)
    def scan(directory, config):
        result = execute('Embedded check defaults', directory, env | {'QUALITY_CONFIG': json.dumps(config)})
        assert result.returncode == 0, result.stdout + result.stderr
        output = pathlib.Path(temporary) / 'scan-output'
        output.write_text('', encoding='utf-8')
        result = execute('Undeclared advisories', directory, env | {'GITHUB_OUTPUT': str(output)})
        assert result.returncode == 0, result.stdout + result.stderr
        return json.loads(output.read_text(encoding='utf-8').removeprefix('findings='))
    assert scan(advisory, {}) == [{'id': 'RUSTSEC-2025-0141', 'title': 'Bincode is unmaintained', 'crates': ['bincode 1.3.3']}]
    entry = {'id': 'RUSTSEC-2025-0141', 'package': 'bincode', 'version': '1.3.3', 'reason': 'reviewed'}
    assert scan(advisory, {'advisory_exceptions': [entry]}) == []
    yanked = pathlib.Path(temporary) / 'scanned-yanks'
    shutil.copytree(root / 'fixtures/yanked-crates', yanked, ignore=shutil.ignore_patterns('target'))
    assert [finding['id'] for finding in scan(yanked, {})] == ['yanked once_cell 0.0.1', 'yanked once_cell 0.0.2']
    print('PASS advisory_scan_reports_undeclared_findings_only', flush=True)

def fake_tool(directory, name):
    # Records each call as {"argv", "body"} (standard input for --body-file), answers `issue list` with $OPEN
    # and exits with $FAKE_STATUS.
    directory.mkdir()
    calls = directory / 'calls.jsonl'
    source = directory / f'{name}-source'
    source.write_text('#!/usr/bin/env python3\nimport json, os, sys\n'
                      'body = sys.stdin.read() if "--body-file" in sys.argv else None\n'
                      'with open(os.environ["CALLS"], "a", encoding="utf-8") as calls:\n'
                      '    calls.write(json.dumps({"argv": sys.argv[1:], "body": body}) + "\\n")\n'
                      'print(os.environ["OPEN"] if sys.argv[1:3] == ["issue", "list"] else "https://example.invalid/1")\n'
                      'sys.exit(int(os.environ.get("FAKE_STATUS", "0")))\n', encoding='utf-8')
    subprocess.run(['install', '-m', '0755', str(source), str(directory / name)], check=True)
    def recorded():
        made = [json.loads(line) for line in calls.read_text(encoding='utf-8').splitlines()] if calls.exists() else []
        calls.unlink(missing_ok=True)
        return made
    return {'PATH': f"{directory}{os.pathsep}{os.environ['PATH']}", 'CALLS': str(calls)}, recorded

def advisory_issues_open_once(temporary):
    source = pathlib.Path('.github/workflows/advisories.yml').read_text(encoding='utf-8')
    step = next(s for s in source.split('      - name: ')[1:] if s.splitlines()[0] == 'Open advisory issues')
    code = textwrap.dedent(re.search(r'^        run: \|\n((?:          .*\n|\n)+)', step, re.MULTILINE).group(1))
    fake, recorded = fake_tool(pathlib.Path(temporary) / 'fake-gh', 'gh')
    findings = [{'id': 'RUSTSEC-2025-0141', 'title': 'Bincode is unmaintained', 'crates': ['bincode 1.3.3']},
                {'id': 'yanked once_cell 0.0.1', 'title': 'detected yanked crate', 'crates': ['once_cell 0.0.1']}]
    def report(scan='success', found=findings, dry='false'):
        result = subprocess.run(['python3', '-c', code], capture_output=True, text=True, env=os.environ | fake | {
            'SCAN': scan, 'FINDINGS': json.dumps(found),
            'DRY_RUN': dry, 'RUN_URL': 'https://example.invalid/run', 'REPOSITORY': 'owner/repo', 'GH_TOKEN': 'token',
            'OPEN': json.dumps([{'title': 'Priority: RUSTSEC-2025-0141 on main: bincode 1.3.3'}])})
        return result, [call['argv'] for call in recorded()]
    result, made = report()
    assert result.returncode != 0 and '2 undeclared advisories on main' in result.stderr, result.stdout + result.stderr
    created = [call for call in made if call[:2] == ['issue', 'create']]
    assert len(created) == 1 and created[0][created[0].index('--title') + 1] == 'Priority: yanked once_cell 0.0.1 on main: once_cell 0.0.1', made
    assert ['--label', 'bug', '--label', 'needs-triage'] == created[0][-6:-2] and created[0][-2:] == ['-R', 'owner/repo'], made
    result, made = report(found=[])
    assert result.returncode == 0 and made == [], result.stdout + result.stderr
    result, made = report(scan='failure')
    assert result.returncode != 0 and 'advisory scan failure' in result.stderr and made == [], result.stdout + result.stderr
    result, made = report(dry='true')
    assert result.returncode != 0 and made == [] and result.stdout.count('dry run: ') == 2, result.stdout + result.stderr
    print('PASS advisory_issues_open_once', flush=True)

def fuzz_target_forwards_limits_without_finding(temporary):
    directory = pathlib.Path(temporary) / 'fuzz-target'
    (directory / 'fuzz').mkdir(parents=True)
    fake, recorded = fake_tool(directory / 'tools', 'cargo-fuzz')
    for status in ['0', '1']:
        output = directory / f'output-{status}'
        output.write_text('', encoding='utf-8')
        result = execute('Fuzz target', directory, os.environ | fake | {
            'TOOLS': str(directory / 'tools'), 'FAKE_STATUS': status, 'RUNNER_TEMP': str(directory),
            'GITHUB_OUTPUT': str(output), 'FUZZ_MANIFEST': 'fuzz/Cargo.toml', 'TARGET': 'planted',
            'SECONDS_BUDGET': '60', 'MAX_LEN': '64', 'TIMEOUT': '5', 'RSS_LIMIT_MB': '512'})
        assert output.read_text(encoding='utf-8') == 'found=false\n', result.stderr
        calls = [call['argv'] for call in recorded()]
        assert calls == [['fuzz', 'run', '--fuzz-dir', 'fuzz', '--target', 'x86_64-unknown-linux-gnu', 'planted', '--',
                          '-max_total_time=60', '-max_len=64', '-timeout=5', '-rss_limit_mb=512']], calls
        if status == '0':
            assert result.returncode == 0, result.stderr
        else:
            assert result.returncode != 0, result.stdout
            assert result.stderr.strip().splitlines()[-1] == 'fuzz target planted exited 1 without a finding', result.stderr
        assert not (directory / 'fuzz-finding').exists()
    print('PASS fuzz_target_forwards_limits_without_finding', flush=True)

def privileged_trigger_workflows_ignore_only_declared_audit(env, temporary):
    import shutil
    directory = pathlib.Path(temporary) / 'privileged-trigger'
    shutil.copytree(root / 'fixtures/sample', directory, ignore=shutil.ignore_patterns('target'))
    workflows = directory / '.github/workflows'
    workflows.mkdir(parents=True)
    gate = workflows / 'gate.yml'
    safe = ('name: Gate\non: pull_request_target\npermissions:\n  contents: read\njobs:\n'
            '  gate:\n    runs-on: ubuntu-latest\n    steps:\n'
            '      - name: Label request\n        run: echo reviewed\n')
    gate.write_text(safe, encoding="utf-8")
    extension = {'privileged_trigger_workflows': ['.github/workflows/gate.yml']}
    failures = []
    for case, config, extra, expected in [
        ('declared', extension, '', None),
        ('undeclared', {}, '', 'dangerous-triggers'),
        ('other-audit', extension, '      - uses: actions/checkout@v7\n', 'unpinned-uses'),
        ('unlisted-file', extension, '', 'dangerous-triggers'),
    ]:
        gate.write_text(safe + extra, encoding="utf-8")
        other = workflows / 'other.yaml'
        if case == 'unlisted-file':
            other.write_text(safe, encoding="utf-8")
        declaration_path = directory / '.github/ci.toml'
        if config:
            declaration_path.write_text('wasm_crates=[]\ncheck_targets=[]\nprivileged_trigger_workflows='
                                        + json.dumps(config['privileged_trigger_workflows']), encoding="utf-8")
        elif declaration_path.exists():
            declaration_path.unlink()
        output_path = pathlib.Path(temporary) / 'declaration-output'
        output_path.write_text('', encoding="utf-8")
        result = execute('Read caller declaration', directory, env | {'GITHUB_OUTPUT': str(output_path)})
        assert result.returncode == 0, result.stdout + result.stderr
        values = dict(line.split('=', 1) for line in output_path.read_text(encoding="utf-8").splitlines())
        result = execute('Embedded check defaults', directory, env | {'QUALITY_CONFIG': values['quality_config']})
        assert result.returncode == 0, result.stdout + result.stderr
        generated = json.loads((pathlib.Path(env['CHECK_DEFAULTS']) / 'zizmor.yaml').read_text(encoding="utf-8"))
        assert generated == ({'rules': {'dangerous-triggers': {'ignore': ['gate.yml']}}}
                             if config else {'rules': {}})
        result = execute('Workflows', directory, env)
        output = result.stdout + result.stderr
        valid = result.returncode == 0 if expected is None else result.returncode != 0 and expected in output
        if case == 'other-audit':
            valid = valid and 'dangerous-triggers' not in output
        print(f'{"PASS" if valid else "FAIL"} privileged-trigger {case}: exit {result.returncode}', flush=True)
        if not valid:
            failures.append(case + ': ' + output)
        if other.exists():
            other.unlink()
    for location in ['zizmor.yml', '.github/zizmor.yml']:
        config_path = directory / location
        config_path.write_text('rules:\n  unpinned-uses:\n    disable: true\n', encoding="utf-8")
    gate.write_text(safe + '      - uses: actions/checkout@v7\n', encoding="utf-8")
    result = execute('Embedded check defaults', directory, env | {'QUALITY_CONFIG': json.dumps(extension)})
    assert result.returncode == 0, result.stdout + result.stderr
    result = execute('Workflows', directory, env)
    assert result.returncode != 0 and 'unpinned-uses' in result.stdout + result.stderr, result.stdout + result.stderr
    print('PASS privileged-trigger repository configs ignored', flush=True)
    gate.write_text(safe + '  # ZiZmOr : IGNORE[dangerous-triggers]\n', encoding="utf-8")
    result = execute('Workflows', directory, env)
    output = result.stdout + result.stderr
    assert result.returncode != 0 and 'gate.yml:11:' in output and 'declaration' in output, output
    print('PASS privileged-trigger inline ignores rejected', flush=True)
    # Quoted supplementary Unicode filenames survive the sole value encoder.
    quoted = workflows / 'gate-𐐀".yaml'
    gate.rename(quoted)
    quoted.write_text(safe, encoding="utf-8")
    config = {'privileged_trigger_workflows': ['.github/workflows/' + quoted.name]}
    result = execute('Embedded check defaults', directory, env | {'QUALITY_CONFIG': json.dumps(config)})
    assert result.returncode == 0, result.stdout + result.stderr
    result = execute('Workflows', directory, env)
    if result.returncode:
        failures.append('quoted Unicode filename: ' + result.stdout + result.stderr)
    else:
        print('PASS privileged-trigger quoted Unicode filename', flush=True)
    assert not failures, '\n'.join(failures)

checks = ['Licenses and dependencies', 'Unused dependencies', 'Spelling', 'Secrets', 'Workflows', 'Coverage']
base = re.findall(r'^        run: (cargo .+)$', workflow, re.MULTILINE) + ['cargo test --workspace --locked']
assert len(base) == 4
with tempfile.TemporaryDirectory() as temporary:
    env = os.environ | {'RUNNER_TEMP': temporary, 'GITHUB_PATH': temporary + '/paths',
                        'GITHUB_ENV': temporary + '/environment', 'QUALITY_CONFIG': '{}'}
    start = time.monotonic()
    installed = execute('Check tools', root, env)
    assert installed.returncode == 0, installed.stdout + installed.stderr
    env.update(dict(line.split('=', 1) for line in pathlib.Path(env['GITHUB_ENV']).read_text(encoding="utf-8").splitlines()))
    assert not pathlib.Path(env['TOOLS'], 'cargo-fuzz').exists()
    fuzz_env = env | {'GITHUB_ENV': temporary + '/fuzz-environment', 'FUZZ_SCAN': 'true'}
    installed = execute('Check tools', root, fuzz_env)
    assert installed.returncode == 0, installed.stdout + installed.stderr
    fuzz_tools = pathlib.Path(fuzz_env['GITHUB_ENV']).read_text(encoding='utf-8').removeprefix('TOOLS=').strip()
    version = subprocess.run([fuzz_tools + '/cargo-fuzz', 'fuzz', '--version'], capture_output=True, text=True)
    assert version.returncode == 0 and version.stdout == 'cargo-fuzz 0.13.2\n', version.stdout + version.stderr
    print('PASS fuzz_tool_only_in_fuzz_scan', flush=True)
    component = subprocess.run(['rustup', 'component', 'add', 'llvm-tools-preview'],
                               cwd=root / 'fixtures/sample', capture_output=True, text=True)
    assert component.returncode == 0, component.stderr
    configured = execute('Embedded check defaults', root, env)
    assert configured.returncode == 0, configured.stdout + configured.stderr
    env.update(dict(line.split('=', 1) for line in pathlib.Path(env['GITHUB_ENV']).read_text(encoding="utf-8").splitlines()))
    secrets_scan_before_restored_build_output(env, temporary)
    fuzz_licences_allow_ncsa_only_for_libfuzzer_sys(env, temporary)
    print('PASS fuzz_licences_allow_ncsa_only_for_libfuzzer_sys', flush=True)
    boost_license_is_allowed_and_business_source_is_not(env, temporary)
    print('PASS boost_license_is_allowed_and_business_source_is_not', flush=True)
    for name in checks:
        result = execute(name, root / 'fixtures/sample', env)
        assert result.returncode == 0, name + '\n' + result.stdout + result.stderr
        print(f'sample: {name}: exit {result.returncode}', flush=True)
    print(f'Added sample CI time including verified installation: {time.monotonic() - start:.2f}s', flush=True)
    # Root self-calls keep the runner-compatible syntax until actionlint accepts $/.
    root_config = pathlib.Path(temporary) / 'root-zizmor.yml'
    root_config.write_text('rules:\n  self-repository:\n    ignore:\n'
                           '      - ci-internal.yml\n'
                           '      - ci-proof.yml\n'
                           '      - advisories.yml\n'
                           '      - fuzz.yml\n', encoding="utf-8")
    # Moving lines inside a root workflow must not revive a self-repository finding.
    shifted = pathlib.Path(temporary) / 'shifted'
    shutil.copytree(root / '.github/workflows', shifted / '.github/workflows')
    for path in (shifted / '.github/workflows').glob('*.yml'):
        path.write_text('# moved\n# moved\n' + path.read_text(encoding='utf-8'), encoding='utf-8')
    zizmor = [env['TOOLS'] + '/zizmor', '--offline', '--strict-collection', '--config', str(root_config), '.github/workflows']
    result = subprocess.run(zizmor, cwd=shifted, env=env, capture_output=True, text=True)
    assert result.returncode == 0, 'zizmor failed after lines moved\n' + result.stdout + result.stderr
    print('PASS self_repository_ignore_survives_line_shifts')
    for argv in [zizmor,
                 [env['TOOLS'] + '/actionlint', '-config-file', env['CHECK_DEFAULTS'] + '/actionlint.yaml', '-shellcheck=', '-pyflakes=',
                  *map(str, (root / '.github/workflows').glob('*.yml'))]]:
        result = subprocess.run(argv, cwd=root, env=env, capture_output=True, text=True)
        assert result.returncode == 0, result.stdout + result.stderr
    print('PASS repository_workflow_checks')
    yanked_exceptions_accept_exact_pairs(env, temporary)
    yanked_exceptions_reject_unlocked_identity(env, temporary)
    yanked_exceptions_reject_malformed_entries(env, temporary)
    yanked_exceptions_keep_other_findings(env, temporary)
    advisory_exception_suppresses_named_advisory(env, temporary)
    advisory_exceptions_cover_all_reported_versions(env, temporary)
    advisories_follow_dependency_changes(env, temporary)
    advisory_issues_open_once(temporary)
    fuzz_target_forwards_limits_without_finding(temporary)
    privileged_trigger_workflows_ignore_only_declared_audit(env, temporary)
    result = execute('Embedded check defaults', root, env)
    assert result.returncode == 0, result.stdout + result.stderr
    # Caller-installed binaries cannot replace verified coverage in either branch.
    caller_tools = pathlib.Path(temporary) / 'caller-tools'
    caller_tools.mkdir()
    shadow = caller_tools / 'cargo-llvm-cov'
    shadow.write_text('#!/bin/sh\necho caller-coverage-bypass\nexit 0\n', encoding="utf-8")
    shadow.chmod(0o755)
    hostile_env = env | {'PATH': str(caller_tools) + ':' + env['PATH']}
    for name, command in [('Coverage', body('Coverage')),
                          ('Isolated coverage', body('Isolated tests'))]:
        # The coverage job reuses the isolated test body with its coverage floor.
        argv = ['bash', '-e', '-o', 'pipefail', '-c', command]
        if os.environ.get('MAESTRO_LOCAL_CAPPED'):
            command = command.replace('env -i PATH=', 'env -i CARGO_BUILD_JOBS=3 PATH=')
            argv = [os.path.expanduser('~/.local/bin/capped'), 'bash', '-e', '-o', 'pipefail', '-c', command]
        result = subprocess.run(argv, cwd=root / 'fixtures/low-coverage', env=hostile_env,
                                capture_output=True, text=True)
        assert result.returncode != 0, name + ': caller coverage bypass accepted\n' + result.stdout + result.stderr
        assert 'coverage' in result.stderr.lower(), result.stdout + result.stderr
        assert 'caller-coverage-bypass' not in result.stdout + result.stderr
        version = subprocess.run([env['TOOLS'] + '/cargo-llvm-cov', 'llvm-cov', '--version'],
                                 env=hostile_env, capture_output=True, text=True)
        assert version.returncode == 0 and '0.9.1' in version.stdout, version.stdout + version.stderr
        print(f'PASS verified_coverage_ignores_caller_path: {name}: exit {result.returncode}; {version.stdout.strip()}', flush=True)
    # Repeated spelling entries remain one valid, effective extension.
    result = execute('Embedded check defaults', root, env | {'QUALITY_CONFIG': json.dumps({'spelling_words': ['teh', 'teh']})})
    assert result.returncode == 0, result.stderr
    with open(env['CHECK_DEFAULTS'] + '/typos.toml', 'rb') as source:
        assert tomllib.load(source)['default']['extend-words'] == {'teh': 'teh'}
    result = execute('Spelling', root / 'fixtures/misspelling', env)
    assert result.returncode == 0, result.stdout + result.stderr
    print('PASS repeated_spelling_words_generate_valid_toml', flush=True)
    # Supplementary Unicode stays valid UTF-8 in the generated configuration.
    result = execute('Embedded check defaults', root, env | {'QUALITY_CONFIG': json.dumps({'spelling_words': ['𐐀']})})
    assert result.returncode == 0, result.stderr
    with open(env['CHECK_DEFAULTS'] + '/typos.toml', 'rb') as source:
        assert tomllib.load(source)['default']['extend-words'] == {'𐐀': '𐐀'}
    result = execute('Spelling', root / 'fixtures/sample', env)
    assert result.returncode == 0, result.stdout + result.stderr
    print('PASS supplementary_spelling_words_generate_valid_toml', flush=True)
    # Extensions grant only the named spelling word and exact license exception.
    extension = {'spelling_words': ['teh'], 'license_exceptions': [
        {'package': 'sample', 'version': '0.1.0', 'licenses': ['GPL-3.0-only']}]}
    result = execute('Embedded check defaults', root, env | {'QUALITY_CONFIG': json.dumps(extension)})
    assert result.returncode == 0, result.stderr
    for fixture, name in [('misspelling', 'Spelling'), ('disallowed-license', 'Licenses and dependencies')]:
        result = execute(name, root / 'fixtures' / fixture, env)
        assert result.returncode == 0, result.stdout + result.stderr
    extension['license_exceptions'][0]['version'] = '0.1.1'
    result = execute('Embedded check defaults', root, env | {'QUALITY_CONFIG': json.dumps(extension)})
    assert result.returncode == 0, result.stderr
    result = execute('Licenses and dependencies', root / 'fixtures/disallowed-license', env)
    assert result.returncode != 0, 'License exception must not match another version\n' + result.stdout + result.stderr
    result = execute('Embedded check defaults', root, env)
    assert result.returncode == 0, result.stderr
    print('PASS quality_extensions_preserve_defaults')
    # The workflow seam also proves the correctness command, independently of security findings.
    workflows = pathlib.Path(temporary) / '.github/workflows'
    workflows.mkdir(parents=True)
    workflow_path = workflows / 'proof.yml'
    workflow_path.write_text('name: Proof\non: push\npermissions:\n  contents: read\njobs:\n'
                             '  proof:\n    runs-on: ubuntu-latest\n    steps:\n'
                             '      - name: Print\n        run: echo success\n', encoding="utf-8")
    result = execute('Workflows', pathlib.Path(temporary), env)
    assert result.returncode == 0, result.stdout + result.stderr
    workflow_path.write_text(workflow_path.read_text(encoding="utf-8").replace('name: Print', 'if: nonexistent.value\n        name: Print'), encoding="utf-8")
    result = execute('Workflows', pathlib.Path(temporary), env)
    assert result.returncode != 0 and 'undefined variable "nonexistent"' in result.stdout + result.stderr, result.stdout + result.stderr
    print('PASS workflow_correctness_is_required')
    failures = []
    for fixture, broken in [('disallowed-license', 0), ('unused-dependency', 1), ('misspelling', 2),
                            ('fake-secret', 3), ('unsafe-workflow', 4), ('low-coverage', 5)]:
        directory = root / 'fixtures' / fixture
        for index, name in enumerate(checks):
            result = execute(name, directory, env)
            print(f'{fixture}: {name}: exit {result.returncode}', flush=True)
            if (result.returncode != 0) != (index == broken):
                failures.append(f'{fixture}: {name}: {result.stdout}{result.stderr}')
        for command in base:
            argv = shlex.split(command)
            child = env | {'RUSTDOCFLAGS': '-D warnings -D missing_docs'}
            if os.environ.get('MAESTRO_LOCAL_CAPPED'):
                argv.insert(0, os.path.expanduser('~/.local/bin/capped'))
                child['CARGO_BUILD_JOBS'] = '3'
            result = subprocess.run(argv, cwd=directory, env=child, capture_output=True, text=True)
            print(f'{fixture}: {command}: exit {result.returncode}', flush=True)
            if result.returncode:
                failures.append(f'{fixture}: {command}: {result.stdout}{result.stderr}')
    assert not failures, '\n'.join(failures)
    # Each bypass is a separate temporary broken copy, never a consumer setting.
    import shutil
    bypasses = [
        ('machete-package', 'unused-dependency', 'Unused dependencies', 'Cargo.toml',
         '\n[package.metadata.cargo-machete]\nignored = ["unused"]\n'),
        ('machete-workspace', 'unused-dependency', 'Unused dependencies', 'Cargo.toml',
         '\n[workspace]\n[workspace.metadata.cargo-machete]\nignored = ["unused"]\n'),
        ('machete-renamed-package', 'unused-dependency', 'Unused dependencies', 'Cargo.toml',
         '\n[package.metadata.cargo-machete]\nrenamed = { unused = "super" }\n'),
        ('machete-renamed-workspace', 'unused-dependency', 'Unused dependencies', 'Cargo.toml',
         '\n[workspace]\n[workspace.metadata.cargo-machete]\nrenamed = { unused = "super" }\n'),
        ('secret-ignore-file', 'fake-secret', 'Secrets', '.gitleaksignore', 'secret.txt:github-pat:1\n'),
        ('secret-config', 'fake-secret', 'Secrets', '.gitleaks.toml',
         '[allowlist]\nregexes = [".*"]\n'),
        ('secret-inline', 'fake-secret', 'Secrets', 'secret.txt', ' # gitleaks:allow\n'),
        ('spelling-config', 'misspelling', 'Spelling', '_typos.toml', '[default]\ncheck-file = false\n'),
        ('license-config', 'disallowed-license', 'Licenses and dependencies', 'deny.toml',
         '[licenses]\nallow = ["GPL-3.0-only"]\n'),
        ('workflow-config', 'unsafe-workflow', 'Workflows', 'zizmor.yml',
         'rules:\n  template-injection:\n    disable: true\n'),
        ('workflow-inline', 'unsafe-workflow', 'Workflows', '.github/workflows/unsafe.yml',
         ' # zizmor: ignore[template-injection]\n'),
    ]
    for case, original, name, filename, addition in bypasses:
        directory = pathlib.Path(temporary) / case
        shutil.copytree(root / 'fixtures' / original, directory, ignore=shutil.ignore_patterns('target'))
        path = directory / filename
        if case == 'secret-inline':
            path.write_text(path.read_text(encoding="utf-8").rstrip() + addition, encoding="utf-8")
        elif case == 'workflow-inline':
            path.write_text(path.read_text(encoding="utf-8").replace('title }}"', 'title }}"' + addition.rstrip()), encoding="utf-8")
        else:
            path.write_text((path.read_text(encoding="utf-8") if path.exists() else '') + addition, encoding="utf-8")
        result = execute(name, directory, env)
        assert result.returncode != 0, f'{case}: bypass accepted\n{result.stdout}{result.stderr}'
        print(f'PASS {case}: exit {result.returncode}')
    # A declared hakari package is accepted as an unused member dependency and is not itself checked.
    def hakari_workspace(name, member_extra='', hack_extra='', config=True):
        directory = pathlib.Path(temporary) / name
        crates = {'app': ('[dependencies]\nworkspace-hack = { path = "../workspace-hack" }\n' + member_extra),
                  'workspace-hack': '[dependencies]\n' + hack_extra,
                  'real': '', 'other': ''}
        members = ', '.join(f'"{crate}"' for crate in crates)
        directory.mkdir()
        (directory / 'Cargo.toml').write_text(f'[workspace]\nresolver = "2"\nmembers = [{members}]\n', encoding="utf-8")
        for crate, dependencies in crates.items():
            (directory / crate / 'src').mkdir(parents=True)
            (directory / crate / 'src/lib.rs').write_text('//! Fixture.\n', encoding="utf-8")
            (directory / crate / 'Cargo.toml').write_text(
                f'[package]\nname = "{crate}"\nversion = "0.1.0"\nedition = "2021"\nlicense = "MIT"\n\n' + dependencies, encoding="utf-8")
        if config:
            (directory / '.config').mkdir()
            (directory / '.config/hakari.toml').write_text('hakari-package = "workspace-hack"\n', encoding="utf-8")
        return directory
    result = execute('Unused dependencies', hakari_workspace('hakari-ok', hack_extra='other = { path = "../other" }\n'), env)
    assert result.returncode == 0, 'hakari package edges must pass\n' + result.stdout + result.stderr
    result = execute('Unused dependencies', hakari_workspace('hakari-real', member_extra='real = { path = "../real" }\n'), env)
    assert result.returncode != 0 and 'real' in result.stdout + result.stderr, 'a real unused dependency must fail\n' + result.stdout + result.stderr
    result = execute('Unused dependencies', hakari_workspace('hakari-none', config=False), env)
    assert result.returncode != 0, 'without hakari config the unifier edge must fail\n' + result.stdout + result.stderr
    print('PASS hakari_package_is_the_only_accepted_unused_dependency')
    # A stale generated feature unifier fails; a regenerated one passes; no hakari config skips the step.
    def unifier_workspace(name):
        directory = hakari_workspace(name, member_extra='either = { version = "1", default-features = false }\n')
        (directory / '.config/hakari.toml').write_text('hakari-package = "workspace-hack"\nresolver = "2"\ndep-format-version = "4"\n', encoding="utf-8")
        (directory / 'workspace-hack/Cargo.toml').write_text(
            '[package]\nname = "workspace-hack"\nversion = "0.1.0"\nedition = "2021"\nlicense = "MIT"\n\n### BEGIN HAKARI SECTION\n### END HAKARI SECTION\n', encoding="utf-8")
        other = directory / 'other/Cargo.toml'
        other.write_text(other.read_text(encoding="utf-8") + '[dependencies]\neither = { version = "1", default-features = false }\n', encoding="utf-8")
        return directory
    def regenerate(directory):
        subprocess.run(['cargo', 'hakari', 'generate'], cwd=directory, env=env | {'PATH': env['TOOLS'] + os.pathsep + env['PATH']}, check=True)
    directory = unifier_workspace('unifier-fresh')
    regenerate(directory)
    result = execute('Feature unifier', directory, env)
    assert result.returncode == 0, 'a fresh unifier must pass\n' + result.stdout + result.stderr
    other = directory / 'other/Cargo.toml'
    other.write_text(other.read_text(encoding="utf-8").replace(', default-features = false', ''), encoding="utf-8")
    result = execute('Feature unifier', directory, env)
    assert result.returncode != 0, 'a stale unifier must fail\n' + result.stdout + result.stderr
    regenerate(directory)
    result = execute('Feature unifier', directory, env)
    assert result.returncode == 0, 'a regenerated unifier must pass\n' + result.stdout + result.stderr
    result = execute('Feature unifier', hakari_workspace('unifier-none', config=False), env)
    assert result.returncode == 0, 'without hakari config the step must skip\n' + result.stdout + result.stderr
    print('PASS feature_unifier_verified_only_with_hakari_config')
    # An external coverage tool must not replace the toolchain's verified LLVM.
    directory = pathlib.Path(temporary) / 'coverage-tool'
    shutil.copytree(root / 'fixtures/low-coverage', directory, ignore=shutil.ignore_patterns('target'))
    fake = pathlib.Path(temporary) / 'fake-llvm-cov'
    fake.write_text('#!/bin/sh\nprintf \'{"data":[{"totals":{"lines":{"percent":100}}}]}\\n\'\n', encoding="utf-8")
    fake.chmod(0o755)
    result = execute('Coverage', directory, env | {'LLVM_COV': str(fake)})
    assert result.returncode != 0, 'Coverage accepted an external LLVM override\n' + result.stdout + result.stderr
    print('PASS coverage-tool: external LLVM override rejected')
    # LLVM flags must not hide the uncovered module from the denominator.
    source = directory / 'src/lib.rs'
    text = source.read_text(encoding="utf-8")
    start = text.index('/// Returns the sum')
    end = text.index('#[cfg(test)]', start)
    (directory / 'src/uncovered.rs').write_text('//! Uncovered arithmetic.\n' + text[start:end], encoding="utf-8")
    source.write_text(text[:start] + 'pub mod uncovered;\n\n' + text[end:], encoding="utf-8")
    for key in ['LLVM_COV_FLAGS', 'CARGO_LLVM_COV_FLAGS']:
        result = execute('Coverage', directory, env | {key: '-ignore-filename-regex=uncovered.rs'})
        assert result.returncode != 0, key + ': uncovered file excluded\n' + result.stdout + result.stderr
        print(f'PASS {key}: exclusions rejected')
    # A caller actionlint runner allowlist cannot suppress correctness errors.
    workflow_path.write_text('name: Proof\non: push\npermissions:\n  contents: read\njobs:\n'
                             '  proof:\n    runs-on: custom-runner\n    steps:\n'
                             '      - name: Print\n        run: echo success\n', encoding="utf-8")
    (pathlib.Path(temporary) / '.github/actionlint.yaml').write_text('self-hosted-runner:\n  labels: [custom-runner]\n', encoding="utf-8")
    result = execute('Workflows', pathlib.Path(temporary), env)
    assert result.returncode != 0 and 'label' in result.stdout + result.stderr, result.stdout + result.stderr
    print('PASS actionlint-config: consumer runner allowlist ignored')

    # A corrupt release must never be extracted or put on PATH.
    hostile = pathlib.Path(temporary) / 'hostile'
    hostile.mkdir()
    curl = hostile / 'curl'
    curl.write_text('#!/bin/sh\nwhile [ "$1" != --output ]; do shift; done\nprintf invalid-archive > "$2"\n', encoding="utf-8")
    curl.chmod(0o755)
    pathlib.Path(env['GITHUB_PATH']).touch()
    before_path = pathlib.Path(env['GITHUB_PATH']).read_text(encoding="utf-8")
    before = pathlib.Path(env['GITHUB_ENV']).read_text(encoding="utf-8")
    result = execute('Check tools', root, env | {'PATH': str(hostile) + ':' + env['PATH']})
    assert result.returncode != 0, result.stdout + result.stderr
    assert pathlib.Path(env['GITHUB_PATH']).read_text(encoding="utf-8") == before_path
    assert pathlib.Path(env['GITHUB_ENV']).read_text(encoding="utf-8") == before
    print('PASS check_tools_reject_corrupt_release')
