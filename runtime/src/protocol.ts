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
export type Snapshot = {
  id: string; checkout: string; model: ModelChoice; status: Status;
  messages: { id: string; role: 'user' | 'assistant'; text: string }[];
  live_text: string;
  tools: { id: string; name: string; status: 'running' | 'done' | 'error' | 'interrupted'; input: string; output: string }[];
  pending_approval: Approval | null; error: Failure | null; truncated: boolean;
};
export type Request = Record<string, string | boolean> & { op: string };
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
export function validate(value: unknown): Request {
  if (!value || typeof value !== 'object' || Array.isArray(value)) fail('invalid_request', 'Expected an object.');
  const request = value as Request;
  const expected = typeof request.op === 'string' && Object.hasOwn(fields, request.op) ? fields[request.op] : undefined;
  if (!expected || Object.keys(request).some(key => key !== 'op' && !expected.includes(key))) fail('invalid_request', 'Unknown operation or field.');
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
