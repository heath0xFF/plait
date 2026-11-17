import { fileURLToPath } from 'node:url';

/** The executable shim tests pass as the Claude Code path. */
export const FAKE_CLAUDE = fileURLToPath(new URL('./fake-claude', import.meta.url));
export const FAKE_CLAUDE_SCRIPT = fileURLToPath(new URL('./fake-claude.ts', import.meta.url));
/** The dev copy of the Agent SDK, which tests load in place of a downloaded one. */
export const DEV_SDK = fileURLToPath(new URL('../../node_modules/@anthropic-ai/claude-agent-sdk', import.meta.url));
/** The session the fake knows how to resume. */
export const FAKE_SESSION = '6f1c2a4e-0000-4000-8000-000000000001';

/** A minimal environment for the fake: no inherited PATH, so nothing real is found or run. */
export function fakeEnv(extra: Record<string, string> = {}): NodeJS.ProcessEnv {
  return { PATH: '/usr/bin:/bin', HOME: '/nonexistent', FAKE_CLAUDE_NODE: process.execPath, ...extra };
}
