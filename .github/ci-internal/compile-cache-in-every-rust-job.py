import pathlib
import re

workflow = pathlib.Path('.github/workflows/ci.yml').read_text(encoding='utf-8')
def snapshot_jobs_share_one_owner():
    import json
    import subprocess
    document = json.loads(subprocess.check_output(['yq', '-o=json', '-'], input=workflow, text=True))
    assert document['permissions'] == {'contents': 'read'}
    assert set(document['jobs']) == {'declaration', 'quality', 'docs', 'test-build', 'test-prepare', 'tests', 'checks', 'advisories',
                                     'targets', 'browser', 'browser-tests', 'fuzz', 'doctests', 'tests-result'}
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
        if name == 'targets':
            for stage in stages:
                parsed = next(item for item in document['jobs'][name]['steps'] if item.get('name') == stage)
                condition = parsed.get('if', '')
                host = "runner.os != 'Windows'"
                if stage == 'Compile cache identity':
                    assert condition == '', 'Windows still needs the shared default-branch writer decision'
                elif stage == 'Compile cache statistics':
                    assert condition == 'always() && ' + host
                elif stage == 'Save compile cache':
                    assert condition == host + " && steps.cache-identity.outputs.writer == 'true' && steps.cache-seal.outputs.eligible == 'true'"
                else:
                    assert condition == host, stage
            upload = next(item for item in document['jobs'][name]['steps'] if item.get('name') == 'Upload compile cache errors')
            assert upload['if'] == "always() && (steps.cache-statistics.outputs.write_errors == 'true' || (inputs.concurrency-key == 'maestro-proof' && steps.cache-statistics.outcome == 'success'))"
            check = next(item for item in document['jobs'][name]['steps'] if item.get('name') == 'Check target')
            assert 'if' not in check
    for name in owners - {'quality'}:
        assert 'env: *sccache-identity-env' in jobs[name]
        for alias in ['sccache-identity', 'sccache-restore', 'sccache-install', 'sccache-stats', 'sccache-seal', 'sccache-save']:
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
