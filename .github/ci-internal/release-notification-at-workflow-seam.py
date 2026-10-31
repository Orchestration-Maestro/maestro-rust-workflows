import ast
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

def workflow(path):
    return json.loads(subprocess.check_output(['yq', '-o=json', path], text=True))

seam = pathlib.Path('.github/ci-internal/added-checks-at-workflow-seam.py').read_text(encoding='utf-8')
tree = ast.parse(seam)
tree.body = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == 'fake_tool']
exec(compile(tree, 'shared fake tool', 'exec'))
release = workflow('.github/workflows/release-please.yml')['jobs']
steps = release['release']['steps']

class ReleaseNotification(unittest.TestCase):
    def test_release_notification_is_new_release_only(self):
        names = [step['name'] for step in steps]
        index = next(i for i, step in enumerate(steps) if step.get('id') == 'release')
        self.assertEqual(names[index + 1:index + 3], ['Mint the Quality sync token', 'Start Quality sync'])
        self.assertNotIn('continue-on-error', release['release'])
        for step in steps[index + 1:index + 3]:
            self.assertEqual(step['if'], "steps.release.outputs.release_created == 'true'")
            self.assertNotIn('continue-on-error', step)
        self.assertEqual(release['release']['outputs'], {
            'release-created': '${' + '{ steps.release.outputs.release_created }}',
            'tag-name': '${' + '{ steps.release.outputs.tag_name }}'})
        self.assertEqual(release['evidence']['needs'], 'release')
        self.assertEqual(release['evidence']['if'], "needs.release.outputs.release-created == 'true'")
        self.assertEqual(release['evidence']['env']['TAG'], '${' + '{ needs.release.outputs.tag-name }}')
        self.assertEqual(release['evidence']['steps'][0]['with']['ref'], '${' + '{ needs.release.outputs.tag-name }}')

    def test_release_notification_token_is_destination_scoped(self):
        token = next(step for step in steps if step.get('id') == 'token')
        action = 'actions/create-github-app-token@bcd2ba49218906704ab6c1aa796996da409d3eb1'
        credentials = {'client-id': '${' + '{ vars.RELEASE_APP_CLIENT_ID }}',
                       'private-key': '${' + '{ secrets.RELEASE_APP_PRIVATE_KEY }}'}
        self.assertEqual(token['uses'], action)
        self.assertEqual(token['with'], credentials | {'permission-contents': 'write',
                         'permission-issues': 'write', 'permission-pull-requests': 'write'})
        release_action = next(step for step in steps if step.get('id') == 'release')
        self.assertEqual(release_action['uses'], 'googleapis/release-please-action@45996ed1f6d02564a971a2fa1b5860e934307cf7')
        self.assertEqual(release_action['with'], {'token': '${' + '{ steps.token.outputs.token }}',
                         'config-file': '.github/release-please/config.json',
                         'manifest-file': '.github/release-please/manifest.json'})
        notification = next(step for step in steps if step['name'] == 'Mint the Quality sync token')
        self.assertEqual(notification['id'], 'sync-token')
        self.assertEqual(notification['uses'], action)
        self.assertEqual(notification['with'], credentials | {
                         'repositories': '.github', 'permission-contents': 'write'})
        dispatch = next(step for step in steps if step['name'] == 'Start Quality sync')
        self.assertEqual(dispatch['env'], {'GH_TOKEN': '${' + '{ steps.sync-token.outputs.token }}'})

    def dispatch(self, status):
        step = next(step for step in steps if step['name'] == 'Start Quality sync')
        with tempfile.TemporaryDirectory() as temporary:
            directory = pathlib.Path(temporary) / 'fake-gh'
            fake, recorded = fake_tool(directory, 'gh')
            env = os.environ | fake | {'FAKE_STATUS': str(status), 'GH_TOKEN': 'test-only'}
            self.assertEqual(pathlib.Path(shutil.which('gh', path=env['PATH'])), directory / 'gh')
            result = subprocess.run(['bash', '--noprofile', '--norc', '-e', '-o', 'pipefail', '-c', step['run']],
                                    env=env, capture_output=True, text=True)
            self.assertEqual(recorded(), [{'argv': ['api', '--method', 'POST',
                             'repos/Orchestration-Maestro/.github/dispatches',
                             '--raw-field', 'event_type=rust-workflows-release'], 'body': None}])
            return result.returncode

    def test_release_notification_posts_the_accepted_event(self):
        self.assertEqual(self.dispatch(0), 0)

    def test_release_notification_propagates_api_failure(self):
        for status in [1, 4]:
            with self.subTest(status=status):
                self.assertEqual(self.dispatch(status), status)

unittest.main(verbosity=2)
