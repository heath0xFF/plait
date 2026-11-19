import { confirmStopped } from './support/host.ts';
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { setup, request, until, delay } from './support/host.ts';

type Observed = { summary: boolean; goal: boolean; length: number };
type Fixture = Awaited<ReturnType<typeof setup>>;
async function observed(f: Fixture): Promise<Observed[]> {
  return (await readFile(join(f.store, 'compaction-observed.jsonl'), 'utf8')).trim().split('\n').map(line => JSON.parse(line));
}
async function submit(f: Fixture, id: string, turn: number) {
  const result = await f.submit(id, `turn-${turn}`, turn === 0 ? 'KEEP_SELF_HOST_GOAL: finish the project.' : `Continue development ${turn}.`);
  assert(result.ok, JSON.stringify(result));
  return until(() => f.watch(id), value => value.status === 'idle' || value.status === 'error');
}

test('automatic context compaction preserves full history and the next run after a host restart', { timeout: 45000 }, async t => {
  const f = await setup(t, 'self-host-compaction'); const thread = await f.create();
  for (let turn = 0; turn < 7; turn++) assert.equal((await submit(f, thread.id, turn)).status, 'idle');
  const before = await observed(f);
  assert(before.some(row => row.summary), 'the real harness never compacted');
  assert(before.every(row => row.goal), 'the goal disappeared from model context');
  const history = await request(f.store, { op: 'history', thread_id: thread.id, protocol_version: 3, limit: 100 });
  const items = (history.result as unknown as { items: { type: string; role?: string; text?: string }[] }).items;
  assert.equal(items.filter(item => item.role === 'user').length, 7);
  assert.equal(items.filter(item => item.role === 'assistant').length, 7);
  assert(items.some(item => item.text === 'KEEP_SELF_HOST_GOAL: finish the project.'));
  f.host.kill('SIGKILL'); await delay(400); await f.start('self-host-compaction');
  // Reopening history cannot issue a model request or replay an effect.
  await request(f.store, { op: 'history', thread_id: thread.id, protocol_version: 3 });
  await delay(100); assert.equal((await observed(f)).length, before.length);
  assert.equal((await submit(f, thread.id, 7)).status, 'idle');
  assert((await observed(f)).at(-1)!.goal);
});

test('a host killed during compaction waits for a new submission before making any model request', { timeout: 45000 }, async t => {
  const f = await setup(t, 'self-host-compaction-crash'); const thread = await f.create();
  // Two large answers cross the default policy's background threshold on the third turn.
  for (let turn = 0; turn < 2; turn++) assert.equal((await submit(f, thread.id, turn)).status, 'idle');
  await f.submit(thread.id, 'turn-2', 'Continue development.');
  await until(() => readFile(join(f.store, 'summary-started'), 'utf8'), value => value === 'ready');
  f.host.kill('SIGKILL'); await delay(400);
  const before = (await observed(f)).length;
  await f.start('self-host-compaction');
  await request(f.store, { op: 'history', thread_id: thread.id, protocol_version: 3 });
  await delay(150); assert.equal((await observed(f)).length, before);
  await confirmStopped(f);
  assert.equal((await submit(f, thread.id, 3)).status, 'idle');
  assert((await observed(f)).at(-1)!.goal);
});

test('failed compaction reports a recoverable error without exposing provider details', { timeout: 45000 }, async t => {
  const f = await setup(t, 'self-host-compaction-failure'); const thread = await f.create();
  let failed = false;
  for (let turn = 0; turn < 8; turn++) {
    const result = await submit(f, thread.id, turn);
    if (result.status !== 'error') continue;
    const current = await request(f.store, { op: 'watch', thread_id: thread.id, protocol_version: 3 });
    assert.equal(current.result.thread.error?.code, 'context_compaction');
    assert.match(current.result.thread.error!.message, /Full history is saved/);
    assert(!JSON.stringify(current).includes('Fixture summary failed'));
    failed = true; break;
  }
  assert(failed, 'the fixture never reached blocking compaction');
  f.host.kill('SIGKILL'); await delay(400); await f.start('self-host-compaction');
  assert.equal((await submit(f, thread.id, 9)).status, 'idle');
  assert((await observed(f)).at(-1)!.goal);
});
