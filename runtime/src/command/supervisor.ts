import { spawn } from 'node:child_process';
import { fail, HostError } from '../protocol.ts';
import { clipTail } from '../text.ts';

export const COMMAND_TIMEOUT_MS = 15 * 60 * 1000;
// A request is either a Seatbelt profile or the explicit trusted entry, never both.
type GuardianCommand = {
  root: string;
  text: string;
  env: NodeJS.ProcessEnv;
  checkoutFd?: number;
} & ({ policy: string } | { trusted: true });

// Only platform policy preparation may supply a production guardian request.
export async function runGuardian(command: GuardianCommand, signal: AbortSignal, output: (chunk: string) => void, timeoutMs = COMMAND_TIMEOUT_MS): Promise<string> {
  if (signal.aborted) fail('permission_denied', 'Run stopped.');
  const executable = process.env.PLAIT_HOST_EXE;
  if (!executable) fail('internal', 'Host command guardian is not configured.');
  return await new Promise<string>((resolve, reject) => {
    const args = 'trusted' in command ? ['trusted-command', command.root, command.text]
      : ['command', command.root, command.policy, command.text];
    const child = spawn(executable, args, {
      cwd: command.root, env: command.env,
      stdio: ['pipe', 'pipe', 'pipe', process.env.PLAIT_LOCK_FD ? Number(process.env.PLAIT_LOCK_FD) : 'ignore', command.checkoutFd ?? 'ignore'],
    });
    let result = ''; let settled = false;
    let drainTimeout: ReturnType<typeof setTimeout> | undefined;
    const cancel = () => { child.stdin!.end(); };
    const timeout = setTimeout(cancel, timeoutMs);
    const finish = (error?: Error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      clearTimeout(drainTimeout);
      signal.removeEventListener('abort', cancel);
      cancel();
      if (error) reject(error);
      else resolve(result || 'Command completed.');
    };
    child.stdin!.on('error', () => {});
    signal.addEventListener('abort', cancel, { once: true });
    const collect = (chunk: string) => {
      result = clipTail(result + chunk, 16384);
      output(clipTail(chunk, 4096));
    };
    // Pipe chunks can split UTF-8 bytes; each stream needs its own decoder.
    child.stdout!.setEncoding('utf8').on('data', collect);
    child.stderr!.setEncoding('utf8').on('data', collect);
    child.once('error', () => finish(new HostError('permission_denied', 'Command guardian could not start.')));
    child.once('exit', () => {
      // Unsupported detached descendants must not hold the output subscription open.
      drainTimeout = setTimeout(() => {
        child.stdout!.destroy(); child.stderr!.destroy();
      }, 1000);
    });
    // Close follows pipe drainage; exit alone can lose the final tool output.
    child.once('close', code => finish(code === 0 ? undefined
      : new HostError('permission_denied', `Command ended without success (${code ?? 'stopped'}). ${result}`)));
  });
}
