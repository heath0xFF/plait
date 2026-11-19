// No default run ceiling: serialize preparation by filesystem, not execution.
import { HostError, fail, type Failure } from './protocol.ts';

const resourceCodes = new Map([
  ['EMFILE', 'The host has no free file descriptors.'], ['ENFILE', 'The system has no free file descriptors.'],
  ['EAGAIN', 'The system cannot start another process right now.'], ['ENOMEM', 'The system has insufficient memory.'],
  ['ENOSPC', 'The host or checkout filesystem is full.'], ['EDQUOT', 'The filesystem quota is exhausted.'],
]);
export function resourceFailure(error: unknown): Failure | undefined {
  const code = (error as { code?: string })?.code;
  const detail = code && resourceCodes.get(code);
  return detail ? { code: 'resource_exhausted', message: `${detail} No automatic retry was scheduled; free resources and retry.` } : undefined;
}
export function resourceError(error: unknown): unknown {
  const detail = resourceFailure(error);
  return detail ? new HostError(detail.code, detail.message) : error;
}
export function configuredCapacity(value = process.env.PLAIT_MAX_ACTIVE_CHECKOUTS): number | undefined {
  if (value === undefined || value === '') return undefined;
  if (!/^[1-9]\d*$/.test(value) || !Number.isSafeInteger(Number(value))) fail('invalid_request', 'PLAIT_MAX_ACTIVE_CHECKOUTS must be a positive whole number.');
  return Number(value);
}
type Waiting = { begin(): void; cancel(): void };
export class RunCapacity {
  readonly maximum: number | undefined;
  private preparing = new Map<string, Waiting[]>();
  readonly phases = new Map<string, 'preparing' | 'resource_wait' | 'running'>();
  constructor(maximum = configuredCapacity()) { this.maximum = maximum; }
  admit(active: number) {
    if (this.maximum !== undefined && active >= this.maximum) fail('resource_exhausted', `The configured limit of ${this.maximum} active checkouts is reached. Nothing was queued; finish a run or raise the host limit.`);
  }
  async prepare<T>(device: string, thread: string, signal: AbortSignal, operation: () => Promise<T>): Promise<T> {
    this.phases.set(thread, this.preparing.has(device) ? 'resource_wait' : 'preparing');
    await this.acquire(device, signal);
    try {
      if (signal.aborted) fail('cancelled', 'Preparation cancelled. No harness was started.');
      this.phases.set(thread, 'preparing');
      return await operation();
    } finally { this.release(device); }
  }
  private acquire(device: string, signal: AbortSignal): Promise<void> {
    if (signal.aborted) return Promise.reject(new HostError('cancelled', 'Preparation cancelled. No harness was started.'));
    const waiting = this.preparing.get(device);
    if (!waiting) { this.preparing.set(device, []); return Promise.resolve(); }
    return new Promise((resolve, reject) => {
      const ticket: Waiting = {
        begin: () => { signal.removeEventListener('abort', ticket.cancel); resolve(); },
        cancel: () => {
          const index = waiting.indexOf(ticket); if (index >= 0) waiting.splice(index, 1);
          signal.removeEventListener('abort', ticket.cancel);
          reject(new HostError('cancelled', 'Preparation cancelled. No harness was started.'));
        },
      };
      waiting.push(ticket); signal.addEventListener('abort', ticket.cancel, { once: true });
      if (signal.aborted) ticket.cancel();
    });
  }
  private release(device: string) {
    const next = this.preparing.get(device)?.shift();
    if (next) next.begin(); else this.preparing.delete(device);
  }
}
