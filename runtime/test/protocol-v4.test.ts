import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';
import { createModels } from '@earendil-works/pi-ai/models';
import { fauxProvider } from '@earendil-works/pi-ai/providers/faux';
import { CodexHarness, codexEffort, codexLevels } from '../src/harness.ts';
import type { CodexDependencies } from '../src/harness.ts';
import { summaryView, validate, view } from '../src/protocol.ts';
import type { Snapshot } from '../src/protocol.ts';
import { Runtime } from '../src/runtime.ts';
import type { AppServer } from '../src/codex/server.ts';
import { setup } from './support/safety.ts';

const empty = (): Snapshot => ({ id: 'x', checkout: '/repo', model: { provider: 'faux', model_id: 'faux-1' }, status: 'idle',
  messages: [], live_text: '', tools: [], pending_approval: null, error: null, truncated: false });

test('version 4 operations and fields validate only with version 4', () => {
  for (const input of [
    { op: 'status', protocol_version: 4 },
    { op: 'harnesses', protocol_version: 4 },
    { op: 'set_harness_path', protocol_version: 4, harness: 'codex' },
    { op: 'set_harness_path', protocol_version: 4, harness: 'codex', path: '/opt/codex' },
    { op: 'set_harness_mode', protocol_version: 4, harness: 'codex', mode: 'ask' },
    { op: 'configure_thread', protocol_version: 4, thread_id: 't', mode: 'read_only' },
    { op: 'create_thread', protocol_version: 4, checkout: '/r', provider: 'codex', model_id: 'm', request_id: 'r', harness: 'codex' },
    { op: 'history', protocol_version: 4, thread_id: 't' },
  ]) assert.doesNotThrow(() => validate(input), JSON.stringify(input));
  for (const input of [
    { op: 'harnesses', protocol_version: 3 }, { op: 'harnesses' },
    { op: 'create_thread', protocol_version: 3, checkout: '/r', provider: 'p', model_id: 'm', request_id: 'r', harness: 'pi' },
    { op: 'create_thread', protocol_version: 4, checkout: '/r', provider: 'p', model_id: 'm', request_id: 'r', harness: 'claude' },
    { op: 'set_harness_path', protocol_version: 4, harness: 'codex', path: 'relative/codex' },
    { op: 'set_harness_path', protocol_version: 4, harness: 'codex', path: 7 },
    { op: 'set_harness_path', protocol_version: 4 },
    { op: 'set_harness_mode', protocol_version: 4, harness: 'codex' },
    { op: 'create_thread', protocol_version: 4, checkout: '/r', provider: 'codex', model_id: 'm', request_id: 'r', harness: 'codex', mode: 'Full Access' },
    { op: 'set_harness_mode', protocol_version: 3, harness: 'codex', mode: 'ask' },
  ]) assert.throws(() => validate(input), JSON.stringify(input));
});

test('only version 4 replies name the harness, and Pi threads saved earlier read as Pi', () => {
  const snapshot = empty();
  assert.equal('harness' in view(snapshot, 3), false);
  assert.deepEqual((view(snapshot, 4) as { harness: unknown }).harness, { id: 'pi', version: null, mode: null, reported: null });
  snapshot.harness = { id: 'codex', version: '0.169.0', mode: 'ask', reported: 'read_only' };
  assert.deepEqual((view(snapshot, 4) as { harness: unknown }).harness, { id: 'codex', version: '0.169.0', mode: 'ask', reported: 'read_only' });
  assert.equal('harness' in view(snapshot, 3), false);
  const summary = { id: 'x', checkout: '/r', model: snapshot.model, status: 'idle', title: '', created_at: 1, updated_at: 1, archived: false, pending_approval: false };
  assert.equal('harness' in summaryView({ ...summary, harness: 'codex' }, 3), false);
  assert.equal(summaryView(summary, 4).harness, 'pi');
  assert.equal(summaryView({ ...summary, harness: 'codex' }, 4).harness, 'codex');
});

test('Codex efforts map to Plait thinking levels both ways', () => {
  assert.deepEqual(codexLevels(['none', 'low', 'medium', 'high', 'xhigh', 'turbo']), ['off', 'low', 'medium', 'high', 'xhigh']);
  assert.equal(codexEffort('off', ['none', 'low']), 'none');
  assert.equal(codexEffort('high', ['low']), undefined);
  assert.equal(codexEffort(undefined, ['low']), undefined);
});

/** An app-server that answers sign-in and model questions and records its lifetime. */
function scripted(signedIn = true) {
  const servers: { closed: boolean }[] = [];
  const start = async () => {
    const record = { closed: false };
    servers.push(record);
    let exit!: () => void;
    const exited = new Promise<{ code: number; signal: null }>(resolve => { exit = () => resolve({ code: 0, signal: null }); });
    return {
      exited,
      request: async (method: string) => method === 'account/read'
        ? { account: signedIn ? { type: 'chatgpt' } : null, requiresOpenaiAuth: true }
        : { data: [{ id: 'gpt-test', model: 'gpt-test', displayName: 'GPT Test', hidden: false, isDefault: true, defaultReasoningEffort: 'medium',
          supportedReasoningEfforts: [{ reasoningEffort: 'low', description: '' }, { reasoningEffort: 'medium', description: '' }] }], nextCursor: null },
      close: async () => { record.closed = true; exit(); },
    } as unknown as AppServer;
  };
  return { servers, start };
}

async function runtime(t: { after(fn: () => Promise<void>): void }, deps: Partial<CodexDependencies>) {
  const { root, base } = await setup(t);
  const store = join(base, 'store'); await mkdir(store);
  const models = createModels(); models.setProvider(fauxProvider().provider);
  const host = new Runtime(store, models, new Set(['faux']));
  host.codex = new CodexHarness(host.store, { environment: async () => ({ PATH: '/bin' }), ...deps });
  t.after(async () => { await host.close(); });
  return { host, directory: root };
}

test('the harness list checks Codex once and closes the server it started to ask', async t => {
  const fake = scripted();
  let finds = 0;
  const { host } = await runtime(t, {
    start: fake.start,
    find: async () => { finds++; return { state: 'ready', path: '/opt/codex', version: '0.169.0', message: 'ready' }; },
  });
  const reply = await host.handle({ op: 'harnesses', protocol_version: 4 }) as { harnesses: { id: string; state: string; models: { model_id: string; thinking_levels: string[] }[] | null }[] };
  assert.deepEqual(reply.harnesses.map(entry => [entry.id, entry.state]), [['pi', 'ready'], ['codex', 'ready']]);
  assert.equal(reply.harnesses[0].models, null);
  assert.deepEqual(reply.harnesses[1].models!.map(model => [model.model_id, model.thinking_levels]), [['gpt-test', ['low', 'medium']]]);
  assert.deepEqual(fake.servers, [{ closed: true }], 'a check leaves no Codex process behind');
  await host.handle({ op: 'harnesses', protocol_version: 4 });
  assert.equal(finds, 1, 'a recent check is reused');
  await host.handle({ op: 'set_harness_path', protocol_version: 4, harness: 'codex', path: '/elsewhere/codex' });
  assert.equal(finds, 2, 'a new path checks again at once');
});

test('Codex threads start only with a Codex model, stay in Codex, and older clients see none of it', async t => {
  const fake = scripted();
  const { host, directory } = await runtime(t, {
    start: fake.start, find: async () => ({ state: 'ready', path: '/opt/codex', version: '0.169.0', message: 'ready' }),
  });
  const create = (extra: Record<string, unknown>) => host.reply({ op: 'create_thread', protocol_version: 4, checkout: directory, request_id: `r${Math.random().toString(36).slice(2)}`, ...extra });
  assert.equal((await create({ provider: 'faux', model_id: 'faux-1', harness: 'codex' })).error?.code, 'invalid_request');
  assert.equal((await create({ provider: 'codex', model_id: 'gpt-test' })).error?.code, 'invalid_request');
  assert.equal((await create({ provider: 'codex', model_id: 'missing', harness: 'codex' })).error?.code, 'model_unavailable');
  const made = await create({ provider: 'codex', model_id: 'gpt-test', harness: 'codex', thinking: 'high' }) as { ok: true; result: { thread: { id: string; harness: unknown; thinking_level: string } } };
  assert.equal(made.ok, true, JSON.stringify(made));
  assert.deepEqual(made.result.thread.harness, { id: 'codex', version: null, mode: 'full_access', reported: null });
  assert.equal(made.result.thread.thinking_level, 'medium', 'the nearest level the model offers');
  const id = made.result.thread.id;
  const configure = (extra: Record<string, unknown>) => host.reply({ op: 'configure_thread', protocol_version: 4, thread_id: id, ...extra });
  assert.equal((await configure({ provider: 'faux', model_id: 'faux-1' })).error?.code, 'invalid_request');
  assert.equal((await configure({ thinking: 'high' })).error?.code, 'invalid_request');
  assert.equal((await configure({ thinking: 'low' })).ok, true);
  const v3 = await host.handle({ op: 'list_threads', protocol_version: 3 }) as { threads: Record<string, unknown>[] };
  assert.equal('harness' in v3.threads[0], false);
  const v4 = await host.handle({ op: 'list_threads', protocol_version: 4 }) as { threads: Record<string, unknown>[] };
  assert.equal(v4.threads[0].harness, 'codex');
  const watched = await host.handle({ op: 'watch', thread_id: id, protocol_version: 3 }) as { thread: Record<string, unknown> };
  assert.equal('harness' in watched.thread, false);
});

test('a Codex that is missing, too old, or signed out is listed with how to fix it', async t => {
  for (const [install, signedIn, state] of [
    [{ state: 'not_installed', message: 'Codex was not found.' }, true, 'not_installed'],
    [{ state: 'unsupported_version', path: '/opt/codex', version: '0.100.0', message: 'Too old.' }, true, 'unsupported_version'],
    [{ state: 'ready', path: '/opt/codex', version: '0.169.0', message: 'ready' }, false, 'signed_out'],
  ] as const) {
    const fake = scripted(signedIn);
    const { host, directory } = await runtime(t, { start: fake.start, find: async () => install });
    const reply = await host.handle({ op: 'harnesses', protocol_version: 4 }) as { harnesses: { state: string; note: string | null }[] };
    assert.equal(reply.harnesses[1].state, state);
    assert.ok(reply.harnesses[1].note);
    const made = await host.reply({ op: 'create_thread', protocol_version: 4, checkout: directory, request_id: 'r', provider: 'codex', model_id: 'gpt-test', harness: 'codex' });
    assert.equal(made.error?.code, 'model_unavailable');
  }
});
