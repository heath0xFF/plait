import { spawn } from 'node:child_process';
import { mkdtemp, realpath, rm } from 'node:fs/promises';
import { homedir } from 'node:os';
import { resolve } from 'node:path';
import type { Writable } from 'node:stream';
import { PipeTransport, StreamableHttpTransport } from '@earendil-works/pi-mcp';
import type { McpPipeHandlers } from '@earendil-works/pi-mcp';
import type { ToolState } from '../tools.ts';
import type { Entry } from './config.ts';
import { commandPlatform } from '../command/platform.ts';
import { hostAccount, loginEnvironment } from '../command/login-environment.ts';
import { runGuardian } from '../command/supervisor.ts';
import { checkRoot } from '../safety.ts';
import { readableFolders } from '../skills.ts';
import { expand, Secrets } from './secrets.ts';

export const MCP_START_TIMEOUT_MS = 10_000;
export const MCP_CALL_TIMEOUT_MS = 60_000;
export const MCP_MESSAGE_BYTES = 16 * 1024 * 1024;
const home = (value: string) => value === '~' ? homedir() : value.startsWith('~/') ? resolve(homedir(), value.slice(2)) : value;

/** fd 5 is the lifetime lease; stdin belongs to MCP. Closing it kills the whole group. */
export function guardianPipe(root: string, policy: string, program: string, args: string[], env: NodeJS.ProcessEnv,
  state: ToolState, handlers: McpPipeHandlers, signal: AbortSignal) {
  state.authorization.check(signal);
  const executable = process.env.PLAIT_HOST_EXE;
  if (!executable) throw new Error('MCP guardian is unavailable.');
  const child = spawn(executable, ['mcp-server', root, policy, program, ...args], {
    cwd: root, env, stdio: ['pipe', 'pipe', 'pipe', process.env.PLAIT_LOCK_FD ? Number(process.env.PLAIT_LOCK_FD) : 'ignore', state.lease?.fd ?? 'ignore', 'pipe'],
  });
  const lifetime = (child.stdio as (Writable | null | undefined)[])[5]!;
  const close = () => { lifetime.end(); child.stdin!.end(); };
  lifetime.on('error', () => {}); child.stdin!.on('error', () => {});
  signal.addEventListener('abort', close, { once: true });
  if (signal.aborted) close();
  child.stdout!.on('data', handlers.onStdout); child.stderr!.on('data', handlers.onStderr);
  const exited = new Promise<void>(resolve => {
    let drain: ReturnType<typeof setTimeout>;
    child.once('error', close);
    child.once('exit', () => { drain = setTimeout(() => { child.stdout!.destroy(); child.stderr!.destroy(); }, 1000); });
    child.once('close', () => { clearTimeout(drain); signal.removeEventListener('abort', close); close(); resolve(); });
  });
  return { write: (data: string | Uint8Array) => { if (!child.stdin!.destroyed) child.stdin!.write(data); }, closeStdin: close, close, exited };
}

/** Secrets and subprocesses are first touched by search, never by discovery or settings. */
export async function transport(entry: Entry, state: ToolState, secrets: Secrets, signal: AbortSignal) {
  const root = state.snapshot.checkout;
  const config = entry.config;
  await checkRoot(root, state.rootIdentity);
  const trusted = state.commandPolicy === 'trusted';
  let scratch: string | undefined;
  let env: NodeJS.ProcessEnv;
  let policy = 'trusted';
  const cleanup = async () => { if (scratch) await rm(scratch, { recursive: true, force: true }); };
  try {
    // HTTP needs a process only when an explicit !command resolves a header.
    const values = 'command' in config ? config.env : config.headers;
    const needsProcess = 'command' in config || Object.values(values ?? {}).some(value => value.startsWith('!'));
    if (trusted && needsProcess) {
      commandPlatform();
      env = (await (state.loginEnvironment ??= loginEnvironment(hostAccount(), signal))).env;
    } else if (!trusted && needsProcess) {
      const platform = commandPlatform();
      scratch = await realpath(await mkdtemp(platform.scratchPrefix));
      const prepared = await platform.prepare(root, scratch, signal, state.buildCache, readableFolders(state.skills ?? []));
      env = prepared.env;
      // Same filesystem permissions as commands; network is the explicit MCP exception.
      policy = prepared.policy + '\n(allow network*)\n(allow mach-lookup (global-name "com.apple.SystemConfiguration.configd") (global-name "com.apple.networkd"))';
    } else env = {};
    const resolved: Record<string, string> = {};
    for (const [key, value] of Object.entries(values ?? {})) {
      if (value.startsWith('!')) {
        // Neither the command nor its output enters a transcript or a command log.
        try {
          let bytes = 0;
          resolved[key] = (await runGuardian({ root, text: `{\n${value.slice(1)}\n} 2>/dev/null`, env, checkoutFd: state.lease?.fd,
            ...(trusted ? { trusted: true as const } : { policy }) }, signal, () => {}, MCP_START_TIMEOUT_MS)).trim();
          bytes = Buffer.byteLength(resolved[key]);
          if (bytes >= 16000) throw new Error();
        } catch { throw new Error('An MCP credential command failed.'); }
        if (!resolved[key] || resolved[key] === 'Command completed.') throw new Error('An MCP credential command returned no value.');
      } else resolved[key] = expand(value, { ...process.env, ...env });
      secrets.add(resolved[key]);
    }
    if ('url' in config) {
      const connection = new StreamableHttpTransport({ url: config.url, headers: resolved,
        maxMessageBytes: MCP_MESSAGE_BYTES, openGetStream: false, reconnect: { maxRetries: 0 },
        // No redirect may forward a configured header to an unconfigured endpoint.
        fetch: (input, init) => fetch(input, { ...init, redirect: 'error', signal: AbortSignal.any([signal, AbortSignal.timeout(Math.min((config.timeout ?? 60) * 1000, 900000)), ...(init?.signal ? [init.signal] : [])]) }),
      });
      return { connection, cleanup, stderr: () => '' };
    }
    const connection = new PipeTransport({ maxMessageBytes: MCP_MESSAGE_BYTES, maxStderrBytes: 8192, closeTimeoutMs: 0,
      open: async handlers => guardianPipe(resolve(root, home(config.cwd ?? '.')), policy, home(config.command), (config.args ?? []).map(home),
        trusted ? { ...env, ...resolved } : { ...resolved, ...env }, state, handlers, signal),
    });
    return { connection, cleanup, stderr: () => connection.stderr };
  } catch (error) { await cleanup(); throw error; }
}
