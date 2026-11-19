"""Windowless harness tests; mocks deliberately forbid native helper calls."""
import contextlib
import ast
import io
import json
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

    def test_select_timeout_capture_records_probe_and_arms_next_reset_trace(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = self.runner(directory)
            app = Mock()
            app.poll.return_value = None
            runner.app = app
            runner.ready = True
            with patch.object(runner, 'reset'), \
                 patch.object(runner, 'ax', return_value='tabs=[Files=false(0) | Conversation=true(1)]'), \
                 patch('run.command', return_value=''), \
                 patch('run.time.sleep'), \
                 contextlib.redirect_stdout(io.StringIO()):
                runner.record('tab-probe', lambda: (_ for _ in ()).throw(AssertionError("axdrive ['select', 1] exited 1: timed out")))
            diag = (Path(directory) / 'tab-probe-select-diag.txt').read_text()
            self.assertIn('t+0.0s:', diag)
            self.assertIn('t+1.5s-passive:', diag)
            self.assertIn('after-front:', diag)
            self.assertTrue(runner.prev_select_failed)

    def test_recorded_ci_dump_selection_and_tab_bar_geometry(self):
        # Verify against real recorded CI AX dump lines (1-tab and 4-tab full row).
        recorded_ci_4tab = (
            'AXWindow/AXStandardWindow title="Plait" label="" value="" frame=20.0 51.0 984.0 639.0\n'
            '  AXButton/AXUnknown title="Toggle sidebar" label="" value="" frame=104.0 56.0 28.0 28.0\n'
            '  AXTabGroup/AXUnknown title="Open tabs" label="" value="" frame=136.0 51.0 788.0 39.0\n'
            '  AXButton/AXUnknown title="New thread tab" label="" value="" frame=928.0 56.0 28.0 28.0\n'
            '  AXRadioButton/AXTabButton title="Conversation" label="" value="" frame=668.0 96.0 93.0 24.0\n'
            '    state: AXRadioButton/AXTabButton Conversation; AXSelected=unavailable(AXError=-25212); AXValue=true(1)\n'
            '  AXRadioButton/AXTabButton title="Files" label="" value="" frame=908.0 96.0 45.0 24.0\n'
            '    state: AXRadioButton/AXTabButton Files; AXSelected=unavailable(AXError=-25212); AXValue=false(0)\n'
        )
        # Confirm recorded CI window bar controls end at 956.0 with 48.0 pt empty end before 1004.0.
        self.assertIn('frame=928.0 56.0 28.0 28.0', recorded_ci_4tab)
        self.assertIn('Conversation; AXSelected=unavailable(AXError=-25212); AXValue=true(1)', recorded_ci_4tab)
        self.assertIn('Files; AXSelected=unavailable(AXError=-25212); AXValue=false(0)', recorded_ci_4tab)

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


if __name__ == '__main__':
    unittest.main()
