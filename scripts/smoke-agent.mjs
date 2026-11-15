#!/usr/bin/env node
import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { once } from 'node:events';
import { mkdtemp, mkdir, readFile, realpath, writeFile } from 'node:fs/promises';
import { createConnection } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const command = 'cargo test --offline --locked';
const edit = { path: 'src/lib.rs', old_text: 'world', new_text: 'plait' };
const followup = {
  path: 'src/lib.rs',
  old_text: 'pub fn greeting() -> &\'static str { "plait" }',
  new_text: 'pub fn greeting() -> &\'static str { concat!("pl", "ait") }',
};
const source = 'pub fn greeting() -> &\'static str { "world" }\n\n#[test]\nfn greeting_is_plait() { assert_eq!(greeting(), "plait"); }\n';
const limit = 1024 * 1024;

function approvalKind(approval, change = edit) {
  assert.equal(approval.dirty_paths.length, 0, 'Disposable baseline must be clean');
  assert(Buffer.byteLength(approval.details) <= 16384, 'Approval arguments exceed the bound');
  const args = JSON.parse(approval.details);
  if (approval.kind === 'edit') {
    assert.deepEqual(args, change, 'Refusing an unexpected file change');
    return 'edit';
  }
  assert.equal(approval.kind, 'command', 'Refusing an unexpected tool');
  assert.deepEqual(args, { command }, 'Refusing an unexpected command');
  return 'command';
}

async function* frames(socket) {
  let pending = Buffer.alloc(0);
  try {
    for await (const chunk of socket) {
      pending = Buffer.concat([pending, chunk]);
      let end;
      while ((end = pending.indexOf(10)) >= 0) {
        assert(end + 1 <= limit, 'Host frame exceeds 1 MiB');
        yield JSON.parse(pending.subarray(0, end).toString('utf8'));
        pending = pending.subarray(end + 1);
      }
      assert(pending.length < limit, 'Unterminated host frame exceeds the bound');
    }
    assert.equal(pending.length, 0, 'Host closed inside a frame');
  } finally {
    socket.destroy();
  }
}

function connection(path, request, signal) {
  const socket = createConnection({ path, signal });
  socket.on('connect', () => socket.end(JSON.stringify(request) + '\n'));
  return socket;
}

async function request(path, input, signal) {
  const deadline = AbortSignal.any([signal, AbortSignal.timeout(30000)]);
  for await (const reply of frames(connection(path, input, deadline))) {
    assert.equal(reply.ok, true, `Host rejected ${input.op}: ${reply.error?.code}`);
    return reply.result;
  }
  throw new Error(`Host closed before acknowledging ${input.op}`);
}

async function* watch(path, id, signal) {
  // Keep the write side open: the host uses EOF to detect detached observers.
  const socket = createConnection({ path, signal });
  socket.on('connect', () => socket.write(JSON.stringify({ op: 'watch', thread_id: id }) + '\n'));
  let first = true;
  for await (const value of frames(socket)) {
    if (first) {
      assert.equal(value.ok, true, 'Watch was not accepted');
      first = false;
      yield value.result.thread;
    } else {
      assert.equal(value.event, 'snapshot');
      yield value.thread;
    }
  }
  throw new Error('Watch closed before completion');
}

function git(checkout, ...args) {
  return execFileSync('/usr/bin/git', ['-c', 'core.hooksPath=/dev/null', '-C', checkout, ...args], {
    encoding: 'utf8', timeout: 10000,
    env: { PATH: '/usr/bin:/bin', HOME: '/nonexistent', GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null', GIT_OPTIONAL_LOCKS: '0' },
  });
}

async function runTurn(socket, thread, change, turn, signal) {
  const submit = { op: 'submit', thread_id: thread.id, request_id: randomUUID(), text: `Read src/lib.rs. Use edit with exactly these arguments: ${JSON.stringify(change)}. Then use command with exactly ${JSON.stringify({ command })}. Do not use any other writing tool or command. After the test passes, reply briefly.` };
  const accepted = await request(socket, submit, signal);
  assert.equal(accepted.accepted, true);
  assert.equal(accepted.request_id, submit.request_id);
  console.error(`Turn ${turn}: host accepted the task.`);
  const approved = new Set();
  const approvals = new Set();
  let detached = false;
  let updates = 0;
  let final;
  while (!final) {
    for await (const snapshot of watch(socket, thread.id, signal)) {
      updates++;
      assert.equal(snapshot.id, thread.id);
      assert.deepEqual(snapshot.model, thread.model);
      if (snapshot.pending_approval && !detached) {
        detached = true;
        break;
      }
      if (snapshot.pending_approval && !approvals.has(snapshot.pending_approval.id)) {
        const approval = snapshot.pending_approval;
        const kind = approvalKind(approval, change);
        assert(!approved.has(kind), `Refusing a second ${kind} effect in one turn`);
        if (kind === 'command') assert(approved.has('edit'), 'Test command arrived before edit');
        approved.add(kind);
        approvals.add(approval.id);
        console.error(`Turn ${turn}: approving the exact ${kind} call.`);
        await request(socket, { op: 'approve', thread_id: thread.id, approval_id: approval.id, allow: true }, signal);
      }
      if (!['running', 'awaiting_approval'].includes(snapshot.status)) {
        assert.equal(snapshot.status, 'idle', `Run failed: ${snapshot.error?.code ?? snapshot.status}`);
        final = snapshot;
        break;
      }
    }
    // An observer reconnect and a lost-ACK retry must not submit a second task.
    assert.deepEqual(await request(socket, submit, signal), accepted);
  }
  assert(detached, 'No tool approval was observed before reconnect');
  assert.deepEqual([...approved], ['edit', 'command']);
  assert.equal(final.messages.filter(message => message.role === 'user').length, turn);
  assert(final.messages.some(message => message.role === 'assistant' && message.text.trim()));
  for (const name of ['edit', 'command']) {
    assert.equal(final.tools.filter(tool => tool.name === name && tool.status === 'done').length, turn, `${name} did not complete once per turn`);
  }
  console.error(`Turn ${turn}: edit and tests completed; observer reconnect and retry passed.`);
  return updates;
}

async function live() {
  const provider = process.env.PI_PROVIDER;
  const model_id = process.env.PI_MODEL;
  assert(provider && model_id, 'Set PI_PROVIDER and PI_MODEL explicitly; this check uses your account');
  const directory = await realpath(await mkdtemp(join(tmpdir(), 'plait-live-')));
  console.error(`Live acceptance fixture: ${directory}`);
  const checkout = join(directory, 'checkout');
  const store = join(directory, 'store');
  await mkdir(join(checkout, 'src'), { recursive: true });
  await writeFile(join(checkout, 'src/lib.rs'), source);
  await writeFile(join(checkout, 'Cargo.toml'), '[package]\nname = "plait-live-fixture"\nversion = "0.1.0"\nedition = "2024"\n');
  await writeFile(join(checkout, 'Cargo.lock'), 'version = 4\n\n[[package]]\nname = "plait-live-fixture"\nversion = "0.1.0"\n');
  await writeFile(join(checkout, '.gitignore'), '/target/\n');
  await writeFile(join(checkout, 'AGENTS.md'), 'This is a disposable acceptance fixture. Change only src/lib.rs. Run only cargo test --offline --locked. Do not alter Git metadata or these instructions.\n');
  git(checkout, 'init', '-q');
  git(checkout, 'add', '.');
  git(checkout, '-c', 'user.name=Plait test', '-c', 'user.email=test@example.invalid', '-c', 'commit.gpgsign=false', 'commit', '-qm', 'fixture');
  const index = await readFile(join(checkout, '.git/index'));
  const binary = process.env.PLAIT_HOST_BINARY ?? join(root, 'target/debug/plait-host');
  const child = spawn(binary, ['run', '--store', store, '--runtime', join(root, 'runtime/src/main.ts')], { stdio: ['ignore', 'ignore', 'ignore'] });
  let startupError;
  child.on('error', error => { startupError = error; });
  const exited = once(child, 'exit').catch(() => undefined);
  const signal = AbortSignal.timeout(240000);
  const socket = join(store, 'host.sock');
  let thread;
  try {
    let status;
    for (let attempt = 0; attempt < 100; attempt++) {
      if (startupError) throw startupError;
      assert(child.exitCode === null && child.signalCode === null, 'Host exited during startup');
      try { status = await request(socket, { op: 'status' }, signal); break; }
      catch (error) { if (signal.aborted || attempt === 99) throw error; }
      await delay(100, undefined, { signal });
    }
    assert.equal(status.protocol_version, 1);
    assert(status.models.some(model => model.provider === provider && model.model_id === model_id && model.authenticated), 'Selected model is unavailable or unauthenticated');
    const create = { op: 'create_thread', checkout, provider, model_id, request_id: randomUUID() };
    ({ thread } = await request(socket, create, signal));
    assert.equal((await request(socket, create, signal)).thread.id, thread.id);
    assert.deepEqual(thread.model, { provider, model_id });
    let updates = await runTurn(socket, thread, edit, 1, signal);
    const first = source.replace('world', 'plait');
    assert.equal(await readFile(join(checkout, 'src/lib.rs'), 'utf8'), first);
    updates += await runTurn(socket, thread, followup, 2, signal);
    assert.equal(await readFile(join(checkout, 'src/lib.rs'), 'utf8'), first.replace(followup.old_text, followup.new_text));
    assert.equal(git(checkout, 'diff', '--name-only'), 'src/lib.rs\n');
    assert.deepEqual(await readFile(join(checkout, '.git/index')), index, 'Git index changed');
    assert((await request(socket, { op: 'list_threads', checkout }, signal)).threads.some(item => item.id === thread.id));
    console.log(JSON.stringify({ result: 'PASS', provider, model_id, thread_id: thread.id, updates, directory, checks: ['real authentication', 'exact model', 'create deduplication', 'two real edits', 'same-thread follow-up', 'sandboxed cargo test on both turns', 'observer reconnect', 'submission deduplication', 'unchanged index'] }));
  } finally {
    if (thread) await request(socket, { op: 'stop', thread_id: thread.id }, AbortSignal.timeout(10000)).catch(() => {});
    child.kill('SIGTERM');
    const cleanupTimer = setTimeout(() => child.kill('SIGKILL'), 10000);
    try { await exited; } finally { clearTimeout(cleanupTimer); }
    console.error(`Disposable fixture retained for inspection: ${directory}`);
  }
}

function selfTest() {
  const base = { dirty_paths: [], kind: 'edit', details: JSON.stringify(edit) };
  assert.equal(approvalKind(base), 'edit');
  assert.equal(approvalKind({ ...base, details: JSON.stringify(followup) }, followup), 'edit');
  assert.throws(() => approvalKind({ ...base, details: JSON.stringify(followup) }));
  assert.equal(approvalKind({ ...base, kind: 'command', details: JSON.stringify({ command }) }), 'command');
  assert.throws(() => approvalKind({ ...base, dirty_paths: ['notes.txt'] }));
  assert.throws(() => approvalKind({ ...base, details: JSON.stringify({ ...edit, path: '.git/config' }) }));
  assert.throws(() => approvalKind({ ...base, kind: 'command', details: JSON.stringify({ command: 'rm -rf .' }) }));
  assert.throws(() => approvalKind({ ...base, details: 'x'.repeat(16385) }));
  console.log('PASS: acceptance check approves only the exact fixture edit and test command.');
}

if (process.argv[2] === '--self-test') selfTest();
else if (process.argv[2] === '--live') await live();
else throw new Error('Usage: node scripts/smoke-agent.mjs --self-test|--live (live requires PI_PROVIDER and PI_MODEL)');
