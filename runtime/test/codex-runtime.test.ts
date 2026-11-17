// Codex threads through the real runtime, against the scripted fake app-server.
// Nothing here starts a real Codex or reaches a model.
import assert from 'node:assert/strict';
import { mkdir, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';
import { createModels } from '@earendil-works/pi-ai/models';
import { fauxProvider } from '@earendil-works/pi-ai/providers/faux';
import { CodexHarness } from '../src/harness.ts';
import { AppServer } from '../src/codex/server.ts';
import type { Snapshot } from '../src/protocol.ts';
import { Runtime } from '../src/runtime.ts';
import { FAKE_CODEX, fakeEnv } from './support/fake-codex-env.ts';
import { setup } from './support/safety.ts';

type Context = { after(fn: () => Promise<void>): void };
type Reply = { ok: boolean; result?: any; error?: { code: string; message: string } };

async function host(t: Context, scenario = 'turn', options: { store?: string; repo?: string } = {}) {
  const { root, base } = options.repo ? { root: options.repo, base: '' } : await setup(t);
  const store = options.store ?? join(base, 'store');
  await mkdir(store, { recursive: true });
  const log = join(store, `frames-${scenario}.jsonl`);
  const servers: AppServer[] = [];
  const models = createModels(); models.setProvider(fauxProvider().provider);
  const runtime = new Runtime(store, models, new Set(['faux']));
  runtime.codex = new CodexHarness(runtime.store, {
    environment: async () => fakeEnv({ FAKE_CODEX_SCENARIO: scenario, FAKE_CODEX_LOG: log }),
    find: async () => ({ state: 'ready', path: FAKE_CODEX, version: '0.169.0', message: 'ready' }),
    start: async (path, env) => { const server = await AppServer.start(path, { env, requestTimeoutMs: 10_000 }); servers.push(server); return server; },
    idleMs: 50,
  });
  let closed = false;
  const close = async () => { if (!closed) { closed = true; await runtime.close(); } };
  t.after(close);
  const call = (request: Record<string, unknown>) => runtime.reply({ protocol_version: 4, ...request }) as Promise<Reply>;
  const create = async () => {
    const reply = await call({ op: 'create_thread', checkout: root, provider: 'codex', model_id: 'gpt-fake', request_id: `c${Date.now()}`, harness: 'codex', thinking: 'high' });
    assert.equal(reply.ok, true, JSON.stringify(reply));
    return reply.result.thread as Snapshot;
  };
  const until = async (id: string, done: (snapshot: Snapshot) => boolean, ms = 8000) => {
    const deadline = Date.now() + ms;
    for (;;) {
      const snapshot = runtime.snapshot(id);
      if (done(snapshot)) return structuredClone(snapshot);
      if (Date.now() > deadline) assert.fail(`Timed out; last snapshot: ${JSON.stringify(snapshot)}`);
      await new Promise(resolve => setTimeout(resolve, 10));
    }
  };
  const idle = (id: string) => until(id, snapshot => !runtime.sessions.has(id) && !['running', 'awaiting_approval'].includes(snapshot.status));
  const frames = async () => (await readFile(log, 'utf8')).trim().split('\n').map(line => JSON.parse(line));
  return { runtime, root, store, call, create, until, idle, frames, servers, close };
}

test('an asked Codex turn shows its approval, finishes, stores its transcript, and leaves no process', async t => {
  const h = await host(t);
  const thread = await h.create();
  assert.equal((await h.call({ op: 'submit', thread_id: thread.id, request_id: 'r1', text: 'List the files', approval: 'ask' })).ok, true);
  const asked = await h.until(thread.id, snapshot => !!snapshot.pending_approval);
  assert.equal(asked.status, 'awaiting_approval');
  assert.equal(asked.pending_approval!.summary, 'ls -la');
  assert.equal(asked.pending_approval!.tool_id, 'cmd-1');
  assert.equal((await h.call({ op: 'approve', thread_id: thread.id, approval_id: asked.pending_approval!.id, allow: true })).ok, true);
  const done = await h.idle(thread.id);
  assert.equal(done.status, 'idle', JSON.stringify(done.error));
  assert.deepEqual(done.harness, { id: 'codex', version: '0.169.0' });
  assert.deepEqual(done.messages.map(message => [message.id, message.role, message.text]), [
    ['request-r1', 'user', 'List the files'], [done.messages[1].id, 'assistant', ''], [done.messages[2].id, 'assistant', 'Two files.'],
  ]);
  assert.equal(done.messages[1].reasoning, 'Looking at the files.');
  assert.deepEqual(done.tools.map(tool => [tool.id, tool.status, tool.output]), [['cmd-1', 'done', 'a.txt\nb.txt\n']]);
  assert.equal(done.usage?.context_window, 272000);
  // What Codex was told: the sandbox and approvals Plait chose, the effort, and the request ID to echo.
  const sent = await h.frames();
  const start = sent.find(frame => frame.method === 'thread/start').params;
  assert.deepEqual([start.sandbox, start.approvalPolicy, start.model], ['workspace-write', 'untrusted', 'gpt-fake']);
  const turn = sent.find(frame => frame.method === 'turn/start').params;
  assert.deepEqual(turn.sandboxPolicy, { type: 'workspaceWrite', writableRoots: [], networkAccess: false, excludeTmpdirEnvVar: false, excludeSlashTmp: false });
  assert.deepEqual([turn.effort, turn.clientUserMessageId, turn.input[0].text], ['high', 'r1', 'List the files']);
  // History reads the stored transcript, live and after the host restarts.
  const history = await h.call({ op: 'history', thread_id: thread.id });
  assert.deepEqual(history.result.items.map((item: { id: string }) => item.id), ['request-r1', done.messages[1].id, 'cmd-1', done.messages[2].id]);
  // One server answered the model check when the thread was made, one ran the turn; both are gone.
  assert.equal(h.servers.length, 2);
  await Promise.all(h.servers.map(server => server.exited));
  await h.close();
  const again = await host(t, 'turn', { store: h.store, repo: h.root });
  const reread = await again.call({ op: 'history', thread_id: thread.id });
  assert.deepEqual(reread.result.items, history.result.items);
  assert.equal(again.servers.length, 0, 'reading history starts no Codex');
});

test('a second run continues the same Codex thread; auto mode never asks', async t => {
  const h = await host(t);
  const thread = await h.create();
  await h.call({ op: 'submit', thread_id: thread.id, request_id: 'r1', text: 'First', approval: 'auto' });
  const first = await h.idle(thread.id);
  assert.equal(first.status, 'idle', JSON.stringify(first.error));
  assert.equal(first.tools[0].status, 'done');
  await h.call({ op: 'submit', thread_id: thread.id, request_id: 'r2', text: 'Second', approval: 'auto' });
  const second = await h.idle(thread.id);
  assert.equal(second.status, 'idle', JSON.stringify(second.error));
  assert.deepEqual(second.messages.filter(message => message.role === 'user').map(message => message.id), ['request-r1', 'request-r2']);
  const sent = await h.frames();
  assert.equal(sent.filter(frame => frame.method === 'thread/start').length, 1);
  assert.equal(sent.find(frame => frame.method === 'thread/resume').params.threadId, 'thread-fake-1');
  assert.ok(sent.filter(frame => frame.method === 'turn/start').every(frame => frame.params.approvalPolicy === 'never'));
});

test('a declined Codex edit is shown as declined and writes nothing to changed files', async t => {
  const h = await host(t, 'edit');
  const thread = await h.create();
  await h.call({ op: 'submit', thread_id: thread.id, request_id: 'r1', text: 'Edit', approval: 'ask' });
  const asked = await h.until(thread.id, snapshot => !!snapshot.pending_approval);
  assert.equal(asked.pending_approval!.kind, 'edit');
  await h.call({ op: 'approve', thread_id: thread.id, approval_id: asked.pending_approval!.id, allow: false });
  const done = await h.idle(thread.id);
  assert.equal(done.tools[0].status, 'error');
  assert.deepEqual((await h.call({ op: 'changed_files', thread_id: thread.id })).result.files, []);
});

test('Stop interrupts the Codex turn; a steering message joins the running turn', async t => {
  const h = await host(t, 'slow');
  const thread = await h.create();
  await h.call({ op: 'submit', thread_id: thread.id, request_id: 'r1', text: 'Slow', approval: 'auto' });
  await h.until(thread.id, snapshot => snapshot.tools.some(tool => tool.status === 'done'));
  assert.equal((await h.call({ op: 'submit', thread_id: thread.id, request_id: 'r2', text: 'Also this', approval: 'auto', when_busy: 'steer' })).ok, true);
  const steered = await h.until(thread.id, snapshot => snapshot.messages.some(message => message.id === 'request-r2'));
  assert.deepEqual(steered.queued, []);
  await h.call({ op: 'stop', thread_id: thread.id });
  const stopped = await h.idle(thread.id);
  assert.equal(stopped.status, 'stopped');
  const sent = await h.frames();
  assert.equal(sent.find(frame => frame.method === 'turn/steer').params.clientUserMessageId, 'r2');
  assert.ok(sent.some(frame => frame.method === 'turn/interrupt'));
});

test('Codex exiting mid-turn ends the run with a fixed error', async t => {
  const h = await host(t, 'exit-on-turn');
  const thread = await h.create();
  await h.call({ op: 'submit', thread_id: thread.id, request_id: 'r1', text: 'Go', approval: 'auto' });
  const failed = await h.idle(thread.id);
  assert.equal(failed.status, 'error');
  assert.equal(failed.error?.code, 'runtime_error');
  assert.ok(!JSON.stringify(failed.error).includes('fake'), 'no Codex output reaches the client');
});

test('a Codex that does not echo the request ID still shows the message once', async t => {
  const h = await host(t, 'no-client-id');
  const thread = await h.create();
  await h.call({ op: 'submit', thread_id: thread.id, request_id: 'r1', text: 'Once', approval: 'auto' });
  const done = await h.idle(thread.id);
  assert.deepEqual(done.messages.filter(message => message.role === 'user').map(message => [message.id, message.text]), [['request-r1', 'Once']]);
});
