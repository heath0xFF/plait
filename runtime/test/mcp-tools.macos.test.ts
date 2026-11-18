import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { TestContext } from 'node:test';
import { DatabaseSync } from 'node:sqlite';
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { createServer } from 'node:http';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { createRegistry } from '@earendil-works/pi-durable';
import type { ToolExecutionApi } from '@earendil-works/pi-durable';
import { loadMcp } from '../src/mcp/config.ts';
import { McpRun, eligible } from '../src/mcp/tools.ts';
import { MCP_START_TIMEOUT_MS, MCP_CALL_TIMEOUT_MS } from '../src/mcp/transport.ts';
import { RunAuthorization } from '../src/authorization.ts';
import { automaticApproval, InteractiveApproval } from '../src/approval.ts';
import { CommandOutputs } from '../src/tool-output.ts';
import { rootIdentity } from '../src/safety.ts';
import type { ToolState } from '../src/tools.ts';
import type { Snapshot } from '../src/protocol.ts';
import { setup } from './support/safety.ts';
import { mcpPolicy, setMcpEnabled } from '../src/mcp/policy.ts';

const secret = 'SEEDED_MCP_SECRET_72919';
async function rig(t: TestContext, policy: 'trusted' | 'sandboxed' = 'trusted', mode: 'ask' | 'auto' = 'auto', behavior = 'normal', extraEnv: Record<string, string> = {}) {
  const { root, base } = await setup(t); const agent = join(base, 'agent'); await mkdir(agent);
  const store = join(base, 'store'); await mkdir(store, { mode: 0o700 });
  const marker = join(root, 'events'); await writeFile(join(root, 'notes.txt'), 'notes');
  const outside = join(base, 'outside'); await writeFile(outside, 'private');
  const script = join(root, 'server.mjs'); await writeFile(script, await readFile(resolve('test/support/mcp-fixture.mjs')));
  const config = { command: process.execPath, args: [script, behavior, ...(policy === 'trusted' ? [marker] : [])],
    env: { FIXTURE_SECRET: secret, FIXTURE_EXPANDED: '${MCP_TEST_TOKEN}', FIXTURE_COMMAND: '!printf fixture-command-secret', FIXTURE_OUTSIDE: outside, ...extraEnv }, timeout: behavior === 'hang-call' ? 0.2 : 2 };
  const previous = process.env.MCP_TEST_TOKEN; process.env.MCP_TEST_TOKEN = 'fixture-variable-secret';
  t.after(() => { if (previous === undefined) delete process.env.MCP_TEST_TOKEN; else process.env.MCP_TEST_TOKEN = previous; });
  await writeFile(join(agent, 'mcp.json'), JSON.stringify({ mcpServers: { docs: config } }));
  const found = await loadMcp(root, policy === 'trusted', agent, true);
  const snapshot = { id: 'fixture-thread', checkout: root, status: 'running', pending_approval: null } as Snapshot;
  const approvals: unknown[] = []; const interactive = new InteractiveApproval(snapshot, () => { if (snapshot.pending_approval) approvals.push(structuredClone(snapshot.pending_approval)); });
  const authorization = new RunAuthorization(mode === 'auto' ? automaticApproval : interactive); authorization.activate();
  const db = new DatabaseSync(':memory:'); const outputs = new CommandOutputs(db);
  const state: ToolState = { snapshot, authorization, mcpApproval: interactive, commandPolicy: policy, baseline: [], protected: [], owned: new Map(),
    rootIdentity: await rootIdentity(root), buildCache: { store, thread: '12345678-1234-1234-1234-123456789abc' }, outputs, record() {},
    loginEnvironment: Promise.resolve({ env: { PATH: '/usr/bin:/bin', HOME: base }, source: 'login-shell' }),
  };
  const settings = new Map<string, string>(); const prefs = { setting: (key: string) => settings.get(key) ?? null, setSetting: (key: string, value: string | null) => { if (value) settings.set(key, value); } };
  setMcpEnabled(prefs, root, state.rootIdentity, 'docs', true);
  const entries = mcpPolicy(prefs, root, state.rootIdentity, found.servers).flatMap(row => eligible(found.entries.get(row.name)!, row) ? [found.entries.get(row.name)!] : []);
  const observations: unknown[] = []; const names = new Map<string, string>();
  const run = new McpRun(state, entries, (entry, observed) => observations.push({ name: entry.name, ...observed }), (name, label) => names.set(name, label));
  const registry = createRegistry(); run.install(registry);
  let serial = 0;
  const api = () => ({ callId: `call-${serial++}`, agent: async () => ({ deferred: registry.snapshot().tools().map(item => item.tool).filter(tool => tool.deferred) }) }) as unknown as ToolExecutionApi;
  const search = () => registry.snapshot().tools()[0].tool.execute({ query: 'fixture', limit: 20 }, api(), context);
  const call = (tool = 'echo', args = { text: 'hello' }) => {
    const name = [...names].find(([, label]) => label === `mcp:docs/${tool}`)?.[0]; assert.ok(name, `registered ${tool}: ${JSON.stringify(observations)}`);
    return registry.snapshot().tools().find(item => item.tool.name === name)!.tool.execute(args, api(), context);
  };
  t.after(async () => { authorization.revoke(); await run.close(); db.close(); });
  return { root, base, marker, snapshot, interactive, approvals, observations, names, registry, run, state, found, search, call, db };
}
async function waitFor(predicate: () => boolean) { for (let i = 0; i < 500; i++) { if (predicate()) return; await new Promise(resolve => setTimeout(resolve, 10)); } throw new Error('fixture wait timed out'); }

for (const policy of ['trusted', 'sandboxed'] as const) for (const mode of ['ask', 'auto'] as const) for (const allow of [true, false]) {
  test(`MCP ${policy} ${mode} ${allow ? 'allow' : 'deny'} preserves exact approval policy`, async t => {
    const f = await rig(t, policy, mode); await f.search(); const pending = f.call();
    if (policy === 'sandboxed' || mode === 'ask') {
      const settled = pending.then(value => ({ value }), error => ({ error }));
      await waitFor(() => !!f.snapshot.pending_approval);
      assert.equal(f.snapshot.pending_approval!.kind, 'command'); assert.equal(f.snapshot.pending_approval!.summary, 'Use echo from docs');
      assert.doesNotMatch(JSON.stringify(f.approvals), /SEEDED_MCP_SECRET|fixture-variable-secret|fixture-command-secret/);
      f.interactive.answer(f.snapshot.pending_approval!.id, allow);
      const result = await settled; assert.ok(allow ? 'value' in result : 'error' in result);
    } else { await pending; assert.equal(f.approvals.length, 0); }
  });
}

test('deferred search, collisions, unsafe replay, secret-free output and overflow storage', async t => {
  const f = await rig(t); await assert.rejects(readFile(f.marker), { code: 'ENOENT' }); await f.search();
  const all = f.registry.snapshot().tools().map(item => item.tool);
  assert.ok(all.slice(1).every(tool => tool.deferred && tool.replay === 'unsafe'));
  assert.equal(new Set(all.map(tool => tool.name)).size, all.length);
  assert.ok(all.slice(1).every(tool => tool.name.startsWith('mcp__') && tool.name !== 'command'));
  const echo = await f.call(); const large = await f.call('large'); assert.ok(JSON.stringify(large).length < 24576);
  const rows = f.db.prepare('SELECT data FROM command_output_chunks').all();
  const serialized = JSON.stringify([all, echo, large, f.observations, f.approvals, rows.map(row => Buffer.from(row.data as Uint8Array).toString())]);
  for (const value of [secret, 'fixture-variable-secret', 'fixture-command-secret', 'SERVER_INSTRUCTIONS_MUST_NOT_REACH_MODEL']) assert.ok(!serialized.includes(value), value);
  assert.ok(rows.some(row => (row.data as Uint8Array).length > 12000));
  assert.match(JSON.stringify(await f.call('sample')), /-32601/);
  assert.equal((await readFile(f.marker, 'utf8')).match(/start /g)?.length, 1);
});

test('startup crash, ten-second startup timeout, and stalled call are bounded and never retried', { timeout: 30000 }, async t => {
  assert.equal(MCP_START_TIMEOUT_MS, 10000); assert.equal(MCP_CALL_TIMEOUT_MS, 60000);
  const crash = await rig(t, 'trusted', 'auto', 'crash-start'); await crash.search();
  assert.match(JSON.stringify(crash.observations), /fixture startup crash/); assert.doesNotMatch(JSON.stringify(crash.observations), new RegExp(secret));
  const malformed = await rig(t, 'trusted', 'auto', 'bad-schema'); await malformed.search();
  assert.equal(malformed.registry.snapshot().tools().length, 1); assert.match(JSON.stringify(malformed.observations), /could not start or list tools/);
  const silent = await rig(t, 'trusted', 'auto', 'silent'); const start = Date.now(); await silent.search();
  assert.ok(Date.now() - start < 13000); assert.match(JSON.stringify(silent.observations), /timed out/);
  const hang = await rig(t, 'trusted', 'auto', 'hang-call'); await hang.search(); assert.equal((await hang.call()).isError, true);
  assert.equal((await readFile(hang.marker, 'utf8')).match(/call echo/g)?.length, 1);
});

test('two servers with identical remote tool names remain separate from each other and builtins', async t => {
  const f = await rig(t); const entry = f.found.entries.get('docs')!; const labels = new Map<string, string>();
  const run = new McpRun(f.state, [entry, { ...entry, name: 'other' }], () => {}, (name, label) => labels.set(name, label));
  t.after(() => run.close()); const registry = createRegistry(); run.install(registry);
  const api = { callId: 'two-servers', agent: async () => ({ deferred: [] }) } as unknown as ToolExecutionApi;
  await registry.snapshot().tools()[0].tool.execute({ query: 'fixture' }, api, context);
  assert.equal(labels.size, 16); assert.equal(registry.snapshot().tools().length, 17);
  assert.ok([...labels.keys()].every(name => name.startsWith('mcp__') && name.length <= 64));
  assert.ok([...labels.values()].includes('mcp:docs/echo')); assert.ok([...labels.values()].includes('mcp:other/echo'));
});

test('Stop kills the MCP server and its child process group', async t => {
  const f = await rig(t, 'trusted', 'auto', 'hang-call'); await f.search();
  const pids = [...(await readFile(f.marker, 'utf8')).matchAll(/(?:start|child) (\d+)/g)].map(match => Number(match[1]));
  assert.equal(pids.length, 2); f.state.authorization.revoke(); await f.run.close();
  await waitFor(() => pids.every(pid => { try { process.kill(pid, 0); return false; } catch { return true; } }));
});

test('sandboxed server reads checkout, denies checkout/Git/outside writes, and writes scratch/cache', async t => {
  const server = createServer((_request, response) => response.end('loopback-ok'));
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve)); t.after(() => { server.closeAllConnections(); server.close(); });
  const f = await rig(t, 'sandboxed', 'auto', 'normal', { FIXTURE_URL: `http://127.0.0.1:${(server.address() as { port: number }).port}` });
  await f.search(); const pending = f.call('inspect');
  await waitFor(() => !!f.snapshot.pending_approval); f.interactive.answer(f.snapshot.pending_approval!.id, true);
  const text = JSON.stringify(await pending);
  for (const field of ['checkoutWrite', 'gitWrite', 'outsideRead', 'outsideWrite']) assert.match(text, new RegExp(field + '.*?denied'));
  for (const field of ['scratchWrite', 'cacheWrite']) assert.match(text, new RegExp(field + '.*?allowed'));
  assert.match(text, /notes/); assert.match(text, /loopback-ok/);
});

test('loopback HTTP resolves header values only at search and never invokes OAuth', async t => {
  const f = await rig(t); let requests = 0; const headers: string[] = [];
  const server = createServer(async (request, response) => {
    requests++; headers.push(request.headers.authorization ?? '');
    let body = ''; for await (const chunk of request) body += chunk;
    if (!body) { response.writeHead(204).end(); return; }
    const message = JSON.parse(body);
    if (!message.id) { response.writeHead(202).end(); return; }
    const result = message.method === 'initialize' ? { protocolVersion: message.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'http fixture', version: '1' } }
      : message.method === 'tools/list' ? { tools: [{ name: 'echo', description: 'fixture HTTP echo', inputSchema: { type: 'object' } }] }
      : { content: [{ type: 'text', text: request.headers.authorization }] };
    response.setHeader('Content-Type', 'application/json'); response.end(JSON.stringify({ jsonrpc: '2.0', id: message.id, result }));
  });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve)); t.after(() => { server.closeAllConnections(); server.close(); });
  const entry = { ...f.found.entries.get('docs')!, config: { url: `http://127.0.0.1:${(server.address() as { port: number }).port}`, headers: { Authorization: 'Bearer ${MCP_TEST_TOKEN}' } } };
  const run = new McpRun(f.state, [entry], () => {}, () => {}); t.after(() => run.close());
  const registry = createRegistry(); run.install(registry); assert.equal(requests, 0);
  const api = { callId: 'http', agent: async () => ({ deferred: registry.snapshot().tools().map(item => item.tool).filter(tool => tool.deferred) }) } as unknown as ToolExecutionApi;
  await registry.snapshot().tools()[0].tool.execute({ query: 'fixture' }, api, context);
  const result = await registry.snapshot().tools()[1].tool.execute({}, api, context);
  assert.ok(headers.every(value => value === 'Bearer fixture-variable-secret')); assert.doesNotMatch(JSON.stringify(result), /fixture-variable-secret/);
});

test('HTTP authentication challenge stays listed for Pi sign-in with no retry or leaked body', async t => {
  const f = await rig(t); let requests = 0;
  const server = createServer((_request, response) => { requests++; response.writeHead(401, { 'WWW-Authenticate': 'Bearer' }).end(secret); });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve)); t.after(() => { server.closeAllConnections(); server.close(); });
  const entry = { ...f.found.entries.get('docs')!, config: { url: `http://127.0.0.1:${(server.address() as { port: number }).port}` } };
  const seen: unknown[] = []; const run = new McpRun(f.state, [entry], (_entry, value) => seen.push(value), () => {}); t.after(() => run.close());
  const registry = createRegistry(); run.install(registry);
  const api = { callId: 'auth', agent: async () => ({ deferred: [] }) } as unknown as ToolExecutionApi;
  await registry.snapshot().tools()[0].tool.execute({ query: 'fixture' }, api, context);
  assert.equal(requests, 1); assert.match(JSON.stringify(seen), /"needs_sign_in":true/); assert.doesNotMatch(JSON.stringify(seen), new RegExp(secret));
  await registry.snapshot().tools()[0].tool.execute({ query: 'again' }, api, context); assert.equal(requests, 1);
  const explicit = { ...entry, config: { ...entry.config, oauth: {} } };
  assert.equal(eligible(explicit, { ...f.found.servers[0], needs_sign_in: true }), false);
});

test('Stop during a pending approval prevents a later answer from reaching the server', async t => {
  const f = await rig(t, 'trusted', 'ask'); await f.search();
  const pending = f.call().then(() => false, () => true); await waitFor(() => !!f.snapshot.pending_approval);
  const id = f.snapshot.pending_approval!.id; f.state.authorization.revoke();
  assert.equal(await pending, true); assert.throws(() => f.interactive.answer(id, true));
  await f.run.close(); assert.doesNotMatch(await readFile(f.marker, 'utf8'), /call echo/);
});

test('a crashed connected server reports the stderr tail without retrying its call', async t => {
  const f = await rig(t); await f.search(); assert.equal((await f.call('crash')).isError, true);
  assert.match(JSON.stringify(f.observations), /fixture crashed/); assert.doesNotMatch(JSON.stringify(f.observations), new RegExp(secret));
  await f.search(); assert.equal((await readFile(f.marker, 'utf8')).match(/start /g)?.length, 1);
});

test('approval binds MCP arguments and rejects a replaced checkout before dispatch', async t => {
  const f = await rig(t, 'trusted', 'ask'); await f.search();
  const args = { text: 'approved-text' }; const first = f.call('echo', args);
  await waitFor(() => !!f.snapshot.pending_approval); args.text = 'changed-after-approval';
  assert.equal(JSON.parse(f.snapshot.pending_approval!.details).text, 'approved-text');
  f.interactive.answer(f.snapshot.pending_approval!.id, true);
  const result = JSON.stringify(await first); assert.match(result, /approved-text/); assert.doesNotMatch(result, /changed-after-approval/);
  const second = f.call().then(() => false, () => true); await waitFor(() => !!f.snapshot.pending_approval);
  const moved = join(f.base, 'original-repo'); await rename(f.root, moved); await mkdir(f.root);
  f.interactive.answer(f.snapshot.pending_approval!.id, true); assert.equal(await second, true);
  assert.equal((await readFile(join(moved, 'events'), 'utf8')).match(/call echo/g)?.length, 1);
  f.state.authorization.revoke(); await f.run.close();
});
