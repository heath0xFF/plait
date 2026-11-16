import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';
import { setup, until } from './support/host.ts';

test('each manual turn creates fresh permission and rejects an earlier approval', { timeout: 30000 }, async t => {
  const f = await setup(t, 'separate-edits');
  const thread = await f.create();
  assert.equal((await f.submit(thread.id)).ok, true);
  const first = await until(() => f.watch(thread.id), value => !!value.pending_approval);
  const firstId = first.pending_approval!.id;
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'before\n');
  assert.equal((await f.approve(thread.id, firstId)).ok, true);
  await until(() => f.watch(thread.id), value => value.status === 'idle');
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'after\n');

  assert.equal((await f.submit(thread.id, 'submit-2', 'Make the next scripted edit.')).ok, true);
  const second = await until(() => f.watch(thread.id), value => !!value.pending_approval);
  assert.notEqual(second.pending_approval!.id, firstId);
  assert.equal((await f.approve(thread.id, firstId)).error?.code, 'not_found');
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'after\n');
  assert.equal((await f.approve(thread.id, second.pending_approval!.id)).ok, true);
  const done = await until(() => f.watch(thread.id), value => value.status === 'idle');
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'followup\n');
  assert.equal(done.tools.filter(tool => tool.name === 'edit' && tool.status === 'done').length, 2);
});

test('denied manual edit has no file effect and cannot be approved later', { timeout: 30000 }, async t => {
  const f = await setup(t, 'edit');
  const thread = await f.create();
  await f.submit(thread.id);
  const pending = await until(() => f.watch(thread.id), value => !!value.pending_approval);
  const id = pending.pending_approval!.id;
  assert.equal((await f.approve(thread.id, id, false)).ok, true);
  assert.equal((await f.approve(thread.id, id, true)).error?.code, 'not_found');
  const done = await until(() => f.watch(thread.id), value => value.status === 'idle');
  assert.equal(done.pending_approval, null);
  assert(done.tools.some(tool => tool.name === 'edit' && tool.status === 'error'));
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'before\n');
});

test('a permissive fixture policy cannot bypass dirty paths, path checks, or command containment', { timeout: 30000 }, async t => {
  const f = await setup(t, 'permissive-policy');
  await writeFile(join(f.repo, 'notes.txt'), 'dirty\n');
  const thread = await f.create();
  assert.equal((await f.submit(thread.id)).ok, true);
  const done = await until(() => f.watch(thread.id), value => value.status === 'idle');
  assert.equal(done.pending_approval, null);
  assert.equal(done.tools.length, 4);
  assert(done.tools.every(tool => tool.status === 'error'), JSON.stringify(done.tools));
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'dirty\n');
  await assert.rejects(readFile(join(f.base, 'outside.txt')), { code: 'ENOENT' });
  await assert.rejects(readFile(join(f.repo, '.git', 'automation.txt')), { code: 'ENOENT' });
});
