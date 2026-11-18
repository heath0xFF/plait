// Metadata only: never resolve config values, load a transport, or refresh a token.
// Merge and validation follow Pi 1.5.2's extensions/mcp/config and core/mcp-servers.
import { constants } from 'node:fs';
import { createHash } from 'node:crypto';
import { open } from 'node:fs/promises';
import { join } from 'node:path';
import { isMcpServerOverride, mcpNamespace, validateMcpServerConfig } from '@earendil-works/pi-mcp';
import type { McpServerConfig, McpServerConfigResult } from '@earendil-works/pi-mcp';
import type { Problem } from '../skills.ts';

export type McpServer = {
  name: string;
  transport: 'stdio' | 'http' | null;
  source: string;
  override_source: string | null;
  program: string | null;
  argument_count?: number;
  host: string | null;
  enabled: boolean;
  can_enable: boolean;
  /** Null until a connection would establish whether an automatic OAuth server requires sign-in. */
  needs_sign_in: boolean | null;
  signed_in: boolean;
  disabled_reason: string | null;
  problem: string | null;
  state?: 'connected' | 'stopped' | 'failed';
  tool_count?: number;
  /** Unresolved definition digest, used to reject a stale settings decision. */
  definition?: string;
  consent?: 'required' | 'changed' | 'restricted';
};
export type Config = McpServerConfig & { exposure?: string; toolExposure?: Record<string, string>; auth?: unknown };
export type Entry = { name: string; source: string; override_source: string | null; config: Config; transport: 'stdio' | 'http';
  project?: 'definition' | 'override'; projectConfig?: unknown; userConfig?: Config; limit?: Config };
/** Include exposure ordering: overlapping wildcard rules are evaluated in configured order. */
export const definitionDigest = (entry: Entry) => createHash('sha256').update(JSON.stringify({
  transport: entry.transport, config: entry.config, project: entry.project, projectConfig: entry.projectConfig, user: entry.userConfig,
})).digest('hex');
type State = { entries: Map<string, Entry>; namespaces: Map<string, string>; invalid: Map<string, McpServer>; problems: Problem[] };
const FILE_LIMIT = 1024 * 1024;
// Darwin sys/fcntl.h: all components; O_NOFOLLOW_ANY cannot be combined with O_NOFOLLOW.
const NO_PROJECT_SYMLINKS = process.platform === 'darwin' ? 0x20000000 : constants.O_NOFOLLOW;
export const MCP_NAME = /^[A-Za-z0-9_.:/-]{1,128}$/;
export const unsafeProgram = (value: string) => /[\p{Cc}\p{Cf}\p{Zl}\p{Zp}]/u.test(value);
export const unsafeLoaderEnvironment = (config: Config) => 'command' in config && Object.keys(config.env ?? {}).some(key =>
  /^(?:DYLD_|LD_|LDR_)/.test(key) || ['LIBPATH', 'SHLIB_PATH', 'GCONV_PATH', 'NODE_OPTIONS', 'NODE_PATH', 'PYTHONPATH', 'PYTHONHOME', 'RUBYOPT', 'RUBYLIB', 'PERL5OPT', 'PERL5LIB', 'BASH_ENV', 'ENV'].includes(key));
const OVERRIDES = new Set(['enabled', 'exposure', 'toolExposure']);
const record = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);

/** A bounded read; parse errors never include snippets of credentials from the file. */
async function json(path: string, problems: Problem[], noFollow = false): Promise<Record<string, unknown> | undefined> {
  try {
    const file = await open(path, constants.O_RDONLY | constants.O_NONBLOCK | (noFollow ? NO_PROJECT_SYMLINKS : 0));
    try {
      if (!(await file.stat()).isFile()) throw new Error();
      const data = Buffer.alloc(FILE_LIMIT + 1);
      let size = 0;
      while (size < data.length) {
        const { bytesRead } = await file.read(data, size, data.length - size, null);
        if (!bytesRead) break;
        size += bytesRead;
      }
      if (size > FILE_LIMIT) {
        problems.push({ path, message: 'MCP configuration exceeds the 1 MiB read limit.' });
        return undefined;
      }
      const parsed: unknown = JSON.parse(data.subarray(0, size).toString('utf8').replace(/^\uFEFF/, ''));
      if (!record(parsed)) throw new Error();
      return parsed;
    } finally { await file.close(); }
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') problems.push({ path, message: 'MCP configuration could not be read as a JSON object.' });
    return undefined;
  }
}

function piFields(result: Exclude<McpServerConfigResult, { kind: 'invalid' }>): string | undefined {
  const config = result.config as McpServerConfig & Record<string, unknown>;
  if ('command' in config && unsafeProgram(String(config.command))) return 'The configured executable contains control or format characters.';
  const exposure = (value: unknown) => ['codemode', 'codemode-deferred', 'deferred', 'direct', 'hidden'].includes(value as string);
  if (config.exposure !== undefined && !exposure(config.exposure)) return 'Invalid exposure; use codemode, deferred, direct, or hidden.';
  if (config.toolExposure !== undefined && (!record(config.toolExposure) || !Object.values(config.toolExposure).every(exposure))) {
    return 'toolExposure must map tool names to valid exposures.';
  }
  if (result.kind === 'http' && config.auth !== undefined) {
    if (!record(config.auth) || typeof config.auth.provider !== 'string' || !config.auth.provider) return 'auth.provider must be a provider name.';
    const url = new URL(result.config.url);
    if (url.protocol !== 'https:' && !['localhost', '127.0.0.1', '[::1]'].includes(url.hostname)) return 'Provider authentication requires HTTPS or a loopback host.';
  }
  return undefined;
}

function validate(name: string, raw: unknown) {
  return validateMcpServerConfig(name, raw, {
    checkMetadataDocumentCallback: callback => {
      if (callback === undefined) return undefined;
      const url = new URL(callback);
      return url.hostname !== '[::1]' && url.pathname === '/callback' ? undefined
        : 'oauth.clientRegistration cimd requires a localhost or 127.0.0.1 callback at /callback.';
    },
  });
}

function invalid(state: State, name: string, source: string, message: string) {
  // An invalid project replacement leaves the valid user entry in place, as in Pi.
  state.problems.push({ path: source, message: `MCP server ${name}: ${message}` });
  if (state.entries.has(name)) return;
  state.invalid.set(name, { name, source, override_source: null, transport: null, program: null, host: null,
    enabled: false, can_enable: false, needs_sign_in: null, signed_in: false, disabled_reason: message, problem: message });
}

async function merge(path: string, project: boolean, state: State) {
  const parsed = await json(path, state.problems, project);
  if (!parsed) return;
  if (parsed.mcpServers !== undefined && !record(parsed.mcpServers)) {
    state.problems.push({ path, message: 'MCP configuration needs an mcpServers object.' }); return;
  }
  if (parsed.autoEnableCodemode !== undefined && typeof parsed.autoEnableCodemode !== 'boolean') {
    state.problems.push({ path, message: 'autoEnableCodemode must be a boolean.' });
  }
  for (const [name, value] of Object.entries(parsed.mcpServers ?? {})) {
    if (!MCP_NAME.test(name)) {
      state.problems.push({ path, message: 'An MCP server name was refused: use 1–128 ASCII letters, digits, _, -, ., :, or /.' });
      continue;
    }
    const base = state.entries.get(name);
    const override = project && record(value) && isMcpServerOverride(value);
    if (override && (!base || Object.keys(value).some(key => !OVERRIDES.has(key)))) {
      invalid(state, name, path, !base ? 'An override needs a user server with this name.' : 'An override can set only enabled, exposure, and toolExposure.');
      continue;
    }
    const result = validate(name, override ? { ...base!.config, ...value } : value);
    // The upstream validator prints only field names and the server name, never values.
    const problem = result.kind === 'invalid' ? result.error : piFields(result);
    if (result.kind === 'invalid' || problem) { invalid(state, name, path, problem!); continue; }
    const namespaceOwner = state.namespaces.get(mcpNamespace(name));
    if (!override && namespaceOwner !== undefined && namespaceOwner !== name) {
      invalid(state, name, path, 'Another server uses the same tool namespace.'); continue;
    }
    if (project && !override && result.kind === 'http' && result.config.auth) {
      invalid(state, name, path, 'Provider authentication is allowed only in the user mcp.json.'); continue;
    }
    state.namespaces.set(mcpNamespace(name), name);
    state.entries.set(name, { name, config: result.config, transport: result.kind,
      source: override ? base!.source : path, override_source: override ? path : null,
      ...(project ? { project: override ? 'override' as const : 'definition' as const, projectConfig: value } : {}),
      ...(override ? { userConfig: base!.config } : {}),
    });
    state.invalid.delete(name);
  }
}

/** Read Pi's saved sign-in metadata without locks, migration, writes, or token refresh. */
function signIn(entry: Entry, auth: Record<string, unknown>) {
  if (entry.transport !== 'http') return { needs_sign_in: false, signed_in: false };
  const config = entry.config as McpServerConfig & { url: string; headers?: Record<string, string>; oauth?: unknown; auth?: unknown };
  if (config.auth || Object.keys(config.headers ?? {}).some(key => key.toLowerCase() === 'authorization')) {
    return { needs_sign_in: false, signed_in: false };
  }
  const url = String(new URL(config.url));
  const key = `${mcpNamespace(entry.name)}|${url}`;
  const saved = (Object.hasOwn(auth, key) ? auth[key] : undefined) ?? (Object.hasOwn(auth, url) ? auth[url] : undefined);
  const state = record(saved) && saved.serverUrl === url ? saved : undefined;
  const tokens = state && record(state.tokens) ? state.tokens : undefined;
  const signed_in = !!tokens && (typeof tokens.access_token === 'string' && !!tokens.access_token
    || typeof tokens.refresh_token === 'string' && !!tokens.refresh_token);
  // Automatic OAuth is only attempted after a 401. No connection means no claim about a public server.
  return { needs_sign_in: config.oauth !== undefined || state !== undefined ? true : null, signed_in };
}

/** Disabled entries remain visible. Pi's built-in MCP switch is an absolute gate. */
export async function listMcp(checkout: string, trusted: boolean, agentDir: string, piEnabled: boolean) {
  const { servers, problems } = await loadMcp(checkout, trusted, agentDir, piEnabled, true);
  return { servers, problems };
}

/** Private execution definitions stay in memory; only the metadata is reported. */
export async function loadMcp(checkout: string, trusted: boolean, agentDir: string, piEnabled: boolean, describe = false) {
  const state: State = { entries: new Map(), namespaces: new Map(), invalid: new Map(), problems: [] };
  await merge(join(agentDir, 'mcp.json'), false, state);
  if (trusted) await merge(join(checkout, '.pi', 'mcp.json'), true, state);
  const auth = describe && [...state.entries.values()].some(entry => entry.transport === 'http')
    ? await json(join(agentDir, 'mcp-auth.json'), state.problems) ?? {} : {};
  const servers = [...state.entries.values()].map((entry): McpServer => {
    const loaderProblem = !trusted && unsafeLoaderEnvironment(entry.config) ? 'MCP loader environment is not allowed in a sandboxed project.' : null;
    if (loaderProblem) state.problems.push({ path: entry.source, message: loaderProblem });
    const enabled = !loaderProblem && piEnabled && trusted && entry.project !== 'definition'
      && entry.config.enabled !== false && entry.userConfig?.enabled !== false;
    const config = entry.config;
    return {
      name: entry.name, transport: entry.transport, source: entry.source, override_source: entry.override_source,
      program: 'command' in config ? config.command : null,
      ...('command' in config ? { argument_count: config.args?.length ?? 0 } : {}),
      host: entry.transport === 'http' && 'url' in config ? new URL(config.url).host : null,
      enabled, can_enable: piEnabled && !loaderProblem, ...signIn(entry, auth), problem: loaderProblem,
      definition: definitionDigest(entry),
      ...(entry.project ? { consent: entry.project === 'definition' || entry.userConfig?.enabled === false ? 'required' as const : 'restricted' as const } : {}),
      disabled_reason: config.auth ? 'Provider authentication is planned; use a configured header key for now.'
        : !piEnabled ? 'MCP is disabled in Pi settings.' : !trusted ? 'Off until enabled for this sandboxed project.'
        : entry.project === 'definition' ? 'Defined by this project. Read its configuration file before turning it on.'
        : entry.project === 'override' && entry.userConfig?.enabled === false ? 'This project cannot enable a server disabled in your user configuration. Read its configuration before turning it on.'
        : entry.project === 'override' && enabled ? 'Project overrides are limited by your user configuration until you explicitly turn this server on here.'
        : !enabled ? 'Disabled in Pi configuration.' : null,
    };
  });
  return { servers: [...servers, ...state.invalid.values()], problems: state.problems, entries: state.entries };
}
