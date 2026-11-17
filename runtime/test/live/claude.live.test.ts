// Real Claude Code turns through the runtime, using the installed `claude`
// and whatever sign-in it has, with the dev copy of the Agent SDK (nothing is
// downloaded). Opt in with PLAIT_LIVE=claude; see support.ts. Each test uses
// the cheapest model Claude Code offers and a tiny prompt. Assertions are
// about structure, never the model's wording, and failures print only states.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createModels } from '@earendil-works/pi-ai/models';
import { fauxProvider } from '@earendil-works/pi-ai/providers/faux';
import { ClaudeHarness } from '../../src/claude/harness.ts';
import { claudeCatalog } from '../../src/claude/install.ts';
import type { Snapshot } from '../../src/protocol.ts';
import { Runtime } from '../../src/runtime.ts';
import { DEV_SDK } from '../support/fake-claude-env.ts';
import { driver, live, liveRepo } from './support.ts';

type Context = { after(fn: () => Promise<void>): void };
type WireModel = { model_id: string; name: string; thinking_levels: string[] };
const TURN_MS = 90_000;

/** Counts every Claude Code start: model checks and runs. */
class Counting extends ClaudeHarness {
  launches = 0;
  override async launch() { this.launches += 1; return super.launch(); }
}

async function claudeHost(t: Context, repo?: { root: string; store: string }) {
  const { root, store } = repo ?? await liveRepo(t);
  const models = createModels(); models.setProvider(fauxProvider().provider);
  const runtime = new Runtime(store, models, new Set(['faux']));
  let checks = 0;
  const harness = new Counting(runtime.store, { sdk: DEV_SDK, cwd: store, catalog: options => { checks += 1; return claudeCatalog(options); } });
  runtime.claude = harness;
  let closed = false;
  const close = async () => { if (!closed) { closed = true; await runtime.close(); } };
  t.after(close);
  const d = driver(runtime);
  const entry = async () => (await d.call({ op: 'harnesses' })).result.harnesses.find((candidate: { id: string }) => candidate.id === 'claude');
  const model = async (): Promise<WireModel> => {
    const listed = await entry();
    assert.equal(listed.state, 'ready', `Claude Code is ${listed.state}`);
    // The smallest model keeps each turn cheap.
    return listed.models.find((candidate: WireModel) => /haiku/i.test(`${candidate.model_id} ${candidate.name}`)) ?? listed.models[0];
  };
  const create = async (mode?: string) => {
    const chosen = await model();
    const reply = await d.call({
      op: 'create_thread', checkout: root, provider: 'claude', model_id: chosen.model_id, request_id: `c${Date.now()}`, harness: 'claude',
      ...(chosen.thinking_levels.length ? { thinking: chosen.thinking_levels[0] } : {}), ...(mode ? { mode } : {}),
    });
    assert.equal(reply.ok, true, reply.error?.code ?? 'refused');
    return reply.result.thread as Snapshot;
  };
  const submit = async (id: string, request: string, text: string, extra: Record<string, unknown> = {}) => {
    const reply = await d.call({ op: 'submit', thread_id: id, request_id: request, text, ...extra });
    assert.equal(reply.ok, true, reply.error?.code ?? 'refused');
  };
  return { ...d, runtime, root, store, create, submit, close, starts: () => harness.launches + checks };
}
const users = (snapshot: Snapshot) => snapshot.messages.filter(message => message.role === 'user').map(message => message.id);
const lastAnswer = (snapshot: Snapshot) => snapshot.messages.filter(message => message.role === 'assistant' && message.text.trim()).at(-1)?.text ?? '';

test('Ask shows the command Claude Code wants to run; a second run resumes the same session', { ...live('claude'), timeout: 2 * TURN_MS }, async t => {
  const h = await claudeHost(t);
  const thread = await h.create('ask');
  assert.equal(thread.harness?.mode, 'ask');
  // A write, since Claude Code runs read-only commands such as `ls` without asking.
  await h.submit(thread.id, 'r1', 'Use your Bash tool to run exactly this command: `echo plait > probe.txt`. Then reply with one short sentence. Remember the word HELIOTROPE for later.');
  const first = await h.until(thread.id, snapshot => !!snapshot.pending_approval || (!h.runtime.sessions.has(thread.id) && snapshot.status !== 'running'), TURN_MS);
  assert.ok(first.pending_approval, 'Ask asked before the command (unless your Claude Code settings already allow it)');
  assert.equal(first.pending_approval.kind, 'command');
  assert.match(first.pending_approval.summary, /probe\.txt/);
  assert.equal(first.harness?.reported, 'ask');
  const reply = await h.call({ op: 'approve', thread_id: thread.id, approval_id: first.pending_approval.id, allow: true });
  assert.equal(reply.ok, true);
  const { done } = await h.approveAll(thread.id);
  assert.equal(done.status, 'idle', done.error?.code ?? 'no error');
  assert.ok(done.tools.some(tool => tool.name === 'command' && tool.status === 'done'), 'the command ran');
  assert.ok(lastAnswer(done), 'an answer');
  assert.deepEqual(users(done), ['request-r1'], 'the message is placed once');
  assert.ok(done.usage && done.usage.total > 0, 'usage reported');
  assert.ok(done.usage.context_window, 'context window reported');
  assert.match(done.harness?.version ?? '', /^\d+\.\d+\.\d+/);

  await h.submit(thread.id, 'r2', 'What word did I ask you to remember? Answer with the word only.');
  const second = await h.idle(thread.id, TURN_MS);
  assert.equal(second.status, 'idle', second.error?.code ?? 'no error');
  assert.deepEqual(users(second), ['request-r1', 'request-r2']);
  assert.match(lastAnswer(second), /heliotrope/i, 'the second run continued the first session');
});

test('a steering message is placed and answered in the same run', { ...live('claude'), timeout: TURN_MS + 10_000 }, async t => {
  const h = await claudeHost(t);
  const thread = await h.create('bypass');
  await h.submit(thread.id, 'r1', 'Use your Bash tool to run `sleep 8`, then reply with the word DONE.');
  await h.until(thread.id, snapshot => snapshot.tools.length > 0, TURN_MS);
  await h.submit(thread.id, 'r2', 'Also include the word MARIGOLD in your reply.', { when_busy: 'steer' });
  const done = await h.idle(thread.id, TURN_MS);
  assert.equal(done.status, 'idle', done.error?.code ?? 'no error');
  assert.deepEqual(users(done), ['request-r1', 'request-r2'], 'both messages placed once, in order');
  assert.deepEqual(done.queued, []);
  assert.match(lastAnswer(done), /marigold/i, 'the steering message was answered');
});

test('a mode change during a run reaches Claude Code, and reported follows', { ...live('claude'), timeout: TURN_MS + 10_000 }, async t => {
  const h = await claudeHost(t);
  const thread = await h.create('bypass');
  await h.submit(thread.id, 'r1', 'Use your Bash tool to run `sleep 6`, then reply with the word DONE.');
  await h.until(thread.id, snapshot => snapshot.harness?.reported === 'bypass' && snapshot.tools.length > 0, TURN_MS);
  const changed = await h.call({ op: 'configure_thread', thread_id: thread.id, mode: 'accept_edits' });
  assert.equal(changed.ok, true, changed.error?.code ?? 'refused');
  assert.equal(changed.result.thread.harness.reported, null, 'cleared until Claude Code confirms');
  const confirmed = await h.until(thread.id, snapshot => snapshot.harness?.reported === 'accept_edits' || !h.runtime.sessions.has(thread.id), TURN_MS);
  assert.equal(confirmed.harness?.reported, 'accept_edits', 'Claude Code reported the new mode during the run');
  const { done } = await h.approveAll(thread.id);
  assert.equal(done.status, 'idle', done.error?.code ?? 'no error');
});

test('Stop during a turn ends it as stopped, promptly', { ...live('claude'), timeout: TURN_MS + 10_000 }, async t => {
  const h = await claudeHost(t);
  const thread = await h.create('bypass');
  await h.submit(thread.id, 'r1', 'Use your Bash tool to run exactly `sleep 60` in the foreground, not in the background, and wait for it.');
  // A model can still answer without running it; that says nothing about Stop, so it fails as such.
  const started = await h.until(thread.id, snapshot => snapshot.tools.some(tool => tool.status === 'running') || snapshot.status !== 'running', TURN_MS);
  assert.ok(started.tools.some(tool => tool.status === 'running'), 'Claude did not start the command, so Stop was not exercised; run again');
  const stopping = Date.now();
  assert.equal((await h.call({ op: 'stop', thread_id: thread.id })).ok, true);
  const stopped = await h.idle(thread.id, 20_000);
  assert.equal(stopped.status, 'stopped');
  assert.ok(Date.now() - stopping < 15_000, 'Stop ended the turn promptly');
  assert.ok(stopped.tools.every(tool => tool.status !== 'running'), 'no tool is left running');
});

test('history reads after the host restarts without starting Claude Code', { ...live('claude'), timeout: TURN_MS + 10_000 }, async t => {
  const first = await claudeHost(t);
  const thread = await first.create('bypass');
  await first.submit(thread.id, 'r1', 'Reply with the single word PEBBLE.');
  const done = await first.idle(thread.id, TURN_MS);
  assert.equal(done.status, 'idle', done.error?.code ?? 'no error');
  const history = await first.call({ op: 'history', thread_id: thread.id });
  assert.equal(history.ok, true);
  await first.close();
  const again = await claudeHost(t, { root: first.root, store: first.store });
  const reread = await again.call({ op: 'history', thread_id: thread.id });
  assert.deepEqual(reread.result.items, history.result.items);
  assert.equal(again.starts(), 0, 'reading history starts no Claude Code');
});
