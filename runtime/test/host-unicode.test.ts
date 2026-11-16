import assert from 'node:assert/strict';
import { test } from 'node:test';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { setup, request, until, delay } from './support/host.ts';

test('a read tool cannot split Unicode at its result limit', { timeout: 15000 }, async t => {
  const f = await setup(t, 'unicode-read');
  await writeFile(join(f.repo, 'unicode.txt'), 'x'.repeat(16383) + '🚀after');
  const thread = await f.create(); await f.submit(thread.id);
  const done = await until(() => f.watch(thread.id), value => value.status === 'idle');
  const tool = done.tools.find(tool => tool.id === 'unicode-read-1');
  assert.equal(tool?.status, 'done');
  assert.equal(tool?.output, 'x'.repeat(4096));
  assert.equal((await request(f.store, { op: 'status' })).ok, true);
});

test('Unicode at the preview boundary survives Rust transport and host restart', { timeout: 20000 }, async t => {
  const f = await setup(t, 'crash');
  const thread = await f.create();
  const text = '🚀' + 'x'.repeat(4095);
  const accepted = await f.submit(thread.id, 'unicode-submit', text);
  assert.equal(accepted.ok, true);
  const current = await f.watch(thread.id);
  assert.equal(current.messages.find(message => message.id === 'request-unicode-submit')?.text, 'x'.repeat(4095));
  await until(() => f.watch(thread.id), value => !!value.pending_approval);
  f.host.kill('SIGKILL'); await delay(500); await f.start();
  const restored = await f.watch(thread.id);
  assert.equal(restored.status, 'interrupted');
  assert.equal(restored.pending_approval, null);
  assert.equal(restored.messages.find(message => message.id === 'request-unicode-submit')?.text, 'x'.repeat(4095));
  assert.deepEqual(await f.submit(thread.id, 'unicode-submit', text), accepted);
  assert.equal((await f.watch(thread.id)).status, 'interrupted');
  assert.equal((await request(f.store, { op: 'status' })).ok, true);
});
