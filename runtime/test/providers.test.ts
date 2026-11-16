import assert from 'node:assert/strict';
import { test } from 'node:test';
import { literalKey } from '../src/providers.ts';
import type { ProviderView } from '../src/providers.ts';
import type { SignInView } from '../src/sign-in.ts';
import { COPY_URL, DEVICE_URL, LOOPBACK_URL, SECRETS, scripted, scriptedProvider } from './support/providers.ts';

// Scripted providers only: no Pi files, no network, and no real sign-in.
const delay = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));
type List = { providers: ProviderView[] };
type Flow = { sign_in: SignInView };
const KEY = 'sk-KEY_ONE_DO_NOT_LEAK';
const NEXT_KEY = 'sk-KEY_TWO_DO_NOT_LEAK';

function assertNoSecrets(transcript: string[], extra: string[] = []) {
  const joined = transcript.join('\n');
  for (const secret of [...Object.values(SECRETS), KEY, NEXT_KEY, ...extra]) assert(!joined.includes(secret), `a reply contains ${secret}`);
}

test('the provider list reports how each provider authenticates, without secrets', async () => {
  const f = await scripted();
  const list = await f.call<List>({ op: 'list_providers' });
  const byId = Object.fromEntries(list.result.providers.map(provider => [provider.id, provider]));
  assert.deepEqual(byId.subscription, {
    id: 'subscription', name: 'Scripted Subscription', kind: 'builtin', authenticated: false, credential: 'none', model_count: 1,
    sign_in: { name: 'Scripted (subscription)', subscription: true }, api_key: null, endpoint: null,
  });
  assert.deepEqual(byId.keyed, {
    id: 'keyed', name: 'Scripted Keyed', kind: 'builtin', authenticated: false, credential: 'none', model_count: 2,
    sign_in: null, api_key: { name: 'Scripted API key' }, endpoint: null,
  });
  assert.deepEqual([byId.both.sign_in !== null, byId.both.api_key !== null], [true, true]);
  // Usable without a stored credential, and with no key this host can set.
  assert.deepEqual([byId.ambient.authenticated, byId.ambient.credential, byId.ambient.api_key], [true, 'external', null]);
  assert.equal((await f.call({ op: 'list_providers', protocol_version: 2 })).ok, true);
  assertNoSecrets(f.transcript);
});

test('provider operations reject unknown fields, wrong types, and oversized values', async () => {
  const f = await scripted();
  for (const input of [
    { op: 'list_providers', extra: true },
    { op: 'list_providers', protocol_version: 99 },
    { op: 'start_sign_in' },
    { op: 'start_sign_in', provider: 7 },
    { op: 'start_sign_in', provider: 'subscription', method: '' },
    { op: 'sign_in_status', sign_in_id: 'not valid!' },
    { op: 'answer_sign_in', sign_in_id: 'abc' },
    { op: 'set_provider_key', provider: 'keyed', key: '' },
    { op: 'set_provider_key', provider: 'keyed', key: `${KEY}\u0000tail` },
    { op: 'set_provider_key', provider: 'keyed', key: KEY.repeat(400) },
    { op: 'set_provider_key', provider: 'keyed', key: KEY, echo: true },
    { op: 'add_endpoint', id: 'lab', name: 'Lab', base_url: 'http://127.0.0.1:1/v1', model_ids: 'one' },
    { op: 'add_endpoint', id: 'lab', name: 'Lab', base_url: 'http://127.0.0.1:1/v1', model_ids: ['a', 'a'] },
    { op: 'add_endpoint', id: 'Lab Server', name: 'Lab', base_url: 'http://127.0.0.1:1/v1', model_ids: ['a'] },
    { op: 'add_endpoint', id: 'lab', name: 'Lab', base_url: `http://user:${KEY}@127.0.0.1:1/v1`, model_ids: ['a'] },
    { op: 'add_endpoint', id: 'lab', name: 'Lab', base_url: `http://127.0.0.1:1/v1?key=${KEY}`, model_ids: ['a'] },
    { op: 'add_endpoint', id: 'lab', name: 'Lab', base_url: 'file:///etc/passwd', model_ids: ['a'] },
    { op: 'check_endpoint' },
    { op: 'check_endpoint', id: 'lab', base_url: 'http://127.0.0.1:1/v1' },
    { op: 'check_endpoint', id: 'lab', key: KEY },
  ]) {
    assert.equal(f.providers.handles(input), true);
    const reply = await f.call(input);
    assert.deepEqual([reply.ok, reply.error?.code], [false, 'invalid_request'], JSON.stringify(input).slice(0, 120));
  }
  assert.equal(f.providers.handles({ op: 'status' }), false);
  assert.equal(f.providers.handles({ op: 'toString' }), false);
  assert.equal(f.providers.handles(['list_providers']), false);
  assert.equal((await f.call({ op: 'start_sign_in', provider: 'missing' })).error?.code, 'not_found');
  assert.equal((await f.call({ op: 'start_sign_in', provider: 'keyed' })).error?.code, 'invalid_request');
  assert.equal((await f.call({ op: 'set_provider_key', provider: 'subscription', key: KEY })).error?.code, 'invalid_request');
  // This scripted host has no models.json; the request still passes validation with an array field.
  const add = await f.call({ op: 'add_endpoint', id: 'lab', name: 'Lab', base_url: 'http://127.0.0.1:1/v1', model_ids: ['a'] });
  assert.match(add.error!.message, /cannot edit custom endpoints/);
  assertNoSecrets(f.transcript);
});

test('a browser sign-in shows an address, takes an optional pasted code, and reloads on success', async () => {
  const f = await scripted();
  const before = f.holder.models;
  const started = await f.call<Flow>({ op: 'start_sign_in', provider: 'subscription', method: 'browser' });
  assert.equal(started.ok, true, JSON.stringify(started));
  const flow = started.result.sign_in;
  assert.match(flow.id, /^[0-9a-f-]{36}$/);
  assert.deepEqual({ ...flow, id: '', expires_in_seconds: 0 }, {
    id: '', provider: 'subscription', state: 'waiting', url: LOOPBACK_URL, user_code: null,
    // The address returns to this machine, so the pasted code is only a fallback.
    input: { kind: 'code', message: 'Paste the authorization code:', placeholder: 'code#state', required: false, choices: [] },
    message: 'Complete login in your browser.', expires_in_seconds: 0, error: null,
  });
  assert(flow.expires_in_seconds > 590 && flow.expires_in_seconds <= 600);
  assert.deepEqual((await f.call<Flow>({ op: 'sign_in_status', sign_in_id: flow.id })).result.sign_in, { ...flow, expires_in_seconds: flow.expires_in_seconds });
  assert.equal((await f.call({ op: 'answer_sign_in', sign_in_id: flow.id, input: '  ' })).error?.code, 'invalid_request');

  const answered = await f.call<Flow>({ op: 'answer_sign_in', sign_in_id: flow.id, input: ` ${SECRETS.code}\n` });
  assert.deepEqual(answered.result.sign_in, {
    id: flow.id, provider: 'subscription', state: 'signed_in', url: null, user_code: null, input: null, message: null, expires_in_seconds: 0, error: null,
  });
  // Success is reported only after the swap, so the catalog already agrees.
  assert.notEqual(f.holder.models, before);
  assert(f.holder.authenticated.has('subscription'));
  const listed = (await f.call<List>({ op: 'list_providers' })).result.providers.find(provider => provider.id === 'subscription')!;
  assert.deepEqual([listed.authenticated, listed.credential], [true, 'sign_in']);
  assert.equal((await f.credentials.read('subscription'))?.type, 'oauth');
  assert.equal((await f.call({ op: 'answer_sign_in', sign_in_id: flow.id, input: 'again' })).error?.code, 'conflict');
  assert.equal((await f.call<Flow>({ op: 'cancel_sign_in', sign_in_id: flow.id })).result.sign_in.state, 'signed_in');

  const out = await f.call<{ provider: ProviderView }>({ op: 'sign_out_provider', provider: 'subscription' });
  assert.deepEqual([out.result.provider.authenticated, out.result.provider.credential], [false, 'none']);
  assert.equal(await f.credentials.read('subscription'), undefined);
  assert(!f.holder.authenticated.has('subscription'));
  assertNoSecrets(f.transcript);
});

test('a sign-in relays the method choice, a required copied code, and a device code', async () => {
  const f = await scripted();
  // Without a method the flow's own question is passed on.
  const asked = (await f.call<Flow>({ op: 'start_sign_in', provider: 'subscription' })).result.sign_in;
  assert.deepEqual([asked.state, asked.url, asked.input?.kind, asked.input?.required], ['waiting', null, 'choice', true]);
  assert.deepEqual(asked.input?.choices.map(choice => choice.id), ['browser', 'copy_code', 'device_code', 'broken']);
  assert.equal((await f.call({ op: 'answer_sign_in', sign_in_id: asked.id, input: 'telepathy' })).error?.code, 'invalid_request');
  const copy = (await f.call<Flow>({ op: 'answer_sign_in', sign_in_id: asked.id, input: 'copy_code' })).result.sign_in;
  // The address does not return to this machine, so the code must be pasted.
  assert.deepEqual([copy.state, copy.url, copy.input?.kind, copy.input?.required], ['waiting', COPY_URL, 'code', true]);

  // A wrong code fails with fixed text; the scripted provider error body is not forwarded.
  const wrong = (await f.call<Flow>({ op: 'answer_sign_in', sign_in_id: asked.id, input: 'wrong-code-DO_NOT_ECHO' })).result.sign_in;
  assert.deepEqual([wrong.state, wrong.url, wrong.error?.code], ['failed', null, 'permission_denied']);
  assert.equal(await f.credentials.read('subscription'), undefined);

  const device = (await f.call<Flow>({ op: 'start_sign_in', provider: 'both', method: 'device_code' })).result.sign_in;
  assert.deepEqual([device.state, device.url, device.user_code, device.input], ['waiting', DEVICE_URL, 'ABCD-1234', null]);
  f.flows.both.approve!();
  const done = async () => (await f.call<Flow>({ op: 'sign_in_status', sign_in_id: device.id })).result.sign_in;
  while ((await done()).state !== 'signed_in') await delay(5);
  assert(f.holder.authenticated.has('both'));

  const broken = (await f.call<Flow>({ op: 'start_sign_in', provider: 'second', method: 'broken' })).result.sign_in;
  assert.deepEqual([broken.state, broken.error?.code], ['failed', 'permission_denied']);
  assertNoSecrets(f.transcript, ['wrong-code-DO_NOT_ECHO']);
});

test('a sign-in can be cancelled, replaced, and expires on its own', async () => {
  const f = await scripted({ ttlMs: 120 });
  const first = (await f.call<Flow>({ op: 'start_sign_in', provider: 'subscription', method: 'browser' })).result.sign_in;
  const cancelled = (await f.call<Flow>({ op: 'cancel_sign_in', sign_in_id: first.id })).result.sign_in;
  assert.deepEqual([cancelled.state, cancelled.url, cancelled.input, cancelled.error, cancelled.expires_in_seconds], ['cancelled', null, null, null, 0]);
  assert.equal((await f.call<Flow>({ op: 'cancel_sign_in', sign_in_id: first.id })).result.sign_in.state, 'cancelled');
  assert.equal((await f.call({ op: 'answer_sign_in', sign_in_id: first.id, input: SECRETS.code })).error?.code, 'conflict');
  assert.equal(f.flows.subscription.aborted, 1);

  // Starting again for the same provider stops the earlier attempt.
  const second = (await f.call<Flow>({ op: 'start_sign_in', provider: 'subscription', method: 'browser' })).result.sign_in;
  const third = (await f.call<Flow>({ op: 'start_sign_in', provider: 'subscription', method: 'browser' })).result.sign_in;
  assert.equal((await f.call<Flow>({ op: 'sign_in_status', sign_in_id: second.id })).result.sign_in.state, 'cancelled');
  assert.equal(third.state, 'waiting');
  assert.equal(f.flows.subscription.aborted, 2);

  await delay(200);
  const expired = (await f.call<Flow>({ op: 'sign_in_status', sign_in_id: third.id })).result.sign_in;
  assert.deepEqual([expired.state, expired.url, expired.input, expired.expires_in_seconds], ['expired', null, null, 0]);
  assert.equal(f.flows.subscription.aborted, 3);
  assert.equal(await f.credentials.read('subscription'), undefined);
  assert.equal((await f.call({ op: 'sign_in_status', sign_in_id: '00000000-0000-4000-8000-000000000000' })).error?.code, 'not_found');
  assertNoSecrets(f.transcript);
});

test('pending sign-ins are bounded, finished ones are forgotten, and close stops the rest', async () => {
  const f = await scripted({ pending: 2, records: 3, retainMs: 60 });
  const one = (await f.call<Flow>({ op: 'start_sign_in', provider: 'subscription', method: 'browser' })).result.sign_in;
  const two = (await f.call<Flow>({ op: 'start_sign_in', provider: 'second', method: 'browser' })).result.sign_in;
  assert.equal((await f.call({ op: 'start_sign_in', provider: 'third', method: 'browser' })).error?.code, 'busy');
  await f.call({ op: 'cancel_sign_in', sign_in_id: one.id });
  const three = (await f.call<Flow>({ op: 'start_sign_in', provider: 'third', method: 'browser' })).result.sign_in;
  await f.call({ op: 'cancel_sign_in', sign_in_id: three.id });
  // Three records are kept; the next start drops the oldest finished one.
  const four = (await f.call<Flow>({ op: 'start_sign_in', provider: 'fourth', method: 'browser' })).result.sign_in;
  assert.equal((await f.call({ op: 'sign_in_status', sign_in_id: one.id })).error?.code, 'not_found');
  assert.equal((await f.call<Flow>({ op: 'sign_in_status', sign_in_id: three.id })).result.sign_in.state, 'cancelled');
  await delay(90);
  assert.equal((await f.call({ op: 'sign_in_status', sign_in_id: three.id })).error?.code, 'not_found');

  await f.providers.close();
  for (const id of [two.id, four.id]) assert.equal((await f.call<Flow>({ op: 'sign_in_status', sign_in_id: id })).result.sign_in.state, 'cancelled');
  assert.deepEqual([f.flows.second.aborted, f.flows.fourth.aborted], [1, 1]);
});

test('an API key is stored once, replaced, removed, and never returned', async () => {
  const f = await scripted();
  const set = await f.call<{ provider: ProviderView }>({ op: 'set_provider_key', provider: 'keyed', key: `  ${KEY}\n` });
  assert.deepEqual(set.result, { provider: {
    id: 'keyed', name: 'Scripted Keyed', kind: 'builtin', authenticated: true, credential: 'api_key', model_count: 2,
    sign_in: null, api_key: { name: 'Scripted API key' }, endpoint: null,
  } });
  assert.deepEqual(await f.credentials.read('keyed'), { type: 'api_key', key: KEY });
  assert(f.holder.authenticated.has('keyed'));

  await f.call({ op: 'set_provider_key', provider: 'keyed', key: NEXT_KEY });
  assert.deepEqual(await f.credentials.read('keyed'), { type: 'api_key', key: NEXT_KEY });

  // One credential per provider: a key replaces a subscription sign-in and stops a pending one.
  const flow = (await f.call<Flow>({ op: 'start_sign_in', provider: 'both', method: 'browser' })).result.sign_in;
  await f.call({ op: 'set_provider_key', provider: 'both', key: KEY });
  assert.equal((await f.call<Flow>({ op: 'sign_in_status', sign_in_id: flow.id })).result.sign_in.state, 'cancelled');
  assert.equal((await f.credentials.read('both'))?.type, 'api_key');

  const removed = await f.call<{ provider: ProviderView }>({ op: 'sign_out_provider', provider: 'keyed' });
  assert.deepEqual([removed.result.provider.authenticated, removed.result.provider.credential], [false, 'none']);
  assert.equal(await f.credentials.read('keyed'), undefined);
  // Signing out twice is not an error.
  assert.equal((await f.call({ op: 'sign_out_provider', provider: 'keyed' })).ok, true);
  assert.equal((await f.call({ op: 'sign_out_provider', provider: 'missing' })).error?.code, 'not_found');
  assertNoSecrets(f.transcript);
});

test('a key from the socket is stored as a literal, never as a Pi command or variable', () => {
  assert.equal(literalKey('sk-plain'), 'sk-plain');
  assert.equal(literalKey('!rm -rf x'), '$!rm -rf x');
  assert.equal(literalKey('$HOME'), '$$HOME');
  assert.equal(literalKey('a$b!c'), 'a$$b!c');
});

test('reload swaps a complete registry at once and keeps the old one when loading fails', async () => {
  let extra = ['late-a'];
  const f = await scripted({}, () => [scriptedProvider('late', extra)]);
  const before = f.holder.models;
  const authenticated = f.holder.authenticated;
  extra = ['late-a', 'late-b'];
  // Nothing changes until the new registry is complete.
  f.state.delay = 40;
  const reload = f.call<List>({ op: 'reload_providers' });
  await delay(10);
  assert.equal(f.holder.models, before);
  assert.equal(f.holder.authenticated, authenticated);
  const reloaded = await reload;
  assert.equal(reloaded.result.providers.find(provider => provider.id === 'late')?.model_count, 2);
  assert.notEqual(f.holder.models, before);
  assert.notEqual(f.holder.authenticated, authenticated);
  assert.equal(f.holder.models.getModels('late').length, 2);
  // The registry a run captured earlier is untouched.
  assert.equal(before.getModels('late').length, 1);

  // Reloads requested together finish in order, so the last request decides the result.
  const current = f.holder.models;
  extra = ['late-a'];
  const first = f.providers.reload();
  await delay(5);
  extra = ['late-a', 'late-b', 'late-c']; f.state.delay = 0;
  await Promise.all([first, f.providers.reload()]);
  assert.equal(f.holder.models.getModels('late').length, 3);

  f.state.delay = 0; f.state.fail = true;
  const kept = f.holder.models;
  const failed = await f.call({ op: 'reload_providers' });
  assert.deepEqual([failed.ok, failed.error?.code], [false, 'internal']);
  assert.equal(f.holder.models, kept);
  assert.notEqual(kept, current);
  assertNoSecrets(f.transcript);
});
