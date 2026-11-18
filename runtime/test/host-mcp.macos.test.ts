import assert from 'node:assert/strict';
import { readFile, writeFile, mkdir, rm } from 'node:fs/promises';
import { createServer } from 'node:http';
import { createServer as unixServer } from 'node:net';
import { DatabaseSync } from 'node:sqlite';
import type { McpServer } from '../src/mcp/config.ts';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import { setup, request, until, delay } from './support/host.ts';

async function config(f: Awaited<ReturnType<typeof setup>>, behavior = 'normal', enabled = true) {
  const agent = join(f.store, 'test-pi'); await mkdir(agent, { recursive: true });
  const marker = join(f.repo, 'mcp-events'); const server = join(f.repo, 'server.mjs');
  await writeFile(server, await readFile(resolve('test/support/mcp-fixture.mjs')));
  await writeFile(join(agent, 'mcp.json'), JSON.stringify({ mcpServers: { docs: {
    command: process.execPath, args: [server, behavior, marker], enabled, exposure: 'codemode',
    env: { FIXTURE_SECRET: 'HOST_MCP_SEEDED_SECRET', FIXTURE_EXPANDED: '!printf HOST_MCP_COMMAND_SECRET' },
  } } }));
  return marker;
}

test('real host searches lazily, records each call, prompts, and stores no seeded MCP secret', { timeout: 30000 }, async t => {
  const f = await setup(t, 'mcp-normal', false, false); const marker = await config(f); const thread = await f.create();
  await assert.rejects(readFile(marker), { code: 'ENOENT' }); await f.submit(thread.id);
  const pending = await until(() => f.watch(thread.id), value => !!value.pending_approval || value.status === 'idle' || value.status === 'error');
  assert.equal(pending.pending_approval?.summary, 'Use echo from docs', JSON.stringify(pending));
  assert.equal(pending.tools.find(tool => tool.id === 'call-mcp')?.name, 'mcp:docs/echo');
  await f.approve(thread.id, pending.pending_approval!.id);
  const done = await until(() => f.watch(thread.id), value => value.status === 'idle' || value.status === 'error');
  assert.equal(done.status, 'idle', JSON.stringify(done)); assert.equal(done.tools.find(tool => tool.id === 'call-mcp')?.status, 'done');
  const history = await request(f.store, { op: 'history', thread_id: thread.id, protocol_version: 3 });
  assert.match(JSON.stringify(history), /mcp:docs\/echo/);
  const resources = await request(f.store, { op: 'agent_resources', checkout: f.repo, protocol_version: 3 });
  assert.match(JSON.stringify(resources), /"state":"stopped"/); assert.match(JSON.stringify(resources), /"tool_count":8/);
  for (const data of [JSON.stringify([pending, done, history, resources]), await readFile(join(f.store, 'mcp-context.jsonl'), 'utf8'),
    (await readFile(join(f.store, 'host.sqlite'))).toString(), (await readFile(join(f.store, thread.id + '.sqlite'))).toString()]) {
    assert.doesNotMatch(data, /HOST_MCP_SEEDED_SECRET|HOST_MCP_COMMAND_SECRET|SERVER_INSTRUCTIONS_MUST_NOT_REACH_MODEL/);
  }
  const events = await readFile(marker, 'utf8'); assert.equal(events.match(/call echo/g)?.length, 1);
  const spawns = await readFile(join(f.store, 'mcp-spawns.jsonl'), 'utf8');
  assert.equal(spawns.match(/mcp-server/g)?.length, 1);
  const db = new DatabaseSync(join(f.store, 'host.sqlite'), { readOnly: true });
  try { assert.equal(db.prepare("SELECT count(*) AS n FROM settings WHERE key LIKE 'mcp_tool:%'").get()!.n, 1); }
  finally { db.close(); }
});

test('first search stores no per-thread tool labels when no MCP tool is called', async t => {
  const f = await setup(t, 'mcp-search-only', false, false); const marker = await config(f); const thread = await f.create();
  await f.submit(thread.id); await until(() => f.watch(thread.id), value => value.status === 'idle');
  assert.match(await readFile(marker, 'utf8'), /start /); assert.doesNotMatch(await readFile(marker, 'utf8'), /call /);
  const db = new DatabaseSync(join(f.store, 'host.sqlite'), { readOnly: true });
  try { assert.equal(db.prepare("SELECT count(*) AS n FROM settings WHERE key LIKE 'mcp_tool:%'").get()!.n, 0); }
  finally { db.close(); }
});

test('off servers start no process and expose neither search nor MCP tools', { timeout: 15000 }, async t => {
  const f = await setup(t, 'mcp-normal', false, false); const marker = await config(f, 'normal', false); const thread = await f.create();
  await f.submit(thread.id); await until(() => f.watch(thread.id), value => value.status === 'idle');
  await assert.rejects(readFile(marker), { code: 'ENOENT' });
  const spawns = await readFile(join(f.store, 'mcp-spawns.jsonl'), 'utf8').catch(() => '');
  assert.doesNotMatch(spawns, /mcp-server|trusted-command|"command"/);
  const contexts = (await readFile(join(f.store, 'mcp-context.jsonl'), 'utf8')).trim().split('\n').map(line => JSON.parse(line));
  assert.ok(contexts.every(context => context.messages.every((message: { toolsAdded?: { name: string }[] }) =>
    (message.toolsAdded ?? []).every(tool => tool.name !== 'tool_search' && !tool.name.startsWith('mcp__')))));
});

test('host death kills the MCP group and restart never replays an interrupted call', { timeout: 30000 }, async t => {
  const f = await setup(t, 'mcp-hang', false, false); const marker = await config(f, 'hang-call'); const thread = await f.create();
  await request(f.store, { op: 'submit', thread_id: thread.id, request_id: 'mcp-start', text: 'fixture', approval: 'auto', protocol_version: 3 });
  const events = await until(() => readFile(marker, 'utf8'), value => value.includes('call echo'));
  const pids = [...events.matchAll(/(?:start|child) (\d+)/g)].map(match => Number(match[1])); assert.equal(pids.length, 2);
  f.host.kill('SIGKILL');
  await until(async () => pids.every(pid => { try { process.kill(pid, 0); return false; } catch { return true; } }), value => value);
  const processes = JSON.parse(await readFile(join(f.store, 'fixture-processes.json'), 'utf8')) as { node: number };
  await until(async () => { try { process.kill(processes.node, 0); return false; } catch { return true; } }, value => value);
  await f.start('answer'); const interrupted = await f.watch(thread.id);
  assert.equal(interrupted.status, 'interrupted');
  assert.equal(interrupted.tools.find(tool => tool.id === 'call-mcp')?.status, 'interrupted');
  await f.submit(thread.id, 'fresh', 'Continue after interruption'); await until(() => f.watch(thread.id), value => value.status === 'idle');
  await delay(200); assert.equal((await readFile(marker, 'utf8')).match(/call echo/g)?.length, 1);
});

for (const approval of ['ask', 'auto'] as const) for (const kind of ['definition', 'replacement', 'enable-override', 'exposure-override'] as const) {
  test(`trusted ${approval} ${kind} runs nothing before definition consent and nothing after it changes`, { timeout: 30000 }, async t => {
    const f = await setup(t, 'mcp-consent', false, false); const marker = await config(f);
    const userPath = join(f.store, 'test-pi', 'mcp.json');
    const user = JSON.parse(await readFile(userPath, 'utf8'));
    const credential = join(f.store, 'credential-called');
    user.mcpServers.docs.env.FIXTURE_COMMAND = `!printf called > '${credential}'; printf fixture-key`;
    let requests = 0;
    const http = createServer((_request, response) => { requests++; response.end(); });
    await new Promise<void>(resolve => http.listen(0, '127.0.0.1', resolve));
    t.after(() => { http.closeAllConnections(); http.close(); });
    const project: { mcpServers: Record<string, Record<string, unknown>> } = { mcpServers: {
      docs: kind.endsWith('override') ? kind === 'enable-override' ? { enabled: true } : { exposure: 'direct' } : user.mcpServers.docs,
      remote: { url: `http://127.0.0.1:${(http.address() as { port: number }).port}`, headers: {
        Key: `!printf called > '${credential}'; printf fixture-key`, Other: '${MCP_MUST_NOT_EXPAND_BEFORE_CONSENT}',
      } },
    } };
    if (kind === 'definition') user.mcpServers = {};
    if (kind === 'replacement') user.mcpServers.docs = { command: 'never-start-replaced-user-server' };
    if (kind === 'enable-override') user.mcpServers.docs.enabled = false;
    if (kind === 'exposure-override') user.mcpServers.docs.exposure = 'hidden';
    await writeFile(userPath, JSON.stringify(user));
    await mkdir(join(f.repo, '.pi'));
    const projectPath = join(f.repo, '.pi/mcp.json'); await writeFile(projectPath, JSON.stringify(project));
    const rows = async () => (await request(f.store, { op: 'agent_resources', checkout: f.repo, protocol_version: 3 }) as unknown as { result: { mcp_servers: McpServer[] } }).result.mcp_servers;
    const enable = (definition?: string) => request(f.store, { op: 'set_mcp_server', checkout: f.repo, name: 'docs', enabled: true, definition, protocol_version: 3 });
    const run = async (id: string) => {
      const thread = await f.create(id);
      await request(f.store, { op: 'submit', thread_id: thread.id, request_id: id, text: 'fixture', approval, protocol_version: 3 });
      return thread;
    };
    const initial = (await rows()).find(row => row.name === 'docs')!;
    assert.equal(initial.consent, kind === 'exposure-override' ? 'restricted' : 'required');
    const off = await run('off'); await until(() => f.watch(off.id), value => value.status === 'idle');
    for (const path of [marker, credential]) await assert.rejects(readFile(path), { code: 'ENOENT' });
    assert.equal(requests, 0);
    assert.doesNotMatch(await readFile(join(f.store, 'mcp-spawns.jsonl'), 'utf8').catch(() => ''), /mcp-server|trusted-command|"command"/);
    assert.ok((await rows()).every(row => row.state === undefined));
    assert.equal((await enable()).error?.code, 'permission_denied');
    assert.equal((await enable(initial.definition)).ok, true);
    const on = await run('on');
    const next = await until(() => f.watch(on.id), value => !!value.pending_approval || value.status === 'idle' || value.status === 'error');
    if (approval === 'ask') {
      assert.equal(next.pending_approval?.summary, 'Use echo from docs');
      await f.approve(on.id, next.pending_approval!.id);
    }
    const done = await until(() => f.watch(on.id), value => value.status === 'idle' || value.status === 'error');
    assert.equal(done.status, 'idle', JSON.stringify(done));
    assert.equal((await readFile(marker, 'utf8')).match(/start /g)?.length, 1);
    assert.equal(await readFile(credential, 'utf8'), 'called'); assert.equal(requests, 0);
    project.mcpServers.docs.toolExposure = { sample: 'hidden' };
    await writeFile(projectPath, JSON.stringify(project)); await rm(credential);
    const changed = (await rows()).find(row => row.name === 'docs')!;
    assert.equal(changed.enabled, false); assert.equal(changed.consent, 'changed');
    assert.equal((await enable(initial.definition)).error?.code, 'permission_denied');
    const after = await run('changed'); await until(() => f.watch(after.id), value => value.status === 'idle');
    assert.equal((await readFile(marker, 'utf8')).match(/start /g)?.length, 1);
    await assert.rejects(readFile(credential), { code: 'ENOENT' }); assert.equal(requests, 0);
    assert.equal((await readFile(join(f.store, 'mcp-spawns.jsonl'), 'utf8')).match(/mcp-server/g)?.length, 1);
  });
}

test('sandboxed MCP and credential commands cannot reach host or outside Unix sockets but retain IP and localhost DNS', { timeout: 30000 }, async t => {
  const f = await setup(t, 'mcp-normal'); await config(f, 'sockets');
  let connections = 0; const outside = join(f.base, 'outside.sock');
  const unix = unixServer(socket => { connections++; socket.end(); });
  await new Promise<void>(resolve => unix.listen(outside, resolve)); t.after(() => { unix.close(); });
  const http = createServer((_request, response) => response.end('loopback-ok'));
  await new Promise<void>(resolve => http.listen(0, '127.0.0.1', resolve)); t.after(() => { http.closeAllConnections(); http.close(); });
  const hostSocket = join(f.store, 'host.sock'); const probe = join(f.repo, 'credential.mjs');
  await writeFile(probe, `import { connect } from 'node:net';
    for (const path of process.argv.slice(2)) await new Promise((resolve, reject) => {
      const socket = connect(path); socket.once('connect', () => { socket.end(); reject(new Error('escaped')); });
      socket.once('error', error => error.code === 'EPERM' ? resolve() : reject(error));
    }); process.stdout.write('credential-socket-denied');`);
  const path = join(f.store, 'test-pi/mcp.json'); const raw = JSON.parse(await readFile(path, 'utf8'));
  raw.mcpServers.docs.args = [join(f.repo, 'server.mjs'), 'sockets'];
  raw.mcpServers.docs.env = { FIXTURE_HOST_SOCKET: hostSocket, FIXTURE_UNIX_SOCKET: outside,
    FIXTURE_URL: `http://127.0.0.1:${(http.address() as { port: number }).port}`,
    KEY: `!${JSON.stringify(process.execPath)} ${JSON.stringify(probe)} ${JSON.stringify(hostSocket)} ${JSON.stringify(outside)}` };
  await writeFile(path, JSON.stringify(raw));
  await request(f.store, { op: 'set_mcp_server', checkout: f.repo, name: 'docs', enabled: true, protocol_version: 3 });
  const thread = await f.create(); await f.submit(thread.id);
  const pending = await until(() => f.watch(thread.id), value => !!value.pending_approval || value.status === 'idle');
  assert.ok(pending.pending_approval, JSON.stringify(pending)); await f.approve(thread.id, pending.pending_approval!.id);
  const done = await until(() => f.watch(thread.id), value => value.status === 'idle');
  const output = done.tools.find(tool => tool.name === 'mcp:docs/echo')!.output;
  const result = JSON.parse(JSON.parse(output.split('\n[Command output:')[0]).content[0].text);
  assert.deepEqual(result, { host: 'EPERM', outside: 'EPERM', scratch: 'allowed', lookup: 'allowed', name: 'loopback-ok', address: 'loopback-ok' });
  assert.equal(connections, 0);
  assert.match(JSON.stringify(await request(f.store, { op: 'agent_resources', checkout: f.repo, protocol_version: 3 })), /"trusted":false/);
});
