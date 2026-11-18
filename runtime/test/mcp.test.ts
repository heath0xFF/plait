import assert from 'node:assert/strict';
import childProcess from 'node:child_process';
import net from 'node:net';
import tls from 'node:tls';
import http from 'node:http';
import https from 'node:https';
import { syncBuiltinESMExports } from 'node:module';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { test } from 'node:test';
import { DefaultPackageManager, SettingsManager } from '@earendil-works/pi-coding-agent';
import { listMcp, loadMcp } from '../src/mcp/config.ts';
import { mcpPolicy, setMcpEnabled } from '../src/mcp/policy.ts';
import { commandPolicy, setCommandPolicy } from '../src/command/policy.ts';
import { report } from '../src/skills.ts';
import { validate } from '../src/protocol.ts';
import { Store } from '../src/store.ts';

async function fixture(t: { after(fn: () => Promise<void>): void }) {
  const base = await mkdtemp(join(tmpdir(), 'plait-mcp-'));
  t.after(() => rm(base, { recursive: true, force: true }));
  const agent = join(base, 'agent'); const repo = join(base, 'repo');
  await mkdir(agent); await mkdir(join(repo, '.pi'), { recursive: true });
  const write = async (path: string, value: unknown) => writeFile(path, JSON.stringify(value));
  const user = (servers: unknown) => write(join(agent, 'mcp.json'), { mcpServers: servers });
  const project = (servers: unknown) => write(join(repo, '.pi', 'mcp.json'), { mcpServers: servers });
  return { base, agent, repo, user, project, write, list: (trusted = true, enabled = true) => listMcp(repo, trusted, agent, enabled) };
}

async function piConfig() {
  const entry = fileURLToPath(import.meta.resolve('@earendil-works/pi-coding-agent'));
  return import(pathToFileURL(join(dirname(entry), 'extensions/mcp/config.js')).href) as Promise<{
    loadMcpConfig(options: { agentDir: string; cwd: string; projectTrusted: boolean }): {
      servers: { name: string; source: string; override?: string; config: { enabled?: boolean } }[]; errors: string[];
    };
  }>;
}

test('MCP merge matches pinned Pi: replacement, restricted overrides, validation and namespaces', async t => {
  const f = await fixture(t); const pi = await piConfig();
  await f.user({
    base: { command: '/bin/node', args: ['user-argument'], env: { KEY: 'secret' } },
    replaced: { url: 'https://old.example/mcp', headers: { Authorization: 'secret' } },
    inherited: { url: 'https://provider.example/mcp', auth: { provider: 'test' } },
    'one-two': { command: 'npx' }, stays: { command: 'uvx' },
    invalid: { type: 'sse', url: 'https://sse.example' },
  });
  await f.project({
    base: { enabled: false, exposure: 'hidden', toolExposure: { find: 'codemode-deferred' } },
    replaced: { command: 'replacement', enabled: true }, inherited: { enabled: false },
    stays: { env: { KEY: 'project-secret' } }, missing: { enabled: true }, one_two: { command: 'collision' },
    projectAuth: { url: 'https://provider.example', auth: { provider: 'test' } },
    good: { url: 'https://docs.example/mcp', oauth: { clientRegistration: 'cimd' } },
    badExposure: { command: 'x', exposure: 'invalid' },
    badToolExposure: { command: 'x', toolExposure: { secret: 'invalid' } },
    badCallback: { url: 'https://example.com', oauth: { clientRegistration: 'cimd', callbackUrl: 'http://[::1]/callback' } },
    badAuth: { url: 'http://remote.example', auth: { provider: 'test' } },
    invalid: { type: 'sse', url: 'https://still-invalid.example' },
  });
  for (const trusted of [false, true]) {
    const ours = await f.list(trusted);
    const theirs = pi.loadMcpConfig({ agentDir: f.agent, cwd: f.repo, projectTrusted: trusted });
    assert.deepEqual(ours.servers.filter(server => !server.problem).map(server => ({
      name: server.name, source: server.source, override: server.override_source ?? undefined, enabled: server.enabled,
    })), theirs.servers.map(server => ({
      name: server.name, source: server.source, override: server.override, enabled: trusted && server.config.enabled !== false,
    })));
    assert.equal(ours.problems.length, theirs.errors.length);
  }
  const { servers } = await f.list();
  const trusted = await loadMcp(f.repo, true, f.agent, true);
  const sandboxed = await loadMcp(f.repo, false, f.agent, true);
  assert.deepEqual(trusted.entries.get('replaced')?.config, { command: 'replacement', enabled: true });
  assert.deepEqual(sandboxed.entries.get('replaced')?.config, { url: 'https://old.example/mcp', headers: { Authorization: 'secret' } });
  assert.deepEqual(trusted.entries.get('base')?.config.toolExposure, { find: 'codemode-deferred' });
  assert.equal(sandboxed.entries.get('base')?.config.toolExposure, undefined);
  assert.equal(servers.find(server => server.name === 'replaced')?.transport, 'stdio');
  assert.equal(servers.find(server => server.name === 'base')?.program, 'node');
  assert.equal(servers.find(server => server.name === 'stays')?.program, 'uvx');
  assert.ok(servers.find(server => server.name === 'invalid')?.problem);
  assert.equal(servers.filter(server => server.name === 'invalid').length, 1);
});

test('MCP JSON errors are safe, BOM is accepted, and reads and replies are bounded', async t => {
  const f = await fixture(t);
  await writeFile(join(f.agent, 'mcp.json'), '\uFEFF{"mcpServers":{"ok":{"command":"node"}}}');
  assert.equal((await f.list()).servers[0].name, 'ok');
  await writeFile(join(f.repo, '.pi', 'mcp.json'), '{"mcpServers":{"secret":"SECRET_BAD_JSON');
  assert.ok(!(JSON.stringify(await f.list())).includes('SECRET_BAD_JSON'));
  assert.equal((await f.list(false)).problems.length, 0);
  await writeFile(join(f.agent, 'mcp.json'), ' '.repeat(1024 * 1024 + 1));
  assert.match((await f.list(false)).problems[0].message, /1 MiB/);
  await f.user(Object.fromEntries(Array.from({ length: 700 }, (_, i) => [`server${i}`, { command: 'node' }])));
  const listed = await f.list(false);
  const reply = report(f.repo, false, { skills: [], templates: [], problems: [], mcpServers: listed.servers });
  assert.equal(reply.mcp_servers.length, 500);
  assert.ok(reply.problems.some(problem => problem.message.includes('500 of 700 MCP servers')));
  assert.ok(Buffer.byteLength(JSON.stringify(reply)) < 1024 * 1024);
});

test('listing resolves no secrets and starts no process, transport, or network request', async t => {
  const f = await fixture(t); const secret = 'SECRET_MCP_VALUE';
  await f.user({
    local: { command: '/opt/tools/node', args: ['--token', `${secret}_ARG`], env: { TOKEN: `${secret}_ENV`, COMMAND: `!touch ${f.base}/spawned`, VAR: '${MCP_SECRET_TEST}' } },
    remote: { url: `https://user:${secret}_PASSWORD@docs.example/private/${secret}_PATH?token=${secret}_QUERY`, headers: { Authorization: `!echo ${secret}_HEADER`, Other: '${MCP_SECRET_TEST}' } },
    oauth: { url: 'https://oauth.example/mcp', oauth: { clientSecret: `!echo ${secret}_CLIENT` } },
  });
  await f.write(join(f.agent, 'mcp-auth.json'), {
    'mcp__oauth|https://oauth.example/mcp': { serverUrl: 'https://oauth.example/mcp', tokens: { access_token: `${secret}_TOKEN`, refresh_token: `${secret}_REFRESH` } },
  });
  const old = process.env.MCP_SECRET_TEST; process.env.MCP_SECRET_TEST = `${secret}_EXPANDED`;
  t.after(async () => { if (old === undefined) delete process.env.MCP_SECRET_TEST; else process.env.MCP_SECRET_TEST = old; });
  const calls: string[] = [];
  const deny = (name: string) => () => { calls.push(name); throw new Error(`Unexpected ${name}`); };
  for (const method of ['spawn', 'spawnSync', 'exec', 'execSync', 'execFile', 'execFileSync', 'fork'] as const) t.mock.method(childProcess, method, deny(method));
  t.mock.method(net.Server.prototype, 'listen', deny('listen'));
  t.mock.method(net.Socket.prototype, 'connect', deny('socket')); t.mock.method(tls, 'connect', deny('tls'));
  t.mock.method(http, 'request', deny('http')); t.mock.method(https, 'request', deny('https'));
  t.mock.method(globalThis, 'fetch', deny('fetch')); syncBuiltinESMExports();
  try {
    const found = await f.list();
    const serialized = JSON.stringify(report(f.repo, true, { skills: [], templates: [], problems: found.problems, mcpServers: found.servers }));
    for (const value of [secret, '!touch', '!echo', 'MCP_SECRET_TEST', '--token', '/private/']) assert.ok(!serialized.includes(value), value);
    assert.equal(found.servers[0].program, 'node'); assert.equal(found.servers[1].host, 'docs.example');
    assert.equal(found.servers[2].signed_in, true); assert.deepEqual(calls, []);
    await assert.rejects(readFile(join(f.base, 'spawned')), { code: 'ENOENT' });
  } finally { t.mock.restoreAll(); syncBuiltinESMExports(); }
});

test('OAuth metadata respects exact URLs, namespaces and legacy keys, without claiming public servers need sign-in', async t => {
  const f = await fixture(t);
  await f.user({
    public: { url: 'https://public.example' }, explicit: { url: 'https://explicit.example', oauth: {} },
    header: { url: 'https://header.example', oauth: {}, headers: { aUtHoRiZaTiOn: 'secret' } },
    provider: { url: 'https://provider.example', auth: { provider: 'p' } },
    'saved-name': { url: 'https://saved.example' }, legacy: { url: 'https://legacy.example' },
    wrong: { url: 'https://wrong.example' }, registered: { url: 'https://registered.example' },
  });
  const tokens = { access_token: 'secret' };
  const auth = {
    'mcp__saved_name|https://saved.example/': { serverUrl: 'https://saved.example/', tokens },
    'https://legacy.example/': { serverUrl: 'https://legacy.example/', tokens },
    'mcp__wrong|https://wrong.example/': { serverUrl: 'https://another.example/', tokens },
    'mcp__registered|https://registered.example/': { serverUrl: 'https://registered.example/', clientInformation: { client_id: 'secret' } },
  };
  await f.write(join(f.agent, 'mcp-auth.json'), auth);
  assert.deepEqual((await f.list()).servers.map(row => [row.name, row.needs_sign_in, row.signed_in]), [
    ['public', null, false], ['explicit', true, false], ['header', false, false], ['provider', false, false],
    ['saved-name', true, true], ['legacy', true, true], ['wrong', null, false], ['registered', true, false],
  ]);
  assert.deepEqual(JSON.parse(await readFile(join(f.agent, 'mcp-auth.json'), 'utf8')), auth, 'No migration or token writes');
});

test('Pi builtin MCP disable and project overrides follow its package resolver', async t => {
  const f = await fixture(t); await f.user({ local: { command: 'node' } });
  const home = process.env.HOME; process.env.HOME = f.base;
  t.after(async () => { if (home === undefined) delete process.env.HOME; else process.env.HOME = home; });
  await f.write(join(f.agent, 'settings.json'), { extensions: ['-builtin:mcp'] });
  await f.write(join(f.repo, '.pi', 'settings.json'), { extensions: ['+builtin:mcp'] });
  for (const trusted of [false, true]) {
    const settingsManager = SettingsManager.create(f.repo, f.agent, { projectTrusted: trusted });
    const resolved = await new DefaultPackageManager({ cwd: f.repo, agentDir: f.agent, settingsManager, builtinExtensions: ['mcp'] }).resolve(async () => 'skip');
    const enabled = resolved.extensions.find(entry => entry.path === 'builtin:mcp')!.enabled;
    assert.equal(enabled, trusted);
    const server = (await f.list(trusted, enabled)).servers[0];
    assert.equal(server.can_enable, trusted); assert.equal(server.enabled, trusted);
  }
});

test('project MCP switches round trip in the host store under both policies and cannot bypass Pi disable', async t => {
  const f = await fixture(t); await f.user({ on: { command: 'node' }, off: { command: 'node', enabled: false } });
  const directory = join(f.base, 'store'); await mkdir(directory);
  let store = new Store(directory); t.after(async () => store.close());
  const identity = { dev: '1', ino: '2' };
  const read = async (root = f.repo, who = identity, pi = true) => mcpPolicy(store, root, who,
    (await f.list(commandPolicy(store, root, who) === 'trusted', pi)).servers);
  assert.deepEqual((await read()).map(row => row.enabled), [true, false]);
  setCommandPolicy(store, f.repo, identity, 'sandboxed');
  assert.deepEqual((await read()).map(row => row.enabled), [false, false]);
  setMcpEnabled(store, f.repo, identity, 'on', true); setMcpEnabled(store, f.repo, identity, 'off', true);
  assert.deepEqual((await read()).map(row => row.enabled), [true, true]);
  store.close(); store = new Store(directory);
  assert.deepEqual((await read()).map(row => row.enabled), [true, true]);
  setCommandPolicy(store, f.repo, identity, 'trusted'); setMcpEnabled(store, f.repo, identity, 'on', false);
  assert.deepEqual((await read()).map(row => row.enabled), [false, true]);
  assert.deepEqual((await read('/other')).map(row => row.enabled), [true, false]);
  assert.deepEqual((await read(f.repo, { dev: '1', ino: '3' })).map(row => row.enabled), [true, false]);
  assert.deepEqual((await read(f.repo, identity, false)).map(row => row.enabled), [false, false]);
  for (const corrupt of ['{', 'null', '{}', 'true']) {
    store.setSetting(`mcp_server:${JSON.stringify([f.repo, 'on'])}`, corrupt);
    assert.equal((await read())[0].enabled, false);
  }
});

test('set_mcp_server is a version 3 additive operation with strict fields', () => {
  const request = { op: 'set_mcp_server', checkout: '/repo', name: 'docs', enabled: true, protocol_version: 3 };
  assert.deepEqual(validate(request), request);
  for (const bad of [{ ...request, protocol_version: 1 }, { ...request, enabled: 'true' }, { ...request, name: '' },
    { ...request, env: 'secret' }, { ...request, enabled: undefined }]) assert.throws(() => validate(bad), { code: 'invalid_request' });
});
