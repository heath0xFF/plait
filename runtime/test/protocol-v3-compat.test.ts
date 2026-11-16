import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { DatabaseSync } from 'node:sqlite';
import { join } from 'node:path';
import { delay, request, setup, until } from './support/host.ts';

// Replies recorded from the host as it was before version 3 existed, for this exact scripted
// session. Thread, checkout, and approval IDs are replaced by placeholders.
const recorded: Record<string, string> = JSON.parse(readFileSync(new URL('./support/replies-before-v3.json', import.meta.url), 'utf8'));

test('version 1 and 2 replies are byte-for-byte what they were before version 3', { timeout: 30000 }, async t => {
  const f = await setup(t, 'edit');
  let id = '<none>'; let approval = '<none>';
  const wire = (reply: unknown) => JSON.stringify(reply).replaceAll(f.repo, '<REPO>').replaceAll(id, '<ID>').replaceAll(approval, '<APPROVAL>');
  const same = async (name: string, value: unknown) => assert.equal(wire(await request(f.store, value)), recorded[name], name);
  await same('status1', { op: 'status' });
  await same('status2', { op: 'status', protocol_version: 2 });
  const created = await request(f.store, { op: 'create_thread', checkout: f.repo, provider: 'faux', model_id: 'faux-1', request_id: 'create-1' });
  id = created.result.thread.id;
  assert.equal(wire(created), recorded.create);
  await same('list1', { op: 'list_threads', checkout: f.repo });
  await same('list2', { op: 'list_threads', checkout: f.repo, protocol_version: 2 });
  await same('submit', { op: 'submit', thread_id: id, request_id: 'submit-1', text: 'Do the scripted task.' });
  await same('submitAgain', { op: 'submit', thread_id: id, request_id: 'submit-1', text: 'Do the scripted task.' });
  await same('busy', { op: 'submit', thread_id: id, request_id: 'submit-2', text: 'Second.' });
  approval = (await until(() => f.watch(id), value => !!value.pending_approval)).pending_approval!.id;
  await same('pending1', { op: 'watch', thread_id: id });
  await same('pending2', { op: 'watch', thread_id: id, protocol_version: 2 });
  assert.equal(wire(await f.approve(id, approval)), recorded.approve);
  await until(() => f.watch(id), value => value.status === 'idle');
  await same('idle1', { op: 'watch', thread_id: id });
  await same('idle2', { op: 'watch', thread_id: id, protocol_version: 2 });

  // Version 3 state on the same thread must not reach an older client.
  const v3 = (value: Record<string, unknown>) => request(f.store, { ...value, thread_id: id, protocol_version: 3 });
  assert.equal((await v3({ op: 'rename_thread', title: 'Renamed by a newer client' })).ok, true);
  assert.equal((await v3({ op: 'archive_thread', archived: true })).ok, true);
  assert.equal((await v3({ op: 'configure_thread', thinking: 'off' })).ok, true);
  await same('idle1', { op: 'watch', thread_id: id });
  await same('idle2', { op: 'watch', thread_id: id, protocol_version: 2 });
  await same('list1', { op: 'list_threads', checkout: f.repo });

  await same('stop1', { op: 'stop', thread_id: id });
  await same('stop2', { op: 'stop', thread_id: id, protocol_version: 2 });
  await same('unknownOp', { op: 'history', thread_id: id });
  await same('unknownField', { op: 'list_threads', checkout: f.repo, limit: 5 });
  await same('missing', { op: 'watch', thread_id: 'missing' });
  await same('listFinal', { op: 'list_threads', checkout: f.repo });
  // Version 3 operations and fields do not exist for a version 1 or 2 request.
  for (const value of [
    { op: 'history', thread_id: id, protocol_version: 2 }, { op: 'watch_threads' }, { op: 'watch_threads', protocol_version: 2 },
    { op: 'rename_thread', thread_id: id, title: 'x' }, { op: 'archive_thread', thread_id: id, archived: true, protocol_version: 2 },
    { op: 'configure_thread', thread_id: id, thinking: 'off' }, { op: 'cancel_queued', thread_id: id, request_id: 'x', protocol_version: 2 },
    { op: 'history_text', thread_id: id, ref: 'q1', offset: 0 },
    { op: 'submit', thread_id: id, request_id: 'r', text: 'hi', protocol_version: 2, when_busy: 'steer' },
  ]) assert.equal(wire(await request(f.store, value)), recorded.unknownOp, JSON.stringify(value));
  // The checkout stays required below version 3.
  assert.equal((await request(f.store, { op: 'list_threads', protocol_version: 2 })).error?.message, 'checkout must be a nonempty string.');
});

test('a store saved before version 3 keeps loading and gains summaries once', { timeout: 30000 }, async t => {
  const f = await setup(t);
  const thread = await f.create();
  await f.submit(thread.id, 'first', '  Migrate\tthis   old thread  ');
  await until(() => f.watch(thread.id), value => value.status === 'idle');
  const before = JSON.stringify(await request(f.store, { op: 'watch', thread_id: thread.id, protocol_version: 2 }));
  f.host.kill('SIGKILL'); await delay(400);
  // Rewrite the thread as an older host left it: no summary columns, no version 3 snapshot fields.
  const db = new DatabaseSync(join(f.store, 'host.sqlite'));
  const row = db.prepare('SELECT snapshot FROM threads WHERE id=?').get(thread.id)!;
  const { title: _title, created_at: _created, updated_at: _updated, live_reasoning: _live, retry: _retry, usage: _usage, queued: _queued, ...old } = JSON.parse(String(row.snapshot));
  db.prepare('UPDATE threads SET snapshot=?,checkout=NULL,status=NULL,model=NULL,title=NULL,created_at=NULL,updated_at=NULL WHERE id=?').run(JSON.stringify(old), thread.id);
  db.close();
  await f.start();
  assert.equal(JSON.stringify(await request(f.store, { op: 'watch', thread_id: thread.id, protocol_version: 2 })), before);
  const listed = (await request(f.store, { op: 'list_threads', protocol_version: 3 })).result.threads as unknown as Record<string, unknown>[];
  assert.equal(listed.length, 1);
  assert.equal(listed[0].title, 'Migrate this old thread');
  assert.equal(listed[0].status, 'idle');
  assert(Number(listed[0].created_at) > 0 && listed[0].created_at === listed[0].updated_at);
  // A second start changes nothing: the migration is idempotent.
  f.host.kill('SIGKILL'); await delay(400); await f.start();
  assert.deepEqual((await request(f.store, { op: 'list_threads', protocol_version: 3 })).result.threads, listed);
  assert.equal((await f.submit(thread.id, 'second', 'Still usable.')).ok, true);
  await until(() => f.watch(thread.id), value => value.status === 'idle');
});
