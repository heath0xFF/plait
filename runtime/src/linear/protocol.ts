import { fail } from '../protocol.ts';
import { validateLinearKey } from './auth.ts';
import { FIELD_LIMITS, LINEAR_COMMENT_BODY_MAX_BYTES, LINEAR_FILTER_PRESETS } from './types.ts';
import type { LinearFilterPreset } from './types.ts';

type OperationSpec = { required: readonly string[]; optional: readonly string[] };

const OPERATIONS: Record<string, OperationSpec> = {
  linear_info: { required: [], optional: [] },
  linear_get_write: { required: ['request_id'], optional: [] },
  linear_auth_status: { required: [], optional: [] },
  linear_set_key: { required: ['key'], optional: [] },
  linear_sign_out: { required: [], optional: [] },
  linear_list_teams: { required: [], optional: ['refresh'] },
  linear_bind_project: { required: ['checkout', 'team_id'], optional: ['team_key', 'team_name'] },
  linear_get_project_binding: { required: ['checkout'], optional: [] },
  linear_list_issues: { required: ['team_id', 'filter'], optional: ['search', 'cursor', 'refresh'] },
  linear_get_issue: { required: ['issue_id'], optional: ['comments_cursor', 'refresh'] },
  linear_link_thread: { required: ['thread_id', 'issue_id', 'identifier', 'url', 'title'], optional: [] },
  linear_unlink_thread: { required: ['thread_id'], optional: [] },
  linear_get_thread_link: { required: ['thread_id'], optional: [] },
  linear_list_thread_links: { required: [], optional: ['limit', 'cursor'] },
  linear_post_comment: { required: ['issue_id', 'body', 'request_id'], optional: [] },
  linear_update_issue_state: { required: ['issue_id', 'state_id', 'request_id'], optional: [] },
};

const ID_PATTERN = /^[A-Za-z0-9_-]{1,128}$/;
const IDENTIFIER_PATTERN = /^[A-Z][A-Z0-9]{0,15}-\d{1,9}$/;
const LINK_CURSOR_PATTERN = /^(\d{1,16}):([A-Za-z0-9_-]{1,128})$/;
const TEAM_KEY_PATTERN = /^[A-Za-z0-9_-]{1,32}$/;
const CONTROL_PATTERN = /[\u0000-\u001f\u007f]/;
const CURSOR_CONTROL_PATTERN = /[\p{Cf}\p{Zl}\p{Zp}\u0000-\u001f\u007f-\u009f]/u;
// Multiline comments permit tabs and newlines, but reject NUL and other C0/DEL control bytes.
const COMMENT_CONTROL_PATTERN = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/;
export const LINEAR_LINKS_MAX_LIMIT = 500;
export const LINEAR_LINKS_DEFAULT_LIMIT = 200;

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
  if (value.length > maxChars) fail('invalid_request', `${field} is too long.`);
  const trimmed = value.trim();
  if (!allowEmpty && !trimmed) fail('invalid_request', `${field} must be a nonempty string.`);
  return trimmed;
}

function checkCursor(value: unknown, field: string): string {
  if (typeof value !== 'string' || !value.length || Buffer.byteLength(value, 'utf8') > FIELD_LIMITS.cursor || CURSOR_CONTROL_PATTERN.test(value)) {
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

function checkHttpsUrl(value: unknown, field: string): string {
  if (typeof value !== 'string' || !value.length || Buffer.byteLength(value) > FIELD_LIMITS.url || CONTROL_PATTERN.test(value) || !URL.canParse(value)) {
    fail('invalid_request', `${field} must be a valid https URL.`);
  }
  const url = new URL(value);
  if (url.protocol !== 'https:' || url.username || url.password) {
    fail('invalid_request', `${field} must be an https URL without embedded credentials.`);
  }
  return url.href;
}

export function parseLinkCursor(value: string): { linkedAt: number; threadId: string } {
  const match = LINK_CURSOR_PATTERN.exec(value);
  if (!match || !Number.isSafeInteger(Number(match[1]))) fail('invalid_request', 'Invalid cursor.');
  return { linkedAt: Number(match[1]), threadId: match[2] };
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
    case 'linear_get_write':
      checkId(request.request_id, 'request_id');
      return request;
    case 'linear_info':
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
    case 'linear_link_thread':
      checkId(request.thread_id, 'thread_id');
      checkId(request.issue_id, 'issue_id');
      if (typeof request.identifier !== 'string' || !IDENTIFIER_PATTERN.test(request.identifier)) {
        fail('invalid_request', 'Invalid identifier.');
      }
      checkHttpsUrl(request.url, 'url');
      checkBoundedText(request.title, 'title', FIELD_LIMITS.title);
      return request;
    case 'linear_unlink_thread':
    case 'linear_get_thread_link':
      checkId(request.thread_id, 'thread_id');
      return request;
    case 'linear_list_thread_links':
      if (request.limit !== undefined && (!Number.isSafeInteger(request.limit) || (request.limit as number) < 1 || (request.limit as number) > LINEAR_LINKS_MAX_LIMIT)) {
        fail('invalid_request', `limit must be an integer from 1 to ${LINEAR_LINKS_MAX_LIMIT}.`);
      }
      if (request.cursor !== undefined) {
        if (typeof request.cursor !== 'string') fail('invalid_request', 'Invalid cursor.');
        parseLinkCursor(request.cursor);
      }
      return request;
    case 'linear_post_comment':
      checkId(request.issue_id, 'issue_id');
      checkId(request.request_id, 'request_id');
      if (typeof request.body !== 'string' || !request.body.trim() || COMMENT_CONTROL_PATTERN.test(request.body)) {
        fail('invalid_request', 'body must be a nonempty string without control characters.');
      }
      if (Buffer.byteLength(request.body, 'utf8') > LINEAR_COMMENT_BODY_MAX_BYTES) {
        fail('invalid_request', 'body must be at most 16 KiB.');
      }
      return request;
    case 'linear_update_issue_state':
      checkId(request.issue_id, 'issue_id');
      checkId(request.state_id, 'state_id');
      checkId(request.request_id, 'request_id');
      return request;
    default:
      fail('invalid_request', 'Unknown operation or field.');
  }
}

export function asFilterPreset(value: unknown): LinearFilterPreset {
  return value as LinearFilterPreset;
}
