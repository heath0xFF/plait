"""Windowless harness tests; mocks deliberately forbid native helper calls."""
import contextlib
import io
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from fixture import ANSWER, Fixture, events, require
from run import Acceptance, Skipped, check_lights, summary


class HarnessTests(unittest.TestCase):
    def test_isolation_does_not_inherit_credentials_or_git_overrides(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict('os.environ', {
            'ANTHROPIC_API_KEY': 'not inherited', 'PLAIT_HOST_STORE': '/not-the-fixture',
            'GIT_CONFIG_COUNT': '1', 'GIT_CONFIG_KEY_0': 'core.hooksPath',
            'GIT_CONFIG_VALUE_0': '/not-the-fixture',
        }):
            fixture = Fixture(Path(directory), Path('/unused-host'))
            self.assertNotIn('ANTHROPIC_API_KEY', fixture.env)
            self.assertNotIn('GIT_CONFIG_COUNT', fixture.env)
            self.assertEqual(fixture.env['GIT_CONFIG_GLOBAL'], '/dev/null')
            for key in ['HOME', 'CFFIXED_USER_HOME', 'PLAIT_HOST_STORE', 'PLAIT_DESKTOP_STATE_DIR', 'PLAIT_DESKTOP_PREFERENCES_FILE', 'PI_CODING_AGENT_DIR']:
                self.assertTrue(Path(fixture.env[key]).is_relative_to(directory), key)

    def test_per_item_failures_do_not_hide_later_results(self):
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(host=Path('/unused'), no_pointer=True)
            runner = Acceptance(args, Path(directory), Path('/never-call-driver'))
            with patch.object(runner, 'ax', side_effect=AssertionError('native access forbidden')):
                output = io.StringIO()
                with contextlib.redirect_stdout(output):
                    runner.record('pass', lambda: 'observed', native=False)
                    runner.record('fail', lambda: require(False, 'observed wrong value'), native=False)
                    runner.record('skip', runner.pointer, native=False)
                    runner.record('dependent', lambda: self.fail('must not run'))
                self.assertEqual([r['status'] for r in runner.results], ['PASS', 'FAIL', 'SKIPPED', 'SKIPPED'])
                self.assertEqual(len(output.getvalue().splitlines()), 4)
                self.assertEqual(json.loads((Path(directory) / 'results.json').read_text()), runner.results)
                self.assertEqual(summary(runner.results), 'SUMMARY pass=1 fail=1 skipped=2')

    def test_no_pointer_guards_every_pointer_item_before_any_ui_call(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = Acceptance(SimpleNamespace(host=Path('/unused'), no_pointer=True), Path(directory), Path('/unused'))
            with patch.object(runner, 'ax') as native:
                for check in [lambda: runner.bar_drag(True), lambda: runner.bar_drag(False), runner.double_click,
                              lambda: runner.resize('sidebar'), lambda: runner.resize('inspector'), runner.selection]:
                    with self.assertRaisesRegex(Skipped, '--no-pointer'):
                        check()
                native.assert_not_called()

    def test_setup_failure_reports_every_item_and_exits_nonzero(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = Acceptance(SimpleNamespace(host=Path('/unused'), no_pointer=True), Path(directory), Path('/unused'))
            with patch.object(runner, 'setup', side_effect=AssertionError('fixture unavailable')), \
                 patch.object(runner, 'launch', side_effect=Skipped('no native setup')), \
                 patch.object(runner, 'ax') as native, contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(runner.run(), 1)
                self.assertEqual(len(runner.results), 42)
                self.assertEqual(runner.results[-1]['status'], 'SKIPPED')
                native.assert_not_called()

    def test_traffic_light_overlap_and_vertical_drift_fail(self):
        values = [100, 100, 1000, 700, 114, 114, 16, 16, 134, 114, 16, 16, 154, 114, 16, 16]
        check_lights(values, [184, 110, 24, 24])
        with self.assertRaisesRegex(AssertionError, 'overlaps'):
            check_lights(values, [170, 110, 24, 24])
        values[5] += 5
        with self.assertRaisesRegex(AssertionError, 'centered'):
            check_lights(values, [184, 110, 24, 24])

    def test_replay_text_and_edit_frames(self):
        frames = list(events({'text': ANSWER, 'chunk': 7}, 2))
        self.assertEqual(''.join(data['delta']['text'] for kind, data in frames if kind == 'content_block_delta'), ANSWER)
        tool = {'id': 'fixture', 'name': 'edit', 'input': {'path': 'approval.txt', 'old_text': 'before\n', 'new_text': 'after\n'}}
        frames = list(events({'tool': tool}, 3))
        self.assertEqual(json.loads(frames[2][1]['delta']['partial_json']), tool['input'])
        self.assertEqual(frames[-2][1]['delta']['stop_reason'], 'tool_use')


if __name__ == '__main__':
    unittest.main()
