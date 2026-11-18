import type { FileIdentity } from '../safety.ts';
import { definitionDigest } from './config.ts';
import type { Entry, McpServer } from './config.ts';

type Settings = { setting(key: string): string | null; setSetting(key: string, value: string | null): void };
const key = (checkout: string, name: string) => `mcp_server:${JSON.stringify([checkout, name])}`;

/** A choice belongs to this checkout directory, never its replacement or another store. */
export function setMcpEnabled(settings: Settings, checkout: string, identity: FileIdentity, entry: Entry, enabled: boolean) {
  settings.setSetting(key(checkout, entry.name), JSON.stringify({ root: identity, enabled, definition: definitionDigest(entry) }));
}

export function mcpPolicy(settings: Settings, checkout: string, identity: FileIdentity, servers: McpServer[], entries: ReadonlyMap<string, Entry>): McpServer[] {
  return servers.map(server => {
    if (!server.can_enable) return server;
    const saved = settings.setting(key(checkout, server.name));
    if (saved === null) return server;
    try {
      const value = JSON.parse(saved) as { root?: Partial<FileIdentity>; enabled?: unknown; definition?: unknown } | null;
      if (typeof value?.root?.dev !== 'string' || typeof value.root.ino !== 'string' || typeof value.enabled !== 'boolean') throw new Error();
      if (value.root.dev !== identity.dev || value.root.ino !== identity.ino) return server;
      const entry = entries.get(server.name);
      if (!entry) return { ...server, enabled: false };
      // Old name-only choices cannot approve repository-controlled definitions.
      if (value.definition !== undefined && value.definition !== definitionDigest(entry)) return {
        ...server, enabled: false, consent: 'changed', disabled_reason: 'The server definition changed. Read its configuration file and turn it on again.',
      };
      if (entry.project && value.enabled && value.definition === undefined) return {
        ...server, enabled: false, consent: 'required', disabled_reason: 'The earlier choice was not bound to this project definition. Read its configuration file before turning it on.',
      };
      return { ...server, enabled: value.enabled, consent: undefined, disabled_reason: value.enabled
        ? server.disabled_reason?.startsWith('Provider authentication') ? server.disabled_reason : null
        : 'Disabled for this project.' };
    } catch {
      return { ...server, enabled: false, disabled_reason: 'The saved project switch could not be read. Set it again.' };
    }
  });
}

/** Unreviewed overrides can only intersect, never replace, the user's tool visibility. */
export function executionEntry(entry: Entry, server: McpServer): Entry {
  return { ...entry, limit: server.consent === 'restricted' ? entry.userConfig : undefined };
}
