import { createModels } from '@earendil-works/pi-ai/models';
import { fauxProvider, fauxAssistantMessage, fauxText, fauxToolCall } from '@earendil-works/pi-ai/providers/faux';
import type { FauxResponseStep } from '@earendil-works/pi-ai/providers/faux';
import { Runtime } from '../src/runtime.ts';
import { defineExtension, hook, ToolTask } from '@earendil-works/pi-durable';
import { appendFile } from 'node:fs/promises';
import { appendFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { ToolState } from '../src/tools.ts';
import { RunAuthorization } from '../src/authorization.ts';
import { serve } from '../src/stdio.ts';

writeFileSync(join(process.argv[2], 'fixture-processes.json'), JSON.stringify({ node: process.pid, host: process.ppid, store: process.argv[2] }));
const models = createModels();
const faux = fauxProvider({ tokensPerSecond: 120, tokenSize: { min: 1, max: 1 } });
const call = (name: string, args: Record<string, string>, id: string) => fauxAssistantMessage([fauxToolCall(name, args, { id })], { stopReason: 'toolUse' });
const mode = process.env.PLAIT_TEST_SCENARIO;
// Records what the model is told (instructions and tool declarations) for one request.
const recordContext = (context: { messages: unknown }) => {
  appendFileSync(join(process.argv[2], 'model-contexts.jsonl'), JSON.stringify(context.messages) + '\n');
  return fauxAssistantMessage([fauxText('Recorded the scripted model context.')]);
};
const responses: FauxResponseStep[] = mode === 'unicode-read' ? [
  call('read', { path: 'unicode.txt' }, 'unicode-read-1'),
] : mode === 'edit' ? [
  call('read', { path: 'notes.txt' }, 'read-1'),
  call('edit', { path: 'notes.txt', old_text: 'before', new_text: 'after' }, 'edit-1'),
] : mode === 'separate-edits' ? [
  call('edit', { path: 'notes.txt', old_text: 'before', new_text: 'after' }, 'first-edit'),
  fauxAssistantMessage([fauxText('First scripted turn finished.')]),
  call('edit', { path: 'notes.txt', old_text: 'after', new_text: 'followup' }, 'second-edit'),
] : mode === 'permissive-policy' ? [
  call('write', { path: '../outside.txt', content: 'DESTROYED' }, 'outside-write'),
  call('write', { path: '.git/automation.txt', content: 'DESTROYED' }, 'git-write'),
  call('edit', { path: 'notes.txt', old_text: 'dirty', new_text: 'DESTROYED' }, 'dirty-edit'),
  call('command', { command: 'printf DESTROYED > notes.txt' }, 'checkout-command'),
] : mode === 'edit-command' ? [
  call('read', { path: 'notes.txt' }, 'read-1'),
  call('edit', { path: 'notes.txt', old_text: 'before', new_text: 'after' }, 'edit-1'),
  call('command', { command: 'test "$(cat notes.txt)" = after; printf passed' }, 'command-1'),
] : mode === 'crash' ? [call('command', { command: 'printf "READY %s\\n" "$$"; sleep 30 & wait' }, 'command-1')]
  : mode === 'followup' ? [call('edit', { path: 'notes.txt', old_text: 'after', new_text: 'followup' }, 'followup-1')]
  : mode === 'effect' || mode === 'unrecorded' ? [call('edit', { path: 'notes.txt', old_text: 'before', new_text: 'after' }, 'effect-1')]
  : mode === 'dirty' ? [call('edit', { path: 'notes.txt', old_text: 'dirty', new_text: 'DESTROYED' }, 'dirty-1'), call('command', { command: 'printf DESTROYED > notes.txt' }, 'dirty-2')]
  : mode === 'trusted-work' ? [
    call('command', { command: 'printf "made by command\\n" > generated.txt && git add generated.txt && git -c user.name=Fixture -c user.email=fixture@invalid commit -qm "trusted commit" && printf COMMITTED' }, 'commit-1'),
    call('edit', { path: 'notes.txt', old_text: 'before', new_text: 'after' }, 'own-edit'),
    call('command', { command: 'printf "appended\\n" >> notes.txt && printf APPENDED' }, 'append-1'),
    call('edit', { path: 'notes.txt', old_text: 'after', new_text: 'again' }, 'later-edit'),
    call('edit', { path: 'dirty.txt', old_text: 'dirty', new_text: 'DESTROYED' }, 'dirty-edit'),
    call('write', { path: 'dirty.txt', content: 'DESTROYED' }, 'dirty-write'),
  ] : mode === 'trusted-stop' ? [call('command', { command: 'printf started > started.txt; sleep 300 & printf "READY %s %s\\n" "$$" "$!"; wait' }, 'stop-1')]
  : mode === 'trusted-twice' ? [
    call('command', { command: 'printf one > first.txt && printf "FIRST %s" "$HOME"' }, 'first-1'),
    fauxAssistantMessage([fauxText('First scripted turn finished.')]),
    call('command', { command: 'printf two > second.txt && printf SECOND' }, 'second-1'),
  ] : mode === 'record-context' ? [recordContext, recordContext]
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
    await super.run(...args);
    // Stop acknowledgement precedes cleanup; tests need a separate completion barrier.
    writeFileSync(join(process.argv[2], `${args[0].snapshot.id}.settled`), JSON.stringify({
      fixture: 'SCRIPTED_FIXTURE_ONLY', request_id: args[1], status: args[0].snapshot.status,
    }));
  }
  override registry(state: ToolState) {
    if (mode === 'permissive-policy') {
      // This test-only policy proves authorization cannot replace file or sandbox checks.
      state.authorization = new RunAuthorization({ async authorize() {} });
    }
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
          state.authorization.signal.addEventListener('abort', () => resolve(), { once: true });
          if (state.authorization.signal.aborted) resolve();
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
