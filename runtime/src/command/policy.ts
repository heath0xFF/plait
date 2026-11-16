import { fail } from '../protocol.ts';
import type { Request } from '../protocol.ts';
import { checkout, rootIdentity } from '../safety.ts';
import type { FileIdentity } from '../safety.ts';

/**
 * How a project's commands run. `sandboxed` is the default Seatbelt containment.
 * `trusted` runs commands as the user, with network and checkout writes.
 */
export type CommandPolicy = 'sandboxed' | 'trusted';

type Settings = {
  setting(key: string): string | null;
  setSetting(key: string, value: string | null): void;
};

const settingKey = (root: string) => `command_policy:${root}`;

export function parseCommandPolicy(value: unknown): CommandPolicy {
  if (value !== 'sandboxed' && value !== 'trusted') fail('invalid_request', 'policy must be sandboxed or trusted.');
  return value;
}

/**
 * Trust is recorded for one directory identity. A missing, unreadable, or
 * mismatched record is sandboxed, so a replaced checkout starts contained again.
 */
export function commandPolicy(settings: Settings, root: string, identity: FileIdentity): CommandPolicy {
  const saved = settings.setting(settingKey(root));
  if (saved === null) return 'sandboxed';
  try {
    const record = JSON.parse(saved) as { policy?: unknown; root?: Partial<FileIdentity> } | null;
    return record?.policy === 'trusted' && record.root?.dev === identity.dev && record.root?.ino === identity.ino
      ? 'trusted' : 'sandboxed';
  } catch { return 'sandboxed'; }
}

export function setCommandPolicy(settings: Settings, root: string, identity: FileIdentity, policy: CommandPolicy) {
  settings.setSetting(settingKey(root), policy === 'trusted' ? JSON.stringify({ policy, root: identity }) : null);
}

/** `command_policy` and `set_command_policy`: desktop operations, never model tools. */
export async function commandPolicyOperation(settings: Settings, request: Request) {
  const policy = request.op === 'set_command_policy' ? parseCommandPolicy(request.policy) : undefined;
  const root = await checkout(String(request.checkout));
  const identity = await rootIdentity(root);
  if (policy) setCommandPolicy(settings, root, identity, policy);
  return { checkout: root, policy: commandPolicy(settings, root, identity) };
}

// Model- and user-facing wording for trusted runs. The sandboxed wording stays
// where it always was, so that mode's text cannot drift when this file changes.
export const TRUSTED_INSTRUCTIONS = 'You are Plait, a local coding assistant. The user enabled trusted command mode for this project. Commands run as the user in the checkout, without a sandbox, with the user\'s login-shell environment and real HOME: they can write the checkout, use Git (including commits), use the network, and install dependencies, with a 15 minute limit. Edits and commands may require the user\'s approval. Prefer the file tools for source changes: they refuse paths that were dirty or ignored before this run, and unchanged files this thread previously wrote can be edited again. A file changed by a command is not editable with the file tools while it is uncommitted. Commands are not checked, so take care: never use them to modify, revert, stash, or delete paths that were already dirty or ignored before this run, do not push, rewrite existing Git history, or change files outside the checkout unless the user asks, never bypass approval, and do not start daemons. Read project instructions below.\n';

export const TRUSTED_COMMAND_DESCRIPTION = 'Run a command (may require approval) in trusted mode: it runs as the user in the checkout without a sandbox, with the login-shell environment, real HOME, network access, and checkout/Git writes; no terminal or stdin; 15 minute limit. Nothing checks what it changes, so do not touch paths that were dirty before this run or files outside the checkout. Files a command changes are not editable with the file tools while uncommitted. Do not start background services.';

export const TRUSTED_COMMAND_SUMMARY = 'Run command with full user access and network';

export const TRUSTED_RISK = 'Only this exact call is approved. Trusted command mode is on for this project: commands run as you, outside the sandbox, with network access, and can change or delete anything your account can, including Git history and files that were dirty before this run. File tools still protect dirty and ignored paths. Read tools and commands can read ignored checkout files and send their contents to the provider.';
