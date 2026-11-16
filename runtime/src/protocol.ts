import { clipTail } from './text.ts';

export type QueueMode = 'follow_up' | 'steer';
export type InputRecord = {
  request_id: string; text: string; entry: number | null; anchor: number;
  /** Set while the input waits behind a run; it is not part of the conversation yet. */
  queued?: { mode: QueueMode; approval: ApprovalMode };
};
export const inputMessage = (input: InputRecord) => ({ id: `request-${input.request_id}`, role: 'user' as const, text: clipTail(input.text, 4096) });

export type ModelChoice = { provider: string; model_id: string };
export type Status = 'idle' | 'running' | 'awaiting_approval' | 'stopped' | 'interrupted' | 'error';
export type Failure = { code: string; message: string };
export type Approval = {
  id: string; tool_id: string; kind: 'edit' | 'command'; summary: string;
  details: string; dirty_paths: string[]; risk: string;
};
export type ApprovalMode = 'ask' | 'auto';
export const THINKING_LEVELS = ['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'] as const;
export type ThinkingLevel = typeof THINKING_LEVELS[number];
export type Queued = { id: string; request_id: string; text: string; mode: QueueMode; approval: ApprovalMode };
export type Usage = {
  input: number; output: number; cache_read: number; cache_write: number; reasoning: number | null; total: number;
  context_used: number | null; context_window: number | null;
};
export type SnapshotMessage = { id: string; role: 'user' | 'assistant'; text: string; reasoning?: string };
export type SnapshotTool = {
  id: string; name: string; status: 'running' | 'done' | 'error' | 'interrupted'; input: string; output: string;
  after?: string | null;
  /** Version 3 order, kept only when it differs: the tool followed a reasoning-only message. */
  after3?: string;
};
// Version 2 adds tool ordering (`after`) and the run's approval mode. Version 3 adds
// thread metadata, reasoning, usage, and queued inputs. Saved snapshots from older
// hosts lack them; `view` fills or strips them per client.
export type Snapshot = {
  id: string; checkout: string; model: ModelChoice; status: Status;
  messages: SnapshotMessage[];
  live_text: string;
  tools: SnapshotTool[];
  pending_approval: Approval | null; error: Failure | null; truncated: boolean;
  approval_mode?: ApprovalMode;
  title?: string; renamed?: boolean; created_at?: number; updated_at?: number; archived?: boolean;
  thinking_level?: ThinkingLevel; queued?: Queued[]; usage?: Usage | null;
  live_reasoning?: string; retry?: { attempt: number; at: number } | null;
};
export type Summary = {
  id: string; checkout: string; model: ModelChoice; status: string;
  title: string; created_at: number; updated_at: number; archived: boolean; pending_approval: boolean;
};
export const PROTOCOL_VERSIONS = [1, 2, 3] as const;
export type Protocol = typeof PROTOCOL_VERSIONS[number];
export type Request = Record<string, string | boolean | number> & { op: string };
export function version(request: Request): Protocol {
  return request.protocol_version === 3 ? 3 : request.protocol_version === 2 ? 2 : 1;
}
const TURN_LIMIT = 'Agent ended without an answer (turn_limit). Inspect the transcript before retrying.';
/**
 * The snapshot exactly as a version 2 host stored it. Reasoning-only messages have
 * no text, so older clients never see them and tool order falls back to `after`.
 */
export function legacy(snapshot: Snapshot) {
  const { id, checkout, model, status, messages, live_text, tools, pending_approval, error, truncated, approval_mode } = snapshot;
  return {
    id, checkout, model, status,
    messages: messages.filter(message => message.text).map(({ id, role, text }) => ({ id, role, text })),
    live_text,
    tools: tools.map(({ after3: _after3, ...tool }) => tool),
    pending_approval,
    // Older clients know this failure only by its earlier generic code.
    error: error?.code === 'turn_limit' ? { code: 'runtime_error', message: TURN_LIMIT } : error,
    truncated,
    ...(approval_mode ? { approval_mode } : {}),
  };
}
/** The wire shape a client negotiated. Version 1 and 2 clients reject unknown fields. */
export function view(snapshot: Snapshot, protocol: Protocol) {
  const { approval_mode, tools, ...rest } = legacy(snapshot);
  if (protocol === 1) return { ...rest, tools: tools.map(({ after: _after, ...tool }) => tool) };
  // Tools saved before version 2 keep no `after`; clients show them in the old order.
  if (protocol === 2) return { ...rest, tools, approval_mode: approval_mode ?? 'ask' };
  return {
    ...rest, error: snapshot.error,
    messages: snapshot.messages.map(({ id, role, text, reasoning }) => ({ id, role, text, reasoning: reasoning ?? '' })),
    tools: snapshot.tools.map(({ after3, ...tool }) => after3 === undefined ? tool : { ...tool, after: after3 }),
    approval_mode: approval_mode ?? 'ask',
    title: snapshot.title ?? '', created_at: snapshot.created_at ?? 0, updated_at: snapshot.updated_at ?? 0,
    archived: snapshot.archived ?? false, thinking_level: snapshot.thinking_level ?? 'off',
    queued: snapshot.queued ?? [], usage: snapshot.usage ?? null,
    live_reasoning: snapshot.live_reasoning ?? '', retry: snapshot.retry ?? null,
  };
}
export const queuedView = (input: InputRecord): Queued => ({
  id: `request-${input.request_id}`, request_id: input.request_id, text: clipTail(input.text, 4096),
  mode: input.queued!.mode, approval: input.queued!.approval,
});
export class HostError extends Error {
  code: string;
  constructor(code: string, message: string) { super(message); this.code = code; }
}
export function fail(code: string, message: string): never { throw new HostError(code, message); }
export function failure(error: unknown): Failure {
  return error instanceof HostError ? { code: error.code, message: error.message }
    : { code: 'internal', message: 'Host operation failed. No credentials or provider details are exposed.' };
}
const fields: Record<string, string[]> = {
  status: [], set_default_model: ['provider', 'model_id'], list_threads: ['checkout'],
  create_thread: ['checkout', 'provider', 'model_id', 'request_id'],
  submit: ['thread_id', 'request_id', 'text'], stop: ['thread_id'],
  approve: ['thread_id', 'approval_id', 'allow'], watch: ['thread_id'],
};
// Operations and fields that exist only for a request that negotiated version 3.
const fields3: Record<string, string[]> = {
  list_threads: [], history: ['thread_id'], history_text: ['thread_id', 'ref', 'offset'],
  rename_thread: ['thread_id', 'title'], archive_thread: ['thread_id', 'archived'], watch_threads: [],
  configure_thread: ['thread_id'], cancel_queued: ['thread_id', 'request_id'],
};
const optional3: Record<string, string[]> = {
  submit: ['when_busy'], list_threads: ['checkout', 'limit', 'cursor', 'archived'], history: ['before', 'limit'],
  watch_threads: ['archived'], configure_thread: ['provider', 'model_id', 'thinking'],
};
const limits: Record<string, number> = { list_threads: 200, history: 500 };
export const CURSOR = /^\d{1,16}:\d{1,16}$/;
export const HISTORY_CURSOR = /^\d{1,16}:[01]:\d{1,16}$/;
export const HISTORY_REF = /^(?:e\d{1,16}\.\d{1,6}\.(?:text|reasoning|output|input\.\d{1,6})|q\d{1,16})$/;
const text = (value: unknown): value is string => typeof value === 'string' && !!value.trim();
function validate3(request: Request) {
  const invalid = (message: string): never => fail('invalid_request', message);
  if (request.checkout !== undefined && !text(request.checkout)) invalid('checkout must be a nonempty string.');
  if (request.limit !== undefined && (!Number.isSafeInteger(request.limit) || (request.limit as number) < 1 || (request.limit as number) > limits[request.op])) {
    invalid(`limit must be an integer from 1 to ${limits[request.op]}.`);
  }
  if (request.cursor !== undefined && (typeof request.cursor !== 'string' || !CURSOR.test(request.cursor))) invalid('Invalid cursor.');
  if (request.before !== undefined && (typeof request.before !== 'string' || !HISTORY_CURSOR.test(request.before))) invalid('Invalid before cursor.');
  if (request.ref !== undefined && !HISTORY_REF.test(request.ref as string)) invalid('Invalid ref.');
  if (request.archived !== undefined && typeof request.archived !== 'boolean') invalid('archived must be boolean.');
  if (request.when_busy !== undefined && request.when_busy !== 'follow_up' && request.when_busy !== 'steer') invalid('when_busy must be follow_up or steer.');
  if (request.op !== 'configure_thread') return;
  if ((request.provider === undefined) !== (request.model_id === undefined)) invalid('provider and model_id must be given together.');
  if (request.provider !== undefined && (!text(request.provider) || !text(request.model_id))) invalid('provider and model_id must be nonempty strings.');
  if (request.thinking !== undefined && !THINKING_LEVELS.includes(request.thinking as ThinkingLevel)) invalid(`thinking must be one of ${THINKING_LEVELS.join(', ')}.`);
  if (request.provider === undefined && request.thinking === undefined) invalid('Give a model, a thinking level, or both.');
}
export function validate(value: unknown): Request {
  if (!value || typeof value !== 'object' || Array.isArray(value)) fail('invalid_request', 'Expected an object.');
  const request = value as Request;
  const third = version(request) === 3;
  const known = typeof request.op === 'string' ? request.op : '';
  const expected = third && Object.hasOwn(fields3, known) ? fields3[known] : Object.hasOwn(fields, known) ? fields[known] : undefined;
  // `approval` stays allowed on every version so version 1 keeps its specific error.
  const allowed = expected && [...expected, 'protocol_version', ...(known === 'submit' ? ['approval'] : []), ...(third ? optional3[known] ?? [] : [])];
  if (!allowed || Object.keys(request).some(key => key !== 'op' && !allowed.includes(key))) fail('invalid_request', 'Unknown operation or field.');
  if (request.protocol_version !== undefined && !PROTOCOL_VERSIONS.includes(request.protocol_version as Protocol)) {
    fail('invalid_request', 'Unsupported protocol_version.');
  }
  if (request.approval !== undefined && (version(request) === 1 || (request.approval !== 'ask' && request.approval !== 'auto'))) {
    fail('invalid_request', 'approval must be ask or auto and requires protocol_version 2.');
  }
  for (const key of expected) {
    if (key === 'allow' || key === 'archived') {
      if (typeof request[key] !== 'boolean') fail('invalid_request', `${key} must be boolean.`);
    } else if (key === 'offset') {
      if (!Number.isSafeInteger(request[key]) || (request[key] as number) < 0) fail('invalid_request', 'offset must be a non-negative integer.');
    } else if (!text(request[key])) fail('invalid_request', `${key} must be a nonempty string.`);
  }
  if (third) validate3(request);
  for (const key of ['request_id', 'thread_id', 'approval_id']) {
    if (request[key] !== undefined && !/^[A-Za-z0-9_-]{1,128}$/.test(request[key] as string)) fail('invalid_request', `Invalid ${key}.`);
  }
  if (Buffer.byteLength(JSON.stringify(request)) > 65000) fail('invalid_request', 'Request is too large.');
  return request;
}
