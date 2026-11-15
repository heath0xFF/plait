import { DatabaseSync } from 'node:sqlite';
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import type { ModelChoice, Snapshot, InputRecord } from './protocol.ts';
import { fail, inputMessage } from './protocol.ts';
import { boundSnapshot } from './transcript.ts';
import type { FileIdentity, Postimage } from './safety.ts';

export class Store {
  db: DatabaseSync;
  constructor(directory: string) {
    this.db = new DatabaseSync(join(directory, 'host.sqlite'));
    this.db.exec(`PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
      CREATE TABLE IF NOT EXISTS threads(id TEXT PRIMARY KEY, snapshot TEXT NOT NULL, head INTEGER NOT NULL DEFAULT 0);
      CREATE TABLE IF NOT EXISTS requests(scope TEXT NOT NULL, id TEXT NOT NULL, hash TEXT NOT NULL, result TEXT NOT NULL, input TEXT, entry INTEGER, anchor INTEGER, PRIMARY KEY(scope,id));
      CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS roots(id TEXT PRIMARY KEY, identity TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS postimages(thread_id TEXT NOT NULL, path TEXT NOT NULL, image TEXT NOT NULL, PRIMARY KEY(thread_id,path));`);
    for (const [table, column, type] of [['threads', 'head', 'INTEGER NOT NULL DEFAULT 0'], ['requests', 'input', 'TEXT'], ['requests', 'entry', 'INTEGER'], ['requests', 'anchor', 'INTEGER']]) {
      if (!this.db.prepare(`PRAGMA table_info(${table})`).all().some(row => row.name === column)) this.db.exec(`ALTER TABLE ${table} ADD COLUMN ${column} ${type}`);
    }
    for (const snapshot of this.recoverable()) {
      if (snapshot.status === 'running' || snapshot.status === 'awaiting_approval') {
        snapshot.status = 'interrupted';
        snapshot.pending_approval = null;
        snapshot.live_text = '';
        snapshot.error = { code: 'interrupted', message: 'Host stopped. Effects may have completed; inspect the checkout before a new submission.' };
        for (const tool of snapshot.tools) if (tool.status === 'running') tool.status = 'interrupted';
        this.save(snapshot);
      }
    }
  }
  recoverable(): Snapshot[] {
    return this.db.prepare("SELECT snapshot FROM threads WHERE json_extract(snapshot,'$.status') IN ('running','awaiting_approval')").all()
      .map(row => JSON.parse(String(row.snapshot)) as Snapshot);
  }
  list(checkout: string) {
    return this.db.prepare("SELECT id,json_extract(snapshot,'$.checkout') AS checkout,json_extract(snapshot,'$.model') AS model,json_extract(snapshot,'$.status') AS status FROM threads WHERE json_extract(snapshot,'$.checkout')=? ORDER BY rowid DESC LIMIT 100").all(checkout)
      .map(row => ({ id: String(row.id), checkout: String(row.checkout), model: JSON.parse(String(row.model)), status: String(row.status) }));
  }
  get(id: string): Snapshot {
    const row = this.db.prepare('SELECT snapshot FROM threads WHERE id=?').get(id);
    if (!row) fail('not_found', 'Thread not found.');
    return JSON.parse(String(row.snapshot));
  }
  save(snapshot: Snapshot, head = 0) {
    this.db.prepare('INSERT INTO threads (id,snapshot,head) VALUES (?,?,?) ON CONFLICT(id) DO UPDATE SET snapshot=excluded.snapshot,head=MAX(threads.head,excluded.head)')
      .run(snapshot.id, JSON.stringify(boundSnapshot(snapshot)), head);
  }
  inputs(id: string): InputRecord[] {
    return this.db.prepare('SELECT id,input,entry,anchor FROM requests WHERE scope=? AND input IS NOT NULL ORDER BY rowid DESC LIMIT 100').all(id).reverse()
      .map(row => ({ request_id: String(row.id), text: JSON.parse(String(row.input)), entry: row.entry === null ? null : Number(row.entry), anchor: Number(row.anchor ?? 0) }));
  }
  place(id: string, requestId: string, entry: number) {
    this.db.prepare('UPDATE requests SET entry=? WHERE scope=? AND id=?').run(entry, id, requestId);
  }
  owned(id: string): Postimage[] {
    return this.db.prepare('SELECT image FROM postimages WHERE thread_id=? LIMIT 500').all(id).map(row => JSON.parse(String(row.image)));
  }
  record(id: string, image: Postimage) {
    // FULL synchronous commits provenance before a tool reports completion.
    this.db.prepare('INSERT INTO postimages VALUES (?,?,?) ON CONFLICT(thread_id,path) DO UPDATE SET image=excluded.image')
      .run(id, image.path, JSON.stringify(image));
  }
  rootIdentity(id: string): FileIdentity {
    const row = this.db.prepare('SELECT identity FROM roots WHERE id=?').get(id);
    if (!row) fail('permission_denied', 'Thread has no saved checkout identity; create a new thread.');
    return JSON.parse(String(row.identity));
  }
  defaultModel(): ModelChoice | null {
    const row = this.db.prepare("SELECT value FROM settings WHERE key='default_model'").get();
    return row ? JSON.parse(String(row.value)) : null;
  }
  setDefault(model: ModelChoice) {
    this.db.prepare("INSERT INTO settings VALUES ('default_model',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").run(JSON.stringify(model));
  }
  duplicate(scope: string, id: string, input: unknown): unknown | undefined {
    const row = this.db.prepare('SELECT hash,result FROM requests WHERE scope=? AND id=?').get(scope, id);
    if (!row) return undefined;
    if (row.hash !== digest(input)) fail('conflict', 'Request ID was already used for different input.');
    return JSON.parse(String(row.result));
  }
  admit(scope: string, id: string, input: unknown, result: unknown, snapshot: Snapshot, root?: FileIdentity) {
    // Acceptance and dedup commit before the harness can execute any effect.
    this.db.exec('BEGIN IMMEDIATE');
    try {
      const head = Number(this.db.prepare('SELECT head FROM threads WHERE id=?').get(snapshot.id)?.head ?? 0);
      if (scope === snapshot.id && typeof input === 'string') {
        snapshot.messages.push(inputMessage({ request_id: id, text: input, entry: null, anchor: head }));
        if (snapshot.messages.length > 100) { snapshot.messages = snapshot.messages.slice(-100); snapshot.truncated = true; }
        if (input.length > 4096) snapshot.truncated = true;
      }
      this.save(snapshot);
      if (root) this.db.prepare('INSERT INTO roots VALUES (?,?)').run(snapshot.id, JSON.stringify(root));
      this.db.prepare('INSERT INTO requests (scope,id,hash,result,input,anchor) VALUES (?,?,?,?,?,?)').run(scope, id, digest(input), JSON.stringify(result), JSON.stringify(input), head);
      this.db.exec('COMMIT');
    } catch (error) { this.db.exec('ROLLBACK'); throw error; }
  }
  close() { this.db.close(); }
}
function digest(input: unknown) { return createHash('sha256').update(JSON.stringify(input)).digest('hex'); }
