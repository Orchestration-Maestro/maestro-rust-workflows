"""Exercise the deployed policy and the declaration's actual runner-selection body."""
import io
import json
import os
import re
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from runner_policy import require_secret_free

ROOT = Path(__file__).resolve().parent
WORKFLOW = ROOT / 'workflows/ci.yml'


class RunnerPolicyTests(unittest.TestCase):
    def test_python_random_tokens_are_not_repository_secrets(self):
        require_secret_free('token = secrets.token_hex(32)')
        require_secret_free('token: ${{ secrets.GITHUB_TOKEN }}')

    def test_repository_secret_references_and_inheritance_are_refused(self):
        for source in ('token: ${{ secrets.KEY }}', 'secrets: inherit',
                       'token: ${{ toJSON(secrets) }}', "token: ${{ secrets['KEY'] }}"):
            with self.subTest(source=source), self.assertRaises(ValueError):
                require_secret_free(source)

    def select(self, availability, source='permissions: {contents: read}', corrupt=False, offline=False):
        workflow = WORKFLOW.read_text()
        step = workflow.split('      - name: Select Linux runner\n', 1)[1].split('      - name: ', 1)[0]
        body = re.search(r"          python3 - <<'PYTHON'\n(.*?)          PYTHON", step, re.DOTALL)[1]
        code = '\n'.join(line[10:] for line in body.splitlines())
        requests = []

        def urlopen(request, **_kwargs):
            requests.append(request)
            if offline:
                raise OSError('unavailable')
            if isinstance(request, str):
                return io.BytesIO(b'corrupt' if corrupt else (ROOT / 'runner_policy.py').read_bytes())
            import base64
            return io.BytesIO(json.dumps({'content': base64.b64encode(source.encode()).decode()}).encode())

        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / 'output'
            env = {'RUNNER_AVAILABILITY': json.dumps(availability), 'GITHUB_OUTPUT': str(output),
                   'CALLER_WORKFLOW': 'Orchestration-Maestro/maestro/.github/workflows/check.yml@refs/heads/main',
                   'GH_TOKEN': 'synthetic-job-token'}
            with patch.dict(os.environ, env), patch('urllib.request.urlopen', side_effect=urlopen), patch('time.time', return_value=1000):
                exec(compile(code, 'Select Linux runner', 'exec'), {})
            labels = json.loads(output.read_text().split('=', 1)[1])
            return labels, requests

    def test_unavailable_expired_future_and_zero_count_use_hosted_linux(self):
        for value in ({}, {'timestamp': 699, 'online': 4}, {'timestamp': 1001, 'online': 4},
                      {'timestamp': 1000, 'online': 0}, {'timestamp': 'invalid', 'online': 4}):
            with self.subTest(value=value):
                labels, requests = self.select(value)
                self.assertEqual(labels, 'ubuntu-latest')
                self.assertEqual(requests, [])

    def test_fresh_availability_and_secret_free_caller_use_aws(self):
        for source in ('token = secrets.token_hex(32)', 'token: ${{ secrets.GITHUB_TOKEN }}'):
            labels, requests = self.select({'timestamp': 700, 'online': 4}, source)
            self.assertEqual(labels, ['self-hosted', 'linux', 'maestro-aws'])
            self.assertEqual(len(requests), 2)

    def test_secret_bearing_caller_and_unverifiable_policy_stay_hosted(self):
        cases = [{'source': 'token: ${{ secrets.KEY }}'}, {'source': 'secrets: inherit'},
                 {'corrupt': True}, {'offline': True}]
        for options in cases:
            with self.subTest(options=options):
                self.assertEqual(self.select({'timestamp': 1000, 'online': 4}, **options)[0], 'ubuntu-latest')


if __name__ == '__main__':
    unittest.main()
