import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// Lock files are never removed where Plait runs (a holder may still use one), so
// tests keep theirs in a private folder, not the account's lease directory. The
// hosts and helpers tests start inherit it.
if (process.env.PLAIT_LEASE_DIR === undefined) {
  const directory = mkdtempSync(join(tmpdir(), 'plait-leases-'));
  process.env.PLAIT_LEASE_DIR = directory;
  process.once('exit', () => rmSync(directory, { recursive: true, force: true }));
}
