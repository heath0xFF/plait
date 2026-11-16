import assert from 'node:assert/strict';
import { mkdir, rename } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';
import { commandPolicy, parseCommandPolicy, setCommandPolicy } from '../src/command/policy.ts';
import { validate } from '../src/protocol.ts';
import { delay, exec, request, setup } from './support/host.ts';

// No command runs here: these tests cover only where the policy is stored and who it applies to.
type Fixture = Awaited<ReturnType<typeof setup>>;
type PolicyReply = { ok: boolean; result: { checkout: string; policy: string }; error?: { code: string; message: string } };
const send = (store: string, value: unknown) => request(store, value) as unknown as Promise<PolicyReply>;
const read = (f: Fixture, checkout = f.repo) => send(f.store, { op: 'command_policy', checkout });
const set = (f: Fixture, policy: string, checkout = f.repo) => send(f.store, { op: 'set_command_policy', checkout, policy });

test('policy records fail closed', () => {
  const values = new Map<string, string>();
  const settings = {
    setting: (key: string) => values.get(key) ?? null,
    setSetting: (key: string, value: string | null) => { if (value === null) values.delete(key); else values.set(key, value); },
  };
  const root = { dev: '1', ino: '2' };
  assert.equal(commandPolicy(settings, '/repo', root), 'sandboxed');
  setCommandPolicy(settings, '/repo', root, 'trusted');
  assert.equal(commandPolicy(settings, '/repo', root), 'trusted');
  assert.equal(commandPolicy(settings, '/repo/nested', root), 'sandboxed');
  assert.equal(commandPolicy(settings, '/repo', { dev: '1', ino: '3' }), 'sandboxed');
  for (const corrupt of ['trusted', '"trusted"', 'null', '{"policy":"trusted"}', '{"policy":"TRUSTED","root":{"dev":"1","ino":"2"}}', '{']) {
    values.set('command_policy:/repo', corrupt);
    assert.equal(commandPolicy(settings, '/repo', root), 'sandboxed', corrupt);
  }
  setCommandPolicy(settings, '/repo', root, 'trusted');
  setCommandPolicy(settings, '/repo', root, 'sandboxed');
  assert.equal(values.size, 0);
  assert.equal(parseCommandPolicy('trusted'), 'trusted');
  for (const value of ['', 'Trusted', 'auto', true, undefined]) assert.throws(() => parseCommandPolicy(value), { code: 'invalid_request' });
});

test('policy operations accept only their own fields', () => {
  validate({ op: 'command_policy', checkout: '/repo' });
  validate({ op: 'set_command_policy', checkout: '/repo', policy: 'trusted', protocol_version: 2 });
  for (const input of [
    { op: 'command_policy' },
    { op: 'command_policy', checkout: '/repo', policy: 'trusted' },
    { op: 'set_command_policy', checkout: '/repo' },
    { op: 'set_command_policy', checkout: '/repo', policy: true },
    { op: 'set_command_policy', checkout: '/repo', policy: 'trusted', thread_id: 't' },
    { op: 'submit', thread_id: 't', request_id: 'r', text: 'hi', policy: 'trusted' },
    { op: 'create_thread', checkout: '/repo', provider: 'p', model_id: 'm', request_id: 'r', policy: 'trusted' },
  ]) assert.throws(() => validate(input), { code: 'invalid_request' }, JSON.stringify(input));
});

test('the policy is per canonical checkout and per store, durable, and sandboxed by default', { timeout: 60000 }, async t => {
  const f = await setup(t);
  const elsewhere = await setup(t);
  const other = join(f.base, 'other'); await exec('/usr/bin/git', ['init', '-q', other]);
  const nested = join(f.repo, 'nested'); await mkdir(nested);
  assert.deepEqual(await read(f), { ok: true, result: { checkout: f.repo, policy: 'sandboxed' } });

  // Any path inside the working tree names its canonical checkout.
  assert.deepEqual(await set(f, 'trusted', nested), { ok: true, result: { checkout: f.repo, policy: 'trusted' } });
  assert.equal((await read(f)).result.policy, 'trusted');
  assert.deepEqual((await read(f, other)).result, { checkout: other, policy: 'sandboxed' });
  assert.equal((await send(elsewhere.store, { op: 'command_policy', checkout: f.repo })).result.policy, 'sandboxed');
  assert.equal((await read(elsewhere)).result.policy, 'sandboxed');

  for (const invalid of [
    { op: 'set_command_policy', checkout: f.repo, policy: 'open' },
    { op: 'set_command_policy', checkout: f.base, policy: 'trusted' },
    { op: 'set_command_policy', checkout: 'repo', policy: 'trusted' },
    { op: 'command_policy', checkout: join(f.base, 'missing') },
  ]) assert.equal((await send(f.store, invalid)).error?.code, 'invalid_request', JSON.stringify(invalid));
  assert.equal((await read(f)).result.policy, 'trusted');

  // Existing replies keep their shape; the policy travels only in its own operations.
  const status = await request(f.store, { op: 'status' });
  assert.deepEqual(Object.keys(status.result).sort(), ['default_model', 'models', 'protocol_version']);
  const thread = await f.create();
  assert(!JSON.stringify(thread).includes('policy'));

  f.host.kill('SIGKILL'); await delay(500); await f.start();
  assert.equal((await read(f)).result.policy, 'trusted');

  // Trust belongs to that directory, not to whatever later appears at its path.
  await rename(f.repo, join(f.base, 'moved'));
  await exec('/usr/bin/git', ['init', '-q', f.repo]);
  assert.deepEqual((await read(f)).result, { checkout: f.repo, policy: 'sandboxed' });
  assert.equal((await read(f, join(f.base, 'moved'))).result.policy, 'sandboxed');

  assert.equal((await set(f, 'trusted')).result.policy, 'trusted');
  assert.deepEqual((await set(f, 'sandboxed')).result, { checkout: f.repo, policy: 'sandboxed' });
  assert.equal((await read(f)).result.policy, 'sandboxed');
});
