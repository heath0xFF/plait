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
import type { RunAuthorization } from './authorization.ts';

export type ToolState = {
  snapshot: Snapshot; baseline: string[]; protected: FileIdentity[];
  authorization: RunAuthorization;
  lease?: FileHandle;
  rootIdentity: FileIdentity;
  buildCache: BuildCache;
  owned: Map<string, Postimage>;
  record(image: Postimage): void;
};
export function registry(state: ToolState) {
  const result = createRegistry();
  const schemas = {
    read: { path: { type: 'string' } },
    edit: { path: { type: 'string' }, old_text: { type: 'string' }, new_text: { type: 'string' } },
    write: { path: { type: 'string' }, content: { type: 'string' } },
    command: { command: { type: 'string' } },
  };
  const tools = (Object.keys(schemas) as (keyof typeof schemas)[]).map(name => defineTool({
    name,
    description: name === 'read' ? 'Read a UTF-8 regular file inside the checkout.'
      : name === 'edit' ? 'Replace one unique exact old_text with new_text. Requires permission; dirty paths are protected.'
      : name === 'write' ? 'Create a new file only; never overwrite. Requires permission.'
      : 'Run a command with permission: checkout read-only, writes only to private TMPDIR and this thread\'s CARGO_HOME/CARGO_TARGET_DIR cache, no external network or checkout/Git writes, 15 minute limit. Tests may use scratch-local Git repositories and Unix sockets. Do not start background services. Use offline locked builds.',
    parameters: { type: 'object', properties: schemas[name], required: Object.keys(schemas[name]), additionalProperties: false },
    replay: 'unsafe' as const,
    executionMode: 'sequential' as const,
    outputLimits: { maxBytes: 16384, maxLines: 500 },
    async execute(input: unknown, api: ToolExecutionApi, context: Context) {
      const args = Object.freeze({ ...input as Record<string, string> });
      const signal = AbortSignal.any([state.authorization.signal, ...(context.abortSignal ? [context.abortSignal] : [])]);
      state.authorization.check(signal);
      if (Buffer.byteLength(JSON.stringify(args)) > 16384) fail('permission_denied', 'Tool arguments exceed the bound.');
      let text: string;
      if (name === 'read') text = (await fileAction(state.snapshot.checkout, 'read', args.path, undefined, state.lease?.fd, state.rootIdentity)).text;
      else {
        const key = args.path ? pathKey(args.path) : '';
        let expectedFile: string | undefined;
        let expectedImage: Postimage | undefined;
        if (name !== 'command') {
          // File protections apply regardless of how the run obtains permission.
          safePath(args.path, state.baseline);
          const currentDirty = await dirtyPaths(state.snapshot.checkout);
          safePath(args.path, await ignoredPaths(state.snapshot.checkout, [args.path]));
          safePath(args.path, currentDirty.filter(path => !state.owned.has(pathKey(path))));
          if (name === 'edit') {
            const current = await fileAction(state.snapshot.checkout, 'read', args.path, undefined, state.lease?.fd, state.rootIdentity);
            expectedFile = current.text; expectedImage = current.postimage;
            if (state.owned.has(key) && !sameImage(state.owned.get(key)!, current.postimage)) fail('permission_denied', 'File changed outside this thread.');
          }
        }
        await state.authorization.authorize(Object.freeze({
          toolId: api.callId, name, arguments: args, dirtyPaths: Object.freeze([...state.baseline]),
        }), signal);
        // Stop can arrive between authorization and this continuation.
        state.authorization.check(signal);
        if (name === 'command') text = await command(state.snapshot.checkout, args.command, state.baseline,
          signal, chunk => api.output(chunk), state.lease?.fd, state.rootIdentity, state.buildCache);
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
