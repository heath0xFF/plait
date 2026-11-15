import { execFile } from 'node:child_process';
import { mkdtemp, mkdir, realpath, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { promisify } from 'node:util';

export const exec = promisify(execFile);
process.env.PLAIT_HOST_EXE = process.env.PLAIT_HOST_BINARY ?? resolve('../target/debug/plait-host');

export async function setup(t: { after: (fn: () => Promise<void>) => void }) {
  const base = await realpath(await mkdtemp(join(tmpdir(), 'plait-sandbox-')));
  const root = join(base, 'repo'); await mkdir(root);
  t.after(async () => { await rm(base, { recursive: true, force: true }); });
  await exec('/usr/bin/git', ['init', '-q', root]);
  return { root, base };
}
