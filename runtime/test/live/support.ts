// Shared pieces for live tests: real harnesses, real models, the person's own
// sign-in. They run only when asked, with `PLAIT_LIVE=codex,claude npm run test:live`,
// and never in `npm test` or check.sh. Each turn is small but uses the person's plan.
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { exec, setup } from '../support/safety.ts';
import type { Snapshot } from '../../src/protocol.ts';
import type { Runtime } from '../../src/runtime.ts';

type Context = { after(fn: () => Promise<void>): void };
export type Reply = { ok: boolean; result?: any; error?: { code: string; message: string } };

/** Test options that skip unless `PLAIT_LIVE` names this harness. */
export function live(harness: string) {
  const asked = (process.env.PLAIT_LIVE ?? '').split(',').map(name => name.trim());
  return { skip: asked.includes(harness) ? false : `set PLAIT_LIVE=${harness} to run against the real ${harness}`, timeout: 180_000 };
}

/** A throwaway checkout with one committed file. */
export async function liveRepo(t: Context) {
  const { root, base } = await setup(t);
  await writeFile(join(root, 'README.md'), 'A scratch checkout for a live test.\n');
  const git = (...args: string[]) => exec('/usr/bin/git', ['-C', root, '-c', 'user.name=Plait', '-c', 'user.email=plait@example.invalid', ...args]);
  await git('add', '.');
  await git('commit', '-qm', 'Start');
  const store = join(base, 'store');
  await mkdir(store);
  return { root, base, store };
}

/** Requests and waits against one runtime. */
export function driver(runtime: Runtime) {
  const call = (request: Record<string, unknown>) => runtime.reply({ protocol_version: 4, ...request }) as Promise<Reply>;
  const until = async (id: string, done: (snapshot: Snapshot) => boolean, ms = 150_000) => {
    const deadline = Date.now() + ms;
    for (;;) {
      const snapshot = runtime.snapshot(id);
      if (done(snapshot)) return structuredClone(snapshot);
      // Only the state is printed: a live transcript can quote the machine or the account.
      if (Date.now() > deadline) assert.fail(`Timed out; status ${snapshot.status}, error ${snapshot.error?.code ?? 'none'}`);
      await new Promise(resolve => setTimeout(resolve, 50));
    }
  };
  const idle = (id: string, ms?: number) => until(id, snapshot => !runtime.sessions.has(id) && !['running', 'awaiting_approval'].includes(snapshot.status), ms);
  /** Approves every request the turn makes, and returns the finished thread and how many were asked. */
  const approveAll = async (id: string) => {
    let asked = 0;
    for (;;) {
      const snapshot = await until(id, snapshot => !!snapshot.pending_approval || (!runtime.sessions.has(id) && snapshot.status !== 'running'));
      if (!snapshot.pending_approval) return { done: snapshot, asked };
      asked += 1;
      const reply = await call({ op: 'approve', thread_id: id, approval_id: snapshot.pending_approval.id, allow: true });
      assert.equal(reply.ok, true, reply.error?.code ?? 'refused');
    }
  };
  return { call, until, idle, approveAll };
}
