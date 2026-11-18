/** Inspect JSON iteratively before serializing or copying untrusted structures. */
export const MCP_JSON_DEPTH = 64;
export function boundJson(value: unknown, limit: number) {
  let bytes = 0;
  const pending = [{ value, depth: 0 }]; const seen = new Set<object>();
  const stringBytes = (text: string) => {
    if (Buffer.byteLength(text) > limit) throw new Error('MCP JSON exceeds its byte limit.');
    return Buffer.byteLength(JSON.stringify(text));
  };
  while (pending.length) {
    const { value: item, depth } = pending.pop()!;
    if (depth > MCP_JSON_DEPTH) throw new Error('MCP JSON exceeds the 64-level depth limit.');
    if (typeof item === 'string') bytes += stringBytes(item);
    else if (item && typeof item === 'object') {
      if (seen.has(item)) throw new Error('MCP JSON contains repeated references.');
      seen.add(item); bytes += 2;
      const entries = Object.entries(item);
      bytes += Math.max(0, entries.length - 1);
      for (const [key, child] of entries) {
        if (!Array.isArray(item)) bytes += stringBytes(key) + 1;
        pending.push({ value: child, depth: depth + 1 });
      }
    } else if (item === null || typeof item === 'boolean' || typeof item === 'number') bytes += String(item).length;
    else throw new Error('MCP JSON contains an invalid value.');
    if (bytes > limit) throw new Error('MCP JSON exceeds its byte limit.');
  }
  return bytes;
}

export function mapJson<T>(value: T, text: (value: string, key: string) => string, keyCheck: (key: string) => void): T {
  boundJson(value, 1024 * 1024);
  const holder: Record<string, unknown> = {};
  const pending = [{ item: value as unknown, parent: holder, key: 'value', schemaKey: '' }];
  while (pending.length) {
    const { item, parent, key, schemaKey } = pending.pop()!;
    let result: unknown = item;
    if (typeof item === 'string') result = text(item, schemaKey);
    else if (item && typeof item === 'object') {
      const copy = (Array.isArray(item) ? [] : {}) as Record<string, unknown>;
      result = copy;
      for (const [name, child] of Object.entries(item)) {
        keyCheck(name); pending.push({ item: child, parent: copy, key: name, schemaKey: name });
      }
    }
    Object.defineProperty(parent, key, { value: result, enumerable: true, writable: true, configurable: true });
  }
  return holder.value as T;
}
