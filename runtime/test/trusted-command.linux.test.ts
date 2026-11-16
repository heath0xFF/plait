import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { mkdtemp, readFile, realpath, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { promisify } from 'node:util';
import { test } from 'node:test';
import { trustedCommand } from '../src/command/trusted.ts';
import { rootIdentity } from '../src/safety.ts';

test('Linux denies trusted commands in both runtime and direct guardian entry points', async t => {
  const root = await realpath(await mkdtemp(join(tmpdir(), 'plait-linux-trusted-denial-')));
  t.after(async () => { await rm(root, { recursive: true, force: true }); });
  const run = { snapshot: { checkout: root }, baseline: [], owned: new Map(), rootIdentity: await rootIdentity(root) };
  const script = 'printf unsafe > marker';
  // A trusted policy is not a way around the missing containment decision.
  await assert.rejects(trustedCommand(run, script, new AbortController().signal, () => {}), /containment is not enabled/);
  await assert.rejects(promisify(execFile)(resolve('../target/debug/plait-host'), ['trusted-command', root, script]), /containment is not enabled/);
  await assert.rejects(readFile(join(root, 'marker')), { code: 'ENOENT' });
});
