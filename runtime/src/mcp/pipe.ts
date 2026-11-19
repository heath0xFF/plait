import { guardianProof } from '../cleanup.ts';
import { spawn } from 'node:child_process';
import type { Writable } from 'node:stream';
import type { McpPipeHandlers } from '@earendil-works/pi-mcp';
import type { ToolState } from '../tools.ts';
import { GUARDIAN_ENV, writeEnvironment } from '../command/environment.ts';

// A token bucket bounds both parsing work and raw output, including stderr.
export const MCP_PIPE_BYTES = 32 * 1024 * 1024;
export const MCP_PIPE_MESSAGE_BURST = 2048;
export const MCP_PIPE_MESSAGES_PER_SECOND = 512;
export const MCP_PIPE_BUDGET_PROBLEM = 'MCP server exceeded its output budget (32 MiB/s, 2,048-message burst, 512 messages/s).';
export class PipeBudget {
  private bytes = MCP_PIPE_BYTES;
  private messages = MCP_PIPE_MESSAGE_BURST;
  private time = performance.now();
  accept(data: Uint8Array, stdout: boolean, now = performance.now()) {
    const seconds = Math.max(0, now - this.time) / 1000; this.time = now;
    this.bytes = Math.min(MCP_PIPE_BYTES, this.bytes + seconds * MCP_PIPE_BYTES) - data.byteLength;
    let messages = 0;
    if (stdout) for (const byte of data) if (byte === 10) messages++;
    this.messages = Math.min(MCP_PIPE_MESSAGE_BURST, this.messages + seconds * MCP_PIPE_MESSAGES_PER_SECOND) - messages;
    return this.bytes >= 0 && this.messages >= 0;
  }
}

/** fd 5 owns the process group; fd 6 carries env after the clean guardian starts. */
export function guardianPipe(root: string, policy: string, program: string, args: string[], env: NodeJS.ProcessEnv,
  state: ToolState, handlers: McpPipeHandlers, signal: AbortSignal, failed: (message: string) => void = () => {}) {
  state.authorization.check(signal);
  const executable = process.env.PLAIT_HOST_EXE;
  if (!executable) throw new Error('MCP guardian is unavailable.');
  const child = spawn(executable, ['mcp-server', root, policy, program, ...args], {
    cwd: root, env: GUARDIAN_ENV,
    stdio: ['pipe', 'pipe', 'pipe', process.env.PLAIT_LOCK_FD ? Number(process.env.PLAIT_LOCK_FD) : 'ignore', state.lease?.fd ?? 'ignore', 'pipe', 'pipe',
      state.lease?.lifecycleFd ?? 'ignore', 'pipe'],
  });
  state.lease?.cleanup?.track(guardianProof(child), 'The MCP guardian did not confirm its process group stopped.');
  writeEnvironment(child, env);
  const lifetime = (child.stdio as (Writable | null | undefined)[])[5]!;
  const close = () => { lifetime.end(); child.stdin!.end(); };
  lifetime.on('error', () => {}); child.stdin!.on('error', () => {});
  signal.addEventListener('abort', close, { once: true });
  if (signal.aborted) close();
  let problem: Error | undefined; const budget = new PipeBudget();
  const consume = (chunk: Buffer, stdout: boolean) => {
    if (problem) return;
    if (!budget.accept(chunk, stdout)) {
      problem = new Error(MCP_PIPE_BUDGET_PROBLEM); failed(problem.message);
      child.stdout!.destroy(); child.stderr!.destroy(); close(); return;
    }
    if (stdout) handlers.onStdout(chunk); else handlers.onStderr(chunk);
  };
  child.stdout!.on('data', chunk => consume(chunk, true)); child.stderr!.on('data', chunk => consume(chunk, false));
  const exited = new Promise<void>((resolve, reject) => {
    let drain: ReturnType<typeof setTimeout>;
    child.once('error', () => { problem ??= new Error('MCP guardian could not start.'); close(); });
    child.once('exit', () => { drain = setTimeout(() => { child.stdout!.destroy(); child.stderr!.destroy(); }, 1000); });
    child.once('close', code => {
      clearTimeout(drain); signal.removeEventListener('abort', close); close();
      if (problem || code !== 0) reject(problem ?? new Error('MCP server failed or stopped.')); else resolve();
    });
  });
  return { write: (data: string | Uint8Array) => { if (!child.stdin!.destroyed) child.stdin!.write(data); }, closeStdin: close, close, exited };
}
