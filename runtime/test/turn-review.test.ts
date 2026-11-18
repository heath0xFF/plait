import assert from 'node:assert/strict';
import { test } from 'node:test';
import { writeFile, readFile, readdir, stat } from 'node:fs/promises';
import { join } from 'node:path';
import { HostError } from '../src/protocol.ts';
import { TurnReview, validateReview } from '../src/turn-review.ts';
import { codex } from './support/record-runtime.ts';
import { exec } from './support/safety.ts';

test('turn review validates additive requests strictly', () => {
  const valid = { op: 'turn_diff', thread_id: 't', record_id: 'r', path: 'src/file.rs' };
  assert.equal(validateReview(valid).op, 'turn_diff');
  for (const input of [null, [], { ...valid, protocol_version: 5 }, { ...valid, protocol_version: 6 }, { ...valid, store: '/' }, { ...valid, record_id: '-x/..' }, { op: 'turn_review_page', token: 't', offset: -1 }, { op: 'turn_review_page', token: 't', offset: 1.5 }]) assert.throws(() => validateReview(input));
  for (const path of ['/a', '../a', '.git/config', 'a//b', './a', 'a\0b', 'x'.repeat(65001)]) assert.throws(() => validateReview({ ...valid, path }));
});

test('recorded reads survive commits and undo and preserve checkout metadata', async t => {
  const h = await codex(t); const thread = await h.create();
  const call = (request: unknown) => h.runtime.handle(request);
  const info: any = await call({ op: 'turn_review_info', thread_id: thread.id });
  assert.deepEqual(info.features, { record_diff: true, attribution: false, stage_turn: false, since_review: false });
  await h.submit(thread.id, 'r1'); await h.approve(thread.id); await h.idle(thread.id);
  const record = h.pairs()[0].after!.id;
  const request = { op: 'turn_diff', thread_id: thread.id, record_id: record, path: 'codex-r1.txt' };
  // Hash all regular files and their times; reads must not alter even Git metadata.
  const inventory = async (path: string): Promise<unknown[]> => {
    const result: unknown[] = [];
    for (const name of (await readdir(path)).sort()) {
      const file = join(path, name); const metadata = await stat(file);
      result.push([name, metadata.mode, metadata.mtimeMs, metadata.ctimeMs, metadata.isDirectory() ? await inventory(file) : (await readFile(file)).toString('base64')]);
    }
    return result;
  };
  const before = await inventory(h.root);
  const first: any = await call(request);
  assert.equal(first.status, 'ready', JSON.stringify(first));
  assert.deepEqual(await inventory(h.root), before);
  const patch = Buffer.concat(first.patch.items.map((item: any) => Buffer.from(item.bytes_base64, 'base64')));
  assert.equal(patch.length, first.patch_bytes); assert.match(patch.toString(), /new file mode/);
  await exec('/usr/bin/git', ['-C', h.root, 'add', '.']);
  await exec('/usr/bin/git', ['-C', h.root, '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', '-c', 'commit.gpgsign=false', 'commit', '-qm', 'fixture']);
  const committed: any = await call(request); assert.deepEqual(committed.patch.items, first.patch.items);
  const undone = await h.call({ op: 'undo_turn', thread_id: thread.id, record_id: record, request_id: 'u1', leave_changed: false }, 5);
  assert.equal(undone.ok, true, JSON.stringify(undone));
  const original: any = await call(request); assert.deepEqual(original.patch.items, first.patch.items);
  const other = await h.create(); const same: any = await call({ ...request, thread_id: other.id }); assert.deepEqual(same.patch.items, first.patch.items);
  const newer: any = await call({ op: 'turn_review_info', thread_id: other.id }); assert.ok(newer.checkout.catalog_revision > info.checkout.catalog_revision);
});

test('immutable pages enforce offsets, retries, frame bounds and idle expiry', async t => {
  const h = await codex(t); let now = 0;
  const review = new TurnReview(h.runtime.store, h.runtime.records, h.store, () => now);
  const items = Array.from({ length: 1200 }, (_, i) => ({ byte_offset: i, bytes_base64: 'a'.repeat(4000) }));
  let page = review.document('checkout', items); const token = page.token; const all = [];
  assert.throws(() => review.page(token, 1));
  for (;;) {
    assert.ok(page.items.length <= 500); assert.ok(Buffer.byteLength(JSON.stringify(page)) <= 750_000);
    assert.deepEqual(review.page(token, page.offset), page);
    all.push(...page.items);
    if (page.next_offset === null) break;
    page = review.page(token, page.next_offset);
  }
  assert.deepEqual(all, items);
  now = 300_000; assert.throws(() => review.page(token, 0), /expired/);
});

test('turn review rejects inherited operation names with invalid_request', () => {
  for (const op of ['toString', 'constructor', '__proto__', 'hasOwnProperty']) {
    assert.throws(() => validateReview({ op }), (error: unknown) => error instanceof HostError && error.code === 'invalid_request');
  }
});

test('recorded patch and hunk pages share one reply frame budget', async t => {
  const h = await codex(t);
  const review = new TurnReview(h.runtime.store, h.runtime.records, h.store);
  const patch = Array.from({ length: 8 }, (_, i) => ({ byte_offset: i * 65_536, bytes_base64: Buffer.alloc(65_536, 'a').toString('base64') }));
  // Leave room for richer hunk metadata without changing the transport contract.
  const hunks = Array.from({ length: 500 }, (_, i) => ({ id: `hunk-${i}`, origin: { title: 't'.repeat(4096) } }));
  const metadata = { schema: 1, path: 'p'.repeat(4096), source_record_id: 'r'.repeat(128), patch_bytes: 524_288 };
  const reply = review.withDocuments('checkout', metadata, patch, hunks);
  assert.ok(Buffer.byteLength(JSON.stringify({ ok: true, result: reply }) + '\n') < 1_048_576);
  for (const [first, expected] of [[reply.patch, patch], [reply.hunks, hunks]] as const) {
    assert.notEqual(first.next_offset, null, 'each document must continue in another frame');
    let page = first; const all = [];
    for (;;) {
      assert.deepEqual(review.page(page.token, page.offset), page, 'page retries must retain the same boundary');
      assert.ok(Buffer.byteLength(JSON.stringify({ ok: true, result: page }) + '\n') < 750_000);
      all.push(...page.items);
      if (page.next_offset === null) break;
      page = review.page(page.token, page.next_offset);
    }
    assert.deepEqual(all, expected);
  }
  assert.throws(() => review.withDocuments('checkout', { title: 't'.repeat(1_048_576) }, [], []), /review limit/);
  assert.throws(() => review.withDocuments('checkout', {}, ['x'.repeat(750_000)], []), /review limit/);
});
