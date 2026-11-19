"""Windowless harness tests; mocks deliberately forbid native helper calls."""
import contextlib
import ast
import io
import json
import os
import subprocess
from pathlib import Path
import tempfile
import signal
import time
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

from fixture import ANSWER, Fixture, events, require, until
from run import (
    Acceptance,
    CommandError,
    RUN_SECONDS,
    RunInterrupted,
    Skipped,
    check_lights,
    failure_lines,
    forced_summary,
    format_exit,
    inspector_skip,
    main,
    stop_run,
    summary,
    tail_log,
)


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
                self.assertEqual(len([line for line in output.getvalue().splitlines() if not line.startswith('START ')]), 4)
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
                self.assertEqual(len(runner.results), 46)
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

    def test_settings_checks_the_named_modal_surface(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = self.runner(directory)
            with patch.object(runner, 'ax', return_value='1') as ax:
                runner.settings()
                ax.assert_any_call('find', '--name', 'Settings', '--role', 'AXWindow')
                runner.project_settings()
                ax.assert_any_call('find', '--name', 'Project settings', '--role', 'AXWindow')

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
                runner.files_dirty = True
                runner.reset()
            self.assertEqual(runner.fixture.reset.call_count, 2)
            self.assertFalse(runner.files_dirty)
            ax.assert_any_call('minimized', 'off')
            ax.assert_any_call('fullscreen', 'off', '--timeout', '10')
            ax.assert_any_call('press', '--name', 'Close find', '--role', 'control')
            ax.assert_any_call('select', '--prefix', 'Thread seed ·', '--role', 'AXRow')
            ax.assert_any_call('select', '--name', 'Files', '--role', 'control')

    def test_reset_skips_unnecessary_files_trip_when_files_not_visited(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = self.runner(directory)
            runner.fixture = Mock(thread={'id': 'seed'})
            with patch.object(runner, 'ax', return_value='0') as ax, \
                 patch.object(runner, 'window', return_value=[20, 51, 984, 639, 0, 0]), \
                 patch.object(runner, 'field'), patch.object(runner, 'sidebar'), \
                 patch('run.time.sleep'):
                runner.reset()
            self.assertFalse(any(c.args[:3] == ('select', '--name', 'Files') for c in ax.call_args_list))
            runner.phase = 'check'
            with patch.object(runner, 'ax', return_value='0'):
                runner.tab('Files')
            self.assertTrue(runner.files_dirty)

    def test_tab_bar_end_without_empty_end_fails_instead_of_skipping(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = self.runner(directory)
            no_room = CommandError('axdrive exited 4: no empty right end', 4, 'no empty right end in the 40-point tab row: 8.0 pt remaining')

            def ax(action, *_args, **_kwargs):
                if action == 'tab-bar-end':
                    raise no_room
                return ''

            with patch.object(runner, 'sidebar'), patch.object(runner, 'window', return_value=[20, 51, 984, 639, 0, 0]), \
                 patch.object(runner, 'ax', side_effect=ax):
                # No exit status turns a missing drag area into a skip.
                for item in (lambda: runner.bar_drag(True), runner.double_click):
                    with self.assertRaisesRegex(AssertionError, 'no empty right end') as caught:
                        item()
                    self.assertNotIsInstance(caught.exception, Skipped)

    def test_bar_drag_uses_tab_bar_end_and_waits_for_sidebar_toggle(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = self.runner(directory)
            calls = []
            windows = iter([[20.0, 51.0, 984.0, 639.0, 0.0, 0.0], [40.0, 70.0, 984.0, 639.0, 0.0, 0.0]])

            def ax(action, *args, **_kwargs):
                calls.append((action, *args))
                if action == 'tab-bar-end':
                    # Full-row right end near window right (1004): must drag left (-35) to stay in bounds.
                    return '980.0 71.0'
                return ''

            with patch.object(runner, 'sidebar'), patch.object(runner, 'window', side_effect=lambda: next(windows)), \
                 patch.object(runner, 'ax', side_effect=ax):
                detail = runner.bar_drag(False)
            self.assertIn('tab-row end (980.0, 71.0)', detail)
            self.assertIn(('tab-bar-end',), calls)
            self.assertIn(('hit', 980.0, 71.0), calls)
            self.assertIn(('drag', 980.0, 71.0, 945.0, 101.0), calls)
            self.assertIn(('press', '--name', 'Toggle sidebar', '--role', 'control'), calls)
            # Must wait via find (for shown=False -> now shown), never instantaneous count.
            self.assertIn(('find', '--name', 'Resize sidebar'), calls)
            self.assertFalse(any(c[0] == 'count' for c in calls))

    def test_app_exited_mid_run_reports_once_with_log_tail_and_skips_rest(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            (base / 'app.log').write_text(
                'line before\n'
                'thread \'main\' panicked at crates/gpui/src/window/a11y.rs:415:13:\n'
                'Duplicate a11y node id: #13414121782461019816.\n'
            )
            runner = self.runner(directory)
            app = Mock()
            app.poll.return_value = None
            app.returncode = None
            runner.app = app
            called = []

            def crash_during_item():
                called.append('settings-editor')
                app.poll.return_value = -signal.SIGABRT
                app.returncode = -signal.SIGABRT
                raise CommandError("axdrive ['select', 99] exited 1: the development copy (process 99) exited", 1, 'exited')

            def launch():
                runner.ready = True
                return 'launched'

            checks = [
                ('first-ok', lambda: called.append('first-ok') or 'ok'),
                ('settings-editor', crash_during_item),
                ('settings-appearance', lambda: self.fail('must not run after app exited')),
                ('settings-about', lambda: self.fail('must not run after app exited')),
            ]
            with patch.object(runner, 'setup', return_value='ok'), \
                 patch.object(runner, 'launch', side_effect=launch), \
                 patch.object(runner, 'reset'), \
                 patch.object(runner, 'checks', return_value=checks), \
                 contextlib.redirect_stdout(io.StringIO()) as log:
                exit_code = runner.run()
            self.assertEqual(exit_code, 1)
            self.assertEqual(called, ['first-ok', 'settings-editor'])
            text = log.getvalue()
            self.assertEqual(text.count('APP EXITED:'), 1)
            self.assertIn('SIGABRT', text)
            self.assertIn('Duplicate a11y node id', text)
            self.assertIn('SKIPPED settings-appearance: not run: app exited', text)
            self.assertIn('SKIPPED settings-about: not run: app exited', text)
            self.assertIn('SUMMARY pass=3 fail=1 skipped=2 incomplete=true', text)
            self.assertEqual([r['status'] for r in runner.results], ['PASS', 'PASS', 'PASS', 'FAIL', 'SKIPPED', 'SKIPPED'])
            self.assertTrue((base / 'app-exit.txt').is_file())
            self.assertIn('Duplicate a11y node id', (base / 'settings-editor-failure.txt').read_text())
            self.assertEqual(format_exit(1), 'code 1')
            self.assertIn('SIGABRT', format_exit(-signal.SIGABRT))
            self.assertIn('Duplicate a11y node id', tail_log(base / 'app.log'))

    def test_select_timeout_probe_runs_before_dump_and_screenshot(self):
        for action in ('select', 'wait-selected'):
            with tempfile.TemporaryDirectory() as directory:
                runner = self.runner(directory)
                app = Mock()
                app.poll.return_value = None
                runner.app = app
                runner.ready = True
                order = []

                def ax(name, *_args, **_kwargs):
                    order.append(name)
                    return '82' if name == 'window-id' else f'tabs after {len(order)} calls'

                def screenshot(args, **_kwargs):
                    order.append('screencapture')
                    return ''

                failing = lambda: (_ for _ in ()).throw(AssertionError(f"axdrive ['{action}', 1] exited 1: timed out"))
                with patch.object(runner, 'reset'), patch.object(runner, 'ax', side_effect=ax), \
                     patch('run.command', side_effect=screenshot), patch('run.time.sleep', side_effect=lambda _s: order.append('sleep')), \
                     contextlib.redirect_stdout(io.StringIO()):
                    runner.record('tab-probe', failing)
                # The capture may wake a stalled frame, so both probes must precede it.
                self.assertEqual(order[:3], ['tab-states', 'sleep', 'tab-states'], action)
                self.assertLess(order.index('tab-states', 2), order.index('dump'))
                self.assertLess(order.index('screencapture'), order.index('front'))
                diag = (Path(directory) / 'tab-probe-select-diag.txt').read_text().splitlines()
                self.assertEqual([line.split(':')[0] for line in diag], ['t+0.0s', 't+1.5s-passive', 'after-screenshot', 'after-front'])
                self.assertTrue(runner.prev_select_failed)

    def test_tab_and_thread_selection_use_idempotent_driver_operation(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = self.runner(directory)
            runner.fixture.thread = {'id': 'seed'}
            with patch.object(runner, 'ax', return_value='0') as ax, patch.object(runner, 'field'), patch.object(runner, 'sidebar'):
                runner.tab('Conversation')
                runner.select_thread()
            ax.assert_any_call('select', '--name', 'Conversation', '--role', 'control')
            ax.assert_any_call('select', '--prefix', 'Thread seed ·', '--role', 'AXRow')
            self.assertFalse(any(call.args[0] == 'press' for call in ax.call_args_list))

    def test_wait_names_expected_state_last_observation_and_short_budget(self):
        clock = iter([0, 0, 1, 2, 3])
        with patch('fixture.time.monotonic', side_effect=lambda: next(clock)), patch('fixture.time.sleep'):
            with self.assertRaisesRegex(AssertionError, "after 3s waiting for file contents to become allowed; last observed: before"):
                until(lambda: 'before', lambda value: value == 'allowed', description='file contents to become allowed')
        # Prevent future ambiguous waits, including the windowless fixture mode.
        for name in ('fixture.py', 'run.py'):
            tree = ast.parse((Path(__file__).parent / name).read_text())
            waits = [node for node in ast.walk(tree) if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id == 'until']
            self.assertTrue(waits)
            for node in waits:
                self.assertIn('description', [keyword.arg for keyword in node.keywords], (name, node.lineno))

    def test_failed_item_is_uploaded_before_capture_or_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory) / 'scratch'
            base.mkdir()
            output = Path(directory) / 'upload'
            output.mkdir()
            runner = Acceptance(SimpleNamespace(host=Path('/unused'), artifacts=output), base, Path('/never-call-driver'))
            runner.app = Mock()

            def cut_off_capture(name):
                self.assertEqual(json.loads((output / 'results.json').read_text())[0]['status'], 'FAIL')
                self.assertIn('named Conversation', (output / f'{name}-failure.txt').read_text())
                self.assertIn('FAILED ITEM fixture-isolation', (output / 'failed-items.txt').read_text())
                self.assertFalse((base / 'results.json').exists())
                (output / f'{name}-ax.txt').write_text('captured tree')
                raise RunInterrupted('cut off during screenshot')

            with patch.object(runner, 'setup', side_effect=AssertionError('named Conversation missing')), \
                 patch.object(runner, 'launch') as launch, patch.object(runner, 'capture', side_effect=cut_off_capture), \
                 contextlib.redirect_stdout(io.StringIO()) as log:
                self.assertEqual(runner.run(), 1)
            launch.assert_not_called()
            self.assertEqual(len(runner.results), 46)
            self.assertIn('SUMMARY pass=0 fail=1 skipped=45 incomplete=true', log.getvalue())
            self.assertTrue((output / 'fixture-isolation-ax.txt').exists())
            self.assertEqual(len(json.loads((output / 'results.json').read_text())), 46)

    def test_deadline_and_sigterm_finish_all_items_without_running_later_ones(self):
        for sig in (signal.SIGALRM, signal.SIGTERM):
            with self.subTest(signal=sig), tempfile.TemporaryDirectory() as directory:
                runner = self.runner(directory)
                with patch.object(runner, 'setup', side_effect=lambda: stop_run(sig, None)), \
                     patch.object(runner, 'launch') as launch, patch('run.signal.setitimer'), \
                     contextlib.redirect_stdout(io.StringIO()) as log:
                    self.assertEqual(runner.run(), 1)
                    runner.finish()
                launch.assert_not_called()
                self.assertEqual(len(runner.results), 46)
                self.assertEqual(log.getvalue().count('SUMMARY'), 1)
                self.assertIn('incomplete=true', log.getvalue())
                self.assertTrue(all(row['status'] == 'SKIPPED' for row in runner.results[1:]))

    @unittest.skipUnless(hasattr(signal, 'setitimer'), 'requires Unix signal timer')
    def test_real_deadline_interrupts_blocking_setup(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = self.runner(directory)
            previous = signal.signal(signal.SIGALRM, stop_run)
            try:
                with patch.object(runner, 'setup', side_effect=lambda: time.sleep(10)), \
                     patch.object(runner, 'launch') as launch, contextlib.redirect_stdout(io.StringIO()) as log:
                    signal.setitimer(signal.ITIMER_REAL, .02)
                    self.assertEqual(runner.run(), 1)
                launch.assert_not_called()
                self.assertIn('run deadline', log.getvalue())
                self.assertIn('SUMMARY pass=0 fail=1 skipped=45', log.getvalue())
            finally:
                signal.setitimer(signal.ITIMER_REAL, 0)
                signal.signal(signal.SIGALRM, previous)

    def test_compile_deadline_also_keeps_summary_and_live_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            base, output = Path(directory) / 'scratch', Path(directory) / 'upload'
            base.mkdir()
            with patch('run.sys.argv', ['run.py', '--artifacts', str(output)]), \
                 patch('run.sys.platform', 'darwin'), patch('run.os.umask'), \
                 patch('run.tempfile.mkdtemp', return_value=str(base)), \
                 patch('run.signal.signal'), patch('run.signal.setitimer') as timer, \
                 patch('run.command', side_effect=RunInterrupted('deadline during compilation')) as command, \
                 patch.object(Acceptance, 'close'), contextlib.redirect_stdout(io.StringIO()) as log:
                self.assertEqual(main(), 1)
            self.assertEqual(command.call_count, 1)
            self.assertEqual(command.call_args.args[0][:2], ['/usr/bin/xcrun', 'swiftc'])
            timer.assert_any_call(signal.ITIMER_REAL, RUN_SECONDS)
            self.assertIn('SUMMARY pass=0 fail=0 skipped=46 incomplete=true', log.getvalue())
            self.assertEqual(len(json.loads((output / 'results.json').read_text())), 46)
            self.assertLessEqual(RUN_SECONDS, 8 * 60 - 60)

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
        self.assertEqual(len(names), 44)
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
            fixture.thread = {'id': 'fixture'}
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


class ForcedFrameTests(unittest.TestCase):
    def runner(self, directory, enabled):
        with patch.dict(os.environ, {'PLAIT_ACCEPTANCE_FORCE_FRAMES': enabled}):
            return Acceptance(SimpleNamespace(host=Path('/unused'), no_pointer=False),
                              Path(directory), Path('/never-call-driver'))

    def replay(self, **spec):
        driver = os.environ.get('PLAIT_ACCEPTANCE_TEST_DRIVER')
        if not driver:
            self.skipTest('Swift wait replay requires macOS --self-test')
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'wait.json'
            path.write_text(json.dumps(spec))
            # This command returns before PID parsing or any native API access.
            result = subprocess.run([driver, 'self-test-wait', str(path)],
                                    check=True, capture_output=True, text=True, timeout=5)
        return json.loads(result.stdout)

    def test_helper_mode_off_replays_original_calls_and_order(self):
        result = self.replay(values=[False, False, True], timeout=1, force=False)
        self.assertTrue(result['success'])
        self.assertEqual(result['calls'], ['read 0.000', 'sleep 0.100', 'read 0.100',
                                           'sleep 0.100', 'read 0.200'])
        timeout = self.replay(values=[False], timeout=.2, force=False)
        self.assertFalse(timeout['success'])
        self.assertEqual(timeout['calls'], result['calls'])
        self.assertEqual(self.replay(values=[True], timeout=0, force=False)['calls'], ['read 0.000'])

    def test_helper_forces_only_pending_states_at_two_hz_within_deadline(self):
        for timeout in [.2, 1.2]:
            result = self.replay(values=[False], timeout=timeout, force=True)
            self.assertFalse(result['success'])
            self.assertAlmostEqual(result['elapsed'], timeout)
            times = [float(c.split()[1]) for c in result['calls'] if c.startswith('force ')]
            self.assertEqual(times[0], 0)
            self.assertTrue(all(b - a >= .4999 for a, b in zip(times, times[1:])))
            self.assertTrue(all(t < timeout for t in times))
            self.assertEqual(len(times), 1 if timeout == .2 else 3)
        ready = self.replay(values=[True], timeout=1, force=True)
        self.assertEqual(ready['calls'], ['read 0.000'])
        zero = self.replay(values=[True], timeout=0, force=True)
        self.assertTrue(zero['success'])
        self.assertEqual(zero['calls'], ['read 0.000'])
        recent = self.replay(values=[False], timeout=.2, force=True, last_frame=-.1)
        self.assertFalse(any(c.startswith('force ') for c in recent['calls']))

    def test_helper_rereads_after_capture_but_never_accepts_after_deadline(self):
        result = self.replay(values=[False, True], timeout=1, force=True, capture_duration=.05)
        self.assertTrue(result['success'])
        self.assertEqual(result['calls'], ['read 0.000', 'force 0.000000 budget 1.000000', 'read 0.050'])
        late = self.replay(values=[False, True], timeout=.2, force=True, capture_duration=1)
        self.assertFalse(late['success'])
        self.assertEqual(late['calls'], ['read 0.000', 'force 0.000000 budget 0.200000'])
        self.assertEqual(late['elapsed'], .2)
        for enabled in [False, True]:
            slow_read = self.replay(values=[True], timeout=.2, force=enabled, read_duration=.3)
            self.assertEqual(slow_read['success'], not enabled)

    def test_streaming_replays_dumps_and_requires_two_lengths_before_done(self):
        # Scripted dump sequence in the observed AccessKit format; the Done row
        # is reduced from the CI failure dump (thread identity omitted).
        short = 'AXGroup/AXApplicationGroup title="Plait is writing: Scripted str" label="" value="" frame=284 468 696 47'
        longer = 'AXGroup/AXApplicationGroup title="Plait is writing: Scripted streaming acceptance." label="" value="" frame=284 468 696 47'
        done = 'AXStaticText/AXUnknown title="" label="" value="Done" frame=204.0 267.0 28.0 18.0'
        recorded = [{'at': 0, 'dump': short}, {'at': .25, 'dump': longer}, {'at': .8, 'dump': done}]
        active = self.replay(kind='growth', force=True, timeout=2, dumps=recorded)
        self.assertTrue(active['success'])
        self.assertEqual(active['lengths'], [30, 48])
        self.assertEqual(active['calls'][:2], ['force 0.000000 budget 2.000000', 'read 0.000'])
        stalled = self.replay(kind='growth', force=False, timeout=2, dumps=recorded)
        self.assertFalse(stalled['success'])
        self.assertIn('control absent; observed=[none]', stalled['failure'])
        self.assertFalse(any(c.startswith('force ') for c in stalled['calls']))
        no_growth = self.replay(kind='growth', force=True, timeout=2,
                                dumps=[recorded[0], recorded[2]])
        self.assertFalse(no_growth['success'])
        self.assertIn('disappeared', no_growth['failure'])
        self.assertLess(no_growth['elapsed'], 2)
        already_done = self.replay(kind='growth', force=True, timeout=1, dumps=[{'at': 0, 'dump': done}])
        self.assertFalse(already_done['success'])
        self.assertEqual(already_done['elapsed'], 1)
        self.assertIn('timed out after 1.0s', already_done['failure'])
        # Normal display: the original 50 ms sequence, with no capture calls.
        natural = self.replay(kind='growth', force=False, timeout=1, spontaneous=True,
                              dumps=[recorded[0], {'at': .1, 'dump': longer}, {'at': .25, 'dump': done}])
        self.assertTrue(natural['success'])
        expected = []
        for at in [0, .05, .1, .15, .2]:
            expected.extend([f'read {at:.3f}', 'sleep 0.050'])
        self.assertEqual(natural['calls'], expected + ['read 0.250'])
        slow_read = self.replay(kind='growth', force=True, timeout=.2, read_duration=.3, dumps=recorded)
        self.assertFalse(slow_read['success'])
        self.assertEqual(slow_read['lengths'], [])

    def test_runner_mode_off_preserves_recorded_reads_sleeps_and_error(self):
        with tempfile.TemporaryDirectory() as directory:
            for enabled in ['', '0', 'true']:
                runner = self.runner(directory, enabled)
                for values in [['old', 'old', 'new'], ['old']]:
                    outcomes = []
                    for wait in [until, runner.ax_until]:
                        now, calls, index = [0.0], [], [0]

                        def read():
                            calls.append(('read', now[0]))
                            value = values[min(index[0], len(values) - 1)]
                            index[0] += 1
                            return value

                        def sleep(delay):
                            calls.append(('sleep', delay))
                            now[0] += delay

                        with patch('fixture.time.monotonic', side_effect=lambda: now[0]), \
                             patch('fixture.time.sleep', side_effect=sleep), \
                             patch.object(runner, 'force_frame', side_effect=AssertionError('must not capture')):
                            try:
                                result = wait(read, lambda v: v == 'new', .3, description='recorded value')
                            except AssertionError as error:
                                result = str(error)
                        outcomes.append((result, calls))
                    self.assertEqual(*outcomes)
                    expected = [('read', 0.0), ('sleep', .1), ('read', .1), ('sleep', .1), ('read', .2)]
                    if len(values) == 1:
                        expected.append(('sleep', .1))
                        self.assertEqual(outcomes[0][0], 'timed out after 0.3s waiting for recorded value; last observed: old')
                    else:
                        self.assertEqual(outcomes[0][0], 'new')
                    self.assertEqual(outcomes[0][1], expected)
                runner.app = Mock(pid=123)
                runner.app.poll.return_value = None
                with patch('run.command', return_value='0') as command:
                    self.assertEqual(runner.ax('count', '--name', 'Done'), '0')
                command.assert_called_once_with([runner.driver, 'count', 123, '--name', 'Done'], timeout=15)
                self.assertFalse(runner.frame_log.exists())

    def test_runner_pending_capture_cannot_extend_wait_or_accept_late_state(self):
        now, calls = [0.0], []

        def capture(deadline):
            calls.append(('force', now[0], deadline))
            now[0] = deadline
            return True

        def read():
            calls.append(('read', now[0]))
            return 'ready' if now[0] >= .3 else 'pending'

        with patch('fixture.time.monotonic', side_effect=lambda: now[0]), \
             patch('fixture.time.sleep', side_effect=AssertionError('no extra sleep')):
            with self.assertRaisesRegex(AssertionError, 'timed out after 0.3s waiting for sample; last observed: pending'):
                until(read, lambda x: x == 'ready', .3, description='sample', on_pending=capture)
        self.assertEqual(calls, [('read', 0), ('force', 0, .3)])

    def test_forced_frames_are_counted_on_success_failure_and_reset_and_logged(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = self.runner(directory, '1')
            runner.app = Mock(pid=123)
            runner.app.poll.return_value = None
            runner.ready = True

            def command(*_args, **kwargs):
                self.assertEqual(kwargs['env']['PLAIT_ACCEPTANCE_FORCE_FRAMES'], '1')
                with runner.frame_log.open('a') as log:
                    log.write(json.dumps({'forced': True, 'time': 0, 'detail': 'forcing frame with screencapture'}) + '\n')
                    log.write(json.dumps({'forced': False, 'time': .1, 'detail': 'capture exited 0'}) + '\n')
                if _args[0][1] == 'select-fails':
                    raise AssertionError('same missing state')
                return 'selected'

            def fail():
                runner.ax('select-fails')

            with patch('run.command', side_effect=command), \
                 patch.object(runner, 'reset', side_effect=lambda: runner.ax('select')), \
                 patch.object(runner, 'capture'), patch.object(runner, 'checks', return_value=[]), \
                 contextlib.redirect_stdout(io.StringIO()) as output:
                runner.record('good', lambda: runner.ax('select'))
                runner.record('bad', fail)
                runner.finish()
            self.assertIn('PASS good: selected (after 2 forced frames)', output.getvalue())
            self.assertIn('FAIL bad: check: same missing state', output.getvalue())
            self.assertEqual(runner.results[0]['forced_frames'], 2)
            self.assertEqual(runner.results[1]['forced_frames'], 2)
            self.assertEqual(runner.results[1]['observed'], 'check: same missing state')
            self.assertIn('FORCED FRAMES items=2 pass=1 fail=1 skipped=0 captures=4', output.getvalue())
            steps = (Path(directory) / 'good-steps.log').read_text()
            self.assertIn('reset: axdrive select: forcing frame', steps)
            self.assertIn('check: PASS good after 2 forced frames', steps)
            self.assertEqual(forced_summary([]), 'FORCED FRAMES items=0 pass=0 fail=0 skipped=0 captures=0')

    def test_ax_wait_inventory_excludes_non_ax_waits_and_keeps_shortcut_check(self):
        # Wiring guard: new waits must choose the same pump, while Git/files and
        # frontmost/clipboard waits must not start captures on their own.
        root = Path(__file__).parent
        source = (root / 'run.py').read_text()
        tree = ast.parse(source)
        routed = []
        for fn in ast.walk(tree):
            if isinstance(fn, ast.FunctionDef):
                if any(isinstance(n, ast.Call) and isinstance(n.func, ast.Attribute)
                       and n.func.attr == 'ax_until' for n in ast.walk(fn)):
                    routed.append(fn.name)
        self.assertEqual(sorted(routed), ['bar_drag', 'double_click', 'field', 'resize'])
        swift = (root / 'axdrive.swift').read_text()
        self.assertEqual(swift.count('axPoll(pid, selector.timeout'), 5)  # waitFor, selected, window, absent, full screen
        self.assertIn('observeGrowth(axTimeout(selector.timeout), pump: forceFrames ? activePump?.force : nil)', swift)
        self.assertIn('let forward = poll(3)', swift)
        self.assertIn('let copied = poll(2', swift)
        self.assertIn("until(path.read_text, lambda text: text == 'allowed\\n', timeout=15, description='Redo to restore approval.txt to allowed')", source)


if __name__ == '__main__':
    unittest.main()
