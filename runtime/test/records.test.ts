// The host's records of a checkout, through the real runtime and the real
// `plait-host record` helper: one before and one after every run, in every
// harness. Codex and Claude Code are the scripted fakes; Pi runs in the real
// host against the faux provider. Nothing here reaches a model.
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { test } from 'node:test';
import { promisify } from 'node:util';
import { createModels } from '@earendil-works/pi-ai/models';
import { fauxProvider } from '@earendil-works/pi-ai/providers/faux';
import { ClaudeHarness } from '../src/claude/harness.ts';
import { AppServer } from '../src/codex/server.ts';
import { CodexHarness } from '../src/harness.ts';
import type { Snapshot } from '../src/protocol.ts';
import { RECORD_FAILED, RETENTION_MS } from '../src/records.ts';
import { Runtime } from '../src/runtime.ts';
import { acquireCheckout } from '../src/safety.ts';
import { DEV_SDK, FAKE_CLAUDE, fakeEnv as claudeEnv } from './support/fake-claude-env.ts';
import { FAKE_CODEX, fakeEnv as codexEnv } from './support/fake-codex-env.ts';
import { childPids, request, setup as realHost, until as eventually } from './support/host.ts';
import { setup } from './support/safety.ts';

const exec = promisify(execFile);
type Context = { after(fn: () => Promise<void>): void };
type Reply = { ok: boolean; result?: any; error?: { code: string; message: string } };
type Row = Record<string, unknown> & { id: string; kind: string; state: string; run_request: string; before: string | null; paths: string[] };
/** A run's two records, as the store links them. */
type Pair = { run: string; before: Row; after?: Row };

function rows(db: DatabaseSync): Row[] {
  const paths = db.prepare('SELECT record,path FROM record_paths ORDER BY path').all();
  return db.prepare('SELECT * FROM records ORDER BY rowid').all().map(row => ({
    ...row, paths: paths.filter(path => path.record === row.id).map(path => String(path.path)),
  }) as Row);
}
function pairs(db: DatabaseSync): Pair[] {
  const all = rows(db);
  return all.filter(row => row.kind === 'before').map(before => ({
    run: before.run_request, before, after: all.find(row => row.kind === 'after' && row.before === before.id),
  }));
}
/** The host store's records, read the way any other process can while the host runs. */
function stored(store: string) {
  const db = new DatabaseSync(join(store, 'host.sqlite'), { readOnly: true });
  try { return pairs(db); } finally { db.close(); }
}
const models = () => { const models = createModels(); models.setProvider(fauxProvider().provider); return models; };

/** A runtime whose Codex is the fake, writing a file in the checkout as each turn starts. */
async function codex(t: Context, scenario = 'turn') {
  const { root, base } = await setup(t);
  const store = join(base, 'store');
  await mkdir(store, { recursive: true });
  const log = join(store, 'frames.jsonl');
  const runtime = new Runtime(store, models(), new Set(['faux']));
  runtime.codex = new CodexHarness(runtime.store, {
    environment: async () => codexEnv({ FAKE_CODEX_SCENARIO: scenario, FAKE_CODEX_LOG: log, FAKE_CODEX_WRITE: root }),
    find: async () => ({ state: 'ready', path: FAKE_CODEX, version: '0.169.0', message: 'ready' }),
    start: (path, env) => AppServer.start(path, { env, requestTimeoutMs: 10_000 }),
    idleMs: 50,
  });
  // Ask mode, so a run waits on its first command until the test allows it.
  const h = harnessed(t, runtime, root, store, { provider: 'codex', model_id: 'gpt-fake', harness: 'codex', mode: 'ask' });
  /** Turns Codex was asked to start. */
  const turns = async () => (await readFile(log, 'utf8').catch(() => '')).trim().split('\n').filter(Boolean)
    .filter(line => JSON.parse(line).method === 'turn/start').length;
  return { ...h, turns };
}

/** A runtime whose Claude Code is the fake, writing a file in the checkout as each turn starts. */
async function claude(t: Context, scenario = 'turn', mode = 'auto') {
  const { root, base } = await setup(t);
  const store = join(base, 'store');
  const sessions = join(store, 'fake-sessions');
  await mkdir(sessions, { recursive: true });
  const runtime = new Runtime(store, models(), new Set(['faux']));
  runtime.codex = new CodexHarness(runtime.store, {
    environment: async () => ({ PATH: '/nonexistent' }), find: async () => ({ state: 'not_installed', message: 'Codex was not found.' }),
  });
  runtime.claude = new ClaudeHarness(runtime.store, {
    sdk: DEV_SDK, cwd: store,
    environment: async () => claudeEnv({ FAKE_CLAUDE_SCENARIO: scenario, FAKE_CLAUDE_STATE: sessions, FAKE_CLAUDE_WRITE: '1' }),
    find: async () => ({ state: 'ready', path: FAKE_CLAUDE, version: '2.1.326', message: 'ready' }),
    fetch: (async () => { throw new Error('No test downloads anything.'); }) as unknown as typeof fetch,
  });
  return harnessed(t, runtime, root, store, { provider: 'claude', model_id: 'default', harness: 'claude', mode });
}

function harnessed(t: Context, runtime: Runtime, root: string, store: string, thread: Record<string, string>) {
  let closed = false;
  const close = async () => { if (!closed) { closed = true; await runtime.close(); } };
  t.after(close);
  const call = (request: Record<string, unknown>, protocol = 4) => runtime.reply({ protocol_version: protocol, ...request }) as Promise<Reply>;
  const create = async () => {
    const reply = await call({ op: 'create_thread', checkout: root, request_id: `c${Date.now()}${Math.random()}`.replace('.', ''), thinking: 'high', ...thread });
    assert.equal(reply.ok, true, JSON.stringify(reply));
    return reply.result.thread as Snapshot;
  };
  const until = async (id: string, done: (snapshot: Snapshot) => boolean, ms = 10000) => {
    const deadline = Date.now() + ms;
    for (;;) {
      const snapshot = runtime.snapshot(id);
      if (done(snapshot)) return structuredClone(snapshot);
      if (Date.now() > deadline) assert.fail(`Timed out; last snapshot: ${JSON.stringify(snapshot)}`);
      await new Promise(resolve => setTimeout(resolve, 10));
    }
  };
  const idle = (id: string) => until(id, snapshot => !runtime.sessions.has(id) && !['running', 'awaiting_approval'].includes(snapshot.status));
  const asked = (id: string) => until(id, snapshot => !!snapshot.pending_approval && (snapshot.queued ?? []).length === 0);
  const approve = async (id: string) => {
    const snapshot = await until(id, snapshot => !!snapshot.pending_approval);
    assert.equal((await call({ op: 'approve', thread_id: id, approval_id: snapshot.pending_approval!.id, allow: true })).ok, true);
  };
  const submit = async (id: string, request: string, extra: Record<string, unknown> = {}) => {
    const reply = await call({ op: 'submit', thread_id: id, request_id: request, text: `Message ${request}`, ...extra });
    assert.equal(reply.ok, true, JSON.stringify(reply));
  };
  /** Allows whatever the run asks until it ends. */
  const finish = async (id: string) => {
    for (;;) {
      const snapshot = await until(id, snapshot => !!snapshot.pending_approval || (!runtime.sessions.has(id) && !['running', 'awaiting_approval'].includes(snapshot.status)));
      if (!snapshot.pending_approval) return snapshot;
      await call({ op: 'approve', thread_id: id, approval_id: snapshot.pending_approval.id, allow: true });
    }
  };
  return { runtime, root, store, call, create, until, idle, asked, approve, submit, finish, close, pairs: () => pairs(runtime.store.db) };
}

/** Both records made, linked, and the later one listing exactly `paths`. */
function recorded(pair: Pair | undefined, run: string, paths: string[]) {
  assert.ok(pair, `no records for run ${run}`);
  assert.equal(pair.run, run);
  assert.equal(pair.before.state, 'ready', JSON.stringify(pair.before));
  assert.match(String(pair.before.commit_id), /^[0-9a-f]{40}([0-9a-f]{24})?$/);
  assert.ok(pair.after, `run ${run} has no later record`);
  assert.equal(pair.after.state, 'ready', JSON.stringify(pair.after));
  assert.equal(pair.after.before, pair.before.id);
  assert.notEqual(pair.after.commit_id, pair.before.commit_id);
  assert.deepEqual(pair.after.paths, paths);
  assert.deepEqual(pair.before.paths, [], 'only a run\'s later record lists paths');
}

test('a Codex run is recorded before and after, with the paths it changed', async t => {
  const h = await codex(t);
  // Already dirty before the run: in both records, so not among the run's paths.
  await writeFile(join(h.root, 'dirty.txt'), 'mine\n');
  const thread = await h.create();
  await h.submit(thread.id, 'r1');
  await h.approve(thread.id);
  assert.equal((await h.idle(thread.id)).status, 'idle');
  const [pair, ...rest] = h.pairs();
  recorded(pair, 'r1', ['codex-r1.txt']);
  assert.deepEqual(rest, []);
  assert.equal(pair.before.thread, thread.id);
  assert.equal(pair.before.checkout, h.root);
  assert.equal(pair.after!.after_restart, 0);
  // No commits yet in this checkout: a record names no HEAD.
  assert.equal(pair.before.head, null);
});

test('a stopped Codex run is still recorded', async t => {
  const h = await codex(t);
  const thread = await h.create();
  await h.submit(thread.id, 'r1');
  await h.asked(thread.id);
  assert.equal((await h.call({ op: 'stop', thread_id: thread.id })).ok, true);
  assert.equal((await h.idle(thread.id)).status, 'stopped');
  recorded(h.pairs()[0], 'r1', ['codex-r1.txt']);
});

test('a failed Codex run is still recorded', async t => {
  const h = await codex(t, 'exit-on-turn');
  const thread = await h.create();
  await h.submit(thread.id, 'r1', { approval: 'auto' });
  assert.equal((await h.idle(thread.id)).status, 'error');
  recorded(h.pairs()[0], 'r1', ['codex-r1.txt']);
});

test('a queued follow-up gets its own pair of records, and a steering message does not', async t => {
  const h = await codex(t);
  const thread = await h.create();
  await h.submit(thread.id, 'r1');
  await h.asked(thread.id);
  await h.submit(thread.id, 's1', { when_busy: 'steer' });
  await h.until(thread.id, snapshot => snapshot.messages.some(message => message.text === 'Message s1'));
  await h.submit(thread.id, 'q1', { when_busy: 'follow_up' });
  await h.approve(thread.id);
  // The follow-up starts as its own run once the first ends, and asks in turn.
  await h.asked(thread.id);
  await h.approve(thread.id);
  assert.equal((await h.idle(thread.id)).status, 'idle');
  const [first, second, ...rest] = h.pairs();
  recorded(first, 'r1', ['codex-r1.txt']);
  recorded(second, 'q1', ['codex-q1.txt']);
  assert.deepEqual(rest, []);
  // Each run starts where the one before it ended.
  assert.equal(second.before.tree, first.after!.tree);
});

test('a Claude Code run is recorded before and after, with the paths it changed', async t => {
  const h = await claude(t);
  const thread = await h.create();
  await h.submit(thread.id, 'r1');
  assert.equal((await h.idle(thread.id)).status, 'idle');
  recorded(h.pairs()[0], 'r1', ['claude-1.txt']);
});

test('a Pi run in the real host is recorded before and after its edit', { timeout: 30000 }, async t => {
  const f = await realHost(t, 'edit');
  const thread = await f.create();
  assert.equal((await f.submit(thread.id)).ok, true);
  const edit = await eventually(() => f.watch(thread.id), value => !!value.pending_approval);
  await f.approve(thread.id, edit.pending_approval!.id);
  await eventually(() => f.watch(thread.id), value => value.status === 'idle');
  // The run is idle only once both records are in the store.
  const [pair, ...rest] = stored(f.store);
  recorded(pair, 'submit-1', ['notes.txt']);
  assert.deepEqual(rest, []);
  assert.match(String(pair.before.head_ref), /^refs\/heads\//);
});

test('a host restart between the two records takes the later one when the host starts again', { timeout: 40000 }, async t => {
  const f = await realHost(t, 'effect');
  const thread = await f.create();
  await f.submit(thread.id);
  const approval = await eventually(() => f.watch(thread.id), value => !!value.pending_approval);
  await f.approve(thread.id, approval.pending_approval!.id);
  // The edit is done and the run waits; the host dies before it ends.
  await eventually(() => readFile(join(f.repo, 'attempts.txt'), 'utf8'), value => value === 'effect\n');
  const [node] = await childPids(f.host.pid!);
  process.kill(node, 'SIGKILL');
  await eventually(async () => f.host.exitCode !== null, value => value);
  const [cut] = stored(f.store);
  assert.equal(cut.before.state, 'ready');
  assert.equal(cut.after, undefined);
  await f.start('answer');
  const [pair] = await eventually(async () => stored(f.store), value => value[0]?.after?.state === 'ready');
  recorded(pair, 'submit-1', ['attempts.txt', 'notes.txt']);
  assert.equal(pair.after!.after_restart, 1);
  assert.equal((await f.watch(thread.id)).status, 'interrupted');
  // The next run in the checkout is recorded as usual, after the late record.
  assert.equal((await f.submit(thread.id, 'submit-2', 'Carry on.')).ok, true);
  await eventually(() => f.watch(thread.id), value => value.status === 'idle');
  const [late, next] = stored(f.store);
  recorded(next, 'submit-2', []);
  assert.equal(next.before.tree, late.after!.tree);
  assert.equal(next.after!.after_restart, 0);
});

test('a run that cannot be recorded ends with record_failed before its harness starts, and holds the queue', async t => {
  const h = await codex(t);
  const records = join(h.store, 'records');
  const block = async () => {
    if (existsSync(records)) await rename(records, join(h.store, `records-moved-${Date.now()}`));
    await writeFile(records, 'not a directory\n');
  };
  await block();
  const thread = await h.create();
  await h.submit(thread.id, 'r1', { approval: 'auto' });
  const failed = await h.idle(thread.id);
  assert.equal(failed.status, 'error');
  assert.deepEqual(failed.error, { code: 'record_failed', message: RECORD_FAILED });
  assert.equal(await h.turns(), 0, 'Codex started a turn');
  assert.equal(existsSync(join(h.root, 'codex-r1.txt')), false);
  assert.deepEqual(h.pairs().map(pair => [pair.run, pair.before.state, pair.after]), [['r1', 'failed', undefined]]);
  // Version 3 and later name the failure; versions 1 and 2 know only the generic code.
  for (const [protocol, code] of [[1, 'runtime_error'], [2, 'runtime_error'], [3, 'record_failed'], [4, 'record_failed']] as const) {
    const watched = await h.call({ op: 'watch', thread_id: thread.id }, protocol);
    assert.equal(watched.ok, true, JSON.stringify(watched));
    assert.deepEqual(watched.result.thread.error, { code, message: RECORD_FAILED });
  }

  // With the store usable again, a run records; then two follow-ups wait behind it.
  await rm(records);
  await h.submit(thread.id, 'r2');
  await h.asked(thread.id);
  await h.submit(thread.id, 'q1', { when_busy: 'follow_up' });
  await h.submit(thread.id, 'q2', { when_busy: 'follow_up' });
  await block();
  await h.approve(thread.id);
  // The run that was already going ends normally; its later record fails and is marked so.
  // The first follow-up cannot be recorded, so it fails before Codex sees it, and the second waits.
  const held = await h.until(thread.id, snapshot => snapshot.status === 'error' && !h.runtime.sessions.has(thread.id));
  assert.deepEqual(held.error, { code: 'record_failed', message: RECORD_FAILED });
  assert.deepEqual((held.queued ?? []).map(queued => queued.request_id), ['q2']);
  assert.equal(await h.turns(), 1, 'only the run that was recorded reached Codex');
  assert.deepEqual(h.pairs().map(pair => [pair.run, pair.before.state, pair.after?.state]),
    [['r1', 'failed', undefined], ['r2', 'ready', 'failed'], ['q1', 'failed', undefined]]);
  await new Promise(resolve => setTimeout(resolve, 200));
  assert.deepEqual((h.runtime.snapshot(thread.id).queued ?? []).map(queued => queued.request_id), ['q2'], 'the held follow-up stayed queued');
});

/** The refs a store keeps for its records, by record ID. */
async function refs(store: string) {
  const { stdout } = await exec('/usr/bin/git', ['--git-dir', join(store, 'records', 'sha1'), 'for-each-ref', '--format=%(refname:lstrip=2)', 'refs/records/'],
    { env: { PATH: '/usr/bin:/bin', GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null', HOME: '/nonexistent' } });
  return stdout.trim().split('\n').filter(Boolean).sort();
}

test('records older than 30 days are pruned at startup and on demand, with their Git refs', async t => {
  const h = await codex(t);
  const thread = await h.create();
  await h.submit(thread.id, 'r1');
  await h.finish(thread.id);
  await h.submit(thread.id, 'r2');
  await h.finish(thread.id);
  const [first, second] = h.pairs();
  assert.deepEqual(await refs(h.store), [first.before.id, first.after!.id, second.before.id, second.after!.id].sort());
  // The first run's records age past retention; the second's do not.
  const old = Date.now() - RETENTION_MS - 1000;
  h.runtime.store.db.prepare('UPDATE records SET created_at=? WHERE run_request=?').run(old, 'r1');
  await h.runtime.records.prune();
  assert.deepEqual(h.pairs().map(pair => pair.run), ['r2']);
  assert.deepEqual(await refs(h.store), [second.before.id, second.after!.id].sort());
  assert.equal(h.runtime.store.db.prepare('SELECT count(*) AS n FROM record_paths WHERE record=?').get(first.after!.id)!.n, 0);

  // A host that starts prunes what expired while it was stopped.
  await h.close();
  const db = new DatabaseSync(join(h.store, 'host.sqlite'));
  db.prepare('UPDATE records SET created_at=? WHERE run_request=?').run(old, 'r2');
  db.close();
  const again = new Runtime(h.store, models(), new Set(['faux']));
  t.after(() => again.close());
  await eventually(async () => pairs(again.store.db).length, count => count === 0);
  assert.deepEqual(await refs(h.store), []);
});

for (const scenario of ['slow', 'exit-mid-turn']) {
  test(`a ${scenario === 'slow' ? 'stopped' : 'failed'} Claude run keeps its pair`, async t => {
    const h = await claude(t, scenario);
    const thread = await h.create();
    await h.submit(thread.id, 'r1');
    if (scenario === 'slow') {
      await h.until(thread.id, snapshot => snapshot.tools.some(tool => tool.status === 'done'));
      await h.call({ op: 'stop', thread_id: thread.id });
    }
    assert.equal((await h.idle(thread.id)).status, scenario === 'slow' ? 'stopped' : 'error');
    recorded(h.pairs()[0], 'r1', ['claude-1.txt']);
  });
}

test('Claude steering stays inside a pair and a follow-up gets its own pair', async t => {
  const h = await claude(t, 'turn', 'ask');
  const thread = await h.create();
  await h.submit(thread.id, 'r1');
  await h.asked(thread.id);
  await h.submit(thread.id, 's1', { when_busy: 'steer' });
  await h.until(thread.id, () => h.runtime.sessions.get(thread.id)?.steered.has('s1') === true);
  await h.submit(thread.id, 'q1', { when_busy: 'follow_up' });
  await h.finish(thread.id);
  const [first, next, ...rest] = h.pairs();
  recorded(first, 'r1', ['claude-1.txt']);
  // The fake starts a new process per run; its second write has identical bytes.
  recorded(next, 'q1', []);
  assert.equal(next.before.tree, first.after!.tree);
  assert.deepEqual(rest, []);
});

for (const scenario of ['effect', 'error']) {
  test(`a ${scenario === 'effect' ? 'stopped' : 'failed'} Pi run keeps its pair`, { timeout: 30000 }, async t => {
    const f = await realHost(t, scenario);
    const thread = await f.create();
    await f.submit(thread.id);
    if (scenario === 'effect') {
      const approval = await eventually(() => f.watch(thread.id), value => !!value.pending_approval);
      await f.approve(thread.id, approval.pending_approval!.id);
      await eventually(() => readFile(join(f.repo, 'attempts.txt'), 'utf8'), value => value === 'effect\n');
      await request(f.store, { op: 'stop', thread_id: thread.id });
    }
    await eventually(async () => stored(f.store), pairs => pairs[0]?.after?.state === 'ready');
    recorded(stored(f.store)[0], 'submit-1', scenario === 'effect' ? ['attempts.txt', 'notes.txt'] : []);
    assert.equal((await f.watch(thread.id)).status, scenario === 'effect' ? 'stopped' : 'error');
  });
}

test('restart recovery waits for a leased next run when another writer held the checkout', { timeout: 40000 }, async t => {
  const f = await realHost(t, 'effect');
  const thread = await f.create();
  await f.submit(thread.id);
  const approval = await eventually(() => f.watch(thread.id), value => !!value.pending_approval);
  await f.approve(thread.id, approval.pending_approval!.id);
  await eventually(() => readFile(join(f.repo, 'attempts.txt'), 'utf8'), value => value === 'effect\n');
  const [node] = await childPids(f.host.pid!);
  process.kill(node, 'SIGKILL');
  await eventually(async () => f.host.exitCode !== null, value => value);
  const lease = await acquireCheckout(f.repo);
  try {
    await f.start('answer');
    const [deferred] = await eventually(async () => stored(f.store), value => value[0]?.after?.state === 'failed');
    assert.equal(deferred.after!.after_restart, 1);
  } finally { await lease.close(); }
  await writeFile(join(f.repo, 'while-down.txt'), 'external change\n');
  await f.submit(thread.id, 'submit-2', 'Continue.');
  await eventually(() => f.watch(thread.id), value => value.status === 'idle');
  const [recovered, next] = stored(f.store);
  recorded(recovered, 'submit-1', ['attempts.txt', 'notes.txt', 'while-down.txt']);
  assert.equal(recovered.after!.after_restart, 1);
  recorded(next, 'submit-2', []);
  assert.equal(next.before.tree, recovered.after!.tree);
});

test('Pi steering stays in the active pair', { timeout: 30000 }, async t => {
  const f = await realHost(t, 'edit');
  const thread = await f.create();
  await f.submit(thread.id);
  const approval = await eventually(() => f.watch(thread.id), value => !!value.pending_approval);
  const reply = await request(f.store, { op: 'submit', protocol_version: 3, thread_id: thread.id, request_id: 'steering', text: 'Also keep it short.', when_busy: 'steer' });
  assert.equal(reply.ok, true);
  await f.approve(thread.id, approval.pending_approval!.id);
  await eventually(() => f.watch(thread.id), value => value.status === 'idle');
  const [pair, ...rest] = stored(f.store);
  recorded(pair, 'submit-1', ['notes.txt']);
  assert.deepEqual(rest, []);
});

test('Pi follow-ups get separate pairs with the right changed paths', { timeout: 30000 }, async t => {
  const f = await realHost(t, 'separate-edits');
  const thread = await f.create();
  await f.submit(thread.id);
  const approval = await eventually(() => f.watch(thread.id), value => !!value.pending_approval);
  const reply = await request(f.store, { op: 'submit', protocol_version: 3, thread_id: thread.id, request_id: 'follow-up', text: 'Next task.', when_busy: 'follow_up', approval: 'auto' });
  assert.equal(reply.ok, true);
  await f.approve(thread.id, approval.pending_approval!.id);
  await eventually(async () => stored(f.store), pairs => pairs[1]?.after?.state === 'ready');
  const [first, next, ...rest] = stored(f.store);
  recorded(first, 'submit-1', ['notes.txt']);
  recorded(next, 'follow-up', ['notes.txt']);
  assert.equal(next.before.tree, first.after!.tree);
  assert.deepEqual(rest, []);
});

test('a failed before record prevents Claude and Pi from starting', { timeout: 30000 }, async t => {
  const h = await claude(t);
  await writeFile(join(h.store, 'records'), 'blocked');
  const claudeThread = await h.create();
  await h.submit(claudeThread.id, 'r1');
  assert.equal((await h.idle(claudeThread.id)).error?.code, 'record_failed');
  assert.equal(existsSync(join(h.root, 'claude-1.txt')), false);
  assert.deepEqual(h.pairs().map(pair => [pair.before.state, pair.after]), [['failed', undefined]]);

  const f = await realHost(t, 'edit');
  await writeFile(join(f.store, 'records'), 'blocked');
  const piThread = await f.create();
  await f.submit(piThread.id);
  const failed = await eventually(() => f.watch(piThread.id), value => value.status === 'error');
  // This helper watches with version 1, which maps the new failure code.
  assert.equal(failed.error?.code, 'runtime_error');
  assert.equal(failed.error?.message, RECORD_FAILED);
  assert.deepEqual(failed.tools, []);
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'before\n');
  assert.deepEqual(stored(f.store).map(pair => [pair.before.state, pair.after]), [['failed', undefined]]);
});

test('retention keeps an active before record and a pair whose after record is recent', async t => {
  const h = await codex(t);
  const thread = await h.create();
  await h.submit(thread.id, 'r1');
  await h.asked(thread.id);
  const before = h.pairs()[0].before;
  const old = Date.now() - RETENTION_MS - 1000;
  h.runtime.store.db.prepare('UPDATE records SET created_at=? WHERE id=?').run(old, before.id);
  await h.runtime.records.prune();
  assert.equal(h.pairs()[0].before.state, 'ready');
  await h.finish(thread.id);
  assert.equal((await h.call({ op: 'archive_thread', thread_id: thread.id, archived: true })).ok, true);
  await h.runtime.records.prune();
  recorded(h.pairs()[0], 'r1', ['codex-r1.txt']);
  assert.deepEqual(await refs(h.store), [before.id, h.pairs()[0].after!.id].sort());
});
