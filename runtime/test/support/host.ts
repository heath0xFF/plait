import assert from 'node:assert/strict';
import { spawn, execFile } from 'node:child_process';
import type { ChildProcess } from 'node:child_process';
import { connect } from 'node:net';
import { mkdtemp, readFile, writeFile, rm, stat, realpath, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { promisify } from 'node:util';
import type { Snapshot } from '../../src/protocol.ts';

export const exec = promisify(execFile);
export const binary = process.env.PLAIT_HOST_BINARY ?? resolve('../target/debug/plait-host');
export const fixture = process.env.PLAIT_TEST_RUNTIME ?? resolve('test/fixture.ts');
export const delay = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));
export async function childPids(parent: number) {
  return (await exec('/bin/ps', ['-axo', 'pid=,ppid='])).stdout.trim().split('\n')
    .map(line => line.trim().split(/\s+/).map(Number)).filter(([, ppid]) => ppid === parent).map(([pid]) => pid);
}
export async function until<T>(read: () => Promise<T>, accept: (value: T) => boolean, timeout = 15000): Promise<T> {
  const deadline = Date.now() + timeout;
  let last: unknown;
  while (Date.now() < deadline) {
    try { const value = await read(); if (accept(value)) return value; last = value; } catch (error) { last = error; }
    await delay(40);
  }
  throw new Error(`Condition timed out: ${JSON.stringify(last)}`);
}
// Tests inspect wire objects so schema failures remain visible rather than coerced.
type Reply = { ok: boolean; result: { thread: Snapshot; threads: Snapshot[]; default_model: unknown; models: unknown[]; accepted: boolean }; error?: { code: string; message: string } };
export function request(store: string, value: unknown): Promise<Reply> {
  return raw(store, JSON.stringify(value) + '\n');
}
export function raw(store: string, input: string): Promise<Reply> {
  return new Promise((resolve, reject) => {
    const socket = connect(join(store, 'host.sock'));
    let output = '';
    // Decode across chunk boundaries: a large reply can split a multi-byte character.
    socket.setEncoding('utf8');
    socket.setTimeout(5000, () => socket.destroy(new Error('Socket timeout')));
    socket.once('connect', () => socket.write(input));
    socket.on('data', chunk => {
      output += chunk.toString();
      const end = output.indexOf('\n');
      if (end >= 0) { socket.destroy(); try { resolve(JSON.parse(output.slice(0, end))); } catch (error) { reject(error); } }
    });
    socket.once('error', reject);
    socket.once('end', () => reject(new Error('Connection closed before reply')));
  });
}
export async function setup(t: { after: (fn: () => Promise<void>) => void }, scenario = 'answer', storeAlias = false) {
  const base = await realpath(await mkdtemp(join(storeAlias ? '/tmp' : tmpdir(), 'plait-host-test-')));
  const repo = join(base, 'repo'); const store = join(storeAlias ? base.replace('/private/tmp/', '/tmp/') : base, 'store');
  await exec('/bin/mkdir', ['-p', repo, store]);
  await exec('/usr/bin/git', ['init', '-q', repo]);
  await writeFile(join(repo, 'notes.txt'), 'before\n');
  await writeFile(join(repo, 'AGENTS.md'), 'Keep the original dirty paths.\n');
  await exec('/usr/bin/git', ['-C', repo, 'add', '.']);
  await exec('/usr/bin/git', ['-C', repo, '-c', 'user.name=Test', '-c', 'user.email=test@invalid', 'commit', '-qm', 'fixture']);
  const children: ChildProcess[] = [];
  const start = async (mode = scenario) => {
    const child = spawn(binary, ['run', '--store', store, '--runtime', fixture], { stdio: ['ignore', 'ignore', 'pipe'], env: { ...process.env, PLAIT_TEST_SCENARIO: mode } });
    children.push(child);
    let stderr = ''; child.stderr!.on('data', chunk => { stderr = (stderr + chunk.toString()).slice(-8192); });
    await until(async () => {
      if (child.exitCode !== null) throw new Error(`Host exited: ${stderr}`);
      return request(store, { op: 'status' });
    }, reply => reply.ok);
    return child;
  };
  t.after(async () => {
    for (const child of children) if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
    await delay(400); await rm(base, { recursive: true, force: true });
  });
  const host = await start();
  const create = async (id = 'create-1') => {
    const reply = await request(store, { op: 'create_thread', checkout: repo, provider: 'faux', model_id: 'faux-1', request_id: id });
    assert.equal(reply.ok, true, JSON.stringify(reply)); return reply.result.thread;
  };
  const watch = async (id: string) => (await request(store, { op: 'watch', thread_id: id })).result.thread;
  const submit = (id: string, request_id = 'submit-1', text = 'Do the scripted task.') => request(store, { op: 'submit', thread_id: id, request_id, text });
  const approve = (id: string, approval_id: string, allow = true) => request(store, { op: 'approve', thread_id: id, approval_id, allow });
  return { base, repo, store, host, start, create, watch, submit, approve };
}
