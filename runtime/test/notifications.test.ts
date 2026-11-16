import assert from 'node:assert/strict';
import { Writable } from 'node:stream';
import { setImmediate as tick } from 'node:timers/promises';
import { test } from 'node:test';
import { forwardChanges } from '../src/notifications.ts';

function source() {
  const listeners = new Set<(id: string) => void>();
  return {
    subscribe(listener: (id: string) => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    change(id: string) { for (const listener of listeners) listener(id); },
    listeners,
  };
}

test('notifications coalesce IDs within a turn and unsubscribe without idle work', async () => {
  const changes = source(); const frames: string[] = [];
  const output = new Writable({ write(chunk, _, done) { frames.push(chunk.toString()); done(); } });
  const stop = forwardChanges(changes, output);
  changes.change('one'); changes.change('one'); changes.change('two');
  await tick();
  assert.deepEqual(frames, ['one\ntwo\n']);
  await tick();
  assert.equal(frames.length, 1);
  changes.change('cancelled'); stop(); await tick();
  assert.equal(frames.length, 1);
  assert.equal(changes.listeners.size, 0);
  assert.equal(output.listenerCount('drain'), 0);
  output.destroy();
});

test('backpressure retains bounded invalidation, then resumes on drain', async () => {
  const changes = source(); const frames: string[] = [];
  let drain: () => void = () => {};
  const output = new Writable({ highWaterMark: 1, write(chunk, _, done) { frames.push(chunk.toString()); drain = done; } });
  const stop = forwardChanges(changes, output);
  changes.change('first'); await tick();
  for (let index = 0; index < 10000; index++) changes.change(`thread-${index}`);
  await tick();
  assert.deepEqual(frames, ['first\n']);
  drain(); await tick();
  assert.deepEqual(frames, ['first\n', '*\n']);
  changes.change('last'); drain(); await tick();
  assert.deepEqual(frames, ['first\n', '*\n', 'last\n']);
  stop(); drain(); output.destroy();
});

test('invalid IDs cannot inject notification frames', async () => {
  const changes = source(); const frames: string[] = [];
  const output = new Writable({ write(chunk, _, done) { frames.push(chunk.toString()); done(); } });
  const stop = forwardChanges(changes, output);
  for (const id of ['', 'injected\nother', 'x'.repeat(129)]) {
    changes.change(id); await tick();
  }
  assert.deepEqual(frames, ['*\n', '*\n', '*\n']);
  stop(); output.destroy();
});
