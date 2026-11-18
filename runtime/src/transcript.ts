import type { ConversationView } from '@earendil-works/pi-durable';
import type { Snapshot, SnapshotMessage, SnapshotTool, InputRecord } from './protocol.ts';
import { inputMessage, legacy, readsAsTyped } from './protocol.ts';
import { clipTail, escapeDisplay } from './text.ts';

export type Part = { type?: string; text?: string; thinking?: string; redacted?: boolean; id?: string; name?: string; arguments?: unknown };
export type Message = {
  role?: string; content?: string | Part[]; toolCallId?: string; isError?: boolean; stopReason?: string;
  usage?: { input?: number; output?: number; cacheRead?: number; cacheWrite?: number; reasoning?: number; totalTokens?: number };
};
/** Tool rounds one run may take before it pauses with `turn_limit`. */
export const MAX_TURNS = 300;
const LEGACY_BOUND = 524288;
const FULL_BOUND = 786432;
export function messageText(message: Message | undefined): string {
  const content = message?.content;
  return typeof content === 'string' ? content : (content ?? []).filter(part => part.type === 'text').map(part => part.text ?? '').join('');
}
/** Readable reasoning only: redacted blocks carry an opaque provider payload. */
export function reasoningText(message: Message | undefined): string {
  const content = message?.content;
  return typeof content === 'string' ? '' : (content ?? []).filter(part => part.type === 'thinking' && !part.redacted && part.thinking)
    .map(part => part.thinking).join('\n\n');
}
const bytes = (value: unknown) => Buffer.byteLength(JSON.stringify(value));
function dropOldestText(snapshot: Snapshot) {
  // Reasoning-only messages before the oldest text message leave with it.
  snapshot.messages.splice(0, snapshot.messages.findIndex(message => message.text) + 1);
}
/** A tool keeps its version 3 position only while that reasoning message is retained. */
function pruneOrder(snapshot: Snapshot) {
  const ids = new Set(snapshot.messages.map(message => message.id));
  for (const tool of snapshot.tools) if (tool.after3 !== undefined && !ids.has(tool.after3)) delete tool.after3;
}
/** Latest 100 messages with text or images, plus the latest 50 reasoning-only messages. */
function retain(messages: SnapshotMessage[]) {
  const text = messages.filter(message => message.text || message.images?.length); const reasoning = messages.filter(message => !message.text && !message.images?.length);
  const keep = new Set([...text.slice(-100), ...reasoning.slice(-50)]);
  return { messages: messages.filter(message => keep.has(message)), clipped: text.length > 100 };
}
export function pushMessage(snapshot: Snapshot, message: SnapshotMessage) {
  const result = retain([...snapshot.messages, message]);
  snapshot.messages = result.messages;
  if (result.clipped) snapshot.truncated = true;
  pruneOrder(snapshot);
}
function boundExtras(snapshot: Snapshot) {
  // Version 3 additions share what is left of the frame; the oldest reasoning goes first.
  while (bytes(snapshot) > FULL_BOUND) {
    const message = snapshot.messages.find(item => item.reasoning);
    const queued = snapshot.queued?.find(item => item.text.length);
    if (message?.text) delete message.reasoning;
    else if (message) snapshot.messages.splice(snapshot.messages.indexOf(message), 1);
    else if (snapshot.live_reasoning) snapshot.live_reasoning = clipTail(snapshot.live_reasoning, Math.floor(snapshot.live_reasoning.length / 2));
    else if (queued) queued.text = clipTail(queued.text, Math.floor(queued.text.length / 2));
    else throw new Error('Non-transcript snapshot exceeds bound');
  }
}
export function boundSnapshot(snapshot: Snapshot) {
  // The version 2 shape is a subset, so a small snapshot needs no second measurement.
  if (bytes(snapshot) <= LEGACY_BOUND - 1024) return snapshot;
  // JSON escaping can use six bytes per character; bound the serialized payload.
  // Older clients get exactly the content the version 2 bound would have kept.
  while (bytes(legacy(snapshot)) > LEGACY_BOUND) {
    snapshot.truncated = true;
    const text = snapshot.messages.filter(message => message.text);
    if (text.length > 1) dropOldestText(snapshot);
    else if (snapshot.tools.length) snapshot.tools.shift();
    else if (snapshot.live_text.length) snapshot.live_text = clipTail(snapshot.live_text, Math.floor(snapshot.live_text.length / 2));
    else if (text[0]?.text.length) text[0].text = clipTail(text[0].text, Math.floor(text[0].text.length / 2));
    else throw new Error('Non-transcript snapshot exceeds bound');
  }
  boundExtras(snapshot);
  pruneOrder(snapshot);
  return snapshot;
}
function usageOf(message: Message, contextWindow: number | null): Snapshot['usage'] {
  const usage = message.usage!;
  const input = usage.input ?? 0, output = usage.output ?? 0, cacheRead = usage.cacheRead ?? 0, cacheWrite = usage.cacheWrite ?? 0;
  const total = usage.totalTokens ?? input + output + cacheRead + cacheWrite;
  // The latest response's total is what the next request carries as context.
  return { input, output, cache_read: cacheRead, cache_write: cacheWrite, reasoning: usage.reasoning ?? null, total, context_used: total, context_window: contextWindow };
}
export function update(snapshot: Snapshot, view: ConversationView, inputs: InputRecord[] = [], contextWindow: number | null = null) {
  let truncated = false;
  const clip = (text: string, limit = 4096) => { if (text.length > limit) truncated = true; return clipTail(text, limit); };
  const messages: SnapshotMessage[] = [];
  const tools = new Map<string, SnapshotTool>();
  // Queued inputs are not in the conversation yet. A started one may carry a later anchor than its neighbors.
  const pending = inputs.filter(input => input.entry === null && !input.queued).sort((a, b) => a.anchor - b.anchor);
  const placed = new Map(inputs.filter(input => input.entry !== null).map(input => [input.entry, input]));
  // Each tool records the message it followed so clients can interleave them.
  let last: string | null = null;
  // Version 3 also orders tools after reasoning-only messages, which older clients never see.
  let last3: string | null = null;
  let answered: Message | undefined;
  const push = (message: SnapshotMessage) => { messages.push(message); last3 = message.id; if (message.text) last = message.id; };
  const order = () => ({ after: last, ...(last3 !== last && last3 !== null ? { after3: last3 } : {}) });
  const appendPending = (before: number) => {
    while (pending.length && pending[0].anchor < before) {
      const input = pending.shift()!;
      push(inputMessage(input));
      if (input.text.length > 4096) truncated = true;
    }
  };
  for (const entry of view.entries) {
    appendPending(Number(entry.id));
    if (entry.kind === 'pi.compaction') continue;
    for (const [index, message] of ((entry.model ?? []) as readonly Message[]).entries()) {
      if (message.role === 'user' || message.role === 'assistant') {
        // A placed `/skill:` input reads as typed; the model received it expanded.
        const input = message.role === 'user' ? placed.get(Number(entry.id)) : undefined;
        const typed = input?.text; const images = input?.images;
        const text = readsAsTyped(typed) ? typed : messageText(message);
        const reasoning = message.role === 'assistant' ? clipTail(reasoningText(message), 4096) : '';
        if (text || reasoning || images?.length) push({
          id: message.role === 'user' && placed.has(Number(entry.id)) ? `request-${placed.get(Number(entry.id))!.request_id}` : `${entry.id}-${index}`,
          role: message.role, text: clip(images?.length ? escapeDisplay(text) : text), ...(images?.length ? { images } : {}), ...(reasoning ? { reasoning } : {}),
        });
        if (message.role === 'assistant' && message.usage && message.stopReason !== 'error' && message.stopReason !== 'aborted') answered = message;
      }
      if (Array.isArray(message.content)) for (const part of message.content) {
        if (part.type === 'toolCall' && part.id && part.name) tools.set(part.id, {
          id: part.id, name: part.name, status: 'running', input: clip(escapeDisplay(JSON.stringify(part.arguments))), output: '', ...order(),
        });
      }
      if (message.role === 'toolResult' && message.toolCallId) {
        const tool = tools.get(message.toolCallId) ?? { id: message.toolCallId, name: 'tool', input: '', output: '', status: 'done' as const, ...order() };
        tool.output = clip(escapeDisplay(messageText(message))); tool.status = message.isError ? 'error' : 'done'; tools.set(tool.id, tool);
      }
    }
  }
  appendPending(Infinity);
  const live = view.docs['pi.live'] as unknown as {
    generation?: { attempt?: number; message?: Message; retry?: { at: number } }; tools?: { callId: string; output?: string }[];
    runFailure?: { reason: string }; compactionFailure?: unknown;
  } | undefined;
  for (const item of live?.tools ?? []) {
    const tool = tools.get(item.callId);
    if (tool && item.output) tool.output = clip(escapeDisplay(item.output));
  }
  const retained = retain(messages);
  snapshot.messages = retained.messages;
  snapshot.tools = [...tools.values()].slice(-50);
  snapshot.live_text = clip(messageText(live?.generation?.message), 16384);
  snapshot.live_reasoning = clipTail(reasoningText(live?.generation?.message), 16384);
  // A backoff before the next model request; the provider's error text stays private.
  snapshot.retry = live?.generation?.retry ? { attempt: live.generation.attempt ?? 1, at: live.generation.retry.at } : null;
  if (answered) snapshot.usage = usageOf(answered, contextWindow);
  snapshot.truncated = truncated || retained.clipped || tools.size > 50 || view.entries.length >= 100;
  pruneOrder(snapshot);
  boundSnapshot(snapshot);
  if (live?.runFailure && snapshot.status !== 'stopped' && snapshot.status !== 'interrupted') {
    snapshot.error = live.runFailure.reason === 'model_error' && live.compactionFailure
      ? { code: 'context_compaction', message: 'Conversation summarization failed. Full history is saved. Send a message to retry or choose another model.' }
      : live.runFailure.reason === 'turn_limit'
      ? { code: 'turn_limit', message: `The run paused after ${MAX_TURNS} tool rounds, its limit. Send a message to continue.` }
      : { code: 'runtime_error', message: `Agent ended without an answer (${live.runFailure.reason}). Inspect the transcript before retrying.` };
  }
}
