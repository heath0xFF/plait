import assert from 'node:assert/strict';
import { test } from 'node:test';
import { DatabaseSync } from 'node:sqlite';
import { join } from 'node:path';
import { writeFile, symlink, link } from 'node:fs/promises';
import { CommandOutputs, OUTPUT_COUNT, OUTPUT_LIMIT, rangeArgs } from '../src/tool-output.ts';
import type { TextRange } from '../src/tool-output.ts';
import { fileAction, rootIdentity } from '../src/safety.ts';
import { setup } from './support/safety.ts';

test('ranged reads reach every byte of a large Unicode file without relaxing path checks', async t => {
  const { root, base } = await setup(t);
  const content = 'line 🚀 é\n'.repeat(30000) + 'LAST LINE';
  await writeFile(join(root, 'large.txt'), content);
  const identity = await rootIdentity(root);
  let offset = 0; let actual = '';
  for (;;) {
    const page = await fileAction<TextRange>(root, 'read_range', 'large.txt', { offset, limit: 12000 }, undefined, identity);
    actual += page.text;
    assert.equal(page.total_bytes, Buffer.byteLength(content));
    assert.equal(page.offset, offset);
    assert(Buffer.byteLength(page.text) <= 12000);
    if (page.next_offset === null) break;
    assert(page.next_offset > offset); offset = page.next_offset;
  }
  assert.equal(actual, content);
  const before = await fileAction(root, 'inspect', 'large.txt');
  await fileAction(root, 'edit', 'large.txt', { expected: 'LAST LINE', content: 'CHANGED', expected_image: before.postimage });
  await assert.rejects(fileAction(root, 'edit', 'large.txt', { expected: 'CHANGED', content: 'STALE', expected_image: before.postimage }), /identity or metadata changed/);
  await assert.rejects(fileAction(root, 'read_range', 'large.txt', { offset: 6, limit: 100 }));
  await symlink(join(root, 'large.txt'), join(root, 'alias'));
  await assert.rejects(fileAction(root, 'read_range', 'alias', { offset: 0 }));
  await link(join(root, 'large.txt'), join(base, 'hard'));
  await assert.rejects(fileAction(root, 'read_range', 'large.txt', { offset: 0 }));
  for (const path of ['../outside', '.git/config']) await assert.rejects(fileAction(root, 'read_range', path, { offset: 0 }));
});

test('command logs survive reopen, preserve Unicode across chunks, and belong to one thread', async t => {
  const { base } = await setup(t);
  let db = new DatabaseSync(join(base, 'logs.sqlite'));
  t.after(() => { db.close(); });
  let outputs = new CommandOutputs(db);
  const log = outputs.begin('thread-a', 'build-1');
  log.append('FIRST\n' + '🚀'.repeat(10000)); log.append('\nLAST');
  assert.match(outputs.read('thread-a', log.id, 0, 12000), /Capture did not finish/);
  assert.match(outputs.read('thread-a', 'build-1', 0, 12000), /^FIRST/);
  assert.match(log.finish(), /read_output/);
  db.close(); db = new DatabaseSync(join(base, 'logs.sqlite')); outputs = new CommandOutputs(db);
  assert.match(outputs.read('thread-a', log.id, 0, 12000), /^FIRST/);
  assert.equal(outputs.read('thread-a', log.id, 40006, 12000).split('\n\n[Bytes')[0], '\nLAST');
  assert.throws(() => outputs.read('thread-a', log.id, 7, 100), /UTF-8/);
  assert.throws(() => outputs.read('thread-b', log.id, 0, 100), /unavailable/);
  assert.throws(() => outputs.read('thread-a', '../host.sqlite', 0, 100), /unavailable/);
  assert.throws(() => outputs.read('thread-a', log.id, 100000, 100), /beyond/);
  const firstPage = outputs.read('thread-a', log.id, 0, 12000);
  assert.match(firstPage, /"offset":11998/);
});

test('command log limits are explicit and retention removes the oldest logs only', () => {
  const db = new DatabaseSync(':memory:');
  try {
    const outputs = new CommandOutputs(db);
    const keep = outputs.begin('other'); keep.append('other project'); keep.finish();
    const log = outputs.begin('thread');
    log.append('x'.repeat(OUTPUT_LIMIT - 1) + '🚀'); log.append('must not be appended');
    assert.match(log.finish(), /exceeded 16 MiB/);
    assert.match(outputs.read('thread', log.id, OUTPUT_LIMIT - 5, 100), /^xxxx\n/);
    assert.equal(db.prepare('SELECT bytes FROM command_outputs WHERE id=?').get(log.id)?.bytes, OUTPUT_LIMIT - 1);
    for (let i = 0; i < OUTPUT_COUNT; i++) outputs.begin('thread').finish();
    assert.throws(() => outputs.read('thread', log.id, 0, 100), /expired/);
    assert.equal(outputs.read('other', keep.id, 0, 100), 'other project');
    assert.equal(db.prepare('SELECT count(*) AS n FROM command_output_chunks WHERE id=?').get(log.id)?.n, 0);
  } finally { db.close(); }
});

test('range arguments reject malformed and unbounded reads', () => {
  for (const args of [{ offset: -1 }, { offset: 0.5 }, { offset: '1' }, { limit: 0 }, { limit: 12001 }, { offset: Infinity }]) {
    assert.throws(() => rangeArgs(args));
  }
  assert.deepEqual(rangeArgs({}), { offset: 0, limit: 8192 });
});
