import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import type { ChildProcess } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { isAbsolute, join } from 'node:path';
import type { Snapshot } from '../../src/protocol.ts';
import { delay, exec, until } from './host.ts';

// The installed app connects to the real fixture host. No sample-view code enters the app.
export async function installedClient(base: string, store: string, repo: string, thread: Snapshot) {
  if (process.env.PLAIT_VERIFY_INSTALLED_UI !== '1') return undefined;
  assert(process.platform === 'darwin' && process.env.GITHUB_ACTIONS === 'true');
  const artifacts = process.env.PLAIT_INSTALLED_UI_ARTIFACTS;
  assert(artifacts && isAbsolute(artifacts));
  const directory = join(base, 'desktop');
  const file = join(directory, 'desktop.json');
  const draft = 'Review the real test output before making the next change.';
  await mkdir(directory, { mode: 0o700 });
  await writeFile(file, JSON.stringify({
    schema_version: 1, host_store: store, checkout: repo, selected: thread.id,
    threads: { [thread.id]: { checkout: repo, model: thread.model, title: '',
      draft: { text: draft, revision: 2, submitted: null }, pending: null } },
  }), { mode: 0o600 });
  let child: ChildProcess | undefined;
  const saved = async () => JSON.parse(await readFile(file, 'utf8'));
  return {
    open: async (first: boolean) => {
      assert(!child || child.exitCode !== null || child.signalCode !== null);
      const state = await saved();
      assert.equal(state.selected, thread.id);
      assert.equal(state.threads[thread.id].draft.text, draft);
      // Deriving a fresh title proves this process observed the host, not just the seeded file.
      state.threads[thread.id].title = '';
      await writeFile(file, JSON.stringify(state), { mode: 0o600 });
      child = spawn('/Applications/Plait.app/Contents/MacOS/plait', first ? [repo] : [], {
        stdio: ['ignore', 'ignore', 'pipe'],
        env: { PATH: process.env.PATH, HOME: process.env.HOME, PLAIT_HOST_STORE: store, PLAIT_DESKTOP_STATE_DIR: directory },
      });
      let errors = '';
      child.stderr!.on('data', chunk => { errors = (errors + chunk.toString()).slice(-8192); });
      child.on('error', error => { errors = error.message; });
      const observed = await until(async () => ({
        state: await saved(),
        error: child!.exitCode !== null || child!.signalCode !== null ? `Installed client exited: ${errors}` : null,
      }), value => value.error !== null || value.state.threads[thread.id].title !== '');
      assert.equal(observed.error, null, observed.error ?? 'Installed client must stay running.');
      await delay(500);
      assert.equal((await saved()).threads[thread.id].draft.text, draft);
    },
    capture: async (name: 'active' | 'coding') => {
      await exec('/usr/sbin/screencapture', ['-x', join(artifacts, `installed-client-${name}.png`)]);
    },
    close: async () => {
      if (!child || child.exitCode !== null || child.signalCode !== null) return;
      child.kill('SIGTERM');
      try { await until(async () => child!.signalCode !== null || child!.exitCode !== null, value => value, 3000); }
      finally { if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL'); }
    },
  };
}
