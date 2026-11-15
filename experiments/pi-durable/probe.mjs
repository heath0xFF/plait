import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { ModelRuntime } from '@earendil-works/pi-coding-agent';
import { cleanupSessionResources } from '@earendil-works/pi-ai';
import { createModels } from '@earendil-works/pi-ai/models';
import { fauxProvider, fauxAssistantMessage, fauxText, fauxToolCall } from '@earendil-works/pi-ai/providers/faux';
import { Harness, createRegistry, defineExtension, hook, ToolTask } from '@earendil-works/pi-durable';
import { NodeExecutionEnv } from '@earendil-works/pi-durable/env/node';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';
import { createReadTool, createEditTool } from '@earendil-works/pi-durable/tools';

const directory = await mkdtemp(join(tmpdir(), 'plait-durable-probe-'));
await writeFile(join(directory, 'notes.txt'), 'hello world\n');
const live = process.argv.includes('--live');
const models = live ? await ModelRuntime.create() : createModels();
const provider = live ? process.env.PI_PROVIDER : 'faux';
const modelId = live ? process.env.PI_MODEL : 'faux-1';
assert(provider && modelId, 'Set PI_PROVIDER and PI_MODEL for a live probe.');
if (!live) {
  const faux = fauxProvider({ tokensPerSecond: 80, tokenSize: { min: 1, max: 1 } });
  faux.setResponses([
    fauxAssistantMessage([fauxToolCall('read', { path: 'notes.txt' }, { id: 'r' })], { stopReason: 'toolUse' }),
    fauxAssistantMessage([fauxToolCall('edit', { path: 'notes.txt', edits: [{ oldText: 'world', newText: 'durable' }] }, { id: 'e' })], { stopReason: 'toolUse' }),
    fauxAssistantMessage([fauxText('The file now greets durable. This answer is deliberately streamed in small parts.')]),
  ]);
  models.setProvider(faux.provider);
}
assert(models.getModel(provider, modelId), 'Configured model is missing');
const registry = createRegistry();
registry.install(defineExtension({
  name: 'probe',
  tools: [createReadTool(), createEditTool()],
  hooks: [hook(ToolTask, {
    beforeTool: call => call.arguments?.path === 'notes.txt' ? undefined : { block: 'This probe only permits notes.txt.' },
  })],
}));
const options = {
  models, registry,
  env: () => new NodeExecutionEnv({ cwd: directory }),
  settings: { retry: { maxRetries: 0 }, stream: { timeoutMs: 60000 } },
};
const open = () => openNodeSqliteStorage(join(directory, 'session.sqlite')).then(storage => Harness.open(storage, options, context));
let harness = await open();
const deadline = setTimeout(() => { void harness.close(context); }, 120000);
try {
  const root = await harness.root(context, { agent: {
    model: { provider, modelId }, cwd: directory, maxTurns: 4, thinkingLevel: 'low',
    instructions: 'This is a Plait integration probe. Only inspect and edit notes.txt in the working directory. Do not access any other path. Keep the answer short.',
  } });
  const watch = await root.watch(context);
  let updates = 0;
  let partials = 0;
  let firstUpdate;
  const changed = new Promise(resolve => { firstUpdate = resolve; });
  watch.start(call => {
    if (call.kind !== 'value') return;
    updates++;
    if (call.value.docs['pi.live']?.generation?.message) {
      partials++;
      firstUpdate();
    }
  });
  const request = { type: 'input', requestId: 'plait-probe-1', content: 'Read notes.txt, then use edit to replace world with durable. Reply with a short confirmation.' };
  const submission = await root.submit(request, context);
  const finished = submission.wait(context);
  await Promise.race([changed, finished]);
  // A detached observer must not own or cancel the agent run.
  await watch.stop();
  const settled = await finished;
  if (!live) assert(partials > 0, 'No incremental model state was observed');
  assert.equal(settled.status, 'done', 'The real model did not finish');
  await harness.waitForIdle(context);
  assert.equal(await readFile(join(directory, 'notes.txt'), 'utf8'), 'hello durable\n');
  const reattached = await root.watch(context);
  const entries = reattached.value.entries;
  assert(entries.some(entry => entry.kind === 'pi.tool-result'));
  assert(entries.some(entry => entry.kind === 'pi.assistant'));
  await reattached.stop();
  await harness.close(context);
  harness = await open();
  const reopened = await harness.root(context);
  assert.equal(reopened.id, root.id);
  const duplicate = await reopened.submit(request, context);
  assert.equal(duplicate.id, submission.id, 'Retry submitted a second input');
  assert.equal((await duplicate.wait(context)).status, 'done');
  const persisted = await reopened.entries({}, 100, undefined, context);
  assert.equal(persisted.items.length, entries.length);
  console.log(JSON.stringify({ result: 'PASS', live, provider, modelId, updates, partials, entries: entries.length,
    checks: [live ? 'real model auth' : 'scripted model stream', 'read and edit', 'observer detach and reattach', 'SQLite reopen', 'stable thread identity', 'request deduplication'], directory }));
} finally {
  clearTimeout(deadline);
  await harness.close(context);
  await models.settleCredentialWrites?.();
  // Provider sockets outlive a Harness and must close when the host exits.
  cleanupSessionResources();
}
