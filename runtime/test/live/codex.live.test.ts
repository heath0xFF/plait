// Real Codex turns through the runtime, using the installed `codex` and its own
// sign-in. Opt in with PLAIT_LIVE=codex; see support.ts. Assertions are about
// structure, never the model's wording.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';
import { createModels } from '@earendil-works/pi-ai/models';
import { fauxProvider } from '@earendil-works/pi-ai/providers/faux';
import { AppServer } from '../../src/codex/server.ts';
import { CodexHarness } from '../../src/harness.ts';
import type { Snapshot } from '../../src/protocol.ts';
import { Runtime } from '../../src/runtime.ts';
import { driver, live, liveRepo } from './support.ts';

type Context = { after(fn: () => Promise<void>): void };

async function codexHost(t: Context, repo?: { root: string; store: string }) {
  const { root, store } = repo ?? await liveRepo(t);
  const models = createModels(); models.setProvider(fauxProvider().provider);
  const runtime = new Runtime(store, models, new Set(['faux']));
  let started = 0;
  runtime.codex = new CodexHarness(runtime.store, { start: async (path, env) => { started += 1; return AppServer.start(path, { env }); } });
  let closed = false;
  const close = async () => { if (!closed) { closed = true; await runtime.close(); } };
  t.after(close);
  const d = driver(runtime);
  const entry = (await d.call({ op: 'harnesses' })).result.harnesses.find((harness: { id: string }) => harness.id === 'codex');
  assert.equal(entry.state, 'ready', `Codex is ${entry.state}: ${entry.note ?? ''}`);
  // The smallest model Codex offers keeps each turn cheap.
  const model = entry.models.find((candidate: { model_id: string }) => /mini/.test(candidate.model_id)) ?? entry.models[0];
  const create = async (mode?: string) => {
    const reply = await d.call({ op: 'create_thread', checkout: root, provider: 'codex', model_id: model.model_id, request_id: `c${Date.now()}`, harness: 'codex', thinking: 'low', ...(mode ? { mode } : {}) });
    assert.equal(reply.ok, true, reply.error?.message ?? 'refused');
    return reply.result.thread as Snapshot;
  };
  const submit = async (id: string, request: string, text: string, extra: Record<string, unknown> = {}) => {
    const reply = await d.call({ op: 'submit', thread_id: id, request_id: request, text, ...extra });
    assert.equal(reply.ok, true, reply.error?.message ?? 'refused');
  };
  return { ...d, runtime, root, store, create, submit, close, started: () => started };
}

test('Full access runs a command without asking, and a second run continues the same Codex thread', live('codex'), async t => {
  const h = await codexHost(t);
  const thread = await h.create();
  assert.equal(thread.harness?.mode, 'full_access');
  await h.submit(thread.id, 'r1', 'Run `ls` in this directory with your shell tool, then answer with the number of entries. Remember the word HELIOTROPE.');
  const { done, asked } = await h.approveAll(thread.id);
  assert.equal(done.status, 'idle', done.error?.code ?? 'no error');
  assert.equal(asked, 0, 'Full access never asks');
  assert.equal(done.harness?.reported, 'full_access');
  assert.ok(done.tools.some(tool => tool.status === 'done'), 'a command ran');
  assert.ok(done.messages.some(message => message.role === 'assistant' && message.text.trim()), 'an answer');
  assert.deepEqual(done.messages.filter(message => message.role === 'user').map(message => message.id), ['request-r1']);
  assert.ok(done.usage, 'usage reported');
  await h.submit(thread.id, 'r2', 'What word did I ask you to remember? Answer with the word only.');
  const second = await h.idle(thread.id);
  assert.equal(second.status, 'idle', second.error?.code ?? 'no error');
  const answer = second.messages.filter(message => message.role === 'assistant').at(-1)!.text;
  assert.match(answer, /heliotrope/i, 'the second run continued the first');
});

test('Read only asks before writing; an approved edit lands and is listed as changed', live('codex'), async t => {
  const h = await codexHost(t);
  const thread = await h.create('read_only');
  await h.submit(thread.id, 'r1', 'Create a file named note.txt containing exactly the word hello. Do not do anything else.');
  const { done, asked } = await h.approveAll(thread.id);
  assert.equal(done.status, 'idle', done.error?.code ?? 'no error');
  assert.equal(done.harness?.reported, 'read_only');
  assert.ok(asked >= 1, 'Read only asked before the write');
  assert.match(await readFile(join(h.root, 'note.txt'), 'utf8'), /hello/);
  // An edit Codex makes with its edit tool is listed; one made by a shell command is not (docs/HOST.md).
  const changed = (await h.call({ op: 'changed_files', thread_id: thread.id })).result.files as string[];
  if (done.tools.some(tool => tool.name === 'edit')) assert.ok(changed.includes('note.txt'), 'the edit is a changed file');
});

test('a steering message joins the running turn, and Stop ends a long one', live('codex'), async t => {
  const h = await codexHost(t);
  const thread = await h.create();
  await h.submit(thread.id, 'r1', 'Run `sleep 8` with your shell tool, then reply with the word DONE.');
  await h.until(thread.id, snapshot => snapshot.tools.length > 0);
  await h.submit(thread.id, 'r2', 'Also include the word MARIGOLD in your reply.', { when_busy: 'steer' });
  const steered = await h.idle(thread.id);
  assert.equal(steered.status, 'idle', steered.error?.code ?? 'no error');
  assert.ok(steered.messages.some(message => message.id === 'request-r2'), 'the steering message is in the thread');
  assert.match(steered.messages.filter(message => message.role === 'assistant').at(-1)!.text, /marigold/i);

  await h.submit(thread.id, 'r3', 'Run `sleep 60` with your shell tool.');
  await h.until(thread.id, snapshot => snapshot.tools.some(tool => tool.status === 'running'));
  const stopping = Date.now();
  assert.equal((await h.call({ op: 'stop', thread_id: thread.id })).ok, true);
  const stopped = await h.idle(thread.id, 20_000);
  assert.equal(stopped.status, 'stopped');
  assert.ok(Date.now() - stopping < 15_000, 'Stop ended the turn promptly');
});

test('history reads after the host restarts without starting Codex', live('codex'), async t => {
  const first = await codexHost(t);
  const thread = await first.create();
  await first.submit(thread.id, 'r1', 'Reply with the single word PEBBLE.');
  assert.equal((await first.idle(thread.id)).status, 'idle');
  const history = await first.call({ op: 'history', thread_id: thread.id });
  await first.close();
  const again = await codexHost(t, { root: first.root, store: first.store });
  const before = again.started();
  const reread = await again.call({ op: 'history', thread_id: thread.id });
  assert.deepEqual(reread.result.items, history.result.items);
  assert.equal(again.started(), before, 'reading history starts no Codex');
});
