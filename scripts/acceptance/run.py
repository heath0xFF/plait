#!/usr/bin/env python3
"""Native acceptance runner. Only --self-test and --fixture-test are windowless."""
import argparse
from collections import Counter
import json
import os
import plistlib
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time

from fixture import ANSWER, Fixture, request, require, until, write_json

ROOT = Path(__file__).resolve().parents[2]


class Skipped(Exception):
    pass


def command(args, timeout=30, env=None):
    result = subprocess.run([str(arg) for arg in args], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout, env=env)
    require(result.returncode == 0, f'{Path(str(args[0])).name} {args[1:3]} exited {result.returncode}: {result.stderr.strip() or result.stdout.strip()}')
    return result.stdout.strip()


def check_lights(values, toggle):
    require(len(values) == 16 and len(toggle) == 4, f'invalid frames: {values}; {toggle}')
    x, y, width, _height = values[:4]
    lights = [values[i:i + 4] for i in (4, 8, 12)]
    for lx, ly, lw, lh in lights:
        require(lw > 0 and lh > 0 and x <= lx < lx + lw <= x + width, f'light outside window: {values}')
        require(y <= ly and ly + lh <= y + 44 and abs(ly + lh / 2 - (y + 22)) <= 3, f'light not centered on 44-point bar: {values}')
    require(abs(lights[0][0] - x - 14) <= 4, f'traffic lights not about 14 pt in: {lights}')
    right = max(lx + lw for lx, _ly, lw, _lh in lights)
    require(toggle[0] >= right + 4, f'header toggle overlaps lights: lights={lights}, toggle={toggle}')
    return f'light centers within 3 pt of bar center; {toggle[0] - right:.1f} pt before header control'


def summary(results):
    counts = Counter(result['status'] for result in results)
    return f'SUMMARY pass={counts["PASS"]} fail={counts["FAIL"]} skipped={counts["SKIPPED"]}'


def failure_lines(results):
    return [f'FAILED ITEM {r["item"]}: {" ".join(r["observed"].split())}'
            for r in results if r['status'] == 'FAIL']


def inspector_skip(width):
    if width < 1150:
        return f'Git inspector needs a wider window (1150 pt minimum; own window is {width:g} pt after sizing to the available display)'
    return None


class Acceptance:
    def __init__(self, args, base, driver):
        self.args, self.base, self.driver = args, base, driver
        self.fixture = Fixture(base, args.host.resolve())
        self.app = None
        self.results = []
        self.ready = False
        self.double_action = 'Maximize'

    def ax(self, action, *args, timeout=15):
        require(self.app is not None and self.app.poll() is None, 'development app is not running')
        return command([self.driver, action, self.app.pid, *args], timeout=timeout)

    def find(self, name, role=None):
        return self.ax('find', '--name', name, *(['--role', role] if role else []))

    def exists(self, name, role=None):
        return self.ax('count', '--name', name, *(['--role', role] if role else [])) != '0'

    def press(self, name, role='control'):
        return self.ax('press', '--name', name, '--role', role)

    def key(self, key):
        self.ax('key', key)
        time.sleep(.15)

    def field(self, name, text, typed=False):
        self.ax('focus', '--name', name, '--role', 'field')
        if typed:
            self.key('cmd+a')
            self.ax('type', '--text', text, '--delay', '0.002', timeout=30)
        else:
            self.ax('set-value', '--name', name, '--role', 'field', '--text', text)
        until(lambda: self.ax('value', '--name', name, '--role', 'field'), lambda value: value == text)

    def frame(self, name):
        return [float(v) for v in self.ax('frame', '--name', name).split()]

    def window(self):
        return [float(v) for v in self.ax('window').split()]

    def selected(self, name):
        until(lambda: self.ax('selected', '--name', name, '--role', 'control'), lambda value: value == '1')

    def tab(self, name):
        # A finished sidebar thread also has static text named Done.
        if self.exists('Done', 'AXButton'):
            self.press('Done')
            self.ax('absent', '--name', 'Done', '--role', 'AXButton')
        self.press(name)
        self.selected(name)

    def sidebar(self, wanted):
        if self.exists('Resize sidebar') != wanted:
            self.press('Toggle sidebar')
        self.find('Resize sidebar') if wanted else self.ax('absent', '--name', 'Resize sidebar')

    def inspector(self, wanted):
        self.tab('Conversation')
        self.ax('place', '1380', '860')
        reason = inspector_skip(self.window()[2])
        if reason:
            raise Skipped(reason)
        self.find('Toggle Git inspector', 'control')
        if self.exists('Resize Git inspector') != wanted:
            self.press('Toggle Git inspector')
        self.find('Resize Git inspector') if wanted else self.ax('absent', '--name', 'Resize Git inspector')

    def pointer(self):
        if self.args.no_pointer:
            raise Skipped('--no-pointer: this item needs real pointer events')

    def select_thread(self):
        self.tab('Conversation')
        self.sidebar(True)
        self.field('Filter projects and threads', '')
        self.ax('press', '--prefix', f'Thread {self.fixture.thread["id"]} ·', '--role', 'AXRow')
        until(lambda: self.ax('selected', '--prefix', f'Thread {self.fixture.thread["id"]} ·', '--role', 'AXRow'), lambda value: value == '1')
        self.find('Message to agent', 'field')

    def reset(self):
        # Establish each precondition, even after a previous item passed halfway.
        self.fixture.reset()
        window = self.window()
        if window[5]:
            self.ax('minimized', 'off')
        if window[4]:
            self.ax('fullscreen', 'off', '--timeout', '10')
        self.ax('front')
        self.ax('place', '1380', '860')
        self.key('escape')
        if self.exists('Close', 'AXButton'):
            self.press('Close')  # Sidebar action menu has its own close control.
        self.select_thread()
        # Find can stay open across tabs, and each field retains its last query.
        for tab in ('Files', 'Conversation'):
            self.tab(tab)
            if self.exists('Close find', 'AXButton'):
                self.press('Close find')
        self.field('Message to agent', '')
        self.key('cmd+r')
        self.ax('find', '--prefix', f'Assistant message: {ANSWER}', '--role', 'AXGroup')

    def record(self, name, fn, native=True):
        try:
            if native and not self.ready:
                raise Skipped('native setup unavailable; see setup/permissions result')
            if native:
                self.reset()
            detail = fn() or 'expected state observed'
            status = 'PASS'
        except Skipped as error:
            status, detail = 'SKIPPED', str(error)
        except Exception as error:
            status, detail = 'FAIL', str(error)
            if self.app:
                self.capture(name)
        self.results.append({'item': name, 'status': status, 'observed': detail})
        # Exactly one result line per item; verbose evidence goes into files.
        print(f'{status} {name}: {" ".join(detail.split())}', flush=True)
        write_json(self.base / 'results.json', self.results)

    def capture(self, name):
        if not self.app or self.app.poll() is not None:
            (self.base / f'{name}-capture.txt').write_text('Development process has exited; no window to capture.\n')
            return
        errors = []
        try:
            (self.base / f'{name}-ax.txt').write_text(self.ax('dump', timeout=30))
        except Exception as error:
            errors.append(f'AX dump: {error}')
        try:
            (self.base / f'{name}-windows.txt').write_text(self.ax('windows'))
        except Exception as error:
            errors.append(f'Window diagnostics: {error}')
        try:
            window_id = self.ax('window-id')
            require(window_id.isdecimal(), f'invalid window ID: {window_id}')
            command(['/usr/sbin/screencapture', '-x', '-l', window_id, self.base / f'{name}.png'])
        except Exception as error:
            errors.append(f'Window capture: {error}; grant Screen Recording to the invoking terminal')
        if errors:
            (self.base / f'{name}-capture.txt').write_text('\n'.join(errors) + '\n')

    def setup(self):
        require(self.args.host.is_file() and os.access(self.args.host, os.X_OK), f'build host first: {self.args.host}')
        self.fixture.start()
        require(self.fixture.git('config', '--get', 'user.email') == 'acceptance@example.invalid', 'fixture Git identity changed')
        return 'production host, replay, Git checkout, home, preferences and desktop journal are disposable'

    def launch(self):
        require(self.args.app.is_file() and os.access(self.args.app, os.X_OK), f'build desktop first: {self.args.app}')
        result = subprocess.run([str(self.driver), 'preflight'], capture_output=True, text=True)
        (self.base / 'permissions.txt').write_text(result.stdout + result.stderr)
        if result.returncode:
            raise Skipped('Accessibility permission unavailable; grant it to the terminal/runner and helper, then rerun (no permission prompt is issued)')
        require(self.fixture.host and self.fixture.host.poll() is None, 'fixture host is unavailable')
        setting = subprocess.run(['/usr/bin/defaults', 'read', '-g', 'AppleActionOnDoubleClick'], capture_output=True, text=True)
        self.double_action = setting.stdout.strip() if setting.returncode == 0 else 'Maximize'
        # CF preferences are isolated too. Mirror this one read-only system choice
        # so the isolated app sees the same double-click behavior as the desktop.
        preferences = self.fixture.home / 'Library/Preferences'
        preferences.mkdir(parents=True, exist_ok=True)
        (preferences / '.GlobalPreferences.plist').write_bytes(plistlib.dumps({'AppleActionOnDoubleClick': self.double_action}))
        # Copy only the development executable; it embeds its assets. Never use open -a.
        executable = self.base / 'plait-acceptance'
        shutil.copy2(self.args.app, executable)
        with (self.base / 'app.log').open('w') as log:
            self.app = subprocess.Popen([str(executable), str(self.fixture.repo)], env=self.fixture.env, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
        self.ax('wait-window', '--timeout', '20', timeout=25)
        self.ax('front')
        self.ax('place', '1380', '860')
        self.find('Message to agent', 'field')
        self.ax('find', '--prefix', f'Assistant message: {ANSWER}')
        self.ready = True
        return f'PID {self.app.pid}; {result.stdout.strip().replace(chr(10), "; ")}'

    def lights(self):
        self.tab('Conversation')
        self.sidebar(False)
        values = [float(v) for v in self.ax('lights').split()]
        return check_lights(values, self.frame('Toggle sidebar'))

    def gap(self, sidebar=False):
        x, y, width, _height = self.window()[:4]
        start, end = (x + 90, self.frame('Resize sidebar')[0] - 8) if sidebar else (x + 90, x + width - 30)
        return [float(v) for v in self.ax('bar-gap', start, end).split()]

    def bar_drag(self, shown):
        self.pointer()
        self.sidebar(shown)
        before = self.window()
        x, y = self.gap(sidebar=shown)
        self.ax('hit', x, y)
        self.ax('drag', x, y, x + 45, y + 30)
        after = until(self.window, lambda rect: abs(rect[0] - before[0]) > 10 or abs(rect[1] - before[1]) > 10)
        # Header controls must still activate after the native move.
        self.press('Toggle sidebar')
        require(self.exists('Resize sidebar') != shown, 'sidebar button stopped responding after dragging')
        self.sidebar(True)
        self.ax('place', '1380', '860')
        return f'window moved from {before[:2]} to {after[:2]}; header control still responds'

    def double_click(self):
        self.pointer()
        action = self.double_action
        if action not in ('None', 'Minimize', 'Maximize', 'Fill'):
            raise Skipped(f'unrecognized system double-click setting: {action!r}; no preference changed')
        self.sidebar(True)
        self.ax('place', '1000', '650')
        before = self.window()
        self.ax('double-click', *self.gap(sidebar=True))
        time.sleep(1)
        if action == 'Minimize':
            require(self.ax('minimized', 'get') == '1', f'setting {action}; observed {self.window()}')
        elif action == 'None':
            after = self.window()
            require(all(abs(a - b) < 3 for a, b in zip(before, after)), f'setting None; before={before}, after={after}')
        else:
            until(self.window, lambda rect: abs(rect[2] - before[2]) > 10 or abs(rect[3] - before[3]) > 10)
        return f'system setting {action}; matching minimize/no-op/resize observed (other settings not changed or tested)'

    def fullscreen(self):
        self.ax('fullscreen', 'on', '--timeout', '10', timeout=15)
        require(self.window()[4] == 1, 'full screen attribute is false')
        self.ax('fullscreen', 'off', '--timeout', '10', timeout=15)
        require(self.window()[4] == 0, 'full screen attribute stayed true')
        self.press('Toggle sidebar')
        self.sidebar(True)
        return 'AXFullScreen entered and exited; header control responds afterwards'

    def toggle(self, panel):
        name, key = ('Resize sidebar', 'cmd+b') if panel == 'sidebar' else ('Resize Git inspector', 'cmd+l')
        ensure = self.sidebar if panel == 'sidebar' else self.inspector
        button = 'Toggle sidebar' if panel == 'sidebar' else 'Toggle Git inspector'
        ensure(True)
        self.press(button)
        self.ax('absent', '--name', name)
        self.key(key)
        self.find(name)
        self.key(key)
        self.ax('absent', '--name', name)
        self.press(button)
        self.find(name)
        return f'AXPress and {key} both hide and restore the panel'

    def resize(self, panel):
        self.pointer()
        self.ax('place', '1380', '860')
        (self.sidebar if panel == 'sidebar' else self.inspector)(True)
        name = 'Resize sidebar' if panel == 'sidebar' else 'Resize Git inspector'
        before = self.frame(name)
        old_width = json.loads(self.fixture.preferences.read_text()).get(f'{panel}_width')
        x, y = before[0] + before[2] / 2, before[1] + before[3] / 2
        delta = 50 if panel == 'sidebar' else -50
        self.ax('drag', x, y, x + delta, y)
        after = until(lambda: self.frame(name), lambda rect: abs(rect[0] - before[0]) >= 20)
        saved = until(lambda: json.loads(self.fixture.preferences.read_text()), lambda prefs: prefs.get(f'{panel}_width') is not None and prefs[f'{panel}_width'] != old_width)
        return f'edge moved {after[0] - before[0]:.1f} pt; saved width {saved[f"{panel}_width"]}'

    def workspace(self, name):
        self.tab(name)
        if name == 'Conversation':
            self.find('Message to agent', 'field')
        elif name == 'Changes':
            self.find('Commit message', 'field')
        elif name == 'History':
            self.ax('find', '--prefix', 'Acceptance baseline,')
        else:
            self.find('Open File…', 'control')
        return f'{name} selected with its content present'

    def settings(self, tab=None):
        if not self.exists('Done', 'AXButton'):
            self.key('cmd+comma')
        self.find('Done', 'control')
        if tab:
            self.press(tab)
            self.selected(tab)
            markers = {'New threads': 'Models', 'Providers': 'Find provider', 'Agents': 'Agent harnesses', 'Keyboard': 'Find shortcut', 'Editor': 'Open files with', 'Appearance': 'Code font', 'About': 'About and updates'}
            self.find(markers[tab])
        return f'{tab or "Settings"} is open; expected controls present'

    def project_settings(self):
        self.tab('Conversation')
        self.sidebar(True)
        self.press('Actions for acceptance project project')
        self.press('Project settings…')
        self.find('Commands')
        self.find('Done', 'control')
        self.ax('absent', '--name', 'New threads', '--role', 'control')
        self.press('Done')
        return 'project sidebar menu opened project Commands settings and returned'

    def palette(self):
        self.tab('Conversation')
        self.key('cmd+k')
        self.field('Command palette', 'Changes', typed=True)
        self.ax('find', '--prefix', 'Changes', '--role', 'AXStaticText')
        self.key('return')
        self.selected('Changes')
        self.find('Commit message', 'field')
        return 'Cmd-K, typed search and Return dispatched Changes'

    def open_file(self):
        self.tab('Files')
        self.key('cmd+p')
        self.field('Open file', 'find me.txt', typed=True)
        self.ax('find', '--prefix', 'find me.txt', '--role', 'AXStaticText')
        self.key('return')
        self.find('Open find me.txt in your editor', 'control')

    def find_text(self, conversation=False):
        if conversation:
            self.tab('Conversation')
        else:
            self.open_file()
        name = 'Find in conversation' if conversation else 'Find in file'
        self.key('cmd+f')
        self.field(name, '')
        self.field(name, 'needle', typed=True)
        # Conversation find starts at the newest match when pinned to the end.
        found = self.ax('find', '--regex', '^[1-3] of 3$')
        current = int(re.search(r'\b([1-3]) of 3\b', found)[1])
        following = current % 3 + 1
        self.key('cmd+g')
        self.find(f'{following} of 3')
        self.key('cmd+shift+g')
        self.find(f'{current} of 3')
        self.press('Next match')
        self.find(f'{following} of 3')
        self.press('Previous match')
        self.find(f'{current} of 3')
        self.key('escape')
        self.ax('absent', '--name', name, '--role', 'field')
        return 'Cmd-F found 3 matches; Cmd-G, Cmd-Shift-G and both buttons stepped; Escape closed find'

    def focus_routing(self):
        self.tab('Conversation')
        self.field('Message to agent', 'focus routing draft', typed=True)
        self.key('cmd+shift+2')
        self.selected('Changes')
        self.key('cmd+shift+3')
        self.selected('History')
        self.key('cmd+shift+1')
        self.selected('Conversation')
        self.ax('type', '--text', ' retained')
        require(self.ax('value', '--name', 'Message to agent', '--role', 'field') == 'focus routing draft retained', 'composer focus/draft not restored by shortcut')
        self.open_file()
        self.selected('Files')
        self.key('cmd+f')
        self.find('Find in file', 'field')
        self.key('escape')
        self.key('cmd+shift+1')
        self.selected('Conversation')
        self.key('cmd+f')
        self.find('Find in conversation', 'field')
        self.key('escape')
        self.key('cmd+b')
        self.sidebar(True)
        self.field('Message to agent', '')
        return 'shortcuts and tab presses leave composer focus; Files/Conversation find route correctly and draft survives'

    def branch(self, create):
        self.tab('Changes')
        current = self.fixture.git('branch', '--show-current')
        self.press(f'Branch {current}')
        name = 'acceptance-created' if create else 'other'
        self.field('Switch branch', name, typed=True)
        self.ax('find', '--prefix', f'Create branch {name}' if create else name, '--role', 'AXStaticText')
        self.key('return')
        until(lambda: self.fixture.git('branch', '--show-current'), lambda branch: branch == name)
        self.find(f'Branch {name}', 'control')
        return f'UI and Git HEAD both name {name}'

    def stage(self):
        self.tab('Changes')
        self.ax('press', '--prefix', 'review.txt, Modified,', '--role', 'control')
        self.press('Stage hunk')
        staged = until(lambda: self.fixture.git('diff', '--cached', '--no-ext-diff', '--no-textconv'), lambda diff: '+first new' in diff)
        unstaged = self.fixture.git('diff', '--no-ext-diff', '--no-textconv')
        require('+last new' not in staged and '+last new' in unstaged, f'hunk staging changed the wrong content: staged={staged}; unstaged={unstaged}')
        return 'first hunk staged in Git; second hunk remains unstaged'

    def commit(self):
        self.fixture.stage_first_hunk()
        self.tab('Changes')
        self.key('cmd+r')
        self.find('Commit 1 file', 'control')
        before = self.fixture.git('rev-parse', 'HEAD')
        self.field('Commit message', 'Native acceptance commit', typed=True)
        self.press('Commit 1 file')
        until(lambda: self.fixture.git('rev-parse', 'HEAD'), lambda head: head != before)
        require(self.fixture.git('log', '-1', '--format=%s') == 'Native acceptance commit', 'unexpected commit subject')
        require('+last new' in self.fixture.git('diff', '--no-ext-diff', '--no-textconv'), 'unstaged hunk was lost')
        require(not self.fixture.git('diff', '--cached', '--name-only'), 'index still contains staged changes')
        return 'new commit contains staged hunk; index empty and second hunk remains'

    def custom_commands(self):
        self.settings('Editor')
        self.field('Open files with', self.fixture.editor_command, typed=True)
        self.field('Open terminals with', self.fixture.terminal_command, typed=True)
        expected = {'open_files_with': self.fixture.editor_command, 'open_terminals_with': self.fixture.terminal_command}
        until(lambda: json.loads(self.fixture.preferences.read_text()),
              lambda prefs: all(prefs.get(k) == {'with': 'custom', 'command': v} for k, v in expected.items()))
        self.press('Done')
        return 'both custom fields contain saved, inert argument recorders'

    def handoff(self, terminal):
        # Re-establish and verify safe commands; never inherit another item's result.
        self.custom_commands()
        self.open_file()
        path = self.base / ('terminal-args.txt' if terminal else 'editor-args.txt')
        path.unlink(missing_ok=True)
        if terminal:
            self.key('cmd+opt+t')
            expected = [str(self.fixture.repo)]
        else:
            self.key('cmd+f')
            self.field('Find in file', '')
            self.field('Find in file', 'needle')
            self.find('1 of 3')
            self.key('cmd+g')
            self.find('2 of 3')
            self.press('Open find me.txt in your editor')
            expected = [str(self.fixture.repo / 'find me.txt'), '3', str(self.fixture.repo)]
        actual = until(lambda: path.read_text().splitlines(), lambda args: args == expected)
        if not terminal:
            self.key('escape')
        return f'recorder received {actual}; no external application opened'

    def finder(self):
        self.open_file()
        self.find('Show find me.txt in Finder', 'control')
        raise Skipped('control is accessible; activation uses hardcoded /usr/bin/open -R, with no custom command; would open Finder')

    def long_message(self):
        self.tab('Conversation')
        self.field('Message to agent', '')
        before = self.frame('Message to agent')
        text = ('A long native acceptance draft with Unicode café 🙂 and wrapped words. ' * 24).strip()
        self.field('Message to agent', text, typed=True)
        after = self.frame('Message to agent')
        require(after[3] > before[3], f'composer did not grow: {before} -> {after}')
        until(lambda: json.loads((self.fixture.desktop / 'desktop.json').read_text()), lambda state: state['threads'][self.fixture.thread['id']]['draft']['text'] == text)
        self.field('Message to agent', '')
        return f'{len(text)} characters typed as native Unicode events; draft exact, grew and persisted; not submitted'

    def selection(self):
        self.pointer()
        self.tab('Conversation')
        return self.ax('select-text', '--prefix', f'Assistant message: {ANSWER}', '--text', ANSWER)

    def approval(self, allow):
        self.fixture.new_thread()
        self.select_thread()
        self.tab('Conversation')
        pending = self.fixture.pending(allow)
        self.find('Allow once' if allow else 'Deny once', 'control')
        self.key('cmd+opt+y' if allow else 'cmd+opt+period')
        done = self.fixture.idle()
        require(not done.get('pending_approval'), f'approval still pending: {pending["pending_approval"]}')
        require((self.fixture.repo / 'approval.txt').read_text() == ('allowed\n' if allow else 'before\n'), 'approval shortcut produced wrong file contents')
        self.ax('absent', '--name', 'Allow once', '--role', 'control')
        return f'{"Allow" if allow else "Deny"} shortcut resolved host approval; exact file effect checked'

    def streaming(self):
        self.fixture.new_thread()
        self.select_thread()
        self.tab('Conversation')
        self.fixture.idle()
        text = 'Scripted streaming acceptance. ' * 16
        error_start = len(self.fixture.replay.errors)
        self.fixture.replay.add({'text': text, 'chunk': 12, 'delay': .15})
        self.fixture.submit('Stream the scripted response slowly.')
        growth = self.ax('growth', '--prefix', 'Assistant is writing:', '--timeout', '15', timeout=20)
        self.fixture.idle()
        self.ax('find', '--prefix', 'Assistant message: Scripted streaming acceptance.')
        errors = self.fixture.replay.errors[error_start:]
        require(not errors, f'replay errors: {errors}')
        return f'{growth}; final reply appeared (no smoothness or latency claim)'

    def turn_undo(self, action):
        record = self.fixture.finished_edit()
        self.select_thread()
        self.key('cmd+r')
        path = self.fixture.repo / 'approval.txt'
        head = self.fixture.git('rev-parse', 'HEAD')
        index = self.fixture.git('ls-files', '--stage')
        unrelated = (self.fixture.repo / 'review.txt').read_bytes()
        if action == 'redo':
            self.fixture.undo(record['id'])
            self.key('cmd+r')
            self.find('Redo', 'control')
            self.press('Redo')
            until(path.read_text, lambda text: text == 'allowed\n')
            self.find('Undo', 'control')
        else:
            self.find('Undo', 'control')
            self.key('cmd+opt+u') if action == 'shortcut' else self.press('Undo')
            until(path.read_text, lambda text: text == 'before\n')
            self.find('Redo', 'control')
            if action == 'shortcut':
                self.key('cmd+opt+u')
                until(path.read_text, lambda text: text == 'allowed\n')
                self.find('Undo', 'control')
        require(self.fixture.git('rev-parse', 'HEAD') == head and self.fixture.git('ls-files', '--stage') == index, 'turn undo changed Git HEAD or index')
        require((self.fixture.repo / 'review.txt').read_bytes() == unrelated, 'turn undo changed unrelated fixture bytes')
        return f'{action}: closing-line control and exact recorded file restoration observed; HEAD, index and unrelated changes retained'

    def checks(self):
        checks = [
            ('traffic-lights-sidebar-hidden', self.lights),
            ('drag-sidebar-bar', lambda: self.bar_drag(True)),
            ('drag-header-bar', lambda: self.bar_drag(False)),
            ('double-click-system-setting', self.double_click),
            ('fullscreen-return', self.fullscreen),
            ('sidebar-toggle-and-shortcut', lambda: self.toggle('sidebar')),
            ('inspector-toggle-and-shortcut', lambda: self.toggle('inspector')),
            ('resize-sidebar', lambda: self.resize('sidebar')),
            ('resize-inspector', lambda: self.resize('inspector')),
        ]
        checks += [(f'tab-{name.lower()}', lambda name=name: self.workspace(name)) for name in ['Conversation', 'Changes', 'History', 'Files']]
        checks += [('settings-open', self.settings)]
        checks += [(f'settings-{name.lower().replace(" ", "-")}', lambda name=name: self.settings(name)) for name in ['New threads', 'Providers', 'Agents', 'Keyboard', 'Editor', 'Appearance', 'About']]
        checks += [
            ('project-settings-sidebar-menu', self.project_settings),
            ('command-palette', self.palette),
            ('find-file', self.find_text),
            ('find-conversation', lambda: self.find_text(True)),
            ('composer-tab-focus-shortcuts', self.focus_routing),
            ('switch-branch', lambda: self.branch(False)),
            ('create-branch', lambda: self.branch(True)),
            ('stage-one-hunk', self.stage), ('commit-staged-hunk', self.commit),
            ('custom-handoff-settings', self.custom_commands),
            ('open-in-editor', lambda: self.handoff(False)),
            ('open-terminal-here', lambda: self.handoff(True)),
            ('show-in-finder', self.finder),
            ('long-message-typing', self.long_message),
            ('conversation-text-selection', self.selection),
            ('approval-allow-shortcut', lambda: self.approval(True)),
            ('approval-deny-shortcut', lambda: self.approval(False)),
            ('streaming-reply', self.streaming),
            ('turn-undo', lambda: self.turn_undo('undo')),
            ('turn-redo', lambda: self.turn_undo('redo')),
            ('turn-undo-redo-shortcut', lambda: self.turn_undo('shortcut')),
            ('follow-conversation-link', lambda: (_ for _ in ()).throw(Skipped('opens the system browser; no URL-opening override exists; left for manual acceptance'))),
        ]
        return checks

    def run(self):
        self.record('fixture-isolation', self.setup, native=False)
        self.record('native-setup-permissions', self.launch, native=False)
        for name, fn in self.checks():
            self.record(name, fn)
        print(summary(self.results), flush=True)
        return 1 if any(r['status'] == 'FAIL' for r in self.results) else 0

    def close(self):
        if self.app and self.app.poll() is None:
            self.app.terminate()
            try:
                self.app.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.app.kill()
                self.app.wait(timeout=5)
        self.fixture.close()


def pure_tests(driver):
    # No AX calls, application launch, clipboard access, or key/pointer events.
    command([driver, 'self-test'])
    command([sys.executable, '-m', 'unittest', 'discover', '-s', ROOT / 'scripts/acceptance', '-p', 'test_*.py'])
    print('PASS self-test: Swift selectors/keys/Unicode/gaps and Python isolation/reporting/geometry/SSE tests; no UI accessed')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--no-pointer', action='store_true', help='skip bar/panel drags, double-click and text selection; still activates the development window and sends PID-scoped keys')
    parser.add_argument('--self-test', action='store_true', help='compile driver and run only pure tests; no window or input')
    parser.add_argument('--fixture-test', action='store_true', help='test real host/replay/approvals in scratch; no desktop process, AX, keys or pointer')
    parser.add_argument('--app', type=Path, default=ROOT / 'target/release/plait', help='built development desktop executable')
    parser.add_argument('--host', type=Path, default=ROOT / 'target/release/plait-host', help='built host executable (development or extracted CI artifact)')
    parser.add_argument('--artifacts', type=Path, help='new directory to keep output and screenshots, even on success')
    args = parser.parse_args()
    require(sys.platform == 'darwin', 'native acceptance requires macOS')
    os.umask(0o077)
    base = Path(tempfile.mkdtemp(prefix='plait-acceptance-', dir='/tmp')).resolve()
    driver = base / 'axdrive'
    runner = None
    result = 1
    keep = False
    try:
        if args.artifacts:
            args.artifacts = args.artifacts.resolve()
            require(not args.artifacts.exists(), '--artifacts must be a new directory')
            args.artifacts.mkdir(parents=True)
        if not (args.self_test or args.fixture_test):
            print('This check will take the pointer for a few minutes. Run it when away.' if not args.no_pointer else 'Pointer checks are disabled; this check still activates its own window and types into it.', flush=True)
            print('Permissions: grant Accessibility to the invoking terminal/runner and axdrive if macOS lists it; Screen Recording is needed for failure screenshots. No Automation permission is used or changed.', flush=True)
            print(f'Disposable folder: {base}', flush=True)
        command(['/usr/bin/xcrun', 'swiftc', '-O', ROOT / 'scripts/acceptance/axdrive.swift', '-o', driver], timeout=120)
        if args.self_test:
            pure_tests(driver)
            result = 0
        elif args.fixture_test:
            fixture = Fixture(base, args.host.resolve())
            try:
                fixture.start()
                require(any(m['text'] == ANSWER for m in fixture.watch()['messages']), 'seeded answer absent')
                for allow in (True, False):
                    fixture.reset()
                    fixture.new_thread()
                    state = fixture.pending(allow)
                    request(fixture.store, op='approve', thread_id=fixture.thread['id'], approval_id=state['pending_approval']['id'], allow=allow)
                    fixture.idle()
                    require((fixture.repo / 'approval.txt').read_text() == ('allowed\n' if allow else 'before\n'), 'fixture approval effect wrong')
                fixture.reset()
                fixture.new_thread()
                fixture.pending(True)
                fixture.reset()  # Abandon an unanswered approval as a failed item would.
                require((fixture.repo / 'approval.txt').read_text() == 'before\n', 'reset raced the stopped run')
                record = fixture.finished_edit()
                head, index = fixture.git('rev-parse', 'HEAD'), fixture.git('ls-files', '--stage')
                unrelated = (fixture.repo / 'review.txt').read_bytes()
                undo = fixture.undo(record['id'])
                require((fixture.repo / 'approval.txt').read_text() == 'before\n', 'recorded undo did not restore exact bytes')
                fixture.undo(undo)
                require((fixture.repo / 'approval.txt').read_text() == 'allowed\n', 'recorded redo did not restore exact bytes')
                require(fixture.git('rev-parse', 'HEAD') == head and fixture.git('ls-files', '--stage') == index, 'undo/redo changed HEAD or index')
                require((fixture.repo / 'review.txt').read_bytes() == unrelated, 'undo/redo changed unrelated fixture bytes')
                fixture.reset()
                fixture.new_thread()
                stream_text = 'Scripted streaming acceptance. ' * 8
                fixture.replay.add({'text': stream_text, 'chunk': 12, 'delay': .08})
                fixture.submit('Stream the scripted response slowly.')
                lengths = set()
                def observed():
                    state = fixture.watch()
                    if state.get('live_text'):
                        lengths.add(len(state['live_text']))
                    return state
                done = until(observed, lambda state: state['status'] in ('idle', 'error'), 15)
                require(done['status'] == 'idle' and len(lengths) >= 2, f'stream did not grow: {lengths}; {done.get("error")}')
                require(any(m['text'] == stream_text for m in done['messages']), 'final streaming reply missing')
                output = base / 'recorder-test.txt'
                command([fixture.recorder, output, 'a file with spaces', '3', 'a project with spaces'])
                require(output.read_text().splitlines() == ['a file with spaces', '3', 'a project with spaces'], 'argument recorder changed its input')
                require(not fixture.replay.errors, f'replay errors: {fixture.replay.errors}')
                print('PASS fixture-test: production host/replay, independent allow/deny, stopped-run reset, recorded undo/redo, growing stream and argument recorder; no window or input')
                result = 0
            finally:
                fixture.close()
        else:
            runner = Acceptance(args, base, driver)
            result = runner.run()
            keep = any(r['status'] == 'SKIPPED' for r in runner.results)
    except (Exception, KeyboardInterrupt) as error:
        print(f'FAIL harness: {error}', flush=True)
        if runner:
            runner.capture('harness')
        print('SUMMARY harness failed before completion', flush=True)
    finally:
        if runner:
            try:
                runner.close()
            except Exception as error:
                result = 1
                print(f'FAIL cleanup: {error}', flush=True)
        if args.artifacts and args.artifacts.exists():
            # Keep only test-generated diagnostics, not the host database or binary.
            for path in base.iterdir():
                if path.is_file() and path.suffix in ('.txt', '.log', '.json', '.png'):
                    shutil.copy2(path, args.artifacts / path.name)
        if result or keep:
            print(f'Disposable folder retained: {base}', flush=True)
        else:
            shutil.rmtree(base)
        if runner:
            for line in failure_lines(runner.results):
                print(line, flush=True)
    return result


if __name__ == '__main__':
    signal.signal(signal.SIGTERM, lambda _signum, _frame: sys.exit(130))
    sys.exit(main())
