import { groupExited } from '../cleanup.ts';
import { spawn } from 'node:child_process';
import type { ChildProcessWithoutNullStreams } from 'node:child_process';
import type { InitializeParams, InitializeResponse, RequestId } from './protocol.ts';

/** Frames above this are refused; the connection cannot resynchronize safely after one. */
export const MAX_LINE_BYTES = 8 * 1024 * 1024;
const STDERR_BYTES = 16384;
const STDERR_LINES = 10;
const CLOSE_GRACE_MS = 2000;
const INITIALIZE_TIMEOUT_MS = 20_000;

/** A JSON-RPC error reply from the app-server. */
export class CodexRpcError extends Error {
  code: number;
  constructor(code: number, message: string) { super(message); this.code = code; }
}
/** The app-server ended, or broke the protocol, before answering. */
export class CodexConnectionError extends Error {}

export type Exit = { code: number | null; signal: string | null };
export type StartOptions = {
  env: NodeJS.ProcessEnv; cwd?: string;
  /** Applies to each request without its own signal. Unset: wait for the reply or the exit. */
  requestTimeoutMs?: number;
  /** Reported to Codex as the client version. */
  clientVersion?: string;
};
type Pending = { resolve: (value: unknown) => void; reject: (error: Error) => void };
type Frame = { id?: RequestId; method?: unknown; params?: unknown; result?: unknown; error?: { code?: unknown; message?: unknown } };
type NotificationHandler = (method: string, params: any) => void;
type RequestHandler = (method: string, params: any) => Promise<unknown>;

/** One `codex app-server` process, spoken to over stdio JSON lines. */
export class AppServer {
  readonly exited: Promise<Exit>;
  readonly cleanup: Promise<void>;
  private readonly child: ChildProcessWithoutNullStreams;
  private readonly pending = new Map<RequestId, Pending>();
  private readonly notificationHandlers = new Set<NotificationHandler>();
  private requestHandler?: RequestHandler;
  private nextId = 1;
  private buffer: Buffer[] = [];
  private buffered = 0;
  private stderr = '';
  private failure?: Error;
  private readonly timeoutMs?: number;

  private constructor(child: ChildProcessWithoutNullStreams, timeoutMs?: number) {
    this.child = child;
    this.cleanup = groupExited(child);
    this.timeoutMs = timeoutMs;
    this.exited = new Promise(resolve => child.once('exit', (code, signal) => {
      this.fail(new CodexConnectionError(`Codex app-server exited (${signal ?? `code ${code}`}).`));
      resolve({ code, signal });
    }));
    // Writes after an exit surface here; the exit itself already failed every request.
    child.stdin.on('error', () => {});
    child.stdout.on('data', (chunk: Buffer) => this.receive(chunk));
    child.stderr.setEncoding('utf8').on('data', (chunk: string) => {
      this.stderr = (this.stderr + chunk).slice(-STDERR_BYTES);
    });
  }

  /** Spawns `<binary> app-server` and completes the `initialize` handshake. */
  static async start(binary: string, options: StartOptions): Promise<AppServer> {
    const child = spawn(binary, ['app-server'], { env: options.env, cwd: options.cwd, stdio: ['pipe', 'pipe', 'pipe'], detached: true });
    await new Promise<void>((resolve, reject) => {
      child.once('spawn', resolve);
      child.once('error', error => reject(new CodexConnectionError(`Could not start Codex: ${(error as NodeJS.ErrnoException).code ?? error.message}.`)));
    });
    const server = new AppServer(child, options.requestTimeoutMs);
    const params: InitializeParams = {
      clientInfo: { name: 'plait', title: 'Plait', version: options.clientVersion ?? '0' },
      // No experimental API and no attestation, so Codex sends only the stable requests.
      capabilities: { experimentalApi: false, requestAttestation: false },
    };
    try {
      await server.request<InitializeResponse>('initialize', params, AbortSignal.timeout(INITIALIZE_TIMEOUT_MS));
      server.notify('initialized');
    } catch (error) {
      await server.close();
      throw error;
    }
    return server;
  }

  request<T>(method: string, params: unknown, signal?: AbortSignal): Promise<T> {
    if (this.failure) return Promise.reject(this.failure);
    const effective = signal ?? (this.timeoutMs === undefined ? undefined : AbortSignal.timeout(this.timeoutMs));
    if (effective?.aborted) return Promise.reject(abortError(method, effective));
    const id = this.nextId++;
    return new Promise<T>((resolve, reject) => {
      const abort = () => { this.pending.get(id)?.reject(abortError(method, effective!)); };
      const done = () => { this.pending.delete(id); effective?.removeEventListener('abort', abort); };
      this.pending.set(id, {
        resolve: value => { done(); resolve(value as T); },
        reject: error => { done(); reject(error); },
      });
      effective?.addEventListener('abort', abort, { once: true });
      this.send({ id, method, params: params ?? {} });
    });
  }

  notify(method: string, params?: unknown) {
    this.send(params === undefined ? { method } : { method, params });
  }

  onNotification(handler: NotificationHandler): () => void {
    this.notificationHandlers.add(handler);
    return () => { this.notificationHandlers.delete(handler); };
  }

  /** Answers server-to-client requests. Without a handler, each is refused. */
  onRequest(handler: RequestHandler): void {
    this.requestHandler = handler;
  }

  /** The last few stderr lines, for diagnostics only. They may hold paths or provider text, so never show them raw. */
  diagnostics(): string {
    return this.stderr.split('\n').filter(Boolean).slice(-STDERR_LINES).join('\n');
  }

  /** Closes stdin, which ends the app-server, then kills it if it lingers. */
  async close(): Promise<void> {
    this.child.stdin.end();
    const timer = setTimeout(() => this.child.kill('SIGKILL'), CLOSE_GRACE_MS);
    await this.exited;
    clearTimeout(timer);
    this.child.stdout.destroy(); this.child.stderr.destroy();
  }

  private send(frame: object) {
    if (this.failure) return;
    this.child.stdin.write(`${JSON.stringify(frame)}\n`);
  }

  private receive(chunk: Buffer) {
    let start = 0;
    for (let index = chunk.indexOf(10); index >= 0; index = chunk.indexOf(10, start)) {
      const piece = chunk.subarray(start, index);
      if (this.buffered + piece.length > MAX_LINE_BYTES) return this.oversized();
      const line = this.buffered ? Buffer.concat([...this.buffer, piece]).toString('utf8') : piece.toString('utf8');
      this.buffer = []; this.buffered = 0; start = index + 1;
      this.dispatch(line);
      if (this.failure) return;
    }
    const rest = chunk.subarray(start);
    if (this.buffered + rest.length > MAX_LINE_BYTES) return this.oversized();
    if (rest.length) { this.buffer.push(rest); this.buffered += rest.length; }
  }

  private oversized() {
    this.buffer = []; this.buffered = 0;
    this.fail(new CodexConnectionError('Codex sent a message larger than 8 MiB. The connection was closed.'));
    this.child.kill('SIGKILL');
  }

  private dispatch(line: string) {
    if (!line.trim()) return;
    let frame: Frame;
    try { frame = JSON.parse(line); } catch { return; }
    if (!frame || typeof frame !== 'object') return;
    if (typeof frame.method === 'string') {
      if (frame.id !== undefined) void this.answer(frame.id, frame.method, frame.params);
      else for (const handler of this.notificationHandlers) handler(frame.method, frame.params);
      return;
    }
    if (frame.id === undefined) return;
    const pending = this.pending.get(frame.id);
    if (!pending) return;
    if (frame.error) pending.reject(new CodexRpcError(Number(frame.error.code ?? 0), String(frame.error.message ?? 'Codex request failed.')));
    else pending.resolve(frame.result);
  }

  private async answer(id: RequestId, method: string, params: unknown) {
    if (!this.requestHandler) return this.send({ id, error: { code: -32601, message: `Plait does not handle ${method}.` } });
    try {
      const result = await this.requestHandler(method, params);
      this.send({ id, result: result ?? {} });
    } catch (error) {
      const code = error instanceof CodexRpcError ? error.code : -32603;
      this.send({ id, error: { code, message: error instanceof Error ? error.message : 'Plait could not answer.' } });
    }
  }

  private fail(error: Error) {
    this.failure ??= error;
    for (const pending of [...this.pending.values()]) pending.reject(this.failure);
  }
}

function abortError(method: string, signal: AbortSignal): Error {
  const timeout = (signal.reason as { name?: string } | undefined)?.name === 'TimeoutError';
  return new CodexConnectionError(timeout ? `Codex did not answer ${method} in time.` : `Stopped waiting for ${method}.`);
}
