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
