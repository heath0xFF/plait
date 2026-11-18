/** Only this run keeps resolved values. Scrub before storage, tool registration, or presentation. */
export class Secrets {
  private values = new Set<string>();
  add(value: string | undefined) {
    // Short values often occur in ordinary prose and identifiers.
    if (!value || [...value].length < 8) return;
    this.values.add(value);
    this.values.add(JSON.stringify(value).slice(1, -1));
    this.values.add(encodeURIComponent(value));
    // An Authorization header can be echoed without its scheme.
    if (/^(Bearer|Basic) /i.test(value)) this.add(value.slice(value.indexOf(' ') + 1));
  }
  contains(value: string) { return [...this.values].some(secret => value.includes(secret)); }
  text(value: string): string {
    for (const secret of [...this.values].sort((a, b) => b.length - a.length)) value = value.split(secret).join('[redacted]');
    return value;
  }
  json<T>(value: T): T {
    const clean = (item: unknown): unknown => typeof item === 'string' ? this.text(item)
      : Array.isArray(item) ? item.map(clean)
      : item && typeof item === 'object' ? Object.fromEntries(Object.entries(item).map(([key, child]) => [key, clean(child)])) : item;
    return clean(value) as T;
  }

  /** Preserve schema identifiers and constraints; only descriptions are free text. */
  schema<T>(value: T): T {
    const clean = (item: unknown, description = false): unknown => {
      if (typeof item === 'string') {
        if (description) return this.text(item);
        if (this.contains(item)) throw new Error('MCP schema contains a credential outside free text.');
        return item;
      }
      if (Array.isArray(item)) return item.map(child => clean(child));
      if (item && typeof item === 'object') return Object.fromEntries(Object.entries(item).map(([key, child]) => {
        if (this.contains(key)) throw new Error('MCP schema contains a credential in a key.');
        return [key, clean(child, key === 'description')];
      }));
      return item;
    };
    return clean(value) as T;
  }
}

/** Pi 1.5.2 templates: $VAR, ${VAR}, $$ and $!, without evaluating a shell. */
export function expand(value: string, env: NodeJS.ProcessEnv): string {
  return value.replace(/\$\$|\$!|\$\{([A-Za-z_][A-Za-z0-9_]*)\}|\$([A-Za-z_][A-Za-z0-9_]*)/g, (match, brace: string, bare: string) => {
    if (match === '$$') return '$';
    if (match === '$!') return '!';
    const found = env[brace ?? bare];
    if (!found) throw new Error('An MCP configuration variable is unavailable.');
    return found;
  });
}
