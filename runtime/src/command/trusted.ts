import type { FileHandle } from 'node:fs/promises';
import { lstat } from 'node:fs/promises';
import { join } from 'node:path';
import { fail, HostError } from '../protocol.ts';
import { checkRoot, pathKey } from '../safety.ts';
import type { FileIdentity, Postimage } from '../safety.ts';
import { restoreOwned } from '../provenance.ts';
import { clipHead, clipTail } from '../text.ts';
import { commandPlatform } from './platform.ts';
import { runGuardian, COMMAND_TIMEOUT_MS } from './supervisor.ts';
import { hostAccount, loginEnvironment } from './login-environment.ts';
import type { Account, LoginEnvironment } from './login-environment.ts';

/** The parts of a run that a trusted command reads and reconciles. */
export type TrustedRun = {
  snapshot: { checkout: string };
  baseline: string[];
  owned: Map<string, Postimage>;
  rootIdentity: FileIdentity;
  lease?: FileHandle;
  forget?: (path: string) => void;
  loginEnvironment?: Promise<LoginEnvironment>;
};
export type TrustedOptions = { account?: Account; timeoutMs?: number };

const RESULT_LIMIT = 16384;
const LISTED_PATHS = 20;
const NOTICE_LIMIT = 2048;

/** Identity and change times of the paths that were dirty before the run. */
async function dirtyStamps(root: string, paths: string[]) {
  const stamps = new Map<string, string>();
  for (const path of paths) {
    try {
      const info = await lstat(join(root, path), { bigint: true });
      // Ignored build directories change with every build; report files only.
      stamps.set(path, info.isDirectory() ? 'directory'
        : [info.dev, info.ino, info.mode, info.size, info.mtimeNs, info.ctimeNs].join(':'));
    } catch { stamps.set(path, 'absent'); }
  }
  return stamps;
}

function changedDirtyNotice(before: Map<string, string>, after: Map<string, string>) {
  const changed = [...before.keys()].filter(path => before.get(path) !== after.get(path));
  if (!changed.length) return '';
  const listed = changed.slice(0, LISTED_PATHS).join(', ');
  const more = changed.length > LISTED_PATHS ? `, and ${changed.length - LISTED_PATHS} more` : '';
  return clipHead(`Plait: paths that were dirty before this run changed during this command: ${listed}${more}`, NOTICE_LIMIT) + '.\n';
}

/**
 * A command can rewrite files this thread wrote without a file tool seeing it.
 * Drop every ownership record that no longer matches, in memory and on disk,
 * so those files return to ordinary dirty-path protection.
 */
export async function reconcileOwned(run: TrustedRun) {
  const records = [...run.owned.values()];
  if (!records.length) return;
  let kept = new Map<string, Postimage>();
  // If verification itself fails, no record keeps its claim.
  try { kept = await restoreOwned(run.snapshot.checkout, run.baseline, records, run.rootIdentity, run.lease?.fd); } catch {}
  for (const record of records) {
    const key = pathKey(record.path);
    if (kept.has(key)) continue;
    run.owned.delete(key);
    run.forget?.(record.path);
  }
}

/**
 * Runs one command as the user. Authorization happened in the tool; this owns
 * the environment, the guardian request, and the bookkeeping afterwards.
 */
export async function trustedCommand(run: TrustedRun, text: string, signal: AbortSignal, output: (chunk: string) => void, options: TrustedOptions = {}): Promise<string> {
  // Linux still rejects every command; trusted mode is not a containment substitute.
  commandPlatform();
  if (signal.aborted) fail('permission_denied', 'Run stopped.');
  const root = run.snapshot.checkout;
  await checkRoot(root, run.rootIdentity);
  // One login-shell read per run keeps later commands fast and consistent.
  const pending = run.loginEnvironment ??= loginEnvironment(options.account ?? hostAccount(), signal);
  let environment: LoginEnvironment;
  try { environment = await pending; }
  catch (error) {
    if (run.loginEnvironment === pending) run.loginEnvironment = undefined;
    throw error;
  }
  let notices = environment.source === 'fallback'
    ? 'Plait: the login-shell environment could not be read; using a minimal PATH.\n' : '';
  const before = await dirtyStamps(root, run.baseline);
  let result = ''; let failure: unknown;
  try {
    result = await runGuardian({ root, text, env: environment.env, checkoutFd: run.lease?.fd, trusted: true },
      signal, output, options.timeoutMs ?? COMMAND_TIMEOUT_MS);
  } catch (error) { failure = error ?? new Error('Command failed.'); }
  // Reconcile after success, failure, and Stop alike: a killed command still ran.
  try {
    await reconcileOwned(run);
    notices += changedDirtyNotice(before, await dirtyStamps(root, run.baseline));
  } catch (error) { failure ??= error; }
  if (failure instanceof HostError) throw new HostError(failure.code, notices + failure.message);
  if (failure) throw failure;
  return notices + clipTail(result, RESULT_LIMIT - notices.length);
}
