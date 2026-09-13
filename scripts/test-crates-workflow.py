#!/usr/bin/env python3
"""Exercise refusal controls, registry outcomes, and release wiring."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parent.parent


class PublicationContract(unittest.TestCase):
    def test_guard_control_and_refusals(self):
        subprocess.run(['bash', 'scripts/ci/crates-publish-guard.sh', '--selftest'],
                       cwd=ROOT, check=True)

    def test_registry_distinguishes_absence_from_failed_observation(self):
        with tempfile.TemporaryDirectory() as directory:
            stub = Path(directory) / 'curl'
            # A separate file, never a symlink to the installed curl.
            stub.write_text('''#!/usr/bin/env python3
import os,sys
from pathlib import Path
args=sys.argv[1:]
assert args[args.index('-H')+1].startswith('User-Agent: bobbin-release-ci')
assert args[-1]=='https://crates.io/api/v1/crates/bobbin-ai/0.16.2'
Path(args[args.index('-o')+1]).write_text(os.environ['REGISTRY_BODY'])
print(os.environ['REGISTRY_STATUS'],end='')
sys.exit(int(os.environ.get('REGISTRY_EXIT','0')))
''')
            stub.chmod(0o700)
            cases = [
                ('200', '{"version":{"num":"0.16.2"}}', '0', 0),
                ('404', '{"errors":[{"detail":"missing"}]}', '0', 1),
                ('200', '{"version":{"num":"0.16.1"}}', '0', 2),
                ('200', 'invalid json', '0', 2),
                ('401', '{}', '0', 2),
                ('403', '{}', '0', 2),
                ('500', '{}', '0', 2),
                ('000', '', '7', 2),
            ]
            for status, body, curl_exit, expected in cases:
                with self.subTest(status=status, body=body):
                    env = dict(os.environ, PATH=directory+os.pathsep+os.environ['PATH'],
                               REGISTRY_STATUS=status, REGISTRY_BODY=body,
                               REGISTRY_EXIT=curl_exit)
                    result = subprocess.run(
                        ['bash', 'scripts/ci/crates-registry-version.sh', '0.16.2'],
                        cwd=ROOT, env=env, capture_output=True, text=True)
                    self.assertEqual(result.returncode, expected, result.stderr)

    def test_release_and_manual_lanes_use_one_guarded_action(self):
        release = yaml.safe_load((ROOT/'.github/workflows/release.yml').read_text())
        manual = yaml.safe_load((ROOT/'.github/workflows/crates.yml').read_text())
        action = yaml.safe_load((ROOT/'.github/actions/crates-publish/action.yml').read_text())
        # PyYAML's YAML 1.1 loader treats the key "on" as True.
        triggers = manual.get('on', manual.get(True))
        self.assertEqual(set(triggers), {'workflow_dispatch'})
        inputs = triggers['workflow_dispatch']['inputs']
        self.assertIs(inputs['dry_run']['default'], True)
        self.assertIs(inputs['version']['required'], True)
        crates = release['jobs']['crates']
        self.assertTrue({'build', 'checksums', 'release'} <= set(crates['needs']))
        for job in ('build', 'checksums', 'release'):
            self.assertIn(f"needs.{job}.result == 'success'", crates['if'])
        self.assertEqual(crates['steps'][0]['with']['ref'], '${{ env.VERSION }}')
        self.assertNotIn('cargo metadata', str(crates))
        self.assertIn('crates-publish-guard.sh --selftest', str(release['jobs']['build']))
        for lane in (crates, manual['jobs']['publish']):
            calls = [step for step in lane['steps']
                     if step.get('uses') == './.github/actions/crates-publish']
            self.assertEqual(len(calls), 1)
            self.assertIn('expected-version', calls[0]['with'])
        steps = action['runs']['steps']
        self.assertIn('crates-publish-guard.sh', steps[0]['run'])
        self.assertNotIn('if', steps[0])
        for step in steps[1:]:
            self.assertNotIn('always()', step.get('if', ''))
            self.assertNotIn('continue-on-error', step)
        auth = next(step for step in steps if step.get('id') == 'crates-auth')
        publish = next(step for step in steps if step.get('run') == 'cargo publish')
        self.assertEqual(auth['if'], publish['if'])
        self.assertIn("steps.before.outputs.already != 'true'", auth['if'])
        self.assertIn('DRY RUN', next(step['run'] for step in steps
                                     if step.get('name') == 'Publish (dry run)'))
        self.assertIn('crates-registry-version.sh', steps[-1]['run'])
        self.assertIn('exit 1', steps[-1]['run'])


if __name__ == '__main__':
    unittest.main()
