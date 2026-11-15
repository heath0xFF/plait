import { createModels } from '@earendil-works/pi-ai/models';
import { fauxProvider, fauxAssistantMessage, fauxText, fauxToolCall } from '@earendil-works/pi-ai/providers/faux';
import { Runtime } from '../src/runtime.ts';
import { defineExtension, hook, ToolTask } from '@earendil-works/pi-durable';
import { appendFile } from 'node:fs/promises';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { ToolState } from '../src/tools.ts';
import { serve } from '../src/stdio.ts';

writeFileSync(join(process.argv[2], 'fixture-processes.json'), JSON.stringify({ node: process.pid, host: process.ppid, store: process.argv[2] }));
const models = createModels();
const faux = fauxProvider({ tokensPerSecond: 120, tokenSize: { min: 1, max: 1 } });
const call = (name: string, args: Record<string, string>, id: string) => fauxAssistantMessage([fauxToolCall(name, args, { id })], { stopReason: 'toolUse' });
const mode = process.env.PLAIT_TEST_SCENARIO;
const responses = mode === 'edit-command' ? [
  call('read', { path: 'notes.txt' }, 'read-1'),
  call('edit', { path: 'notes.txt', old_text: 'before', new_text: 'after' }, 'edit-1'),
  call('command', { command: 'test "$(cat notes.txt)" = after; printf passed' }, 'command-1'),
] : mode === 'crash' ? [call('command', { command: 'printf "READY %s\\n" "$$"; sleep 30 & wait' }, 'command-1')]
  : mode === 'followup' ? [call('edit', { path: 'notes.txt', old_text: 'after', new_text: 'followup' }, 'followup-1')]
  : mode === 'effect' || mode === 'unrecorded' ? [call('edit', { path: 'notes.txt', old_text: 'before', new_text: 'after' }, 'effect-1')]
  : mode === 'dirty' ? [call('edit', { path: 'notes.txt', old_text: 'dirty', new_text: 'DESTROYED' }, 'dirty-1'), call('command', { command: 'printf DESTROYED > notes.txt' }, 'dirty-2')]
  : mode === 'error' ? [fauxAssistantMessage([], { stopReason: 'error', errorMessage: 'SECRET_PROVIDER_TOKEN_DO_NOT_LEAK' })]
  : [];
responses.push(...Array.from({ length: 8 }, () => fauxAssistantMessage([fauxText('Scripted test response with incremental real runtime events. This is not a live model.')] )));
faux.setResponses(responses);
models.setProvider(faux.provider);
class FixtureRuntime extends Runtime {
  override async open(...args: Parameters<Runtime['open']>) {
    await super.open(...args);
    if (mode === 'cleanup') {
      const harness = args[0].harness!;
      const close = harness.close.bind(harness);
      harness.close = async context => { await new Promise(resolve => setTimeout(resolve, 200)); await close(context); };
    }
  }
  override async run(...args: Parameters<Runtime['run']>) {
    if (mode === 'before-submit') {
      writeFileSync(join(process.argv[2], 'accepted.marker'), 'accepted before durable submit');
      process.kill(process.pid, 'SIGKILL');
      await new Promise(() => {});
    }
    return super.run(...args);
  }
  override registry(state: ToolState) {
    const result = super.registry(state);
    if (mode === 'unrecorded') state.record = () => {
      writeFileSync(join(state.snapshot.checkout, 'before-record.marker'), 'effect committed, provenance absent');
      process.kill(process.pid, 'SIGKILL');
    };
    if (mode === 'effect') result.install(defineExtension({ name: 'effect-crash-window', hooks: [hook(ToolTask, {
      async afterTool(call, result) {
        if (call.name !== 'edit' || result.isError) return;
        await appendFile(join(state.snapshot.checkout, 'attempts.txt'), 'effect\n');
        await new Promise<void>(resolve => {
          state.controller.signal.addEventListener('abort', () => resolve(), { once: true });
          if (state.controller.signal.aborted) resolve();
        });
        return result;
      },
    })] }));
    return result;
  }
}
const runtime = new FixtureRuntime(process.argv[2], models, new Set(['faux']));
try { await serve(runtime); }
finally { await runtime.close(); }
