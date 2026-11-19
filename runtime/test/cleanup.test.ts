import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { EventEmitter } from 'node:events';
import { test } from 'node:test';
import { CleanupProof, groupExited, guardianProof, NEVER } from '../src/cleanup.ts';
import type { ChildProcess } from 'node:child_process';

const tick = () => new Promise(resolve => setTimeout(resolve, 5));

test('a close call and a failed exit check never count as cleanup proof', async () => {
  const proof = new CleanupProof();
  const process = new EventEmitter() as ChildProcess;
  proof.track(groupExited(process), 'Claude Code exit is unconfirmed.');
  process.emit('close', 0, null);
  await proof.wait(1);
  assert.match(proof.reason()!, /unconfirmed/);
  // A spawn failure proves nothing was started, unlike an exit-check failure.
  process.emit('error', new Error('spawn failed'));
  await proof.proved();
  assert.equal(proof.reason(), undefined);
  proof.track(Promise.reject(new Error('EPERM')), 'Group could not be checked.');
  await proof.wait(1);
  assert.match(proof.reason()!, /could not be checked/);
});

test('a crashed harness releases only after its normal process group disappears', async () => {
  const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { detached: true, stdio: 'ignore' });
  const proof = new CleanupProof();
  proof.track(groupExited(child), 'Group still alive.');
  await new Promise<void>(resolve => child.once('spawn', resolve));
  await proof.wait(1);
  assert.ok(proof.reason());
  child.kill('SIGKILL');
  await proof.proved();
  assert.equal(proof.reason(), undefined);
});

test('a helper close without a private receipt retains uncertainty; late proof clears it', async () => {
  const child = new EventEmitter() as ChildProcess;
  const receipt = new EventEmitter();
  Object.assign(child, { pid: 12345, stdio: [...Array(8).fill(null), receipt] });
  const proof = new CleanupProof();
  proof.track(guardianProof(child), 'Helper did not confirm.');
  child.emit('close', 0, null);
  await tick();
  assert.ok(proof.reason());
  receipt.emit('data', Buffer.from('clean\n'));
  await proof.proved();
  assert.equal(proof.reason(), undefined);
  proof.track(NEVER, 'Another independent helper is uncertain.');
  await proof.wait(1);
  assert.ok(proof.reason());
});
