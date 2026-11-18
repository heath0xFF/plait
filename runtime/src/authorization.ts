import { fail } from './protocol.ts';

export type EffectCall = Readonly<{
  toolId: string;
  name: 'edit' | 'write' | 'command';
  arguments: Readonly<Record<string, unknown>>;
  dirtyPaths: readonly string[];
  // Present only for a trusted run, so approval text can describe it honestly.
  commandPolicy?: 'trusted';
  mcp?: Readonly<{ server: string; tool: string }>;
}>;

export interface AuthorizationPolicy {
  authorize(call: EffectCall, signal: AbortSignal): Promise<void>;
}

// Authorization belongs to one admission, never to the saved conversation.
export class RunAuthorization {
  private enabled = false;
  private readonly controller = new AbortController();
  private readonly policy: AuthorizationPolicy;

  constructor(policy: AuthorizationPolicy) {
    this.policy = policy;
  }

  get signal(): AbortSignal { return this.controller.signal; }
  get active(): boolean { return this.enabled && !this.signal.aborted; }

  activate(): void {
    if (this.signal.aborted) fail('permission_denied', 'Run permission has been revoked.');
    this.enabled = true;
  }

  revoke(): void {
    this.enabled = false;
    this.controller.abort();
  }

  check(signal?: AbortSignal): void {
    if (!this.active || signal?.aborted) fail('permission_denied', 'Run is not active.');
  }

  async authorize(call: EffectCall, signal?: AbortSignal): Promise<void> {
    this.check(signal);
    const combined = signal ? AbortSignal.any([this.signal, signal]) : this.signal;
    await this.policy.authorize(call, combined);
    // An answer does not outrank a Stop received while the decision was pending.
    this.check(combined);
  }
}
