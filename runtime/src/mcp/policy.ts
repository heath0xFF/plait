import type { FileIdentity } from '../safety.ts';
import type { McpServer } from './config.ts';

type Settings = { setting(key: string): string | null; setSetting(key: string, value: string | null): void };
const key = (checkout: string, name: string) => `mcp_server:${JSON.stringify([checkout, name])}`;

/** A choice belongs to this checkout directory, never its replacement or another store. */
export function setMcpEnabled(settings: Settings, checkout: string, identity: FileIdentity, name: string, enabled: boolean) {
  settings.setSetting(key(checkout, name), JSON.stringify({ root: identity, enabled }));
}

export function mcpPolicy(settings: Settings, checkout: string, identity: FileIdentity, servers: McpServer[]): McpServer[] {
  return servers.map(server => {
    if (!server.can_enable) return server;
    const saved = settings.setting(key(checkout, server.name));
    if (saved === null) return server;
    try {
      const value = JSON.parse(saved) as { root?: Partial<FileIdentity>; enabled?: unknown } | null;
      if (typeof value?.root?.dev !== 'string' || typeof value.root.ino !== 'string' || typeof value.enabled !== 'boolean') throw new Error();
      if (value.root.dev !== identity.dev || value.root.ino !== identity.ino) return server;
      return { ...server, enabled: value.enabled, disabled_reason: value.enabled
        ? server.disabled_reason?.startsWith('Provider authentication') ? server.disabled_reason : null
        : 'Disabled for this project.' };
    } catch {
      return { ...server, enabled: false, disabled_reason: 'The saved project switch could not be read. Set it again.' };
    }
  });
}
