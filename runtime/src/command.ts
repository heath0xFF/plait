import { mkdtemp, realpath, rm } from 'node:fs/promises';
import { checkRoot } from './safety.ts';
import type { FileIdentity } from './safety.ts';
import type { BuildCache } from './build-cache.ts';
import { fail } from './protocol.ts';
import { commandPlatform } from './command/platform.ts';
import { runGuardian } from './command/supervisor.ts';

export { COMMAND_TIMEOUT_MS } from './command/supervisor.ts';

export async function command(root: string, text: string, _baseline: string[], signal: AbortSignal, output: (chunk: string) => void, checkoutFd?: number, identity?: FileIdentity, cache?: BuildCache): Promise<string> {
  const platform = commandPlatform();
  if (signal.aborted) fail('permission_denied', 'Run stopped.');
  // Platform paths keep native Unix socket names within the operating-system limit.
  const scratch = await realpath(await mkdtemp(platform.scratchPrefix));
  try {
    if (identity) await checkRoot(root, identity);
    const environment = await platform.prepare(root, scratch, signal, cache);
    return await runGuardian({ root, text, ...environment, checkoutFd }, signal, output);
  } finally { await rm(scratch, { recursive: true, force: true }); }
}
