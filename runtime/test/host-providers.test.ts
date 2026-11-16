import assert from 'node:assert/strict';
import { connect } from 'node:net';
import { readFile, rm, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import type { ProviderView } from '../src/providers.ts';
import type { SignInView } from '../src/sign-in.ts';
import { LOOPBACK_URL, SECRETS } from './support/providers.ts';

// The real host binary with a sidecar whose providers and sign-in flows are scripted.
process.env.PLAIT_TEST_RUNTIME = resolve('test/providers-fixture.ts');
const { request, setup, until } = await import('./support/host.ts');

type Catalog = { models: { provider: string; model_id: string; authenticated: boolean }[] };
const ask = async <T>(store: string, value: unknown) => {
  const reply = await request(store, value);
  return { ok: reply.ok, error: reply.error, result: reply.result as unknown as T };
};
const open = (port: number) => new Promise<boolean>(done => {
  const socket = connect(port, '127.0.0.1');
  socket.once('connect', () => { socket.destroy(); done(true); });
  socket.once('error', () => done(false));
});

test('a reload during an active run changes the catalog and leaves the run alone', { timeout: 60000 }, async t => {
  const f = await setup(t);
  const thread = await f.create();
  assert.equal((await f.submit(thread.id)).ok, true);
  const waiting = await until(() => f.watch(thread.id), snapshot => snapshot.status === 'awaiting_approval');

  // The next registry has a new provider and no longer has the one this run is using.
  await writeFile(join(f.store, 'drop-faux'), '');
  await writeFile(join(f.store, 'late-provider'), '');
  const reloaded = await ask<{ providers: ProviderView[] }>(f.store, { op: 'reload_providers' });
  assert.equal(reloaded.ok, true, JSON.stringify(reloaded.error));
  const ids = reloaded.result.providers.map(provider => provider.id);
  assert(ids.includes('late') && !ids.includes('faux'));
  const catalog = (await ask<Catalog>(f.store, { op: 'status' })).result.models;
  assert.deepEqual(catalog.filter(model => model.provider === 'late'), [{ provider: 'late', model_id: 'late-1', name: 'late-1', authenticated: true }]);
  assert(!catalog.some(model => model.provider === 'faux'));

  // The run is still waiting on the same approval and finishes on the registry it started with.
  const still = await f.watch(thread.id);
  assert.deepEqual([still.status, still.pending_approval?.id], ['awaiting_approval', waiting.pending_approval!.id]);
  assert.equal((await f.approve(thread.id, waiting.pending_approval!.id)).ok, true);
  const done = await until(() => f.watch(thread.id), snapshot => snapshot.status === 'idle' || snapshot.status === 'error');
  assert.deepEqual([done.status, done.error], ['idle', null]);
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'after\n');
  assert.equal(done.messages.at(-1)?.text, 'Scripted reply after the provider reload.');

  // New work sees the new registry, and a second reload brings the provider back without a restart.
  assert.equal((await f.submit(thread.id, 'submit-2')).error?.code, 'model_unavailable');
  await rm(join(f.store, 'drop-faux'));
  assert.equal((await ask(f.store, { op: 'reload_providers' })).ok, true);
  assert.equal((await f.submit(thread.id, 'submit-3')).ok, true);
  const again = await until(() => f.watch(thread.id), snapshot => snapshot.status === 'idle' && snapshot.messages.at(-1)?.role === 'assistant');
  assert.equal(again.error, null);
});

test('provider operations cross the socket, and a pending sign-in is cleaned up at shutdown', { timeout: 60000 }, async t => {
  const f = await setup(t);
  const list = await ask<{ providers: ProviderView[] }>(f.store, { op: 'list_providers' });
  assert.deepEqual(list.result.providers.find(provider => provider.id === 'subscription'), {
    id: 'subscription', name: 'Scripted Subscription', kind: 'builtin', authenticated: false, credential: 'none', model_count: 1,
    sign_in: { name: 'Scripted (subscription)', subscription: true }, api_key: null, endpoint: null,
  });
  assert.equal((await ask(f.store, { op: 'list_providers', extra: 1 })).error?.code, 'invalid_request');
  // An array field passes the transport; this scripted host simply has no models.json to edit.
  const add = await ask(f.store, { op: 'add_endpoint', id: 'lab', name: 'Lab', base_url: 'http://127.0.0.1:1/v1', model_ids: ['a', 'b'] });
  assert.match(add.error!.message, /cannot edit custom endpoints/);

  const key = await ask<{ provider: ProviderView }>(f.store, { op: 'set_provider_key', provider: 'keyed', key: 'sk-SOCKET_KEY_DO_NOT_LEAK' });
  assert.deepEqual([key.result.provider.authenticated, key.result.provider.credential], [true, 'api_key']);
  assert(!JSON.stringify(key).includes('SOCKET_KEY'));
  const status = (await ask<Catalog>(f.store, { op: 'status' })).result.models;
  assert(status.filter(model => model.provider === 'keyed').every(model => model.authenticated));

  // A complete sign-in over the socket.
  const both = (await ask<{ sign_in: SignInView }>(f.store, { op: 'start_sign_in', provider: 'both', method: 'copy_code' })).result.sign_in;
  assert.deepEqual([both.state, both.input?.kind, both.input?.required], ['waiting', 'code', true]);
  const signed = await ask<{ sign_in: SignInView }>(f.store, { op: 'answer_sign_in', sign_in_id: both.id, input: SECRETS.code });
  assert.equal(signed.result.sign_in.state, 'signed_in');
  assert(!JSON.stringify(signed).includes(SECRETS.access) && !JSON.stringify(signed).includes(SECRETS.code));
  assert((await ask<Catalog>(f.store, { op: 'status' })).result.models.some(model => model.provider === 'both' && model.authenticated));

  const flow = (await ask<{ sign_in: SignInView }>(f.store, { op: 'start_sign_in', provider: 'subscription', method: 'browser' })).result.sign_in;
  assert.deepEqual([flow.state, flow.url, flow.input?.required], ['waiting', LOOPBACK_URL, false]);
  const port = Number(await readFile(join(f.store, 'sign-in-listening'), 'utf8'));
  assert.equal(await open(port), true);

  // Stopping the host ends the flow and closes its loopback listener.
  f.host.kill('SIGTERM');
  await until(async () => existsSync(join(f.store, 'sidecar-closed')), Boolean);
  assert(existsSync(join(f.store, 'sign-in-released')));
  assert.equal(await open(port), false);
});
