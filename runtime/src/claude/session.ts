import type { ImageData } from '../images.ts';
// One Claude Code session for one Plait run, through the pinned Agent SDK in
// streaming-input mode. Claude Code owns the work: its tools, its permission
// mode, the person's settings, CLAUDE.md files, skills, slash commands, hooks,
// and MCP servers, all loaded as `claude` itself loads them. Plait sends
// messages, answers permission requests, and reads the messages back.
import { randomUUID } from 'node:crypto';
import type {
  CanUseTool, EffortLevel, Options, PermissionMode, Query, SDKControlInitializeResponse, SDKMessage, SDKUserMessage, ThinkingConfig,
} from '@anthropic-ai/claude-agent-sdk';
import type { ThinkingLevel } from '../protocol.ts';
import type { ClaudeModel } from './install.ts';
import type { QueryFunction } from './sdk.ts';

/** Claude Code's permission modes that Plait offers; `dontAsk` can only come from the person's settings. */
export type ClaudeModeId = 'default' | 'acceptEdits' | 'plan' | 'auto' | 'bypassPermissions';

/** Token budgets for older models that take one instead of an effort level. */
const BUDGETS: Partial<Record<ThinkingLevel, number>> = { low: 4096, medium: 16384, high: 32768 };
const EFFORTS = new Set<string>(['low', 'medium', 'high', 'xhigh', 'max']);

/**
 * The SDK options for a Plait thinking level. Thinking text is asked for as a
 * summary, since newer models omit it by default and the thread would show
 * none; display changes what is returned, not what is billed. An unknown or
 * unsupported level leaves Claude Code's default.
 */
export function claudeThinking(level: ThinkingLevel | undefined, model: Pick<ClaudeModel, 'levels' | 'adaptive'> | undefined): { effort?: EffortLevel; thinking?: ThinkingConfig } {
  const adaptive = model?.adaptive !== false;
  const offered = level !== undefined && (model === undefined || model.levels.includes(level));
  if (adaptive) {
    const thinking: ThinkingConfig = { type: 'adaptive', display: 'summarized' };
    return offered && EFFORTS.has(level!) ? { thinking, effort: level as EffortLevel } : { thinking };
  }
  if (!offered) return {};
  if (level === 'off') return { thinking: { type: 'disabled' } };
  const budget = BUDGETS[level!];
  return budget ? { thinking: { type: 'enabled', budgetTokens: budget, display: 'summarized' } } : {};
}

export type ClaudeSessionOptions = {
  /** The SDK's `query`, from the copy `loadSdk` checked. */
  query: QueryFunction;
  path: string;
  /** The login environment; the SDK passes it to Claude Code as is. */
  env: NodeJS.ProcessEnv;
  cwd: string;
  /** A model from the catalog; `default` or none leaves Claude Code's own choice. */
  model?: string;
  modelInfo?: ClaudeModel;
  level?: ThinkingLevel;
  mode: ClaudeModeId;
  /** The Claude session this thread continues. Without one a new session starts under a fresh ID. */
  resume?: string;
  canUseTool: CanUseTool;
  /** Claude Code's stderr; it can quote the machine or the account, so it is for local logs only. */
  stderr?: (text: string) => void;
};

/** Messages for the SDK to send, queued until it asks for the next one. */
class Input implements AsyncIterable<SDKUserMessage> {
  private readonly queued: SDKUserMessage[] = [];
  private waiting?: (result: IteratorResult<SDKUserMessage>) => void;
  private ended = false;
  push(message: SDKUserMessage) {
    if (this.ended) throw new Error('This Claude Code session is closed.');
    if (this.waiting) { const resolve = this.waiting; this.waiting = undefined; resolve({ done: false, value: message }); }
    else this.queued.push(message);
  }
  end() {
    this.ended = true;
    this.waiting?.({ done: true, value: undefined }); this.waiting = undefined;
  }
  [Symbol.asyncIterator](): AsyncIterator<SDKUserMessage> {
    return {
      next: () => {
        const message = this.queued.shift();
        if (message) return Promise.resolve({ done: false, value: message });
        if (this.ended) return Promise.resolve({ done: true, value: undefined });
        return new Promise(resolve => { this.waiting = resolve; });
      },
    };
  }
}

export type Interrupted = { cancelled: string[]; stillQueued: string[] };

export class ClaudeSession {
  /** Known from the start: a new session is given its ID, a resumed one keeps its own. */
  readonly sessionId: string;
  readonly initialized: Promise<SDKControlInitializeResponse>;
  private readonly query: Query;
  private readonly input = new Input();
  private mode: PermissionMode | null = null;
  private capabilities: readonly string[] = [];
  private closed = false;

  constructor(options: ClaudeSessionOptions) {
    this.sessionId = options.resume ?? randomUUID();
    const model = options.model && options.model !== 'default' ? options.model : undefined;
    // The SDK edits the object it is given; the caller's stays as it was.
    const env: NodeJS.ProcessEnv = { ...options.env, CLAUDE_AGENT_SDK_CLIENT_APP: 'plait' };
    // Plait never re-runs an interrupted turn on its own, so Claude Code must not either.
    delete env.CLAUDE_CODE_RESUME_INTERRUPTED_TURN;
    const sdkOptions: Options = {
      pathToClaudeCodeExecutable: options.path,
      env,
      cwd: options.cwd,
      ...(model ? { model } : {}),
      ...claudeThinking(options.level, options.modelInfo),
      permissionMode: options.mode,
      // Lets the person switch to Bypass during a run; it bypasses nothing by itself.
      allowDangerouslySkipPermissions: true,
      canUseTool: options.canUseTool,
      includePartialMessages: true,
      ...(options.resume ? { resume: options.resume } : { sessionId: this.sessionId }),
      // Echoes each sent message when a turn takes it, which is how the transcript places it.
      extraArgs: { 'replay-user-messages': null },
      // `settingSources` stays unset: Claude Code loads user, project, and local settings as `claude` does.
      stderr: options.stderr ?? (() => {}),
    };
    this.query = options.query({ prompt: this.input, options: sdkOptions });
    this.initialized = this.query.initializationResult().then(result => {
      // Reported by the CLI beside the documented fields.
      const current = (result as { current_permission_mode?: unknown }).current_permission_mode;
      if (typeof current === 'string' && this.mode === null) this.mode = current as PermissionMode;
      return result;
    });
    // A failed start is reported by `messages()`; this keeps the promise from being unhandled.
    this.initialized.catch(() => {});
  }

  /** The mode Claude Code reports: from `initialize`, then each `init` and `status` message. Null until it says. */
  permissionMode(): PermissionMode | null { return this.mode; }

  /**
   * Sends a message, before or during a turn. During one, Claude Code takes it
   * at its next step (steering) or runs it as the next turn. Returns the uuid
   * its echo and reply frames carry; register it with `ClaudeTranscript.sent`.
   * The request ID stays on Plait's side: every send gets a fresh uuid, since
   * Claude Code treats a repeated uuid as a re-send of the same message.
   */
  send(text: string, _requestId: string, images: ImageData[] = []): string {
    const uuid = randomUUID();
    this.input.push({ type: 'user', message: { role: 'user', content: images.length ? [{ type: 'text', text }, ...images.map(image => ({ type: 'image' as const, source: { type: 'base64' as const, media_type: image.ref.media_type, data: image.data } }))] : text }, parent_tool_use_id: null, uuid });
    return uuid;
  }

  /** Switches mode, also during a turn; the reported mode follows in a `status` message. */
  async setPermissionMode(mode: ClaudeModeId): Promise<void> {
    await this.query.setPermissionMode(mode);
  }

  /**
   * Stops the running turn. Where Claude Code supports it, messages still
   * queued are cancelled too, so nothing runs after Stop; `cancelled` and
   * `stillQueued` name them by uuid.
   */
  async interrupt(): Promise<Interrupted> {
    const cancelQueued = this.capabilities.includes('interrupt_cancel_queued_v1');
    // The SDK's interrupt takes `{ cancelQueued }` (sdk.mjs 0.3.326) though its declaration lists no parameter.
    const interrupt = this.query.interrupt as (options?: { cancelQueued?: boolean }) => ReturnType<Query['interrupt']>;
    const receipt = await interrupt.call(this.query, cancelQueued ? { cancelQueued: true } : undefined);
    return { cancelled: receipt?.cancelled ?? [], stillQueued: receipt?.still_queued ?? [] };
  }

  /** Every message Claude Code sends, until it exits or the session closes. Read by one consumer. */
  async *messages(): AsyncGenerator<SDKMessage, void> {
    try {
      for await (const message of this.query) {
        if (message.type === 'system' && (message.subtype === 'init' || message.subtype === 'status')) {
          if (typeof message.permissionMode === 'string') this.mode = message.permissionMode;
          if (message.subtype === 'init' && Array.isArray(message.capabilities)) this.capabilities = message.capabilities;
        }
        yield message;
      }
    } finally {
      // Once Claude Code has exited, a later send fails at once instead of waiting forever.
      this.close();
    }
  }

  /** Ends the session and its process. Safe to call more than once. */
  close() {
    if (this.closed) return;
    this.closed = true;
    this.input.end();
    this.query.close();
  }
}
