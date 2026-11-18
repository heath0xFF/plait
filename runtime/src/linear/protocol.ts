import { fail } from '../protocol.ts';
import { validateLinearKey } from './auth.ts';
import { FIELD_LIMITS, LINEAR_FILTER_PRESETS } from './types.ts';
import type { LinearFilterPreset } from './types.ts';

type OperationSpec = { required: readonly string[]; optional: readonly string[] };

const OPERATIONS: Record<string, OperationSpec> = {
  linear_auth_status: { required: [], optional: [] },
  linear_set_key: { required: ['key'], optional: [] },
  linear_sign_out: { required: [], optional: [] },
  linear_list_teams: { required: [], optional: ['refresh'] },
  linear_bind_project: { required: ['checkout', 'team_id'], optional: ['team_key', 'team_name'] },
  linear_get_project_binding: { required: ['checkout'], optional: [] },
  linear_list_issues: { required: ['team_id', 'filter'], optional: ['search', 'cursor', 'refresh'] },
  linear_get_issue: { required: ['issue_id'], optional: ['comments_cursor', 'refresh'] },
};

const ID_PATTERN = /^[A-Za-z0-9_-]{1,128}$/;
const TEAM_KEY_PATTERN = /^[A-Za-z0-9_-]{1,32}$/;
const CONTROL_PATTERN = /[\u0000-\u001f\u007f]/;

export type LinearRequest = Record<string, unknown> & { op: string };

export function handlesLinearOp(input: unknown): boolean {
  if (!input || typeof input !== 'object' || Array.isArray(input)) return false;
  const op = (input as { op?: unknown }).op;
  return typeof op === 'string' && Object.hasOwn(OPERATIONS, op);
}

function checkId(value: unknown, field: string): string {
  if (typeof value !== 'string' || !ID_PATTERN.test(value)) fail('invalid_request', `Invalid ${field}.`);
  return value;
}

function checkBoundedText(value: unknown, field: string, maxChars: number, allowEmpty = false): string {
  if (typeof value !== 'string' || CONTROL_PATTERN.test(value)) fail('invalid_request', `Invalid ${field}.`);
  const trimmed = value.trim();
  if (!allowEmpty && !trimmed) fail('invalid_request', `${field} must be a nonempty string.`);
  if (trimmed.length > maxChars) fail('invalid_request', `${field} is too long.`);
  return trimmed;
}

function checkCursor(value: unknown, field: string): string {
  if (typeof value !== 'string' || !value.length || Buffer.byteLength(value) > FIELD_LIMITS.cursor || CONTROL_PATTERN.test(value)) {
    fail('invalid_request', `Invalid ${field}.`);
  }
  return value;
}

function checkCheckout(value: unknown): string {
  if (typeof value !== 'string' || !value.startsWith('/') || Buffer.byteLength(value) > 4096 || CONTROL_PATTERN.test(value)) {
    fail('invalid_request', 'checkout must be an absolute path.');
  }
  return value;
}

/** Validates additive Linear requests strictly: refuses unknown fields (including protocol_version) and bounds every value. */
export function validateLinearRequest(input: unknown): LinearRequest {
  if (!input || typeof input !== 'object' || Array.isArray(input)) {
    fail('invalid_request', 'Invalid Linear request.');
  }
  const request = input as LinearRequest;
  const spec = typeof request.op === 'string' && Object.hasOwn(OPERATIONS, request.op) ? OPERATIONS[request.op] : undefined;
  if (!spec) fail('invalid_request', 'Unknown operation or field.');
  const allowed = new Set([...spec.required, ...spec.optional]);
  if (Object.keys(request).some(key => key !== 'op' && !allowed.has(key))
      || spec.required.some(key => request[key] === undefined)) {
    fail('invalid_request', 'Unknown operation or field.');
  }
  if (Buffer.byteLength(JSON.stringify(request)) > 65_000) {
    fail('invalid_request', 'Request is too large.');
  }

  switch (request.op) {
    case 'linear_auth_status':
    case 'linear_sign_out':
      return request;
    case 'linear_set_key':
      validateLinearKey(request.key);
      return request;
    case 'linear_list_teams':
      if (request.refresh !== undefined && typeof request.refresh !== 'boolean') fail('invalid_request', 'refresh must be boolean.');
      return request;
    case 'linear_get_project_binding':
      checkCheckout(request.checkout);
      return request;
    case 'linear_bind_project':
      checkCheckout(request.checkout);
      if (request.team_id !== null) checkId(request.team_id, 'team_id');
      if (request.team_key !== undefined && request.team_key !== null) {
        if (typeof request.team_key !== 'string' || !TEAM_KEY_PATTERN.test(request.team_key)) fail('invalid_request', 'Invalid team_key.');
      }
      if (request.team_name !== undefined && request.team_name !== null) {
        checkBoundedText(request.team_name, 'team_name', FIELD_LIMITS.name);
      }
      return request;
    case 'linear_list_issues':
      checkId(request.team_id, 'team_id');
      if (typeof request.filter !== 'string' || !(LINEAR_FILTER_PRESETS as readonly string[]).includes(request.filter)) {
        fail('invalid_request', `filter must be one of ${LINEAR_FILTER_PRESETS.join(', ')}.`);
      }
      if (request.search !== undefined) checkBoundedText(request.search, 'search', 200, true);
      if (request.cursor !== undefined) checkCursor(request.cursor, 'cursor');
      if (request.refresh !== undefined && typeof request.refresh !== 'boolean') fail('invalid_request', 'refresh must be boolean.');
      return request;
    case 'linear_get_issue':
      checkId(request.issue_id, 'issue_id');
      if (request.comments_cursor !== undefined) checkCursor(request.comments_cursor, 'comments_cursor');
      if (request.refresh !== undefined && typeof request.refresh !== 'boolean') fail('invalid_request', 'refresh must be boolean.');
      return request;
    default:
      fail('invalid_request', 'Unknown operation or field.');
  }
}

export function asFilterPreset(value: unknown): LinearFilterPreset {
  return value as LinearFilterPreset;
}
