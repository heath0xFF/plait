"""Windowless harness tests; mocks deliberately forbid native helper calls."""
import contextlib
import io
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

from fixture import ANSWER, Fixture, events, require
from run import Acceptance, Skipped, check_lights, failure_lines, inspector_skip, summary


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
                self.assertEqual(len(runner.results), 45)
                self.assertEqual(runner.results[-1]['status'], 'SKIPPED')
                native.assert_not_called()

    def runner(self, directory):
        return Acceptance(SimpleNamespace(host=Path('/unused'), no_pointer=False), Path(directory), Path('/never-call-driver'))

    def test_done_status_does_not_close_or_prevent_opening_settings(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = self.runner(directory)
            calls = []

            def ax(action, *args, **_kwargs):
                calls.append((action, *args))
                if action == 'count':
                    # The first run always exposed Done as static status text.
                    return '0' if '--role' in args else '1'
                return '1'

            with patch.object(runner, 'ax', side_effect=ax), patch('run.time.sleep'):
                runner.tab('Changes')
                runner.settings()
            self.assertIn(('count', '--name', 'Done', '--role', 'AXButton'), calls)
            self.assertNotIn(('press', '--name', 'Done', '--role', 'control'), calls)
            self.assertIn(('key', 'cmd+comma'), calls)

    def test_existing_settings_closes_once_and_already_open_settings_stays_open(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = self.runner(directory)
            with patch.object(runner, 'ax', return_value='1') as ax:
                runner.settings()
                self.assertFalse(any(c.args[0] == 'key' for c in ax.call_args_list))
                runner.tab('Conversation')
                ax.assert_any_call('press', '--name', 'Done', '--role', 'control')
                ax.assert_any_call('absent', '--name', 'Done', '--role', 'AXButton')

    def test_every_item_resets_even_after_failed_reset_or_failed_action(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = self.runner(directory)
            runner.ready = True
            events = []

            def reset():
                events.append('reset')
                if len(events) == 1:
                    raise AssertionError('reset failed at missing Conversation')

            def fail():
                events.append('action failed')
                raise AssertionError('missing control')

            with patch.object(runner, 'reset', side_effect=reset), contextlib.redirect_stdout(io.StringIO()):
                runner.record('reset-failed', lambda: self.fail('must not run'))
                runner.record('action-failed', fail)
                runner.record('unrelated', lambda: events.append('action passed'))
            self.assertEqual(events, ['reset', 'reset', 'action failed', 'reset', 'action passed'])
            self.assertEqual([r['status'] for r in runner.results], ['FAIL', 'FAIL', 'PASS'])
            self.assertIn('missing Conversation', failure_lines(runner.results)[0])
            self.assertEqual(len(failure_lines(runner.results)), 2)

    def test_reset_closes_transient_views_and_normalizes_native_window(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = self.runner(directory)
            runner.fixture = Mock(thread={'id': 'seed'})
            with patch.object(runner, 'ax', return_value='1') as ax, \
                 patch.object(runner, 'window', return_value=[20, 51, 984, 639, 1, 1]), \
                 patch.object(runner, 'field'), patch.object(runner, 'selected'), \
                 patch('run.time.sleep'):
                runner.reset()
                runner.reset()
            self.assertEqual(runner.fixture.reset.call_count, 2)
            ax.assert_any_call('minimized', 'off')
            ax.assert_any_call('fullscreen', 'off', '--timeout', '10')
            ax.assert_any_call('press', '--name', 'Close find', '--role', 'control')
            ax.assert_any_call('press', '--prefix', 'Thread seed ·', '--role', 'AXRow')

    def test_inspector_skips_only_after_sizing_its_own_narrow_window(self):
        self.assertIn('984 pt', inspector_skip(984))
        self.assertIsNone(inspector_skip(1150))
        with tempfile.TemporaryDirectory() as directory:
            runner = self.runner(directory)
            with patch.object(runner, 'tab'), patch.object(runner, 'window', return_value=[20, 51, 984, 639]), \
                 patch.object(runner, 'ax') as ax:
                with self.assertRaisesRegex(Skipped, '1150 pt minimum'):
                    runner.inspector(True)
                ax.assert_called_once_with('place', '1380', '860')
            # A wide window missing the control is an app/check failure, not a skip.
            with patch.object(runner, 'tab'), patch.object(runner, 'window', return_value=[20, 51, 1380, 860]), \
                 patch.object(runner, 'ax'), patch.object(runner, 'find', side_effect=AssertionError('missing Toggle Git inspector')):
                with self.assertRaisesRegex(AssertionError, 'missing Toggle'):
                    runner.inspector(True)

    def test_items_are_unique_and_mutating_runs_follow_seed_reading(self):
        with tempfile.TemporaryDirectory() as directory:
            names = [name for name, _ in self.runner(directory).checks()]
        self.assertEqual(len(names), 43)
        self.assertEqual(len(set(names)), len(names))
        for read in ['find-conversation', 'conversation-text-selection']:
            for write in ['approval-allow-shortcut', 'approval-deny-shortcut', 'streaming-reply', 'turn-undo', 'turn-redo', 'turn-undo-redo-shortcut']:
                self.assertLess(names.index(read), names.index(write))

    def test_checkout_reset_removes_prior_commit_branch_and_staging(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory), Path('/unused'))
            fixture.prepare_checkout()
            fixture.seed_thread = {'id': 'seed'}
            fixture.thread = {'id': 'failed'}
            fixture.replay = Mock()
            fixture.git('switch', '-c', 'acceptance-created')
            fixture.git('add', '.')
            fixture.git('commit', '-qm', 'previous item')
            (fixture.repo / 'approval.txt').write_text('allowed\n')
            with patch.object(fixture, 'watch', return_value={'status': 'waiting', 'pending_approval': {'id': 'p'}}), \
                 patch('fixture.request') as call:
                fixture.reset()
                call.assert_called_once_with(fixture.store, op='stop', thread_id='failed')
            fixture.replay.clear.assert_called_once()
            self.assertEqual(fixture.thread, fixture.seed_thread)
            self.assertEqual(fixture.git('branch', '--show-current'), 'main')
            self.assertEqual(fixture.git('rev-parse', 'HEAD'), fixture.baseline)
            self.assertEqual(fixture.git('diff', '--cached', '--name-only'), '')
            self.assertEqual(fixture.git('branch', '--list', 'acceptance-created'), '')
            self.assertEqual((fixture.repo / 'approval.txt').read_text(), 'before\n')
            # Commit gets its own first hunk even when the stage item never ran.
            fixture.stage_first_hunk()
            staged = fixture.git('diff', '--cached')
            self.assertIn('+first new', staged)
            self.assertNotIn('+last new', staged)
            self.assertIn('+last new', fixture.git('diff'))

    def test_deny_edit_uses_its_own_preimage(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = Fixture(Path(directory), Path('/unused'))
            fixture.prepare_checkout()
            fixture.replay = Mock()
            with patch.object(fixture, 'submit'), patch.object(fixture, 'watch', return_value={'pending_approval': {'id': 'p'}}):
                fixture.pending(False)
            edit = fixture.replay.add.call_args.args[0]['tool']['input']
            self.assertEqual(edit['old_text'], 'before\n')
            self.assertEqual(edit['new_text'], 'denied\n')

    def test_failure_digest_keeps_first_missing_control_or_refused_owner(self):
        results = [{'item': 'settings', 'status': 'FAIL', 'observed': 'not found: role AXButton, named "Done"'},
                   {'item': 'drag', 'status': 'FAIL', 'observed': 'refused: point=[177.5 73.0];\nfrontmost=Other pid=8 frame=[0 31 900 600]'},
                   {'item': 'skip', 'status': 'SKIPPED', 'observed': 'narrow window'}]
        lines = failure_lines(results)
        self.assertEqual(len(lines), 2)
        self.assertIn('named "Done"', lines[0])
        self.assertIn('frontmost=Other pid=8 frame=[0 31 900 600]', lines[1])
        self.assertNotIn('\n', lines[1])

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
