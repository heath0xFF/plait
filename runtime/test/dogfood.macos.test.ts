import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import type { ChildProcess } from 'node:child_process';
import { cp, mkdir, mkdtemp, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { connect } from 'node:net';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import { binary, delay, exec, request, until } from './support/host.ts';
import { replayKey, replayModel, replayServer } from './support/openai-replay.ts';

// This is real Plait source and real Cargo execution, but a scripted model, not a live account.
test('production runtime edits Plait source, runs Cargo, reconnects and resumes without replay', { timeout: 240000 }, async t => {
  const base = await realpath(await mkdtemp('/tmp/plait-dogfood-'));
  const repo = join(base, 'repo'); const store = join(base, 'store'); const agent = join(base, 'pi');
  const source = resolve(import.meta.dirname, '../..');
  const path = 'crates/plait-core/src/session.rs';
  const before = await readFile(join(source, path), 'utf8');
  const oldTest = 'assert_eq!(title(&"🙂".repeat(100)).chars().count(), 72);';
  const newTest = oldTest.replace(', 72)', ', 80)');
  assert(before.includes('.take(72)') && before.includes(oldTest), 'Update the dogfood fixture when the title contract changes.');
  const edit = (id: string, old_text: string, new_text: string) => ({ tool: { id, name: 'edit', input: { path, old_text, new_text } } });
  const command = (id: string) => ({ tool: { id, name: 'command', input: { command: 'sleep 1; cargo test --offline --locked -p plait-core --lib' } } });
  const replay = await replayServer([
    edit('extend_title', '.take(72)', '.take(80)'), edit('extend_test', oldTest, newTest),
    command('test_extended_title'), { text: 'Scripted fixture: the changed Plait core passed its tests.' },
    edit('restore_title', '.take(80)', '.take(72)'), edit('restore_test', newTest, oldTest),
    command('test_restored_title'), { text: 'Scripted fixture: the follow-up passed with the original limit restored.' },
  ]);
  const hosts: ChildProcess[] = [];
  t.after(async () => {
    for (const host of hosts) if (host.exitCode === null && host.signalCode === null) host.kill('SIGKILL');
    await delay(500); await replay.close(); await rm(base, { recursive: true, force: true });
  });
  for (const directory of [repo, store, agent]) await mkdir(directory, { recursive: true, mode: 0o700 });
  // No user's Git metadata, ignored files, credentials, or build output enter the disposable checkout.
  await cp(join(source, 'crates'), join(repo, 'crates'), { recursive: true });
  for (const file of ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'AGENTS.md']) {
    await cp(join(source, file), join(repo, file));
  }
  const environment = { PATH: process.env.PATH, HOME: process.env.HOME, PI_CODING_AGENT_DIR: agent, PI_OFFLINE: '1' };
  assert(environment.HOME, 'Installed Rust toolchain discovery needs the test account home.');
  const git = (args: string[]) => exec('/usr/bin/git', ['-C', repo, ...args], {
    env: { PATH: process.env.PATH, HOME: process.env.HOME, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null' },
  });
  await git(['init', '-q']); await git(['add', '.']);
  await git(['-c', 'user.name=Plait Fixture', '-c', 'user.email=test@invalid', '-c', 'commit.gpgsign=false', 'commit', '-qm', 'fixture']);
  await writeFile(join(repo, 'existing-dirty.txt'), 'Do not change this pre-existing work.\n');
  const index = await readFile(join(repo, '.git/index'));
  await writeFile(join(agent, 'models.json'), JSON.stringify({ providers: { 'plait-replay': {
    baseUrl: replay.baseUrl, api: 'openai-completions', apiKey: replayKey,
    models: [{ id: replayModel, name: 'Local replay fixture', reasoning: false, contextWindow: 32768, maxTokens: 1024 }],
  } } }), { mode: 0o600 });
  const start = async () => {
    const host = spawn(binary, ['run', '--store', store], { stdio: ['ignore', 'ignore', 'pipe'], env: environment });
    hosts.push(host);
    let errors = ''; host.stderr!.on('data', chunk => { errors = (errors + chunk.toString()).slice(-8192); });
    await until(async () => {
      if (host.exitCode !== null || host.signalCode !== null) throw new Error(`Fixture host exited: ${errors}`);
      return request(store, { op: 'status' });
    }, reply => reply.ok);
    return host;
  };
  let host = await start();
  const created = await request(store, { op: 'create_thread', checkout: repo, provider: 'plait-replay', model_id: replayModel, request_id: 'dogfood-create' });
  assert.equal(created.ok, true, JSON.stringify(created));
  const id = created.result.thread.id;
  const watch = async () => (await request(store, { op: 'watch', thread_id: id })).result.thread;
  const submit = (request_id: string) => request(store, { op: 'submit', thread_id: id, request_id, text: 'Perform the next scripted fixture turn.' });
  for (const turn of [1, 2]) {
    const requestId = `dogfood-${turn}`;
    assert.equal((await submit(requestId)).ok, true);
    const answered = new Set<string>();
    let disconnected = false;
    const done = await until(async () => {
      const snapshot = await watch();
      if (snapshot.pending_approval && !answered.has(snapshot.pending_approval.id)) {
        answered.add(snapshot.pending_approval.id);
        assert.equal((await request(store, { op: 'approve', thread_id: id, approval_id: snapshot.pending_approval.id, allow: true })).ok, true);
        if (snapshot.pending_approval.kind === 'command') {
          const observer = connect(join(store, 'host.sock'));
          observer.on('error', () => {}); observer.on('data', () => {});
          await new Promise<void>((resolve, reject) => { observer.once('error', reject); observer.once('connect', resolve); });
          observer.write(JSON.stringify({ op: 'watch', thread_id: id }) + '\n');
          await delay(100); observer.destroy(); disconnected = true;
          assert.equal((await watch()).status, 'running');
        }
      }
      return snapshot;
    }, snapshot => snapshot.status === 'idle' || snapshot.status === 'error', 180000);
    assert.equal(done.status, 'idle', JSON.stringify(done));
    assert.equal(answered.size, 3);
    assert(disconnected);
    assert(done.tools.every(tool => tool.status === 'done'), JSON.stringify(done.tools));
    const commands = done.tools.filter(tool => tool.name === 'command');
    assert.equal(commands.length, turn);
    assert.match(commands.at(-1)!.output, /test result: ok/);
    assert.equal(replay.requests.length, turn * 4);
    assert.equal((await submit(requestId)).ok, true);
    assert.equal(replay.requests.length, turn * 4, 'Retry must not call the model again.');
    assert.deepEqual(await readFile(join(repo, '.git/index')), index);
    assert.equal(await readFile(join(repo, 'existing-dirty.txt'), 'utf8'), 'Do not change this pre-existing work.\n');
    if (turn === 1) {
      const messages = done.messages.map(message => message.id);
      const tools = done.tools.map(tool => tool.id);
      host.kill('SIGTERM'); await until(async () => host.signalCode !== null, value => value); await delay(500);
      host = await start();
      const restored = await watch();
      assert.equal(restored.status, 'idle');
      assert.deepEqual(restored.messages.map(message => message.id), messages);
      assert.deepEqual(restored.tools.map(tool => tool.id), tools);
      await delay(100); assert.equal(replay.requests.length, 4);
    }
  }
  assert.equal(await readFile(join(repo, path), 'utf8'), before);
  assert.equal(await readFile(join(source, path), 'utf8'), before, 'The source checkout must not be edited by the acceptance fixture.');
  for (const captured of replay.requests) {
    assert.equal(captured.key, `Bearer ${replayKey}`); assert.equal(captured.body.model, replayModel);
  }
});
