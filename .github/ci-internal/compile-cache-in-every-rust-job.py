import pathlib
import re

workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
def snapshot_jobs_share_one_owner():
    import json
    import subprocess
    document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
    assert document['permissions'] == {'contents': 'read'}
    writers = {'quality', 'docs', 'test-build', 'test-prepare', 'tests', 'checks', 'targets', 'browser', 'browser-tests', 'doctests'}
    assert set(document['jobs']) == {'declaration', 'replaced-groups', 'advisories', 'fuzz', 'tests-result', 'sandbox-runtime'} | writers | {name + '-prune' for name in writers}
    jobs = dict(re.findall(r'^  ([\w-]+):\n(.*?)(?=^  [\w-]+:\n|\Z)',
                           workflow.split('\njobs:\n', 1)[1], re.M | re.S))
    owners = {'quality', 'docs', 'test-build', 'test-prepare', 'tests', 'checks', 'targets', 'browser', 'browser-tests'}
    stages = ['Compile cache identity', 'Restore compile cache', 'Compile cache',
              'Compile cache statistics', 'Seal compile cache', 'Save compile cache']
    for name in owners:
        job = jobs[name]
        offsets = [job.index('- name: ' + stage + '\n') for stage in stages]
        assert offsets == sorted(offsets), name
        assert all(job.count('- name: ' + stage + '\n') == 1 for stage in stages), name
        save = job.split('- name: Save compile cache\n', 1)[1].split('      - name:', 1)[0]
        assert "steps.cache-identity.outputs.writer == 'true'" in save, (name, 'save must require the shared writer decision')
        if name in ('targets', 'test-build', 'test-prepare'):
            for stage in stages:
                parsed = next(item for item in document['jobs'][name]['steps'] if item.get('name') == stage)
                condition = parsed.get('if', '')
                host = "runner.os != 'Windows'"
                if stage == 'Compile cache identity' and name == 'targets':
                    assert condition == host, 'a Windows target check restores no compile cache, so it identifies none'
                elif stage == 'Compile cache statistics':
                    assert condition == 'always() && ' + host
                elif stage == 'Restore compile cache' and name == 'targets':
                    assert condition == host + " && steps.cache-identity.outputs.snapshot == 'true'"
                elif stage == 'Save compile cache':
                    assert condition == host + " && steps.cache-identity.outputs.writer == 'true' && steps.cache-seal.outputs.eligible == 'true'"
                elif stage == 'Compile cache identity' and name in ('test-build', 'test-prepare'):
                    assert condition == '', 'identity also scopes Windows setup bundles'
                else:
                    assert condition == host, stage
            upload = next(item for item in document['jobs'][name]['steps'] if item.get('name') == 'Upload compile cache errors')
            assert upload['if'] == "always() && (steps.cache-statistics.outputs.write_errors == 'true' || (inputs.concurrency-key == 'maestro-proof' && steps.cache-statistics.outcome == 'success'))"
            if name == 'targets':
                check = next(item for item in document['jobs'][name]['steps'] if item.get('name') == 'Check target')
                assert 'if' not in check
    for name in owners - {'quality'}:
        assert 'env: *sccache-identity-env' in jobs[name]
        for alias in ['sccache-identity', 'sccache-restore', 'sccache-install-env', 'sccache-install', 'sccache-stats', 'sccache-seal', 'sccache-save']:
            assert '*' + alias in jobs[name], (name, alias)
    assert 'CACHE_TARGET: $' + '{{ matrix.target }}' in jobs['targets']
    assert jobs['checks'].index('- name: Secrets\n') < jobs['checks'].index('- name: Restore compile cache\n')
    for obsolete in ['Compile cache credentials', 'SCCACHE_GHA_VERSION', 'SCCACHE_GHA_RW_MODE',
                     'SCCACHE_GHA_ENABLED=true', 'cache-proof-bulk', 'ACTIONS_RUNTIME_TOKEN']:
        assert obsolete not in workflow, obsolete
    installer = next(item['run'] for item in document['jobs']['quality']['steps'] if item.get('name') == 'Compile cache')
    assert 'version=0.18.0' in installer and "hashlib.file_digest(source, 'sha256')" in installer
    assert 'SCCACHE_GHA_ENABLED=false' in workflow and 'CARGO_INCREMENTAL=0' in workflow

snapshot_jobs_share_one_owner()
print('PASS snapshot_jobs_share_one_owner')

def prunes_on(condition, event, ref):
    # GitHub's substitutions in one job condition, so the routing is read from behaviour, not from the text.
    values = {'github.event_name': event, 'github.ref': ref, 'github.event.repository.default_branch': 'main'}
    expression = condition.removeprefix('$' + '{{').removesuffix('}}').strip()
    for key, value in values.items():
        expression = expression.replace(key, repr(value))
    expression = expression.replace("format('refs/heads/{0}', 'main')", repr('refs/heads/main'))
    atoms = [atom.split(' == ') for atom in expression.split(' && ')]
    assert all(len(atom) == 2 for atom in atoms), condition
    return all(left.strip() == right.strip() for left, right in atoms)

def every_cache_writer_has_one_pruner_without_caller_code():
    import json
    import subprocess
    jobs = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))['jobs']
    # The sandbox runtime job keeps no compile snapshot and no cache.
    writers = {name for name, job in jobs.items()
               if any(step.get('uses', '').startswith('actions/cache/save@') and step['with']['path'].endswith('maestro-sccache')
                      for step in job.get('steps', []))}
    assert writers == {'quality', 'docs', 'test-build', 'test-prepare', 'tests', 'doctests', 'checks', 'targets', 'browser', 'browser-tests'}
    pruners = {name: job for name, job in jobs.items() if name.endswith('-prune')}
    # The replaced-group canceller holds the only other write grant; its own seam proves it runs no caller code.
    assert {name for name, job in jobs.items() if job.get('permissions', {}).get('actions') == 'write'} == set(pruners) | {'replaced-groups'}
    # Each writer is followed by exactly one pruner, and only pruners may delete caches.
    assert sorted(job['needs'] for job in pruners.values()) == sorted(writers), pruners.keys()
    steps = next(iter(pruners.values()))['steps']
    for name, job in pruners.items():
        assert job['permissions'] == {'actions': 'write'}, name
        # Only a default-branch push prunes: the write grant never reaches a pull request, a merge group or another branch.
        prunes = [(event, ref) for event, ref in [('push', 'refs/heads/main'), ('push', 'refs/heads/topic'),
                                                 ('pull_request', 'refs/pull/363/merge'),
                                                 ('merge_group', 'refs/heads/main')]
                  if prunes_on(job['if'], event, ref)]
        assert prunes == [('push', 'refs/heads/main')], (name, prunes)
        assert job['runs-on'] == 'ubuntu-latest', name
        # No checkout and no action: the job holding the write token never runs caller code.
        assert job['steps'] == steps and [step['name'] for step in steps] == ['Runner hardware', 'Prune superseded caches'], name
        assert all('uses' not in step for step in steps) and steps[1]['continue-on-error'] is True
        assert steps[1]['env'] == {'GH_TOKEN': '${{ github.token }}'}

every_cache_writer_has_one_pruner_without_caller_code()
print('PASS every_cache_writer_has_one_pruner_without_caller_code')
