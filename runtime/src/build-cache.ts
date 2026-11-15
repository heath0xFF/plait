import { lstat, mkdir, realpath } from 'node:fs/promises';
import { join } from 'node:path';
import { fail } from './protocol.ts';

export type BuildCache = { store: string; thread: string };

async function privateDirectory(path: string, create: boolean) {
  if (create) {
    try { await mkdir(path, { mode: 0o700 }); }
    catch (error) { if ((error as { code?: string }).code !== 'EEXIST') throw error; }
  }
  const info = await lstat(path);
  if (!info.isDirectory() || info.uid !== process.getuid!() || (info.mode & 0o077) !== 0 || await realpath(path) !== path) {
    fail('permission_denied', 'Build cache must be canonical, private, owned, and free of symlink traversal.');
  }
}

export async function prepareBuildCache(cache: BuildCache) {
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(cache.thread)) {
    fail('permission_denied', 'Invalid build cache thread identity.');
  }
  await privateDirectory(cache.store, false);
  const parent = join(cache.store, 'build-cache');
  await privateDirectory(parent, true);
  const root = join(parent, cache.thread);
  await privateDirectory(root, true);
  const cargo = join(root, 'cargo'); const target = join(root, 'target');
  // The sandbox cannot rename these roots; host setup never writes below target.
  await privateDirectory(cargo, true); await privateDirectory(target, true);
  return { cargo, target };
}
