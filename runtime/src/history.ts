import type { EntryRecord } from '@earendil-works/pi-durable';
import type { Placed } from './store.ts';
import { fail } from './protocol.ts';
import type { PendingInput } from './store.ts';
import { messageText, reasoningText } from './transcript.ts';
import type { Message } from './transcript.ts';
import { clipHead, splitsCharacter, escapeDisplay } from './text.ts';

/** UTF-16 units of one field carried inline, and of one `history_text` reply. */
export const INLINE_UNITS = 65536;
const PAGE_BYTES = 614400;
const HARD_BYTES = 921600;
const CHUNK = 64;
const LOOKAHEAD = 256;
const NEWEST = Number.MAX_SAFE_INTEGER;
const FIELDS = ['text', 'reasoning', 'input', 'output'] as const;

type Field = typeof FIELDS[number];
type More = { field: Field; ref: string; offset: number; length: number };
type MessageItem = { type: 'message'; id: string; role: 'user' | 'assistant'; text: string; reasoning: string; more?: More[] };
type ToolItem = { type: 'tool'; id: string; name: string; status: 'running' | 'done' | 'error' | 'interrupted'; input: string; output: string; more?: More[] };
export type HistoryItem = MessageItem | ToolItem;
/** Where a thread's complete conversation is read from. `entries` is absent before the first run. */
export type HistorySource = {
  entries?(query: { minEntryId?: number; maxEntryId?: number; order: 'ascending' | 'descending' }, limit: number): Promise<readonly EntryRecord[]>;
  pending(anchor: number, rowid: number, limit: number): PendingInput[];
  placed(low: number, high: number): Map<number, Placed>;
  /** The typed text of the input placed at an entry. */
  typed(entry: number): string | undefined;
  journal(rowid: number): string | undefined;
  /** A tool call with no result is still running only while its run is. */
  active: boolean;
};
type Result = { text: string; error: boolean; ref: string };
type Unit = { key: string; items: HistoryItem[]; results: Map<string, Result>; bytes: number; entry?: number };
const bytes = (value: unknown) => Buffer.byteLength(JSON.stringify(value));
// Where each field's full text lives, so any field can move behind a reference later.
const sources = new WeakMap<HistoryItem, Partial<Record<Field, { ref: string; length: number }>>>();

function setInline(item: HistoryItem, field: Field, inline: string) {
  const { ref, length } = sources.get(item)![field]!;
  (item as unknown as Record<Field, string>)[field] = inline;
  const more = (item.more ?? []).filter(part => part.field !== field);
  if (inline.length < length) more.push({ field, ref, offset: inline.length, length });
  if (more.length) item.more = more; else delete item.more;
}
/** Sets a field; text beyond the inline bound stays behind a `history_text` reference. */
function assign(item: HistoryItem, field: Field, value: string, ref: string) {
  sources.set(item, { ...sources.get(item), [field]: { ref, length: value.length } });
  setInline(item, field, clipHead(value, INLINE_UNITS));
}
function entryUnit(entry: EntryRecord, placed: Map<number, Placed>): Unit {
  const id = Number(entry.id);
  const unit: Unit = { key: `${id}:0:0`, items: [], results: new Map(), bytes: 0, entry: id };
  // A model-context summary is not a message the user sent; original entries remain readable.
  if (entry.kind === 'pi.compaction') return unit;
  for (const [index, message] of ((entry.model ?? []) as readonly Message[]).entries()) {
    const ref = `e${id}.${index}`;
    if (message.role === 'user' || message.role === 'assistant') {
      // A placed `/skill:` input reads as typed (the store gives no text for others).
      const input = message.role === 'user' ? placed.get(id) : undefined;
      const typed = input?.text !== undefined;
      const text = typed ? input!.text! : messageText(message); const reasoning = message.role === 'assistant' ? reasoningText(message) : '';
      if (text || reasoning) {
        // The same stable IDs the snapshot uses.
        const item: MessageItem = { type: 'message', id: input ? `request-${input.request_id}` : `${id}-${index}`, role: message.role, text: '', reasoning: '' };
        assign(item, 'text', text, typed ? `p${id}` : `${ref}.text`); assign(item, 'reasoning', reasoning, `${ref}.reasoning`);
        unit.items.push(item);
      }
    }
    if (Array.isArray(message.content)) for (const [part, content] of message.content.entries()) {
      if (content.type !== 'toolCall' || !content.id || !content.name) continue;
      const item: ToolItem = { type: 'tool', id: content.id, name: content.name, status: 'running', input: '', output: '' };
      assign(item, 'input', escapeDisplay(JSON.stringify(content.arguments)), `${ref}.input.${part}`);
      unit.items.push(item);
    }
    if (message.role === 'toolResult' && message.toolCallId) unit.results.set(message.toolCallId, { text: escapeDisplay(messageText(message)), error: !!message.isError, ref: `${ref}.output` });
  }
  // A result is counted where it is stored; its tool item may sit in an older entry of the page.
  unit.bytes = bytes(unit.items) + [...unit.results.values()].reduce((sum, result) => sum + bytes(clipHead(result.text, INLINE_UNITS)), 0);
  return unit;
}
function pendingUnit(input: PendingInput): Unit {
  const item: MessageItem = { type: 'message', id: `request-${input.request_id}`, role: 'user', text: '', reasoning: '' };
  assign(item, 'text', input.text, `q${input.rowid}`);
  return { key: `${input.anchor}:1:${input.rowid}`, items: [item], results: new Map(), bytes: bytes(item) };
}
/** Walks both sources newest first: an unplaced input sits after the entries up to its anchor. */
async function* unitsBefore(source: HistorySource, before: string | undefined): AsyncGenerator<Unit> {
  const [entry, kind, rowid] = (before ?? `${NEWEST}:1:${NEWEST}`).split(':').map(Number);
  let maxEntry = kind === 1 ? entry : entry - 1;
  let bound = { anchor: entry, rowid: kind === 1 ? rowid : 0 };
  let entries: EntryRecord[] = []; let inputs: PendingInput[] = [];
  let placed = new Map<number, Placed>(); let entriesDone = !source.entries; let inputsDone = false;
  for (;;) {
    if (!entries.length && !entriesDone && maxEntry >= 0) {
      entries = [...await source.entries!({ maxEntryId: maxEntry, order: 'descending' }, CHUNK)];
      entriesDone = entries.length < CHUNK;
      if (entries.length) {
        maxEntry = Number(entries.at(-1)!.id) - 1;
        placed = source.placed(Number(entries.at(-1)!.id), Number(entries[0].id));
      }
    }
    if (!inputs.length && !inputsDone) {
      inputs = source.pending(bound.anchor, bound.rowid, CHUNK);
      inputsDone = inputs.length < CHUNK;
      if (inputs.length) bound = { anchor: inputs.at(-1)!.anchor, rowid: inputs.at(-1)!.rowid };
    }
    if (!entries.length && !inputs.length) return;
    if (inputs.length && (!entries.length || inputs[0].anchor >= Number(entries[0].id))) yield pendingUnit(inputs.shift()!);
    else yield entryUnit(entries.shift()!, placed);
  }
}
function settle(open: Map<string, ToolItem>, results: Map<string, Result>) {
  for (const [id, result] of results) {
    const tool = open.get(id);
    if (!tool) continue;
    tool.status = result.error ? 'error' : 'done';
    assign(tool, 'output', result.text, result.ref);
    open.delete(id);
  }
}
/** Finds results for tool calls whose result entries are newer than this page. */
async function lookAhead(source: HistorySource, after: number, open: Map<string, ToolItem>) {
  let scanned = 0; let next = after + 1;
  while (open.size && scanned < LOOKAHEAD && source.entries) {
    const entries = await source.entries({ minEntryId: next, order: 'ascending' }, CHUNK);
    for (const entry of entries) settle(open, entryUnit(entry, new Map()).results);
    if (entries.length < CHUNK) return;
    scanned += entries.length; next = Number(entries.at(-1)!.id) + 1;
  }
}
/** Keeps a reply inside the frame even when many items are at their inline bound. */
function shrink(items: HistoryItem[]) {
  while (bytes(items) > HARD_BYTES) {
    let largest: { item: HistoryItem; field: Field; value: string } | undefined;
    for (const item of items) for (const field of FIELDS) {
      const value = (item as unknown as Partial<Record<Field, string>>)[field] ?? '';
      if (value.length > (largest?.value.length ?? 0)) largest = { item, field, value };
    }
    if (!largest) throw new Error('History page exceeds bound');
    // Only the inline share shrinks; `history_text` still serves the whole field.
    setInline(largest.item, largest.field, clipHead(largest.value, Math.floor(largest.value.length / 2)));
  }
}
/**
 * One page of the complete conversation, older than `before`, in conversation order.
 * A page ends on a transcript entry boundary, so it can hold a few more items than `limit`.
 */
export async function historyPage(source: HistorySource, before: string | undefined, limit: number) {
  const units: Unit[] = []; let count = 0; let size = 0; let older: string | null = null;
  for await (const unit of unitsBefore(source, before)) {
    if (units.length && (count + unit.items.length > limit || size + unit.bytes > PAGE_BYTES)) { older = units.at(-1)!.key; break; }
    units.push(unit); count += unit.items.length; size += unit.bytes;
  }
  units.reverse();
  // A result belongs to the nearest earlier call with its ID; a result whose call is on an older page is skipped here.
  const open = new Map<string, ToolItem>();
  for (const unit of units) {
    for (const item of unit.items) if (item.type === 'tool') open.set(item.id, item);
    settle(open, unit.results);
  }
  const newest = units.findLast(unit => unit.entry !== undefined)?.entry;
  if (newest !== undefined) await lookAhead(source, newest, open);
  for (const tool of open.values()) tool.status = source.active ? 'running' : 'interrupted';
  const items = units.flatMap(unit => unit.items);
  shrink(items);
  return { items, before: older };
}
async function resolve(source: HistorySource, ref: string): Promise<string | undefined> {
  if (ref.startsWith('q')) return source.journal(Number(ref.slice(1)));
  if (ref.startsWith('p')) return source.typed(Number(ref.slice(1)));
  const [entry, index, field, part] = ref.slice(1).split('.');
  const found = (await source.entries?.({ minEntryId: Number(entry), maxEntryId: Number(entry), order: 'ascending' }, 1))?.[0];
  const message = ((found?.model ?? []) as readonly Message[])[Number(index)];
  if (!message) return undefined;
  if (field === 'text') return message.role === 'user' || message.role === 'assistant' ? messageText(message) : undefined;
  if (field === 'reasoning') return message.role === 'assistant' ? reasoningText(message) : undefined;
  if (field === 'output') return message.role === 'toolResult' ? escapeDisplay(messageText(message)) : undefined;
  const call = Array.isArray(message.content) ? message.content[Number(part)] : undefined;
  return call?.type === 'toolCall' ? escapeDisplay(JSON.stringify(call.arguments)) : undefined;
}
/** One bounded part of a field too large for its history item. */
export async function historyText(source: HistorySource, ref: string, offset: number) {
  const text = await resolve(source, ref);
  if (text === undefined) fail('not_found', 'History text not found.');
  if (offset > text.length || splitsCharacter(text, offset)) fail('invalid_request', 'offset is outside the text or splits a character.');
  const part = clipHead(text.slice(offset), INLINE_UNITS);
  const end = offset + part.length;
  return { text: part, offset, next_offset: end < text.length ? end : null, length: text.length };
}
