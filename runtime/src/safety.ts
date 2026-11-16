import { execFile, spawn } from 'node:child_process';
import { realpath, lstat, open } from 'node:fs/promises';
import { isAbsolute, join } from 'node:path';
import { constants } from 'node:fs';
import { promisify } from 'node:util';
import { fail, HostError } from './protocol.ts';

const execute = promisify(execFile);
const gitEnv = { PATH: '/usr/bin:/bin:/usr/sbin:/sbin', HOME: '/nonexistent', GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null', GIT_OPTIONAL_LOCKS: '0', GIT_TERMINAL_PROMPT: '0', LC_ALL: 'C' };
const gitOptions = ['-c', 'core.fsmonitor=false', '-c', 'core.untrackedCache=false', '-c', 'core.hooksPath=/dev/null', '-c', 'diff.external='];

async function git(root: string, args: string[], input?: string) {
  const operation = execute('/usr/bin/git', [...gitOptions, '-C', root, ...args], { env: gitEnv, timeout: 10000, maxBuffer: 1048576, encoding: 'buffer' });
  if (input !== undefined) operation.child.stdin!.end(input);
  const result = await operation;
  return new TextDecoder('utf-8', { fatal: true }).decode(result.stdout);
}
export async function checkout(path: string): Promise<string> {
  if (!isAbsolute(path)) fail('invalid_request', 'Checkout must be absolute.');
  try {
    const root = await realpath(path);
    return await realpath((await git(root, ['rev-parse', '--show-toplevel'])).trim());
  } catch { fail('invalid_request', 'Checkout must be an accessible Git working tree.'); }
}
export async function dirtyPaths(root: string): Promise<string[]> {
  let names = '';
  try { names = await git(root, ['config', '--name-only', '--get-regexp', '^filter\\..*\\.(clean|process)$']); }
  catch (error) { if ((error as { code?: number }).code !== 1) throw error; }
  const filters = names.trim().split('\n').filter(Boolean).flatMap(key => ['-c', `${key}=`]);
  const result = await git(root, [...filters, 'status', '--porcelain=v1', '-z', '--untracked-files=all', '--ignored=matching', '--ignore-submodules=all']);
  const entries = result.split('\0');
  const paths: string[] = [];
  for (let i = 0; i < entries.length; i++) {
    const item = entries[i];
    if (!item) continue;
    paths.push(item.slice(3).replace(/\/$/, ''));
    if (item[0] === 'R' || item[0] === 'C') paths.push(entries[++i]);
  }
  // Submodules are separate checkouts; never run their status hooks or edit through them.
  const tracked = await git(root, ['ls-files', '--stage', '-z']);
  for (const entry of tracked.split('\0')) if (entry.startsWith('160000 ')) paths.push(entry.slice(entry.indexOf('\t') + 1));
  if (paths.length > 500 || Buffer.byteLength(JSON.stringify(paths)) > 32768) fail('permission_denied', 'Dirty baseline exceeds the safety bound.');
  return [...new Set(paths)];
}
export type FileIdentity = { dev: string; ino: string };
export type Postimage = FileIdentity & { path: string; sha256: string; mode: string; size: string; mtime: string; ctime: string };
export type FileResult = { text: string; postimage: Postimage };
export function sameImage(left: Postimage, right: Postimage) {
  return (['path', 'sha256', 'dev', 'ino', 'mode', 'size', 'mtime', 'ctime'] as const).every(key => left[key] === right[key]);
}
export const pathKey = (path: string) => path.normalize('NFD').toLowerCase();
export async function ignoredPaths(root: string, paths: string[]): Promise<string[]> {
  if (!paths.length) return [];
  try { return (await git(root, ['check-ignore', '--no-index', '-z', '--stdin'], paths.join('\0') + '\0')).split('\0').filter(Boolean); }
  catch (error) { if ((error as { code?: number }).code === 1) return []; throw error; }
}
export async function rootIdentity(root: string): Promise<FileIdentity> {
  const info = await lstat(root, { bigint: true });
  if (!info.isDirectory() || await realpath(root) !== root) fail('permission_denied', 'Checkout root changed.');
  return { dev: String(info.dev), ino: String(info.ino) };
}
export async function checkRoot(root: string, expected: FileIdentity) {
  const current = await rootIdentity(root);
  if (current.dev !== expected.dev || current.ino !== expected.ino) fail('permission_denied', 'Checkout root identity changed. Create a new thread for the replacement.');
}
export async function identities(root: string, paths: string[]): Promise<FileIdentity[]> {
  const result: FileIdentity[] = [];
  for (const path of paths) {
    try {
      const info = await lstat(join(root, path), { bigint: true });
      result.push({ dev: String(info.dev), ino: String(info.ino) });
    } catch (error) { if ((error as { code?: string }).code !== 'ENOENT') throw error; }
  }
  return result;
}
export function safePath(path: string, dirty: string[] = []) {
  const parts = path.split('/');
  if (!path || isAbsolute(path) || parts.some(part => !part || part === '.' || part === '..' || part.toLowerCase() === '.git') || path.includes('\0')) {
    fail('permission_denied', 'Path must stay inside the checkout and outside Git metadata.');
  }
  const folded = path.normalize('NFD').toLowerCase();
  if (dirty.some(raw => {
    const item = raw.normalize('NFD').toLowerCase();
    return folded === item || folded.startsWith(`${item}/`) || item.startsWith(`${folded}/`);
  })) {
    fail('permission_denied', 'Pre-existing dirty paths are protected.');
  }
}
export async function acquireCheckout(root: string) {
  // Lock the directory inode itself: stores, TMPDIR, and cleanup cannot split ownership.
  const expected = await rootIdentity(root);
  const lease = await open(root, constants.O_RDONLY | constants.O_DIRECTORY | constants.O_NOFOLLOW);
  try {
    await new Promise<void>((resolve, reject) => {
      const child = spawn(process.env.PLAIT_HOST_EXE!, ['lock-fd'], { stdio: ['ignore', 'ignore', 'ignore', lease.fd] });
      child.once('error', reject);
      child.once('exit', code => code === 0 ? resolve() : reject(new HostError('busy', 'This checkout already has a host writer.')));
    });
    const held = await lease.stat({ bigint: true });
    await checkRoot(root, expected);
    if (String(held.dev) !== expected.dev || String(held.ino) !== expected.ino) fail('permission_denied', 'Checkout changed while taking its lease.');
    return lease;
  } catch (error) { await lease.close(); throw error; }
}
export async function fileAction<T = FileResult>(root: string, action: string, path: string, input?: unknown, checkoutFd?: number, identity?: FileIdentity) {
  safePath(path);
  const executable = process.env.PLAIT_HOST_EXE;
  if (!executable) fail('internal', 'Host file helper is not configured.');
  try {
    const child = spawn(executable, ['file', action, root, path, ...(identity ? [identity.dev, identity.ino] : [])], {
      stdio: ['pipe', 'pipe', 'pipe', process.env.PLAIT_LOCK_FD ? Number(process.env.PLAIT_LOCK_FD) : 'ignore', checkoutFd ?? 'ignore'],
    });
    child.stdin!.on('error', () => {});
    child.stdin!.end(input === undefined ? undefined : JSON.stringify(input));
    return await new Promise<T>((resolve, reject) => {
      let output = ''; let diagnostic = '';
      child.stdout!.setEncoding('utf8');
      child.stderr!.setEncoding('utf8');
      child.stderr!.on('data', data => { diagnostic = (diagnostic + data).slice(-4096); });
      const timer = setTimeout(() => child.kill('SIGKILL'), 10000);
      child.stdout!.on('data', data => { output += data; if (output.length > 1048576) child.kill('SIGKILL'); });
      child.once('error', error => { clearTimeout(timer); reject(error); });
      child.once('close', code => {
        clearTimeout(timer);
        if (code !== 0) {
          // Only fixed helper messages cross the boundary; never echo raw stderr.
          const reason = new Map([
            ['plait-host: invalid read offset', 'Read denied: offset must be a non-negative integer byte offset.'],
            ['plait-host: invalid read limit', 'Read denied: limit must be an integer from 4 to 12000 bytes.'],
            ['plait-host: read offset beyond EOF', 'Read denied: offset is beyond the end of the file.'],
            ['plait-host: read offset inside UTF-8 character', 'Read denied: offset is inside a UTF-8 character. Use the returned next offset.'],
            ['plait-host: file contents are not valid UTF-8', 'Read denied: file contents are not valid UTF-8.'],
            ['plait-host: edit must match exactly once', 'Edit denied: old_text must match exactly once.'],
            ['plait-host: file identity or metadata changed after approval', 'Edit denied: file identity or metadata changed after approval.'],
            ['plait-host: file changed after approval was requested', 'Edit denied: file contents changed after approval was requested.'],
          ]).get(diagnostic.trim());
          reject(new HostError('permission_denied', reason ?? 'File access denied, changed, non-unique edit, or unsafe path.'));
        }
        else { try { resolve(JSON.parse(output)); } catch { reject(new Error('Invalid helper output')); } }
      });
    });
  } catch (error) {
    if (error instanceof HostError) throw error;
    fail('permission_denied', 'File operation failed safely.');
  }
}
