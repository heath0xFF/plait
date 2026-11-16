import { randomUUID } from 'node:crypto';
import type { AuthorizationPolicy, EffectCall } from './authorization.ts';
import { fail } from './protocol.ts';
import type { Approval, Snapshot } from './protocol.ts';

type SettlementFailure = { error: unknown };
type PendingApproval = {
  id: string;
  signal: AbortSignal;
  settle(allow: boolean): SettlementFailure | undefined;
};

export class InteractiveApproval implements AuthorizationPolicy {
  private pending?: PendingApproval;
  private readonly snapshot: Snapshot;
  private readonly save: () => void;

  constructor(snapshot: Snapshot, save: () => void) {
    this.snapshot = snapshot;
    this.save = save;
  }

  async authorize(call: EffectCall, signal: AbortSignal): Promise<void> {
    if (signal.aborted) fail('permission_denied', 'Tool permission denied or revoked.');
    if (this.pending) fail('busy', 'Another tool is awaiting approval.');
    const approval = describeApproval(call);
    const allowed = await new Promise<boolean>((resolve, reject) => {
      const clear = () => {
        signal.removeEventListener('abort', cancel);
        if (this.pending !== pending) return;
        this.pending = undefined;
        if (this.snapshot.pending_approval?.id === approval.id) {
          this.snapshot.pending_approval = null;
          if (this.snapshot.status === 'awaiting_approval') this.snapshot.status = 'running';
        }
      };
      const settle = (allow: boolean): SettlementFailure | undefined => {
        if (this.pending !== pending) return;
        clear();
        try {
          this.save();
          resolve(allow);
        } catch (error) {
          reject(error);
          return { error };
        }
      };
      // Cancellation reports save failures through the waiter, not an event callback.
      const cancel = () => { settle(false); };
      const pending: PendingApproval = { id: approval.id, signal, settle };
      this.pending = pending;
      this.snapshot.pending_approval = approval;
      this.snapshot.status = 'awaiting_approval';
      signal.addEventListener('abort', cancel, { once: true });
      try {
        this.save();
        if (signal.aborted) cancel();
      } catch (error) {
        clear();
        reject(error);
      }
    });
    if (!allowed || signal.aborted) fail('permission_denied', 'Tool permission denied or revoked.');
  }

  answer(id: string, allow: boolean): void {
    const pending = this.pending;
    if (!pending || pending.id !== id || pending.signal.aborted || this.snapshot.pending_approval?.id !== id) {
      fail('not_found', 'Approval is stale, stopped, or already answered.');
    }
    const failure = pending.settle(allow);
    if (failure) throw failure.error;
  }
}

function describeApproval(call: EffectCall): Approval {
  return {
    id: randomUUID(), tool_id: call.toolId, kind: call.name === 'command' ? 'command' : 'edit',
    summary: call.name === 'command' ? 'Run sandboxed command' : `Change ${call.arguments.path}`,
    details: JSON.stringify(call.arguments), dirty_paths: [...call.dirtyPaths],
    risk: 'Only this exact call is approved. Dirty and ignored paths are protected. Commands write only private scratch and a private per-thread build cache; no external network or checkout/Git writes. Tests may use scratch-local Git repositories and Unix sockets. Read tools and commands can read ignored checkout files and send their contents to the provider.',
  };
}
