import { randomUUID } from 'node:crypto';
import type { FileHandle } from 'node:fs/promises';
import { defineTool, createRegistry, defineExtension } from '@earendil-works/pi-durable';
import type { ToolExecutionApi } from '@earendil-works/pi-durable';
import type { Context } from '@earendil-works/chord/context';
import { fileAction, safePath, dirtyPaths, ignoredPaths, pathKey, sameImage } from './safety.ts';
import { command } from './command.ts';
import type { FileIdentity, Postimage } from './safety.ts';
import type { BuildCache } from './build-cache.ts';
import { fail } from './protocol.ts';
import { clipHead } from './text.ts';
import type { Snapshot } from './protocol.ts';

export type ToolState = {
  snapshot: Snapshot; baseline: string[]; protected: FileIdentity[]; controller: AbortController; ready: boolean;
  approval?: (allow: boolean) => void;
  lease?: FileHandle;
  rootIdentity: FileIdentity;
  buildCache: BuildCache;
  owned: Map<string, Postimage>;
  record(image: Postimage): void;
  save(): void;
};
async function approve(state: ToolState, api: ToolExecutionApi, name: string, args: Record<string, string>, context: Context) {
  if (!state.ready || state.controller.signal.aborted) fail('permission_denied', 'Run is not active.');
  const details = JSON.stringify(args);
  if (Buffer.byteLength(details) > 16384) fail('permission_denied', 'Tool arguments exceed the approval bound.');
  if (name !== 'command') safePath(args.path, state.baseline);
  const signal = AbortSignal.any([state.controller.signal, ...(context.abortSignal ? [context.abortSignal] : [])]);
  state.snapshot.pending_approval = {
    id: randomUUID(), tool_id: api.callId, kind: name === 'command' ? 'command' : 'edit',
    summary: name === 'command' ? 'Run sandboxed command' : `Change ${args.path}`,
    details, dirty_paths: state.baseline,
    risk: 'Only this exact call is approved. Dirty and ignored paths are protected. Commands write only private scratch and a private per-thread build cache; no external network or checkout/Git writes. Tests may use scratch-local Git repositories and Unix sockets. Read tools and commands can read ignored checkout files and send their contents to the provider.',
  };
  state.snapshot.status = 'awaiting_approval'; state.save();
  const allowed = await new Promise<boolean>(resolve => {
    const cancel = () => resolve(false);
    state.approval = allow => { signal.removeEventListener('abort', cancel); resolve(allow); };
    signal.addEventListener('abort', cancel, { once: true });
    if (signal.aborted) cancel();
  });
  state.approval = undefined;
  state.snapshot.pending_approval = null;
  if (state.snapshot.status === 'awaiting_approval') state.snapshot.status = 'running';
  state.save();
  if (!allowed || signal.aborted) fail('permission_denied', 'Tool permission denied or revoked.');
}

export function registry(state: ToolState) {
  const result = createRegistry();
  const schemas = {
    read: { path: { type: 'string' } },
    edit: { path: { type: 'string' }, old_text: { type: 'string' }, new_text: { type: 'string' } },
    write: { path: { type: 'string' }, content: { type: 'string' } },
    command: { command: { type: 'string' } },
  };
  const tools = Object.entries(schemas).map(([name, properties]) => defineTool({
    name,
    description: name === 'read' ? 'Read a UTF-8 regular file inside the checkout.'
      : name === 'edit' ? 'Replace one unique exact old_text with new_text. Requires permission; dirty paths are protected.'
      : name === 'write' ? 'Create a new file only; never overwrite. Requires permission.'
      : 'Run a command with permission: checkout read-only, writes only to private TMPDIR and this thread\'s CARGO_HOME/CARGO_TARGET_DIR cache, no external network or checkout/Git writes, 15 minute limit. Tests may use scratch-local Git repositories and Unix sockets. Do not start background services. Use offline locked builds.',
    parameters: { type: 'object', properties, required: Object.keys(properties), additionalProperties: false },
    replay: 'unsafe' as const,
    executionMode: 'sequential' as const,
    outputLimits: { maxBytes: 16384, maxLines: 500 },
    async execute(input: unknown, api: ToolExecutionApi, context: Context) {
      const args = input as Record<string, string>;
      if (!state.ready || state.controller.signal.aborted) fail('permission_denied', 'Run is not active.');
      if (Buffer.byteLength(JSON.stringify(args)) > 16384) fail('permission_denied', 'Tool arguments exceed the bound.');
      let text: string;
      if (name === 'read') text = (await fileAction(state.snapshot.checkout, 'read', args.path, undefined, state.lease?.fd, state.rootIdentity)).text;
      else {
        const key = args.path ? pathKey(args.path) : '';
        let expectedFile: string | undefined;
        let expectedImage: Postimage | undefined;
        if (name !== 'command') {
          const currentDirty = await dirtyPaths(state.snapshot.checkout);
          safePath(args.path, await ignoredPaths(state.snapshot.checkout, [args.path]));
          safePath(args.path, currentDirty.filter(path => !state.owned.has(pathKey(path))));
          if (name === 'edit') {
            const current = await fileAction(state.snapshot.checkout, 'read', args.path, undefined, state.lease?.fd, state.rootIdentity);
            expectedFile = current.text; expectedImage = current.postimage;
            if (state.owned.has(key) && !sameImage(state.owned.get(key)!, current.postimage)) fail('permission_denied', 'File changed outside this thread.');
          }
        }
        await approve(state, api, name, args, context);
        if (name === 'command') text = await command(state.snapshot.checkout, args.command, state.baseline,
          AbortSignal.any([state.controller.signal, ...(context.abortSignal ? [context.abortSignal] : [])]), chunk => api.output(chunk), state.lease?.fd, state.rootIdentity, state.buildCache);
        else {
          const result = await fileAction(state.snapshot.checkout, name, args.path,
            name === 'edit' ? { expected: args.old_text, content: args.new_text, expected_file: expectedFile, expected_image: expectedImage, protected: state.protected } : { content: args.content, protected: state.protected }, state.lease?.fd, state.rootIdentity);
          state.record(result.postimage);
          state.owned.set(pathKey(result.postimage.path), result.postimage);
          text = result.text;
        }
      }
      return { output: [{ type: 'text' as const, text: clipHead(text, 16384) }] };
    },
  }));
  result.install(defineExtension({ name: 'plait-local', tools }));
  return result;
}
