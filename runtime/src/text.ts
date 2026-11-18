function requireLimit(limit: number) {
  if (!Number.isSafeInteger(limit) || limit < 0) throw new RangeError('Text limit must be a non-negative integer.');
}

function splitsPair(text: string, offset: number) {
  const before = text.charCodeAt(offset - 1);
  const after = text.charCodeAt(offset);
  return before >= 0xd800 && before <= 0xdbff && after >= 0xdc00 && after <= 0xdfff;
}

// Keep the existing UTF-16 budgets without creating an unpaired surrogate in JSON.
export function clipHead(text: string, limit: number): string {
  requireLimit(limit);
  let end = Math.min(text.length, limit);
  if (splitsPair(text, end)) end--;
  return text.slice(0, end);
}

export function clipTail(text: string, limit: number): string {
  requireLimit(limit);
  let start = Math.max(0, text.length - limit);
  if (splitsPair(text, start)) start++;
  return text.slice(start);
}

/** Whether a UTF-16 offset falls between the two halves of one character. */
export function splitsCharacter(text: string, offset: number): boolean {
  return offset > 0 && offset < text.length && splitsPair(text, offset);
}

// Rust's `char::is_whitespace` set, so host and desktop titles agree.
const WHITE_SPACE = /[\u0009-\u000D\u0020\u0085\u00A0\u1680\u2000-\u200A\u2028-\u2029\u202F\u205F\u3000]+/u;
function plain(text: string, limit: number): string {
  // Collapse whitespace, drop control characters, and count whole code points.
  return Array.from(text.split(WHITE_SPACE).filter(Boolean).join(' ').replace(/\p{Cc}/gu, '')).slice(0, limit).join('');
}
/** A thread title derived from its first message, by the desktop's existing rule. */
export const title = (text: string) => plain(text, 72);
/** A title the user chose: longer than a derived one, still one plain line. */
export const customTitle = (text: string) => plain(text, 200);

/** Keep layout controls visible in approvals and tool data, preserving ordinary newlines/tabs. */
export function escapeDisplay(value: string): string {
  return value.replace(/[\p{Cf}\p{Zl}\p{Zp}\u0000-\u0008\u000b-\u001f\u007f-\u009f]/gu,
    character => character.split('').map(unit => '\\u' + unit.charCodeAt(0).toString(16).padStart(4, '0')).join(''));
}
