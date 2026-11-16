import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { ConversationView } from '@earendil-works/pi-durable';
import { inputMessage } from '../src/protocol.ts';
import type { Snapshot } from '../src/protocol.ts';
import { boundSnapshot, update } from '../src/transcript.ts';
import { clipHead, clipTail } from '../src/text.ts';

function snapshot(): Snapshot {
  return { id: 'fixture', checkout: '/fixture', model: { provider: 'faux', model_id: 'faux-1' },
    status: 'idle', messages: [], tools: [], live_text: '', pending_approval: null, error: null, truncated: false };
}

function wellFormed(text: string) {
  assert(Buffer.from(text, 'utf8').toString('utf8') === text, 'Text contains an unpaired UTF-16 surrogate');
}

test('text clipping keeps complete code points at every budget boundary', () => {
  for (const text of ['', 'ASCII', '€é中文', '🚀a🚀b', 'a🚀b🚀', 'e\u0301👩‍💻']) {
    for (let limit = 0; limit <= text.length + 2; limit++) {
      const head = clipHead(text, limit); const tail = clipTail(text, limit);
      wellFormed(head); wellFormed(tail);
      assert(head.length <= limit); assert(tail.length <= limit);
      assert(text.startsWith(head)); assert(text.endsWith(tail));
      if (limit >= text.length) { assert.equal(head, text); assert.equal(tail, text); }
    }
  }
  assert.equal(clipHead('x'.repeat(16383) + '🚀', 16384), 'x'.repeat(16383));
  assert.equal(clipTail('🚀' + 'x'.repeat(4095), 4096), 'x'.repeat(4095));
  for (const limit of [-1, 0.5, NaN, Infinity]) {
    assert.throws(() => clipHead('text', limit), RangeError);
    assert.throws(() => clipTail('text', limit), RangeError);
  }
});

test('accepted input previews cannot split a Unicode code point', () => {
  for (const padding of [4094, 4095, 4096]) {
    const text = '🚀' + 'x'.repeat(padding);
    const message = inputMessage({ request_id: 'unicode', text, entry: null, anchor: 0 });
    wellFormed(message.text);
    assert(message.text.length <= 4096);
    assert.equal(message.text, padding === 4094 ? text : 'x'.repeat(Math.min(padding, 4096)));
  }
});

test('transcript messages, tool previews, and live text keep whole code points', () => {
  const state = snapshot();
  const text = '🚀' + 'x'.repeat(4095);
  // This fixture supplies only the fields read by the transcript adapter.
  const view = { entries: [{ id: 1, model: [
    { role: 'user', content: text },
    { role: 'assistant', content: [{ type: 'text', text }, { type: 'toolCall', id: 'tool-1', name: 'read', arguments: { path: '🚀' + 'x'.repeat(4093) } }] },
    { role: 'toolResult', toolCallId: 'tool-1', content: text },
  ] }], docs: { 'pi.live': { generation: { message: { content: '🚀' + 'x'.repeat(16383) } }, tools: [{ callId: 'tool-1', output: text }] } } } as unknown as ConversationView;
  update(state, view);
  assert(state.truncated);
  for (const message of state.messages) { wellFormed(message.text); assert(message.text.length <= 4096); }
  for (const tool of state.tools) { wellFormed(tool.input); wellFormed(tool.output); assert(tool.output.length <= 4096); }
  wellFormed(state.live_text); assert(state.live_text.length <= 16384);
});

for (const field of ['live_text', 'message'] as const) test(`snapshot byte-budget reduction preserves Unicode in ${field}`, () => {
  const state = snapshot();
  const text = '🚀'.repeat(200000) + 'x';
  if (field === 'live_text') state.live_text = text;
  else state.messages.push({ id: 'message', role: 'assistant', text });
  boundSnapshot(state);
  wellFormed(field === 'live_text' ? state.live_text : state.messages[0].text);
  assert(state.truncated);
  assert(Buffer.byteLength(JSON.stringify(state)) <= 524288);
});
