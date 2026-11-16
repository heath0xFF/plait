import { clipTail } from './text.ts';

export type InputRecord = { request_id: string; text: string; entry: number | null; anchor: number };
export const inputMessage = (input: InputRecord) => ({ id: `request-${input.request_id}`, role: 'user' as const, text: clipTail(input.text, 4096) });

export type ModelChoice = { provider: string; model_id: string };
export type Status = 'idle' | 'running' | 'awaiting_approval' | 'stopped' | 'interrupted' | 'error';
export type Failure = { code: string; message: string };
export type Approval = {
  id: string; tool_id: string; kind: 'edit' | 'command'; summary: string;
  details: string; dirty_paths: string[]; risk: string;
};
export type ApprovalMode = 'ask' | 'auto';
// Version 2 adds tool ordering (`after`) and the run's approval mode. Saved
// snapshots from version 1 hosts lack both; `view` fills or strips them per client.
export type Snapshot = {
  id: string; checkout: string; model: ModelChoice; status: Status;
  messages: { id: string; role: 'user' | 'assistant'; text: string }[];
  live_text: string;
  tools: { id: string; name: string; status: 'running' | 'done' | 'error' | 'interrupted'; input: string; output: string; after?: string | null }[];
  pending_approval: Approval | null; error: Failure | null; truncated: boolean;
  approval_mode?: ApprovalMode;
};
export const PROTOCOL_VERSIONS = [1, 2] as const;
export type Request = Record<string, string | boolean | number> & { op: string };
export function version(request: Request): 1 | 2 { return request.protocol_version === 2 ? 2 : 1; }
/** The wire shape a client negotiated. Version 1 clients reject unknown fields. */
export function view(snapshot: Snapshot, protocol: 1 | 2) {
  const { approval_mode, tools, ...rest } = snapshot;
  if (protocol === 1) return { ...rest, tools: tools.map(({ after: _after, ...tool }) => tool) };
  // Tools saved before version 2 keep no `after`; clients show them in the old order.
  return { ...rest, tools, approval_mode: approval_mode ?? 'ask' };
}
export class HostError extends Error {
  code: string;
  constructor(code: string, message: string) { super(message); this.code = code; }
}
export function fail(code: string, message: string): never { throw new HostError(code, message); }
export function failure(error: unknown): Failure {
  return error instanceof HostError ? { code: error.code, message: error.message }
    : { code: 'internal', message: 'Host operation failed. No credentials or provider details are exposed.' };
}
// Optional on every operation; absent means version 1.
const optional: Record<string, string[]> = { submit: ['approval'] };
const fields: Record<string, string[]> = {
  status: [], set_default_model: ['provider', 'model_id'], list_threads: ['checkout'],
  create_thread: ['checkout', 'provider', 'model_id', 'request_id'],
  submit: ['thread_id', 'request_id', 'text'], stop: ['thread_id'],
  approve: ['thread_id', 'approval_id', 'allow'], watch: ['thread_id'],
};
export function validate(value: unknown): Request {
  if (!value || typeof value !== 'object' || Array.isArray(value)) fail('invalid_request', 'Expected an object.');
  const request = value as Request;
  const expected = typeof request.op === 'string' && Object.hasOwn(fields, request.op) ? fields[request.op] : undefined;
  const allowed = expected && [...expected, 'protocol_version', ...(optional[request.op] ?? [])];
  if (!allowed || Object.keys(request).some(key => key !== 'op' && !allowed.includes(key))) fail('invalid_request', 'Unknown operation or field.');
  if (request.protocol_version !== undefined && !PROTOCOL_VERSIONS.includes(request.protocol_version as 1 | 2)) {
    fail('invalid_request', 'Unsupported protocol_version.');
  }
  if (request.approval !== undefined && (version(request) !== 2 || (request.approval !== 'ask' && request.approval !== 'auto'))) {
    fail('invalid_request', 'approval must be ask or auto and requires protocol_version 2.');
  }
  for (const key of expected) {
    if (key === 'allow') {
      if (typeof request[key] !== 'boolean') fail('invalid_request', 'allow must be boolean.');
    } else if (typeof request[key] !== 'string' || !(request[key] as string).trim()) fail('invalid_request', `${key} must be a nonempty string.`);
  }
  for (const key of ['request_id', 'thread_id', 'approval_id']) {
    if (request[key] !== undefined && !/^[A-Za-z0-9_-]{1,128}$/.test(request[key] as string)) fail('invalid_request', `Invalid ${key}.`);
  }
  if (Buffer.byteLength(JSON.stringify(request)) > 65000) fail('invalid_request', 'Request is too large.');
  return request;
}
