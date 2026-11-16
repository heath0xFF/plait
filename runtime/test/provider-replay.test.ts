import assert from 'node:assert/strict';
import { test } from 'node:test';
import { normalizeContext } from '@earendil-works/pi-ai';
import type { Model } from '@earendil-works/pi-ai';
import { stream } from '@earendil-works/pi-ai/api/openai-completions';
import { replayKey, replayModel, replayServer } from './support/openai-replay.ts';

test('loopback coding fixture exercises the pinned provider decoder without credentials', { timeout: 10000 }, async t => {
  const input = { path: 'src/test.rs', old_text: 'before', new_text: '🙂 after\n' };
  const replay = await replayServer([{ tool: { id: 'edit_fixture', name: 'edit', input } }, { text: 'Fixture complete.' }]);
  t.after(replay.close);
  const model: Model<'openai-completions'> = {
    id: replayModel, name: 'Replay fixture', api: 'openai-completions', provider: 'plait-replay', baseUrl: replay.baseUrl,
    reasoning: false, input: ['text'], contextWindow: 32768, maxTokens: 1024,
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  };
  const context = normalizeContext({ messages: [{ role: 'user', content: 'Run the local fixture.', timestamp: 1 }] });
  const first = await stream(model, context, { apiKey: replayKey }).result();
  assert.equal(first.stopReason, 'toolUse', JSON.stringify(first));
  assert.deepEqual(first.content, [{ type: 'toolCall', id: 'edit_fixture', name: 'edit', arguments: input }]);
  const second = await stream(model, context, { apiKey: replayKey }).result();
  assert.equal(second.stopReason, 'stop', JSON.stringify(second));
  assert(second.content.some(block => block.type === 'text' && block.text === 'Fixture complete.'));
  assert.equal(replay.requests.length, 2);
  assert(replay.requests.every(request => request.key === `Bearer ${replayKey}` && request.body.model === replayModel));
});
