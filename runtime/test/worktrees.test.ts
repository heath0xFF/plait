import assert from 'node:assert/strict';
import { chmod, mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';
import { validateWorktree, type Operation } from '../src/worktrees.ts';
import { acquireCheckout, acquireCommonMetadata } from '../src/safety.ts';
import { codex, claude } from './support/record-runtime.ts';
import { exec } from './support/safety.ts';

async function commit(root: string) {
  await writeFile(join(root, 'source.txt'), 'committed\n');
  await exec('/usr/bin/git', ['-C', root, 'add', '.']);
  await exec('/usr/bin/git', ['-C', root, '-c', 'user.name=Test', '-c', 'user.email=test@invalid', '-c', 'commit.gpgsign=false', 'commit', '-qm', 'base']);
}
async function done(runtime: { reply(input: unknown): Promise<any> }, id: string) {
  const deadline = Date.now() + 20_000;
  for (;;) {
    const reply = await runtime.reply({ op: 'worktree_status', request_id: id });
    assert.equal(reply.ok, true, JSON.stringify(reply));
    const operation = reply.result.operation as Operation;
    if (operation.state !== 'preparing' && operation.state !== 'removing') return operation;
    assert.ok(Date.now() < deadline, JSON.stringify(operation));
    await new Promise(resolve => setTimeout(resolve, 25));
  }
}
for (const harness of ['codex', 'claude'] as const) test(`worktree binds selected ${harness}, cold base, permissions and idempotent lost reply`, async t => {
  const h = await (harness === 'codex' ? codex(t) : claude(t));
  await chmod(h.store, 0o700); await commit(h.root);
  await mkdir(join(h.root, 'target')); await writeFile(join(h.root, 'target', 'artifact'), 'do not copy');
  await writeFile(join(h.root, 'source.txt'), 'dirty\n');
  const info: any = await h.runtime.reply({ op: 'worktree_info', checkout: h.root }); assert.equal(info.ok, true, JSON.stringify(info));
  const request = { op: 'create_worktree_thread', request_id: 'creation', parent: info.result.parent, harness, provider: harness, model_id: harness === 'codex' ? 'gpt-fake' : 'default', mode: harness === 'codex' ? 'ask' : 'auto', approval: 'ask', thinking: 'high' };
  const [first, duplicate]: any[] = await Promise.all([h.runtime.reply(request), h.runtime.reply(request)]);
  assert.equal(first.ok, true, JSON.stringify(first)); assert.equal(duplicate.ok, true, JSON.stringify(duplicate));
  assert.equal(first.result.operation.thread_id, duplicate.result.operation.thread_id);
  // Discard the first acknowledgement: status recovers the complete result.
  const ready = await done(h.runtime, 'creation'); assert.equal(ready.state, 'ready', JSON.stringify(ready));
  const tree = ready.worktree!;
  assert.equal(await readFile(join(tree.path, 'source.txt'), 'utf8'), 'committed\n');
  await assert.rejects(readFile(join(tree.path, 'target', 'artifact')), { code: 'ENOENT' });
  const snapshot = h.runtime.snapshot(ready.thread_id);
  assert.equal(snapshot.harness?.id, harness); assert.equal(snapshot.harness?.mode, request.mode); assert.equal(snapshot.model.model_id, request.model_id); assert.equal(snapshot.checkout, tree.path);
  assert.equal(tree.branch, `plait/${ready.thread_id.replaceAll('-', '').slice(0, 12)}`);
  assert.equal((await h.runtime.reply({ ...request, model_id: 'different' })).error?.code, 'invalid_request');
  assert.equal((await h.runtime.reply({ ...request, protocol_version: 6 })).error?.code, 'invalid_request');
  const replay: any = await h.runtime.reply(request); assert.equal(replay.result.operation.thread_id, ready.thread_id);
  assert.equal(h.runtime.store.db.prepare('SELECT COUNT(*) AS count FROM worktree_operations').get()?.count, 1);
});

test('worktree restart reconciles creation and window closure before acknowledgement', async t => {
  const h = await codex(t); await chmod(h.store, 0o700); await commit(h.root);
  const info: any = await h.runtime.reply({ op: 'worktree_info', checkout: h.root });
  const request = { op: 'create_worktree_thread', request_id: 'restart', parent: info.result.parent, harness: 'codex', provider: 'codex', model_id: 'gpt-fake', mode: 'ask', approval: 'ask' };
  await h.runtime.reply(request); const ready = await done(h.runtime, 'restart'); assert.equal(ready.state, 'ready', JSON.stringify(ready));
  // Crash after the helper finished but before the runtime persisted its reply.
  const creating = { ...ready, state: 'preparing', worktree: null };
  h.runtime.store.db.prepare('UPDATE worktree_operations SET result=? WHERE request_id=?').run(JSON.stringify(creating), 'restart');
  const journalPath = join(h.store, 'worktrees', `${ready.thread_id}.json`);
  const journal = JSON.parse(await readFile(journalPath, 'utf8')); journal.stage = 'creating';
  await writeFile(journalPath, JSON.stringify(journal));
  await h.close(); const reopened = h.reopen();
  const recovered = await done(reopened.runtime, 'restart'); assert.equal(recovered.state, 'ready', JSON.stringify(recovered));
  assert.equal(recovered.thread_id, ready.thread_id); assert.equal(recovered.worktree!.path, ready.worktree!.path);
  const listed: any = await reopened.runtime.reply({ op: 'list_worktrees' }); assert.equal(listed.result.operations[0].thread_id, ready.thread_id);
  assert.equal(reopened.runtime.store.db.prepare('SELECT COUNT(*) AS count FROM threads').get()?.count, 1);
});

test('host rejects forged parent, missing filesystem and imported deletion authority', async t => {
  const h = await codex(t); await chmod(h.store, 0o700); await commit(h.root);
  const info: any = await h.runtime.reply({ op: 'worktree_info', checkout: h.root });
  const request = { op: 'create_worktree_thread', request_id: 'forged', parent: { ...info.result.parent, main: join(h.root, 'missing') }, harness: 'codex', provider: 'codex', model_id: 'gpt-fake', approval: 'ask' };
  assert.equal((await h.runtime.reply(request)).ok, true);
  assert.equal((await done(h.runtime, 'forged')).state, 'failed');
  assert.equal((await h.runtime.reply({ op: 'worktree_info', checkout: join(h.root, 'missing') })).ok, false);
  assert.equal((await h.runtime.reply({ op: 'remove_worktree', request_id: 'remove', creation_id: 'desktop-import', identity: { dev: '1', ino: '2' }, allow_unpushed: true })).error?.code, 'not_found');
  assert.equal(h.runtime.store.db.prepare('SELECT COUNT(*) AS count FROM threads').get()?.count, 0);
});

test('host removal refuses uncommitted, unpushed and stale busy summaries and keeps history', async t => {
  const h = await codex(t); await chmod(h.store, 0o700); await commit(h.root);
  const info: any = await h.runtime.reply({ op: 'worktree_info', checkout: h.root });
  await h.runtime.reply({ op: 'create_worktree_thread', request_id: 'create', parent: info.result.parent, harness: 'codex', provider: 'codex', model_id: 'gpt-fake', approval: 'ask' });
  const ready = await done(h.runtime, 'create'); assert.equal(ready.state, 'ready', JSON.stringify(ready));
  const tree = ready.worktree!;
  const remove = async (id: string, allow = false) => {
    const result = await h.runtime.reply({ op: 'remove_worktree', request_id: id, creation_id: 'create', identity: tree.identity, allow_unpushed: allow });
    assert.equal(result.ok, true, JSON.stringify(result)); return done(h.runtime, id);
  };
  assert.match((await remove('unpushed')).error!.message, /unpushed|publication/);
  await writeFile(join(tree.path, 'source.txt'), 'preserve');
  assert.match((await remove('dirty', true)).error!.message, /uncommitted/);
  await writeFile(join(tree.path, 'source.txt'), 'committed\n');
  const lease = await acquireCheckout(tree.path);
  try { assert.match((await remove('stale-summary', true)).error!.message, /in use|busy/); } finally { await lease.close(); }
  const removed = await remove('remove', true); assert.equal(removed.state, 'removed', JSON.stringify(removed));
  assert.equal((await remove('remove', true)).state, 'removed');
  assert.equal(h.runtime.snapshot(ready.thread_id).checkout, tree.path);
  await exec('/usr/bin/git', ['-C', h.root, 'show-ref', '--verify', `refs/heads/${tree.branch}`]);
});

test('worktree operation validation rejects unknown fields, malformed identities and lengths', () => {
  const identity = { dev: '1', ino: '2' };
  const request = { op: 'create_worktree_thread', request_id: 'create', parent: { root: '/repo', main: '/repo', common: '/repo/.git', identity, main_identity: identity, common_identity: identity, base: 'a'.repeat(40) }, harness: 'codex', provider: 'codex', model_id: 'model', approval: 'ask' };
  assert.deepEqual(validateWorktree(request), request);
  for (const bad of [{ ...request, protocol_version: 6 }, { ...request, unknown: true }, { ...request, parent: { ...request.parent, base: 'HEAD' } }, { ...request, request_id: 'x'.repeat(129) }, { ...request, parent: { ...request.parent, root: '/a/../b' } }, { ...request, parent: { ...request.parent, identity: { dev: '1', ino: '18446744073709551616' } } }, { ...request, harness: 'unknown' }, { ...request, model_id: 'm'.repeat(257) }]) assert.throws(() => validateWorktree(bad));
});

test('real host loss during checkout resumes the admitted UUID without repeating hooks', { timeout: 40_000 }, async t => {
  const { setup, request, childPids, until } = await import('./support/host.ts');
  const f = await setup(t);
  const calls = join(f.base, 'hook-calls');
  const hook = join(f.repo, '.git', 'hooks', 'post-checkout');
  await writeFile(hook, `#!/bin/sh\nprintf x >> '${calls}'\nsleep 1\n`); await chmod(hook, 0o755);
  const info = await request(f.store, { op: 'worktree_info', checkout: f.repo }) as any;
  const create = { op: 'create_worktree_thread', request_id: 'lost-client', parent: info.result.parent, harness: 'pi', provider: 'faux', model_id: 'faux-1', approval: 'ask' };
  const accepted = await request(f.store, create) as any; assert.equal(accepted.ok, true, JSON.stringify(accepted));
  await until(() => readFile(calls, 'utf8'), value => value === 'x');
  const [node] = await childPids(f.host.pid!); process.kill(node, 'SIGKILL');
  await until(async () => f.host.exitCode !== null || f.host.signalCode !== null, value => value);
  await f.start();
  const recovered = await until(async () => (await request(f.store, { op: 'worktree_status', request_id: 'lost-client' }) as any).result.operation, value => value.state !== 'preparing');
  assert.equal(recovered.state, 'ready', JSON.stringify(recovered));
  assert.equal(recovered.thread_id, accepted.result.operation.thread_id);
  assert.equal(await readFile(calls, 'utf8'), 'x');
  const retried = await request(f.store, create) as any;
  assert.equal(retried.result.operation.thread_id, recovered.thread_id);
});

test('worktree supervisor reclaims a background checkout hook before acknowledging', async t => {
  const h = await codex(t); await chmod(h.store, 0o700); await commit(h.root);
  const later = join(h.store, 'late-hook-effect');
  const hook = join(h.root, '.git', 'hooks', 'post-checkout');
  await writeFile(hook, `#!/bin/sh\n(sleep 1; printf unexpected > '${later}') >/dev/null 2>&1 &\nexit 0\n`); await chmod(hook, 0o755);
  const info: any = await h.runtime.reply({ op: 'worktree_info', checkout: h.root });
  await h.runtime.reply({ op: 'create_worktree_thread', request_id: 'hook', parent: info.result.parent, harness: 'codex', provider: 'codex', model_id: 'gpt-fake', approval: 'ask' });
  const ready = await done(h.runtime, 'hook'); assert.equal(ready.state, 'ready', JSON.stringify(ready));
  await new Promise(resolve => setTimeout(resolve, 1200));
  await assert.rejects(readFile(later), { code: 'ENOENT' });
});

test('killed worktree worker leaves checkout and common leases with Git descendants', { timeout: 30_000 }, async t => {
  const { spawn } = await import('node:child_process');
  const { binary, childPids, until } = await import('./support/host.ts');
  const h = await codex(t); await chmod(h.store, 0o700); await commit(h.root);
  const marker = join(h.store, 'hook-started');
  const hook = join(h.root, '.git', 'hooks', 'post-checkout');
  await writeFile(hook, `#!/bin/sh\nprintf started > '${marker}'\nsleep 20\n`); await chmod(hook, 0o755);
  const info: any = await h.runtime.reply({ op: 'worktree_info', checkout: h.root });
  const id = '12345678-1234-4abc-8abc-123456789abc';
  const helper = spawn(binary, ['worktree'], { stdio: ['pipe', 'ignore', 'ignore'] });
  helper.stdin!.end(JSON.stringify({ action: 'create', store: h.store, thread_id: id, parent: info.result.parent }));
  let gitGroup: number | undefined;
  try {
    await until(() => readFile(marker, 'utf8'), value => value === 'started');
    const [worker] = await childPids(helper.pid!);
    const [supervisor] = await childPids(worker);
    [gitGroup] = await childPids(supervisor);
    assert.ok(gitGroup, 'nested Git worker has its own process group');
    // Freeze every Git descendant so its lifetime-pipe cleanup cannot hide an early unlock.
    process.kill(-gitGroup, 'SIGSTOP');
    process.kill(worker, 'SIGKILL');
    await until(async () => helper.exitCode !== null || helper.signalCode !== null, Boolean);
    const journal = JSON.parse(await readFile(join(h.store, 'worktrees', `${id}.json`), 'utf8'));
    await assert.rejects(async () => {
      const lease = await acquireCheckout(journal.path); await lease.close();
    }, /in use|busy/);
    await assert.rejects(async () => {
      const lease = await acquireCommonMetadata(h.root, 0); await lease.close();
    }, /busy|metadata/);
    process.kill(-gitGroup, 'SIGCONT');
    await until(async () => {
      const lease = await acquireCheckout(journal.path);
      await lease.close();
      const common = await acquireCommonMetadata(h.root, 0);
      await common.close();
      return true;
    }, Boolean);
  } finally {
    if (gitGroup) { try { process.kill(-gitGroup, 'SIGKILL'); } catch { /* Already reaped. */ } }
    if (helper.exitCode === null && helper.signalCode === null) helper.kill('SIGKILL');
  }
});
