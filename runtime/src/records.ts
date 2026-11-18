// The host's records of a checkout: one before and one after every run, in
// every harness. `plait-host record` writes each one into the host's own store
// (`STORE/records`) and never into the person's repository; this file decides
// when, keeps the list in `host.sqlite`, and prunes what has expired. Nothing
// here reaches a client yet: undo will read these rows.
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { existsSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { HostError } from './protocol.ts';
import type { Store } from './store.ts';
import { acquireCheckout } from './safety.ts';

/** Records are kept this long, those of archived threads included. */
export const RETENTION_MS = 30 * 24 * 60 * 60 * 1000;
const PRUNE_INTERVAL_MS = 24 * 60 * 60 * 1000;
/** A checkout's first record reads every file; later ones take a fraction of a second. */
const RECORD_TIMEOUT_MS = 300_000;
/** A run that rewrote many files lists each one; the reply is bounded all the same. */
const REPLY_LIMIT = 256 * 1024 * 1024;
/** Record IDs per prune request, well inside the helper's 1 MiB request limit. */
const PRUNE_BATCH = 5000;
export const RECORD_FAILED = 'The host could not record the checkout before this run, so the run did not start.';

type Uncovered = { path: string; reason: string };
type ChangedPath = {
  path: string; before_mode: string | null; before_oid: string | null; after_mode: string | null; after_oid: string | null;
  added: number | null; removed: number | null;
};
/** What `plait-host record` replies. */
type Made = {
  commit: string; tree: string; object_format: string; head: string | null; head_ref: string | null;
  not_covered: Uncovered[]; not_covered_total: number; paths: ChangedPath[] | null; elapsed_ms: number;
};

/** Runs one `plait-host` record helper: JSON in on stdin, JSON out on stdout. */
function helper<T>(operation: 'record' | 'prune', input: unknown, lease?: number): Promise<T> {
  const executable = process.env.PLAIT_HOST_EXE;
  if (!executable) return Promise.reject(new Error('Host record helper is not configured.'));
  return new Promise<T>((resolve, reject) => {
    // The store lock travels with the helper, as with every helper: a new host
    // cannot start while one still writes the store. Its own process group lets a
    // timeout stop the sandboxed copy it starts as well.
    const child = spawn(executable, [operation], {
      stdio: ['pipe', 'pipe', 'pipe', process.env.PLAIT_LOCK_FD ? Number(process.env.PLAIT_LOCK_FD) : 'ignore', lease ?? 'ignore'],
      detached: true,
    });
    const chunks: Buffer[] = []; let size = 0; let diagnostic = '';
    const stop = () => { try { process.kill(-child.pid!, 'SIGKILL'); } catch { child.kill('SIGKILL'); } };
    const timer = setTimeout(stop, RECORD_TIMEOUT_MS);
    child.stdin!.on('error', () => {});
    child.stdin!.end(JSON.stringify(input));
    child.stdout!.on('data', (chunk: Buffer) => { size += chunk.length; if (size > REPLY_LIMIT) stop(); else chunks.push(chunk); });
    child.stderr!.setEncoding('utf8');
    child.stderr!.on('data', data => { diagnostic = (diagnostic + data).slice(-4096); });
    child.once('error', error => { clearTimeout(timer); reject(error); });
    child.once('close', code => {
      clearTimeout(timer);
      // The diagnostic stays inside the runtime; clients only ever see a fixed message.
      if (code !== 0 || size > REPLY_LIMIT) return reject(new Error(diagnostic.trim() || `plait-host ${operation} failed`));
      try { resolve(JSON.parse(Buffer.concat(chunks).toString('utf8'))); } catch { reject(new Error(`plait-host ${operation} replied badly`)); }
    });
  });
}

export class Records {
  private store: Store;
  /** The host store; the helper keeps records in its `records` directory. */
  private directory: string;
  /** Records in one checkout are made one at a time, in the order asked. */
  private tails = new Map<string, Promise<unknown>>();
  /** Records being made now; a prune waits for them and holds new ones back. */
  private running = new Set<Promise<unknown>>();
  private pruning?: Promise<void>;
  private pending = new Set<Promise<unknown>>();
  private timer?: NodeJS.Timeout;
  private closing = false;

  constructor(store: Store, directory: string) {
    this.store = store; this.directory = resolve(directory);
  }
  /** Takes any record a host stop left unmade, then prunes now and once a day. */
  start() {
    this.recover();
    void this.prune().catch(() => {});
    this.timer = setInterval(() => void this.prune().catch(() => {}), PRUNE_INTERVAL_MS);
    this.timer.unref();
  }
  /** Admission can defer behind startup recovery without blocking the socket reply. */
  busy(checkout: string) { return this.tails.has(checkout); }
  async settled(checkout: string) { await this.tails.get(checkout); }
  /**
   * The record a run starts from, made before any harness starts. A run that
   * cannot be recorded does not run: this throws `record_failed`.
   */
  async before(thread: string, run: string, checkout: string, lease?: number): Promise<string> {
    const id = randomUUID();
    try {
      this.insert(id, thread, run, 'before', checkout);
      this.ready(id, await this.queue(checkout, async () => {
        await this.recoverBefore(checkout, lease);
        return this.make(checkout, id, undefined, lease);
      }));
      return id;
    } catch {
      try { this.mark(id, 'failed'); } catch { /* The database itself may be unavailable. */ }
      throw new HostError('record_failed', RECORD_FAILED);
    }
  }
  /**
   * The record a run ends at, after every outcome, linked to its `before`
   * record. It never throws: a run that ends has ended, and a failed record is
   * only marked so that undo knows it cannot rely on it.
   */
  async after(before: string, lease?: number, restart = false): Promise<void> {
    let id: string | undefined;
    try {
      const row = this.store.db.prepare("SELECT thread,run_request,checkout,commit_id FROM records WHERE id=? AND kind='before' AND state='ready'").get(before);
      if (!row) return;
      const checkout = String(row.checkout);
      const existing = this.store.db.prepare("SELECT id FROM records WHERE before=? AND kind='after'").get(before);
      id = existing ? String(existing.id) : randomUUID();
      if (existing) this.store.db.prepare("UPDATE records SET state='recording',after_restart=?,created_at=? WHERE id=?").run(restart ? 1 : 0, Date.now(), id);
      else this.insert(id, String(row.thread), String(row.run_request), 'after', checkout, before, restart);
      const after = id;
      this.ready(id, await this.queue(checkout, async () => {
        const recoveryLease = restart ? await acquireCheckout(checkout) : undefined;
        try { return await this.make(checkout, after, String(row.commit_id), recoveryLease?.fd ?? lease); }
        finally { await recoveryLease?.close(); }
      }));
    } catch {
      if (id) try { this.mark(id, 'failed'); } catch { /* The store is closing. */ }
    }
  }
  /**
   * A host that stopped mid-run never made that run's later record. It is made
   * now, before anything else in that checkout, and marked as taken after a
   * restart: it may include changes made while the host was down.
   */
  private recover() {
    const db = this.store.db;
    const open = db.prepare(`SELECT b.id FROM records b WHERE b.kind='before' AND b.state='ready'
      AND NOT EXISTS (SELECT 1 FROM records a WHERE a.kind='after' AND a.before=b.id AND (a.state='ready' OR (a.state='failed' AND a.after_restart=0))) ORDER BY b.created_at, b.rowid`).all().map(row => String(row.id));
    // A record cut short by the stop left nothing usable; the before record of a run that never started its harness stays failed.
    db.prepare("UPDATE records SET state='failed' WHERE kind='before' AND state='recording'").run();
    for (const before of open) this.track(this.after(before, undefined, true));
  }
  /** A busy checkout may have deferred startup recovery until its next leased run. */
  private async recoverBefore(checkout: string, lease?: number) {
    const pending = this.store.db.prepare(`SELECT a.id,b.commit_id FROM records a JOIN records b ON b.id=a.before
      WHERE a.checkout=? AND a.kind='after' AND a.state='failed' AND a.after_restart=1 AND b.state='ready'`).all(checkout);
    for (const row of pending) {
      const id = String(row.id);
      this.store.db.prepare("UPDATE records SET state='recording',created_at=? WHERE id=?").run(Date.now(), id);
      try { this.ready(id, await this.make(checkout, id, String(row.commit_id), lease)); }
      catch { this.mark(id, 'failed'); }
    }
  }
  /**
   * Drops records older than `RETENTION_MS`, and the store's copies of
   * checkouts that no longer exist. No record is made while it runs, so Git's
   * prune never sees an object a record has written but not yet named.
   */
  prune(now = Date.now()): Promise<void> {
    if (this.pruning) return this.pruning;
    if (this.closing || !existsSync(join(this.directory, 'records'))) return Promise.resolve();
    this.pruning = (async () => {
      await Promise.allSettled([...this.running]);
      const expired = this.store.db.prepare(`SELECT id FROM records r WHERE created_at<? AND state<>'recording'
        AND NOT EXISTS (SELECT 1 FROM records pair WHERE pair.thread=r.thread AND pair.run_request=r.run_request
          AND (pair.created_at>=? OR pair.state='recording'))
        AND NOT (kind='before' AND state='ready' AND NOT EXISTS (SELECT 1 FROM records a WHERE a.before=r.id))
        ORDER BY created_at`).all(now - RETENTION_MS, now - RETENTION_MS).map(row => String(row.id));
      // One request even when nothing expired, so checkouts that are gone are dropped.
      for (let start = 0; start === 0 || start < expired.length; start += PRUNE_BATCH) {
        const batch = expired.slice(start, start + PRUNE_BATCH);
        await helper('prune', { store: this.directory, remove: batch });
        this.forget(batch);
      }
    })().finally(() => { this.pruning = undefined; });
    this.track(this.pruning.catch(() => {}));
    return this.pruning;
  }
  /** Waits for every record and prune under way, and stops the daily prune. */
  async close() {
    this.closing = true;
    clearInterval(this.timer);
    while (this.pending.size) await Promise.allSettled([...this.pending]);
  }

  private make(checkout: string, id: string, before: string | undefined, lease?: number) {
    return helper<Made>('record', { checkout, store: this.directory, id, ...(before ? { before } : {}) }, lease);
  }
  private queue<T>(checkout: string, work: () => Promise<T>): Promise<T> {
    const turn = async () => {
      while (this.pruning) await this.pruning.catch(() => {});
      const task = work();
      this.running.add(task);
      try { return await task; } finally { this.running.delete(task); }
    };
    const result = (this.tails.get(checkout) ?? Promise.resolve()).then(turn, turn);
    const tail = result.catch(() => {});
    this.tails.set(checkout, tail);
    this.track(tail);
    void tail.then(() => { if (this.tails.get(checkout) === tail) this.tails.delete(checkout); });
    return result;
  }
  private track(work: Promise<unknown>) {
    this.pending.add(work);
    void work.finally(() => this.pending.delete(work));
  }
  private insert(id: string, thread: string, run: string, kind: 'before' | 'after', checkout: string, before?: string, restart = false) {
    this.store.db.prepare("INSERT INTO records (id,thread,run_request,kind,checkout,before,state,after_restart,created_at) VALUES (?,?,?,?,?,?,'recording',?,?)")
      .run(id, thread, run, kind, checkout, before ?? null, restart ? 1 : 0, Date.now());
  }
  private mark(id: string, state: 'failed') {
    this.store.db.prepare('UPDATE records SET state=? WHERE id=?').run(state, id);
  }
  /** Saves a made record and, for a run's later record, every path the run changed. */
  private ready(id: string, made: Made) {
    this.transaction(() => {
      this.store.db.prepare("UPDATE records SET state='ready',commit_id=?,tree=?,head=?,head_ref=?,not_covered=?,not_covered_total=? WHERE id=?")
        .run(made.commit, made.tree, made.head, made.head_ref, JSON.stringify(made.not_covered), made.not_covered_total, id);
      const path = this.store.db.prepare('INSERT INTO record_paths VALUES (?,?,?,?,?,?,?,?)');
      for (const changed of made.paths ?? []) {
        path.run(id, changed.path, changed.before_mode, changed.before_oid, changed.after_mode, changed.after_oid, changed.added, changed.removed);
      }
    });
  }
  private forget(ids: string[]) {
    const paths = this.store.db.prepare('DELETE FROM record_paths WHERE record=?');
    const record = this.store.db.prepare('DELETE FROM records WHERE id=?');
    this.transaction(() => { for (const id of ids) { paths.run(id); record.run(id); } });
  }
  private transaction(work: () => void) {
    this.store.db.exec('BEGIN IMMEDIATE');
    try { work(); this.store.db.exec('COMMIT'); } catch (error) { this.store.db.exec('ROLLBACK'); throw error; }
  }
}
