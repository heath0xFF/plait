"""Credential-free native acceptance data, through the production host protocol.

The SSE shape follows runtime/test/support/replay.ts. This is a scripted local
provider, not a model or a replacement desktop view. Nothing here ships.
"""
import fcntl
import json
import os
from pathlib import Path
import queue
import socket
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MODEL = 'plait-native-acceptance'
PROVIDER = 'plait-acceptance'
KEY = 'PLAIT_ACCEPTANCE_NOT_A_SECRET'
ANSWER = 'Acceptance needle one. Acceptance needle two. Acceptance needle three.'


def require(condition, detail):
    if not condition:
        raise AssertionError(detail)


def until(read, accept, timeout=3, *, description):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        try:
            last = read()
            if accept(last):
                return last
        except (OSError, ValueError) as error:
            last = str(error)
        time.sleep(.1)
    raise AssertionError(f'timed out after {timeout:g}s waiting for {description}; last observed: {str(last)[:1500]}')


def request(store, protocol_version=4, **body):
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(20)
        connection.connect(str(store / 'host.sock'))
        connection.sendall((json.dumps(dict(protocol_version=protocol_version, **body)) + '\n').encode())
        data = b''
        while b'\n' not in data:
            chunk = connection.recv(65536)
            if not chunk:
                raise OSError('host closed without a reply')
            data += chunk
            require(len(data) <= 8 * 1024 * 1024, 'host reply exceeded bound')
    reply = json.loads(data.split(b'\n', 1)[0])
    require(reply.get('ok'), f'host refused: {reply}')
    return reply['result']


def events(answer, index):
    """Return SSE frames; a delay between deltas makes streaming observable."""
    yield 'message_start', {'message': {
        'id': f'msg_acceptance_{index}', 'type': 'message', 'role': 'assistant',
        'model': MODEL, 'content': [], 'stop_reason': None, 'stop_sequence': None,
        'usage': {'input_tokens': 10, 'output_tokens': 0},
    }}
    if 'text' in answer:
        yield 'content_block_start', {'index': 0, 'content_block': {'type': 'text', 'text': ''}}
        text = answer['text']
        for start in range(0, len(text), answer.get('chunk', len(text))):
            yield 'content_block_delta', {'index': 0, 'delta': {'type': 'text_delta', 'text': text[start:start + answer.get('chunk', len(text))]}}
    else:
        tool = answer['tool']
        yield 'content_block_start', {'index': 0, 'content_block': {'type': 'tool_use', **tool, 'input': {}}}
        yield 'content_block_delta', {'index': 0, 'delta': {'type': 'input_json_delta', 'partial_json': json.dumps(tool['input'])}}
    yield 'content_block_stop', {'index': 0}
    yield 'message_delta', {'delta': {'stop_reason': 'end_turn' if 'text' in answer else 'tool_use', 'stop_sequence': None}, 'usage': {'output_tokens': 5}}
    yield 'message_stop', {}


class Replay:
    def __init__(self):
        self.answers = queue.Queue()
        self.requests = []
        self.errors = []
        replay = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_POST(self):
                try:
                    size = int(self.headers.get('Content-Length', 0))
                    require(0 < size <= 1024 * 1024, 'invalid replay request size')
                    body = json.loads(self.rfile.read(size))
                    require(self.path.split('?')[0] == '/v1/messages', self.path)
                    require(body['model'] == MODEL and body['stream'], 'unexpected model or non-stream request')
                    require(self.headers.get('x-api-key') == KEY, 'unexpected replay key')
                    answer = replay.answers.get_nowait()
                    replay.requests.append(body)
                    self.send_response(200)
                    self.send_header('Content-Type', 'text/event-stream')
                    self.end_headers()
                    for kind, fields in events(answer, len(replay.requests)):
                        if kind == 'content_block_delta':
                            time.sleep(answer.get('delay', 0))
                        self.wfile.write(f'event: {kind}\ndata: {json.dumps(dict(type=kind, **fields))}\n\n'.encode())
                        self.wfile.flush()
                except (BrokenPipeError, ConnectionResetError):
                    # Stopping a failed item cancels its in-flight provider read.
                    pass
                except Exception as error:
                    replay.errors.append(str(error))
                    self.send_error(500, 'Unexpected acceptance fixture request')

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    @property
    def url(self):
        return f'http://127.0.0.1:{self.server.server_port}'

    def add(self, *answers):
        for answer in answers:
            self.answers.put(answer)

    def clear(self):
        # Call only after the host has stopped its run and released the checkout.
        while True:
            try:
                self.answers.get_nowait()
            except queue.Empty:
                return

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)


def write_json(path, data):
    temporary = path.with_name(path.name + '.tmp')
    temporary.write_text(json.dumps(data, indent=2) + '\n')
    temporary.chmod(0o600)
    temporary.replace(path)


class Fixture:
    def __init__(self, base, host_binary):
        self.base = base
        self.repo = base / 'acceptance project'
        self.store = base / 'host'
        self.home = base / 'home'
        self.desktop = base / 'desktop'
        self.preferences = base / 'preferences.json'
        self.host_binary = host_binary
        self.host = None
        self.replay = None
        self.sequence = 0
        self.env = {
            'PATH': os.environ.get('PATH', '/usr/bin:/bin:/usr/sbin:/sbin'),
            'HOME': str(self.home), 'CFFIXED_USER_HOME': str(self.home),
            'XDG_CONFIG_HOME': str(self.home / '.config'),
            'XDG_DATA_HOME': str(self.home / '.local/share'),
            'TMPDIR': str(base / 'tmp'), 'PI_OFFLINE': '1',
            'PI_CODING_AGENT_DIR': str(self.home / '.pi/agent'),
            'PLAIT_HOST_STORE': str(self.store),
            'PLAIT_DESKTOP_STATE_DIR': str(self.desktop),
            'PLAIT_DESKTOP_PREFERENCES_FILE': str(self.preferences),
            'GIT_CONFIG_NOSYSTEM': '1', 'GIT_CONFIG_GLOBAL': '/dev/null',
            'GIT_ATTR_NOSYSTEM': '1', 'GIT_TERMINAL_PROMPT': '0',
            'GIT_OPTIONAL_LOCKS': '0',
        }

    def git(self, *args):
        return subprocess.check_output(['/usr/bin/git', '-C', str(self.repo), *args], env=self.env, text=True, stderr=subprocess.STDOUT).strip()

    def prepare_checkout(self):
        self.repo.mkdir(parents=True, mode=0o700, exist_ok=True)
        self.git('init', '-q', '-b', 'main')
        for key, value in [('user.name', 'Plait Acceptance'), ('user.email', 'acceptance@example.invalid'), ('commit.gpgsign', 'false'), ('core.hooksPath', '/dev/null')]:
            self.git('config', key, value)
        (self.repo / 'review.txt').write_text('first old\n' + ''.join(f'context {i}\n' for i in range(24)) + 'last old\n')
        (self.repo / 'find me.txt').write_text('needle first\nplain line\nneedle second\nneedle third\n')
        (self.repo / 'approval.txt').write_text('before\n')
        (self.repo / 'AGENTS.md').write_text('Disposable native acceptance fixture. Only scripted edits to approval.txt are requested.\n')
        self.git('add', '.')
        self.git('commit', '-qm', 'Acceptance baseline')
        self.baseline = self.git('rev-parse', 'HEAD')
        self.git('branch', 'other')
        self.write_review()

    def start(self):
        for path in [self.store, self.home, self.desktop, self.base / 'tmp', Path(self.env['PI_CODING_AGENT_DIR'])]:
            path.mkdir(parents=True, mode=0o700, exist_ok=True)
        self.prepare_checkout()
        self.replay = Replay()
        write_json(Path(self.env['PI_CODING_AGENT_DIR']) / 'models.json', {'providers': {PROVIDER: {
            'baseUrl': self.replay.url, 'api': 'anthropic-messages', 'apiKey': KEY,
            'models': [{'id': MODEL, 'name': 'Scripted native acceptance fixture', 'reasoning': False, 'contextWindow': 32768, 'maxTokens': 2048}],
        }}})
        with (self.base / 'host.log').open('w') as log:
            self.host = subprocess.Popen([str(self.host_binary), 'run', '--store', str(self.store)], env=self.env, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
        until(lambda: request(self.store, op='status'), lambda status: any(m['provider'] == PROVIDER and m['authenticated'] for m in status.get('models', [])), 30, description='host catalog to offer the authenticated fixture model')
        self.thread = request(self.store, op='create_thread', checkout=str(self.repo), provider=PROVIDER, model_id=MODEL, request_id='acceptance-create')['thread']
        self.seed_thread = self.thread
        self.replay.add({'text': ANSWER})
        self.submit('Native acceptance fixture, with scripted replies and no real model.')
        self.idle()
        write_json(self.desktop / 'desktop.json', {
            'schema_version': 1, 'host_store': str(self.store), 'checkout': str(self.repo), 'selected': self.thread['id'],
            'threads': {self.thread['id']: {'checkout': str(self.repo), 'model': self.thread['model'], 'title': '', 'draft': {'text': '', 'revision': 0, 'submitted': None}, 'pending': None}},
        })
        # Safe commands are present even if the Settings interaction later fails.
        self.recorder = self.base / 'record-arguments'
        self.recorder.write_text('#!/bin/sh\nset -eu\noutput=$1\nshift\nprintf "%s\\n" "$@" > "$output"\n')
        self.recorder.chmod(0o700)
        self.editor_command = f'"{self.recorder}" "{self.base / "editor-args.txt"}" "{{file}}" "{{line}}" "{{project}}"'
        self.terminal_command = f'"{self.recorder}" "{self.base / "terminal-args.txt"}" "{{project}}"'
        write_json(self.preferences, {'open_files_with': {'with': 'custom', 'command': self.editor_command}, 'open_terminals_with': {'with': 'custom', 'command': self.terminal_command}})

    def submit(self, text):
        self.sequence += 1
        return request(self.store, op='submit', thread_id=self.thread['id'], text=text, request_id=f'acceptance-{self.sequence}', approval='ask')

    def watch(self):
        return request(self.store, op='watch', thread_id=self.thread['id'])['thread']

    def idle(self):
        state = until(self.watch, lambda state: state['status'] in ('idle', 'error'), 25, description=f'fixture thread {self.thread["id"]} to finish')
        require(state['status'] == 'idle', f'thread error: {state.get("error")}')
        return state

    def pending(self, allow):
        before = (self.repo / 'approval.txt').read_text()
        after = 'allowed\n' if allow else 'denied\n'
        self.replay.add({'tool': {'id': f'acceptance_edit_{self.sequence}', 'name': 'edit', 'input': {'path': 'approval.txt', 'old_text': before, 'new_text': after}}}, {'text': 'The scripted approval turn is complete.'})
        self.submit('Propose the scripted fixture edit.')
        return until(self.watch, lambda state: state.get('pending_approval'), 15, description=f'edit approval in fixture thread {self.thread["id"]}')

    def reset(self):
        # A failed approval/stream item must not hold the checkout lease or leave
        # scripted answers for a later, unrelated run to consume.
        state = self.watch()
        if state['status'] not in ('idle', 'error', 'stopped', 'interrupted') or state.get('pending_approval') or state.get('queued'):
            request(self.store, op='stop', thread_id=self.thread['id'])
        # This repository was made by start(), inside the owned temporary folder.
        require(self.repo == self.base / 'acceptance project' and self.repo.is_dir() and not self.repo.is_symlink(), 'fixture checkout identity changed')
        lease = os.open(self.repo, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            # Stop replies before the final capture. The host locks this inode;
            # wait for its lease, not the early 'stopped' snapshot, before writes.
            until(lambda: fcntl.flock(lease, fcntl.LOCK_EX | fcntl.LOCK_NB), lambda _: True, 25, description='stopped fixture run to release its checkout lease')
            self.replay.clear()
            self.thread = self.seed_thread
            self.git('reset', '--hard', self.baseline)
            self.git('switch', '--discard-changes', 'main')
            self.git('reset', '--hard', self.baseline)
            if self.git('branch', '--list', 'acceptance-created'):
                self.git('branch', '-D', 'acceptance-created')
            self.write_review()
        finally:
            os.close(lease)

    def write_review(self, last=True):
        text = self.git('show', f'{self.baseline}:review.txt') + '\n'
        (self.repo / 'review.txt').write_text(text.replace('first old', 'first new').replace('last old', 'last new' if last else 'last old'))

    def stage_first_hunk(self):
        self.write_review(last=False)
        self.git('add', '--', 'review.txt')
        self.write_review()

    def new_thread(self):
        self.sequence += 1
        self.thread = request(self.store, op='create_thread', checkout=str(self.repo), provider=PROVIDER, model_id=MODEL, request_id=f'acceptance-create-{self.sequence}')['thread']

    def records(self):
        page = request(self.store, protocol_version=5, op='turn_records', thread_id=self.thread['id'])
        require(page.get('before') is None, 'fixture unexpectedly needs record pagination')
        return page['records']

    def finished_edit(self):
        self.new_thread()
        pending = self.pending(True)
        request(self.store, op='approve', thread_id=self.thread['id'], approval_id=pending['pending_approval']['id'], allow=True)
        self.idle()
        require((self.repo / 'approval.txt').read_text() == 'allowed\n', 'scripted edit did not finish')
        records = until(self.records, lambda rows: len(rows) == 1 and rows[0]['state'] == 'ready', 15, description='one ready record for the scripted edit')
        require([p['path'] for p in records[0]['paths']] == ['approval.txt'], 'run record does not cover exactly the scripted edit')
        return records[0]

    def undo(self, record_id):
        self.sequence += 1
        result = request(self.store, protocol_version=5, op='undo_turn', thread_id=self.thread['id'], record_id=record_id, request_id=f'acceptance-undo-{self.sequence}')
        require(result['state'] == 'undone', f'fixture undo refused: {result}')
        return result['record_id']

    def close(self):
        if self.host:
            # The host and its sidecar are in this check's own process group.
            import signal
            try:
                os.killpg(self.host.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                self.host.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(self.host.pid, signal.SIGKILL)
                self.host.wait(timeout=5)
        if self.replay:
            write_json(self.base / 'replay.json', {'requests': len(self.replay.requests), 'errors': self.replay.errors})
            self.replay.close()
