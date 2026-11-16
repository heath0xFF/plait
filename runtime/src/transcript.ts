import type { ConversationView } from '@earendil-works/pi-durable';
import type { Snapshot, InputRecord } from './protocol.ts';
import { inputMessage } from './protocol.ts';
import { clipTail } from './text.ts';

type Part = { type?: string; text?: string; id?: string; name?: string; arguments?: unknown };
type Message = { role?: string; content?: string | Part[]; toolCallId?: string; isError?: boolean };
export function messageText(message: Message | undefined): string {
  const content = message?.content;
  return typeof content === 'string' ? content : (content ?? []).filter(part => part.type === 'text').map(part => part.text ?? '').join('');
}
export function boundSnapshot(snapshot: Snapshot) {
  // JSON escaping can use six bytes per character; bound the serialized payload.
  while (Buffer.byteLength(JSON.stringify(snapshot)) > 524288) {
    snapshot.truncated = true;
    if (snapshot.messages.length > 1) snapshot.messages.shift();
    else if (snapshot.tools.length) snapshot.tools.shift();
    else if (snapshot.live_text.length) snapshot.live_text = clipTail(snapshot.live_text, Math.floor(snapshot.live_text.length / 2));
    else if (snapshot.messages[0]?.text.length) snapshot.messages[0].text = clipTail(snapshot.messages[0].text, Math.floor(snapshot.messages[0].text.length / 2));
    else throw new Error('Non-transcript snapshot exceeds bound');
  }
  return snapshot;
}
export function update(snapshot: Snapshot, view: ConversationView, inputs: InputRecord[] = []) {
  let truncated = false;
  const clip = (text: string, limit = 4096) => { if (text.length > limit) truncated = true; return clipTail(text, limit); };
  const messages: Snapshot['messages'] = [];
  const tools = new Map<string, Snapshot['tools'][number]>();
  const pending = inputs.filter(input => input.entry === null);
  const placed = new Map(inputs.filter(input => input.entry !== null).map(input => [input.entry, input]));
  // Each tool records the message it followed so clients can interleave them.
  let last: string | null = null;
  const push = (message: Snapshot['messages'][number]) => { messages.push(message); last = message.id; };
  const appendPending = (before: number) => {
    while (pending.length && pending[0].anchor < before) {
      const input = pending.shift()!;
      push(inputMessage(input));
      if (input.text.length > 4096) truncated = true;
    }
  };
  for (const entry of view.entries) {
    appendPending(Number(entry.id));
    for (const [index, message] of ((entry.model ?? []) as readonly Message[]).entries()) {
      if (message.role === 'user' || message.role === 'assistant') {
        const text = messageText(message);
        if (text) push({ id: message.role === 'user' && placed.has(Number(entry.id)) ? `request-${placed.get(Number(entry.id))!.request_id}` : `${entry.id}-${index}`, role: message.role, text: clip(text) });
      }
      if (Array.isArray(message.content)) for (const part of message.content) {
        if (part.type === 'toolCall' && part.id && part.name) tools.set(part.id, {
          id: part.id, name: part.name, status: 'running', input: clip(JSON.stringify(part.arguments)), output: '', after: last,
        });
      }
      if (message.role === 'toolResult' && message.toolCallId) {
        const tool = tools.get(message.toolCallId) ?? { id: message.toolCallId, name: 'tool', input: '', output: '', status: 'done' as const, after: last };
        tool.output = clip(messageText(message)); tool.status = message.isError ? 'error' : 'done'; tools.set(tool.id, tool);
      }
    }
  }
  appendPending(Infinity);
  const live = view.docs['pi.live'] as unknown as {
    generation?: { message?: Message }; tools?: { callId: string; output?: string }[];
    runFailure?: { reason: string };
  } | undefined;
  for (const item of live?.tools ?? []) {
    const tool = tools.get(item.callId);
    if (tool && item.output) tool.output = clip(item.output);
  }
  snapshot.messages = messages.slice(-100);
  snapshot.tools = [...tools.values()].slice(-50);
  snapshot.live_text = clip(messageText(live?.generation?.message), 16384);
  snapshot.truncated = truncated || messages.length > 100 || tools.size > 50 || view.entries.length >= 100;
  boundSnapshot(snapshot);
  if (live?.runFailure && snapshot.status !== 'stopped' && snapshot.status !== 'interrupted') {
    snapshot.error = { code: 'runtime_error', message: `Agent ended without an answer (${live.runFailure.reason}). Inspect the transcript before retrying.` };
  }
}
