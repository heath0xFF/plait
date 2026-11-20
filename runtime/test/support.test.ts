// The shared fixtures have rules of their own. These tests hold them.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdir, stat } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';
import { setup } from './support/safety.ts';

const pause = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));

test('the shared test folder is removed while another process still writes into it', async () => {
  const hooks: (() => Promise<void>)[] = [];
  const { base } = await setup({ after: fn => { hooks.push(fn); } });
  const store = join(base, 'store'); await mkdir(store);
  const seeds = join(store, 'build-output-seeds');
  // Like the build-seed helper after a run: it outlives the test body for a moment, and the hook that closes a test's
  // runtime runs after this folder's removal.
  const writer = spawn(process.execPath, ['-e', `
    const fs = require('node:fs'), path = require('node:path'), dir = ${JSON.stringify(seeds)};
    for (let n = 0, end = Date.now() + 200; Date.now() < end; n++) {
      try { fs.mkdirSync(dir); } catch (error) { if (error.code !== 'EEXIST') break; }
      try { fs.writeFileSync(path.join(dir, n + '.lock'), ''); } catch { break; }
    }`], { stdio: 'ignore' });
  const exited = new Promise(resolve => writer.once('exit', resolve));
  for (const end = Date.now() + 10000; !await stat(seeds).catch(() => null);) { assert.ok(Date.now() < end, 'the writer never started'); await pause(2); }
  await hooks[0]();
  await exited;
  await assert.rejects(stat(base));
});
