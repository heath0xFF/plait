import { confirmStopped } from './support/host.ts';
import assert from 'node:assert/strict';
import { readFile, writeFile, stat, symlink } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';
import { exec, binary, fixture, delay, childPids, until, request, setup } from './support/host.ts';

test('absolute store aliases resolve once while symlink leaves remain rejected', { timeout: 15000 }, async t => {
  const f = await setup(t, 'edit-command', true);
  const thread = await f.create(); await f.submit(thread.id);
  for (const kind of ['edit', 'command']) {
    const pending = await until(() => f.watch(thread.id), state => state.pending_approval?.kind === kind);
    assert.equal((await f.approve(thread.id, pending.pending_approval!.id)).ok, true);
  }
  const done = await until(() => f.watch(thread.id), state => state.status === 'idle');
  assert(done.tools.every(tool => tool.status === 'done'));
  assert((await stat(join(f.store, 'build-cache', thread.id, 'target'))).isDirectory());
  const alias = join(f.base, 'store-symlink'); await symlink(f.store, alias);
  await assert.rejects(exec(binary, ['run', '--store', alias, '--runtime', fixture]), /owned directory/);
});

test('approval exact details, client reconnect, checkout exclusion, own edit then command', { timeout: 30000 }, async t => {
  const f = await setup(t, 'edit-command'); const thread = await f.create(); const other = await f.create('create-2');
  assert.equal((await f.submit(thread.id)).ok, true);
  const edit = await until(() => f.watch(thread.id), value => !!value.pending_approval);
  assert.equal(edit.pending_approval!.kind, 'edit');
  assert.deepEqual(JSON.parse(edit.pending_approval!.details), { path: 'notes.txt', old_text: 'before', new_text: 'after' });
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'before\n');
  assert.equal((await f.submit(other.id)).error?.code, 'busy');
  assert.equal((await f.approve(thread.id, edit.pending_approval!.id)).ok, true);
  const command = await until(() => f.watch(thread.id), value => value.pending_approval?.kind === 'command');
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'after\n');
  assert.equal((await f.approve(thread.id, command.pending_approval!.id)).ok, true);
  const done = await until(() => f.watch(thread.id), value => value.status === 'idle' || value.status === 'error');
  assert.equal(done.status, 'idle', JSON.stringify(done));
  assert(done.tools.some(tool => tool.output.includes('passed')), JSON.stringify(done));
  assert(done.tools.some(tool => tool.name === 'command' && tool.status === 'done'), JSON.stringify(done));
});

test('provider errors are safe, stop revokes approval and kills command tree', { timeout: 30000 }, async t => {
  const e = await setup(t, 'error'); const errorThread = await e.create(); await e.submit(errorThread.id);
  const failed = await until(() => e.watch(errorThread.id), value => value.status === 'error');
  assert(!JSON.stringify(failed).includes('SECRET_PROVIDER_TOKEN'));
  const f = await setup(t, 'crash'); const thread = await f.create(); await f.submit(thread.id);
  let approval = await until(() => f.watch(thread.id), value => !!value.pending_approval);
  const approvalId = approval.pending_approval!.id;
  assert.equal((await request(f.store, { op: 'stop', thread_id: thread.id })).ok, true);
  assert.equal((await f.approve(thread.id, approvalId)).error?.code, 'not_found');
  assert.equal((await f.watch(thread.id)).status, 'stopped');
  // A new host fixture supplies a fresh scripted command for the second run.
  f.host.kill('SIGKILL'); await delay(500); await f.start('crash');
  await f.submit(thread.id, 'submit-2');
  approval = await until(() => f.watch(thread.id), value => !!value.pending_approval);
  await f.approve(thread.id, approval.pending_approval!.id);
  const running = await until(() => f.watch(thread.id), value => value.tools.some(tool => /READY \d+/.test(tool.output)));
  const pid = Number(running.tools.map(tool => tool.output).join('').match(/READY (\d+)/)![1]);
  const stoppedAt = Date.now();
  assert.equal((await request(f.store, { op: 'stop', thread_id: thread.id })).ok, true);
  assert(Date.now() - stoppedAt < 5000);
  await until(async () => { try { process.kill(-pid, 0); return false; } catch { return true; } }, value => value);
  assert.equal((await f.watch(thread.id)).status, 'stopped');
});

for (const target of ['host', 'node']) test(`${target} SIGKILL after effect never replays command or submission`, { timeout: 30000 }, async t => {
  const f = await setup(t, 'crash'); const thread = await f.create(); await f.submit(thread.id);
  const approval = await until(() => f.watch(thread.id), value => !!value.pending_approval);
  await f.approve(thread.id, approval.pending_approval!.id);
  const running = await until(() => f.watch(thread.id), value => value.tools.some(tool => /READY \d+/.test(tool.output)));
  const pid = Number(running.tools.map(tool => tool.output).join('').match(/READY (\d+)/)![1]);
  if (target === 'host') f.host.kill('SIGKILL');
  else {
    const children = await childPids(f.host.pid!);
    assert.equal(children.length, 1); process.kill(children[0], 'SIGKILL');
  }
  await until(async () => f.host.exitCode !== null || f.host.signalCode !== null, value => value);
  await delay(500); await f.start('answer');
  const reopened = await f.watch(thread.id);
  assert.equal(reopened.status, 'interrupted'); assert.equal(reopened.pending_approval, null);
  assert.equal((await f.submit(thread.id)).ok, true);
  await until(async () => { try { process.kill(-pid, 0); return false; } catch { return true; } }, value => value);
  assert.equal((await f.watch(thread.id)).status, 'interrupted');
  await confirmStopped(f);
  assert.equal((await f.submit(thread.id, 'submit-new', 'Inspect recovery safely.')).ok, true);
  const done = await until(() => f.watch(thread.id), value => value.status === 'idle');
  assert.equal(done.messages.filter(message => message.role === 'user').length, 2);
  assert.equal(done.tools.filter(tool => tool.name === 'command').length, 1);
});

for (const change of ['unchanged', 'external', 'other-thread', 'ignored']) test(`reopened postimage provenance: ${change}`, { timeout: 30000 }, async t => {
  const f = await setup(t, 'edit-command'); const thread = await f.create(); await f.submit(thread.id);
  for (const kind of ['edit', 'command']) {
    const pending = await until(() => f.watch(thread.id), value => value.pending_approval?.kind === kind);
    await f.approve(thread.id, pending.pending_approval!.id);
  }
  await until(() => f.watch(thread.id), value => value.status === 'idle');
  f.host.kill('SIGKILL'); await delay(400);
  if (change === 'external') await writeFile(join(f.repo, 'notes.txt'), 'after\nexternal user change\n');
  if (change === 'ignored') await writeFile(join(f.repo, '.gitignore'), 'notes.txt\n');
  await f.start('followup');
  const target = change === 'other-thread' ? await f.create('other-thread') : thread;
  await f.submit(target.id, 'followup-submit');
  if (change === 'unchanged') {
    const pending = await until(() => f.watch(target.id), value => !!value.pending_approval);
    await f.approve(target.id, pending.pending_approval!.id);
  }
  const done = await until(() => f.watch(target.id), value => value.status === 'idle');
  assert(done.tools.some(tool => tool.id === 'followup-1' && tool.status === (change === 'unchanged' ? 'done' : 'error')), JSON.stringify(done));
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), change === 'unchanged' ? 'followup\n' : change === 'external' ? 'after\nexternal user change\n' : 'after\n');
});
