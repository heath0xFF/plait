import { fileURLToPath } from 'node:url';

/** The executable shim tests pass as the Codex path. */
export const FAKE_CODEX = fileURLToPath(new URL('./fake-codex', import.meta.url));
export const FAKE_CODEX_SCRIPT = fileURLToPath(new URL('./fake-codex.ts', import.meta.url));

/** A minimal environment for the fake: no inherited PATH, so nothing real is found or run. */
export function fakeEnv(extra: Record<string, string> = {}): NodeJS.ProcessEnv {
  return { PATH: '/usr/bin:/bin', FAKE_CODEX_NODE: process.execPath, ...extra };
}
