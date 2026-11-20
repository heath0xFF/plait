// Host policy is private state; repository files may suggest but never approve output.
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { fail, HostError } from './protocol.ts';
import { rootIdentity, type FileIdentity } from './safety.ts';
import type { Store } from './store.ts';
import type { Managed } from './worktrees.ts';

export type Counts = { files: number; links: number; skipped: number; bytes: number; elapsed_ms: number; notes: string[] };
export type Outcome = { warm: boolean; reason: string; directories: string[]; counts: Counts };
export type BuildState = { phase: 'waiting' | 'cloning' | 'warm' | 'cold'; files: number; outcome: Outcome | null };
export type BuildInfo = { schema: 1; worktree_build_output_v1: true; policy: { schema: 1; project: string; identity: FileIdentity; revision: number; paths: string[] }; suggestions: string[] };
const emptyCounts = (): Counts => ({ files: 0, links: 0, skipped: 0, bytes: 0, elapsed_ms: 0, notes: [] });
const cold = (reason: string): Outcome => ({ warm: false, reason, directories: [], counts: emptyCounts() });

export function buildHelper<T>(input: unknown, signal?: AbortSignal, progress?: (counts: Counts) => void): Promise<T> {
  if (signal?.aborted) return Promise.reject(new HostError('cancelled', 'Build output cloning was skipped.'));
  const executable = process.env.PLAIT_HOST_EXE;
  if (!executable) fail('internal', 'Build output helper is not configured.');
  return new Promise((resolve, reject) => {
    const child = spawn(executable, ['build-output'], { env: { ...process.env, PLAIT_BUILD_LIFETIME_FD: '4' }, stdio: ['pipe', 'pipe', 'pipe', process.env.PLAIT_LOCK_FD ? Number(process.env.PLAIT_LOCK_FD) : 'ignore', 'pipe'] });
    let pending = ''; let value: T | undefined; let error: string | undefined;
    const stop = () => { child.kill('SIGKILL'); };
    const timeout = setTimeout(stop, 150_000);
    signal?.addEventListener('abort', stop, { once: true });
    child.stdin!.on('error', () => {}); child.stdin!.end(JSON.stringify(input)); child.stderr!.resume();
    child.stdout!.setEncoding('utf8').on('data', (data: string) => {
      pending += data;
      if (Buffer.byteLength(pending) > 65536) { error = 'Build output reply exceeds its limit.'; stop(); return; }
      for (;;) {
        const end = pending.indexOf('\n'); if (end < 0) break;
        const line = pending.slice(0, end); pending = pending.slice(end + 1);
        try {
          const reply = JSON.parse(line) as { progress?: Counts; value?: T; error?: string };
          if (reply.progress) progress?.(reply.progress);
          if (reply.error) error = reply.error;
          if (reply.value !== undefined) value = reply.value;
        } catch { error = 'Invalid build output reply.'; stop(); }
      }
    });
    const cleanup = () => { clearTimeout(timeout); signal?.removeEventListener('abort', stop); child.stdio[4]?.destroy(); };
    child.once('error', error => { cleanup(); reject(error); });
    child.once('close', code => {
      cleanup();
      if (signal?.aborted) return reject(new HostError('cancelled', 'Build output cloning was skipped.'));
      if (error || code !== 0 || value === undefined) return reject(new HostError('build_output', error ?? 'Build output preparation was interrupted.'));
      resolve(value);
    });
  });
}
type Job = { abort: AbortController; done: Promise<unknown> };
export class BuildOutputs {
  private seeds = new Map<string, Job>();
  private clones = new Map<string, Job>();
  private closing = false;
  private seedErrors = new Map<string, string>();
  private store: Store; private directory: string; private changed: () => void; private busy: (root: string) => boolean;
  constructor(store: Store, directory: string, changed: () => void, busy: (root: string) => boolean) {
    this.store = store; this.directory = directory; this.changed = changed; this.busy = busy;
  }
  async info(checkout: string): Promise<BuildInfo> { return buildHelper({ action: 'info', store: this.directory, root: checkout }); }
  async set(checkout: string, identity: FileIdentity, revision: number, paths: string[]) {
    const info = await buildHelper<BuildInfo>({ action: 'set', store: this.directory, root: checkout, identity, revision, paths });
    this.scheduleSeed(info.policy.project);
    return info;
  }
  state(thread: string): BuildState {
    const value = this.store.setting(`build_output:${thread}`);
    return value ? JSON.parse(value) as BuildState : { phase: 'waiting', files: 0, outcome: null };
  }
  private put(thread: string, state: BuildState) { this.store.setSetting(`build_output:${thread}`, JSON.stringify(state)); this.changed(); }
  async warm(tree: Managed, privateTarget: boolean): Promise<Outcome> {
    const prior = this.state(tree.thread_id);
    if (prior.outcome) return prior.outcome;
    // An interrupted installation is rolled back, never silently resumed file by file.
    if (prior.phase === 'cloning') {
      const outcome = await buildHelper<Outcome>({ action: 'recover', store: this.directory, worktree: tree });
      this.put(tree.thread_id, { phase: outcome.warm ? 'warm' : 'cold', files: outcome.counts.files, outcome }); return outcome;
    }
    const abort = new AbortController();
    const started = Date.now();
    if (this.closing) abort.abort();
    if (this.store.setting(`build_skip:${tree.thread_id}`) === 'yes') abort.abort();
    this.put(tree.thread_id, { phase: 'cloning', files: 0, outcome: null });
    const done = (async () => {
      let outcome: Outcome;
      try {
        if (!abort.signal.aborted && !this.busy(tree.parent.root)) {
          const cancelSeed = () => this.seeds.get(tree.parent.root)?.abort.abort();
          abort.signal.addEventListener('abort', cancelSeed, { once: true });
          try { await this.refreshSeed(tree.parent.root); } finally { abort.signal.removeEventListener('abort', cancelSeed); }
        }
        if (abort.signal.aborted) throw new HostError('cancelled', 'Build output cloning was skipped.');
        outcome = await buildHelper<Outcome>({ action: 'clone', store: this.directory, worktree: tree, private_target: privateTarget }, abort.signal,
          counts => this.put(tree.thread_id, { phase: 'cloning', files: counts.files, outcome: null }));
        const seedError = this.seedErrors.get(tree.parent.root);
        if (!outcome.warm && outcome.reason.includes('no idle build seed') && seedError) outcome = cold(seedError);
      } catch (error) {
        // Recovery finishes before admitting a first message, including after Skip.
        const recovered = await buildHelper<Outcome>({ action: 'recover', store: this.directory, worktree: tree });
        outcome = recovered.warm ? recovered : cold(error instanceof Error ? error.message : 'Build output cloning failed.');
      }
      outcome.counts.elapsed_ms = Date.now() - started;
      this.put(tree.thread_id, { phase: outcome.warm ? 'warm' : 'cold', files: outcome.counts.files, outcome }); return outcome;
    })();
    this.clones.set(tree.thread_id, { abort, done });
    try { return await done; } finally { this.clones.delete(tree.thread_id); }
  }
  skip(thread: string) {
    this.store.setSetting(`build_skip:${thread}`, 'yes'); this.clones.get(thread)?.abort.abort();
    return this.state(thread);
  }
  scheduleSeed(root: string, sourceCache?: string) {
    if (sourceCache !== undefined) this.store.setSetting(`build_source:${root}`, sourceCache);
    // Queue draining reserves its run synchronously first. A user run always wins.
    setImmediate(() => { if (!this.closing && !this.busy(root)) void this.refreshSeed(root); });
  }
  private async refreshSeed(root: string): Promise<void> {
    if (this.closing || this.busy(root)) return;
    const previous = this.seeds.get(root); if (previous) { await previous.done; return; }
    const abort = new AbortController();
    const done = (async () => {
      const identity = await rootIdentity(root);
      if (abort.signal.aborted || this.busy(root)) return;
      await buildHelper({ action: 'seed', store: this.directory, root, identity, generation: randomUUID(), source_cache: this.store.setting(`build_source:${root}`) || null }, abort.signal);
      this.seedErrors.delete(root);
    })().catch(error => { this.seedErrors.set(root, error instanceof Error ? error.message : 'The build seed could not be prepared.'); });
    this.seeds.set(root, { abort, done });
    try { await done; } finally { if (this.seeds.get(root)?.done === done) this.seeds.delete(root); }
  }
  /** Stops the low-priority helper before a local run attempts its writer lease. */
  async yieldToRun(root: string) { const job = this.seeds.get(root); if (job) { job.abort.abort(); await job.done; } }
  async close() {
    this.closing = true; for (const job of [...this.seeds.values(), ...this.clones.values()]) job.abort.abort();
    await Promise.allSettled([...this.seeds.values(), ...this.clones.values()].map(job => job.done));
  }
}
