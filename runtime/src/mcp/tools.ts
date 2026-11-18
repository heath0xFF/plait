import { createHash } from 'node:crypto';
import { defineExtension, defineTool } from '@earendil-works/pi-durable';
import { createToolSearchTool } from '@earendil-works/pi-durable/tool-search';
import type { Registry, ToolRegistration } from '@earendil-works/pi-durable';
import { McpClient, McpAuthRequiredError, McpTimeoutError, createMcpToolName } from '@earendil-works/pi-mcp';
import type { Tool } from '@earendil-works/pi-mcp';
import type { ToolState } from '../tools.ts';
import { definitionDigest, MCP_NAME } from './config.ts';
import type { Config, Entry, McpServer } from './config.ts';
import { Secrets } from './secrets.ts';
import { MCP_CALL_TIMEOUT_MS, MCP_START_TIMEOUT_MS, transport } from './transport.ts';
import { fail } from '../protocol.ts';
import { outputTail } from '../tool-output.ts';
import { clipHead, clipTail } from '../text.ts';
import { dropDrifted, refreshChanged } from '../command/trusted.ts';
import { checkRoot } from '../safety.ts';

export type Observation = { state: 'connected' | 'stopped' | 'failed'; tool_count?: number; problem?: string; needs_sign_in?: boolean };
type Connection = Awaited<ReturnType<typeof transport>>;
export const fingerprint = definitionDigest;
export const nameKey = (thread: string, name: string) => `mcp_tool:${JSON.stringify([thread, name])}`;
const decoded = (value: string) => { try { return decodeURIComponent(value); } catch { return value; } };

function exposure(config: Config, name: string) {
  const overrides = config.toolExposure ?? {};
  if (overrides[name]) return overrides[name];
  for (const [pattern, value] of Object.entries(overrides)) {
    if (pattern.includes('*') && new RegExp('^' + pattern.split('*').map(part => part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('.*') + '$').test(name)) return value;
  }
  return config.exposure ?? 'codemode';
}

export function toolExposure(entry: Entry, name: string) {
  return entry.limit && exposure(entry.limit, name) === 'hidden' ? 'hidden' : exposure(entry.config, name);
}
const visible = (config: Config) => config.exposure !== 'hidden' || Object.values(config.toolExposure ?? {}).some(value => value !== 'hidden');

export function eligible(entry: Entry, server: McpServer) {
  return server.enabled && server.can_enable && server.needs_sign_in !== true && !entry.config.auth
    && visible(entry.config) && (!entry.limit || visible(entry.limit));
}

/** One owner per run; a failed connection is never restarted or a call retried. */
export class McpRun {
  private readonly state: ToolState;
  private readonly entries: Entry[];
  private readonly observe: (entry: Entry, observation: Observation) => void;
  private readonly remember: (name: string, label: string) => void;
  private readonly secrets = new Secrets();
  private readonly clients = new Map<string, { client: McpClient; pipe: Connection; entry: Entry; count?: number; failed: boolean; problem?: string }>();
  private readonly tools: ToolRegistration[] = [];
  private readonly names = new Set<string>();
  private readonly remembered = new Set<string>();
  private readonly problems: string[] = [];
  private discovering?: Promise<void>;
  private closed = false;

  constructor(state: ToolState, entries: Entry[], observe: McpRun['observe'], remember: McpRun['remember']) {
    this.state = state; this.entries = entries; this.observe = observe; this.remember = remember;
    // URL userinfo is a credential; paths, queries, arguments, and inherited env are not.
    for (const { config } of entries) {
      if ('url' in config) {
        const url = new URL(config.url);
        this.secrets.add(url.username); this.secrets.add(url.password);
        this.secrets.add(decoded(url.username));
        this.secrets.add(decoded(url.password));
      }
    }
  }

  install(registry: Registry) {
    if (!this.entries.length) return;
    const search = createToolSearchTool();
    registry.install(defineExtension({ name: 'plait-mcp-search', tools: [defineTool({ ...search,
      replay: 'unsafe', executionMode: 'sequential', outputLimits: { maxBytes: 24576, maxLines: 1000 },
      execute: async (args, api, context) => {
        this.state.authorization.check(context.abortSignal);
        await (this.discovering ??= this.discover(registry));
        this.state.authorization.check(context.abortSignal);
        // Pi freezes api.agent() at the start of this tool task, before we connected.
        const found = await search.execute({ ...args, limit: Math.min(args.limit ?? 8, 20) }, {
          ...api, agent: async ctx => ({ ...await api.agent(ctx), deferred: this.tools }),
        }, context);
        const problems = this.problems.length ? '\nMCP problems:\n' + this.problems.join('\n').slice(0, 8000) : '';
        return { ...found, output: [...found.output ?? [], { type: 'text' as const, text: problems || 'MCP search complete.' }] };
      },
    })] }));
  }

  private async discover(registry: Registry) {
    // Parallel connection attempts do not make one silent server delay every healthy one.
    for (let index = 0; index < this.entries.length; index += 4) {
      await Promise.all(this.entries.slice(index, index + 4).map(entry => this.connect(entry)));
    }
    if (!this.closed) registry.install(defineExtension({ name: 'plait-mcp-tools', tools: this.tools }));
  }

  private problem(entry: Entry, pipe: Connection | undefined, message: string, signIn = false) {
    const tail = clipTail(this.secrets.text(pipe?.stderr() ?? '').split('\n').slice(-8).join('\n'), 2000);
    const problem = `${message}${tail ? '\n' + tail : ''}`;
    this.problems.push(`${entry.name}: ${problem}`);
    this.observe(entry, { state: 'failed', problem, ...(signIn ? { needs_sign_in: true } : {}) });
    return problem;
  }

  private async connect(entry: Entry) {
    let pipe: Connection | undefined;
    const client = new McpClient({ name: 'plait', version: '1', capabilities: {}, requestTimeoutMs: MCP_START_TIMEOUT_MS });
    const deadline = new AbortController();
    const timer = setTimeout(() => deadline.abort(), MCP_START_TIMEOUT_MS);
    const signal = AbortSignal.any([this.state.authorization.signal, deadline.signal]);
    const stop = () => { void client.close().catch(() => {}); };
    signal.addEventListener('abort', stop, { once: true });
    try {
      this.state.authorization.check(signal);
      pipe = await transport(entry, this.state, this.secrets, signal);
      this.state.authorization.check(signal);
      // No handlers for sampling or elicitation: pinned McpClient replies -32601.
      await client.connect(pipe.connection);
      // Limit the total list, including pagination, independently of a server's page size.
      const listed: Tool[] = []; let cursor: string | undefined; let metadataBytes = 0;
      const cursors = new Set<string>();
      do {
        const page = await client.request<{ tools: Tool[]; nextCursor?: string }>('tools/list', cursor ? { cursor } : {}, { signal });
        metadataBytes += Buffer.byteLength(JSON.stringify(page));
        if (!Array.isArray(page.tools) || listed.length + page.tools.length > 500 || metadataBytes > 1024 * 1024) throw new Error();
        listed.push(...page.tools);
        cursor = page.nextCursor || undefined;
        if (cursor !== undefined && typeof cursor !== 'string') throw new Error();
        if (cursor && cursors.has(cursor)) throw new Error();
        if (cursor) cursors.add(cursor);
      } while (cursor);
      const active = { client, pipe, entry, count: listed.length, failed: false, problem: undefined as string | undefined };
      this.clients.set(entry.name, active);
      const used = new Set<string>(); const offered: ToolRegistration[] = [];
      const refused: string[] = [];
      for (const [index, tool] of listed.entries()) {
        try {
          if (!tool || typeof tool.name !== 'string' || !MCP_NAME.test(tool.name) || used.has(tool.name)
            || this.secrets.contains(tool.name) || [tool.title, tool.annotations?.title].some(title => title !== undefined && (typeof title !== 'string' || this.secrets.contains(title)))
            || !tool.inputSchema || typeof tool.inputSchema !== 'object' || Array.isArray(tool.inputSchema)
            || tool.inputSchema.type !== 'object' || Buffer.byteLength(JSON.stringify(tool)) > 65536) throw new Error();
          used.add(tool.name);
          const parameters = this.secrets.schema(tool.inputSchema);
          if (toolExposure(entry, tool.name) !== 'hidden') offered.push(this.tool(entry, tool, client, parameters));
        } catch {
          // Never echo a rejected name, title, or schema: it may be a credential or control text.
          const problem = `Tool ${index + 1} was refused: invalid name or schema, duplicate name, or credential in an identifier or constraint.`;
          refused.push(problem); this.problems.push(`${entry.name}: ${problem}`);
        }
      }
      active.problem = refused.length ? refused.join('\n').slice(0, 2000) : undefined;
      this.tools.push(...offered);
      client.onClose(() => {
        if (!this.closed && !this.state.authorization.signal.aborted && !active.failed) {
          active.failed = true; this.problem(entry, pipe, 'MCP server disconnected.');
        }
      });
      client.onError(() => { if (!active.failed) { active.failed = true; this.problem(entry, pipe, 'MCP server protocol failed.'); stop(); } });
      this.observe(entry, { state: 'connected', tool_count: listed.length, ...(active.problem ? { problem: active.problem } : {}) });
    } catch (error) {
      this.problem(entry, pipe, error instanceof McpAuthRequiredError ? 'Sign in with Pi; browser sign-in support is planned.'
        : deadline.signal.aborted ? 'MCP server startup timed out after 10 seconds.' : 'MCP server could not start or list tools.', error instanceof McpAuthRequiredError);
      this.clients.delete(entry.name);
      await client.close().catch(() => {}); await this.cleanup(entry, pipe);
    } finally { clearTimeout(timer); signal.removeEventListener('abort', stop); }
  }

  private tool(entry: Entry, tool: Tool, client: McpClient, parameters: Tool['inputSchema']): ToolRegistration {
    // Always hash: names stay stable regardless of list order, collisions, or later searches.
    const safeTool = tool.name;
    const suffix = createHash('sha256').update(`${entry.name}\0${tool.name}`).digest('hex').slice(0, 16);
    const name = createMcpToolName(entry.name, safeTool).slice(0, 47) + '_' + suffix;
    if (this.names.has(name)) throw new Error('MCP tool namespace collision.');
    this.names.add(name);
    const label = `mcp:${entry.name}/${safeTool}`;
    return defineTool({ name, description: `${label}: ${clipHead(this.secrets.text(tool.description ?? ''), 2048)}`,
      parameters, deferred: true, replay: 'unsafe', executionMode: 'sequential',
      outputLimits: { maxBytes: 24576, maxLines: 20000 },
      execute: async (input, api, context) => {
        if (!this.remembered.has(name)) { this.remember(name, label); this.remembered.add(name); }
        const state = this.state;
        const signal = AbortSignal.any([state.authorization.signal, ...(context.abortSignal ? [context.abortSignal] : [])]);
        state.authorization.check(signal);
        if (!input || typeof input !== 'object' || Array.isArray(input) || Buffer.byteLength(JSON.stringify(input)) > 16384) fail('permission_denied', 'MCP tool arguments exceed the bound.');
        const args = structuredClone(input) as Record<string, unknown>;
        const call = Object.freeze({ toolId: api.callId, name: 'command' as const,
          arguments: Object.freeze(this.secrets.json(args)), dirtyPaths: [],
          mcp: Object.freeze({ server: entry.name, tool: safeTool }),
          ...(state.commandPolicy === 'trusted' ? { commandPolicy: 'trusted' as const } : {}),
        });
        if (state.commandPolicy !== 'trusted') {
          if (!state.mcpApproval) fail('permission_denied', 'Interactive MCP approval is unavailable.');
          await state.mcpApproval.authorize(call, signal);
        } else await state.authorization.authorize(call, signal);
        await checkRoot(state.snapshot.checkout, state.rootIdentity);
        state.authorization.check(signal);
        const log = state.outputs.begin(state.snapshot.id, api.callId);
        let text: string; let isError = false;
        try {
          if (state.commandPolicy === 'trusted') await dropDrifted(state);
          const result = await client.callTool(tool.name, args, {
            signal, timeoutMs: Math.min((entry.config.timeout ?? MCP_CALL_TIMEOUT_MS / 1000) * 1000, 900000),
          });
          // Images and embedded binaries await the message-images milestone. Keep text and structured data.
          const content = result.content.filter(part => part.type === 'text');
          text = this.secrets.text(JSON.stringify({ content,
            ...(content.length < result.content.length ? { notice: 'Non-text MCP content is not supported in this stage.' } : {}),
            ...(result.structuredContent !== undefined ? { structuredContent: result.structuredContent } : {}) }));
          isError = result.isError === true;
        } catch (error) {
          isError = true;
          const active = this.clients.get(entry.name);
          if (active) active.failed = true;
          const signIn = error instanceof McpAuthRequiredError;
          text = this.problem(entry, active?.pipe, signIn ? 'Sign in with Pi; browser sign-in support is planned.'
            : error instanceof McpTimeoutError ? `MCP call timed out after ${Math.min(entry.config.timeout ?? 60, 900)} seconds; it was not retried.`
            : 'MCP call failed or was stopped; it was not retried.', signIn);
          await client.close().catch(() => {});
        } finally {
          if (state.commandPolicy === 'trusted') await refreshChanged(state);
        }
        log.append(text);
        return { output: [{ type: 'text', text: outputTail(text) + log.finish() }], isError };
      },
    });
  }

  private async cleanup(entry: Entry, pipe: Connection | undefined) {
    await pipe?.cleanup().catch(() => { this.problem(entry, undefined, 'MCP scratch cleanup failed.'); });
  }

  async close() {
    this.closed = true;
    await this.discovering?.catch(() => {});
    await Promise.all([...this.clients.values()].map(async ({ client, pipe, entry, count, failed, problem }) => {
      await client.close().catch(() => {});
      if (!failed) this.observe(entry, { state: 'stopped', tool_count: count, ...(problem ? { problem } : {}) });
      await this.cleanup(entry, pipe);
    }));
  }
}
