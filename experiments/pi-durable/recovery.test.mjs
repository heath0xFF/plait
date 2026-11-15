import assert from 'node:assert/strict';
import { fork } from 'node:child_process';
import { appendFile, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { createModels } from '@earendil-works/pi-ai/models';
import { fauxProvider, fauxAssistantMessage, fauxText, fauxToolCall } from '@earendil-works/pi-ai/providers/faux';
import { Harness, createRegistry, defineExtension, defineTool } from '@earendil-works/pi-durable';
import { openNodeSqliteStorage } from '@earendil-works/pi-durable/storage/sqlite/node';

const request = { type: 'input', requestId: 'recovery-probe', content: 'Apply the test effect once.' };

async function runRuntime(phase, replay, directory) {
  assert(['crash', 'recover'].includes(phase));
  assert(['safe', 'unsafe'].includes(replay));
  const faux = fauxProvider();
  faux.setResponses(phase === 'crash' ? [
    fauxAssistantMessage([fauxToolCall('effect', {}, { id: 'effect-1' })], { stopReason: 'toolUse' }),
  ] : [fauxAssistantMessage([fauxText('Recovery complete.')])]);
  const models = createModels();
  models.setProvider(faux.provider);
  let admit;
  const admitted = new Promise(resolve => { admit = resolve; });
  let threadId;
  const registry = createRegistry();
  registry.install(defineExtension({
    name: 'recovery-probe',
    tools: [defineTool({
      name: 'effect',
      description: 'Write the test result.',
      parameters: { type: 'object', properties: {}, additionalProperties: false },
      replay,
      async execute() {
        const submissionId = phase === 'crash' ? await admitted : undefined;
        // The overwrite is idempotent; the attempt log measures whether the runtime replays it.
        await writeFile(join(directory, 'result.txt'), 'durable\n');
        await appendFile(join(directory, 'attempts.txt'), 'attempt\n');
        if (phase === 'crash') {
          process.send({ type: 'effect_written', threadId, submissionId });
          // Keep the effect outside a completed tool checkpoint until the parent kills this process.
          await new Promise(() => {});
        }
        return { output: [{ type: 'text', text: 'Result written.' }] };
      },
    })],
  }));
  const storage = await openNodeSqliteStorage(join(directory, 'session.sqlite'));
  const harness = await Harness.open(storage, { models, registry }, context);
  try {
    const root = await harness.root(context, {
      agent: { model: { provider: 'faux', modelId: 'faux-1' }, maxTurns: 3 },
    });
    threadId = root.id;
    if (phase === 'crash') {
      const submission = await root.submit(request, context);
      admit(submission.id);
      await submission.wait(context);
      throw new Error('The parent should kill this process before the tool finishes.');
    }
    harness.resume();
    await harness.waitForIdle(context);
    const duplicate = await root.submit(request, context);
    assert.equal((await duplicate.wait(context)).status, 'done');
    await harness.waitForIdle(context);
    const { items } = await root.entries({}, 100, undefined, context);
    const results = items.filter(entry => entry.kind === 'pi.tool-result');
    assert.equal(results.length, 1);
    return {
      type: 'recovered', threadId, submissionId: duplicate.id,
      userEntries: items.filter(entry => entry.kind === 'pi.user').length,
      interrupted: results[0].data?.diagnostics?.some(diagnostic => diagnostic.code === 'interrupted') ?? false,
      toolError: results[0].model[0].isError,
    };
  } finally {
    await harness.close(context);
  }
}

function startWorker(phase, replay, directory) {
  const child = fork(fileURLToPath(import.meta.url), ['--worker', phase, replay, directory], {
    execArgv: [], stdio: ['ignore', 'ignore', 'pipe', 'ipc'],
  });
  let stderr = '';
  child.stderr.on('data', data => { stderr = (stderr + data).slice(-8192); });
  const exited = new Promise(resolve => {
    child.once('exit', (code, signal) => resolve({ code, signal, stderr }));
    child.once('error', error => resolve({ error }));
  });
  const message = new Promise((resolve, reject) => {
    child.once('message', resolve);
    child.once('error', reject);
    child.once('exit', (code, signal) => reject(new Error(`Worker exited before reporting: ${code}/${signal}\n${stderr}`)));
  });
  return { child, exited, message };
}

if (process.argv[2] === '--worker') {
  // A pending tool promise alone does not keep a Node process alive.
  process.channel.ref();
  const result = await runRuntime(...process.argv.slice(3));
  process.send(result);
  process.disconnect();
} else {
  for (const replay of ['unsafe', 'safe']) {
    test(`SIGKILL recovery follows the ${replay} tool replay policy`, { timeout: 20000 }, async t => {
      const directory = await mkdtemp(join(tmpdir(), 'plait-recovery-'));
      const workers = [];
      t.after(async () => {
        for (const worker of workers) {
          if (worker.child.exitCode === null && worker.child.signalCode === null) worker.child.kill('SIGKILL');
          await worker.exited;
        }
        await rm(directory, { recursive: true, force: true });
      });
      const first = startWorker('crash', replay, directory);
      workers.push(first);
      const before = await first.message;
      assert.equal(before.type, 'effect_written');
      assert.equal(await readFile(join(directory, 'attempts.txt'), 'utf8'), 'attempt\n');
      first.child.kill('SIGKILL');
      assert.equal((await first.exited).signal, 'SIGKILL');

      const second = startWorker('recover', replay, directory);
      workers.push(second);
      const after = await second.message;
      const exit = await second.exited;
      assert.equal(exit.code, 0, JSON.stringify(exit));
      assert.equal(after.type, 'recovered');
      assert.equal(after.threadId, before.threadId);
      assert.equal(after.submissionId, before.submissionId);
      assert.equal(after.userEntries, 1);
      assert.equal(after.interrupted, replay === 'unsafe');
      assert.equal(after.toolError, replay === 'unsafe');
      assert.equal(await readFile(join(directory, 'result.txt'), 'utf8'), 'durable\n');
      assert.equal(await readFile(join(directory, 'attempts.txt'), 'utf8'),
        replay === 'unsafe' ? 'attempt\n' : 'attempt\nattempt\n');
    });
  }
}
