import { fail } from '../protocol.ts';
import type { LinearIssueDetail, LinearIssueSummary } from './types.ts';

const IDENTIFIER = /^[A-Z][A-Z0-9]{0,15}-\d{1,9}$/;

/** Accept only the documented issue URL shape, never a numeric/title search result. */
export function issueReference(reference: string): { identifier: string; workspaceKey: string | null } {
  const value = reference.trim();
  if (IDENTIFIER.test(value.toUpperCase())) return { identifier: value.toUpperCase(), workspaceKey: null };
  if (URL.canParse(value)) {
    const url = new URL(value);
    const parts = url.pathname.split('/');
    const identifier = parts[3]?.toUpperCase() ?? '';
    if (url.protocol === 'https:' && url.hostname === 'linear.app' && !url.port && !url.username && !url.password
        && parts.length >= 4 && parts.length <= 6 && parts[1] && parts[2] === 'issue'
        && IDENTIFIER.test(identifier) && (parts.length < 6 || (parts[4] !== '' && parts[5] === ''))) {
      return { identifier, workspaceKey: parts[1] };
    }
  }
  fail('invalid_request', 'reference must be an issue identifier or an https Linear issue URL.');
}

export function matchesIssueUrl(url: string, identifier: string, workspaceKey: string): boolean {
  try {
    const parsed = issueReference(url);
    return parsed.identifier === identifier && parsed.workspaceKey === workspaceKey;
  } catch {
    return false;
  }
}

export function issueSummary(issue: LinearIssueDetail): LinearIssueSummary {
  const { description: _description, attachments: _attachments, comments: _comments,
    comments_has_more: _hasMore, comments_next_cursor: _cursor, ...summary } = issue;
  return summary;
}
