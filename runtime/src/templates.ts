// Pi prompt templates: a Markdown file whose name is a `/` command and whose body,
// with the command's arguments substituted, becomes the message. Pi does not export
// its loader or expander, so these follow the rules Pi documents for both.
import { constants } from 'node:fs';
import { open } from 'node:fs/promises';
import { parseFrontmatter } from '@earendil-works/pi-coding-agent';
import { fail } from './protocol.ts';
import { fileAction } from './safety.ts';
import type { FileIdentity } from './safety.ts';

/** A template's own text, when `/name` places it in a message. */
export const TEMPLATE_LIMIT = 65536;
/** Pi's description for a template without one: its first line, cut to this length. */
const FIRST_LINE = 60;

export type Origin = 'user' | 'project' | 'package' | 'path';
/**
 * A template found for a run. `root` is the resolved folder of the file it names
 * (following a link, as Pi does) and `identity` that folder's device and inode,
 * so a later read through the no-symlink helper cannot land anywhere else.
 */
export type TemplateEntry = {
  name: string; path: string; root: string; file: string; identity: FileIdentity; origin: Origin;
  /** Read only when the person's setup is listed, never for a run. */
  description?: string; argumentHint?: string;
};

/**
 * Splits a command's arguments as a shell would, in Pi's simplified way: a quote opens
 * only at the start of an argument, `\` escapes only that quote inside it, text right
 * after a closing quote joins the same argument, and an unclosed quote is plain text.
 */
export function parseArgs(text: string): string[] {
  const args: string[] = [];
  let at = 0;
  while (at < text.length) {
    if (/\s/.test(text[at])) { at++; continue; }
    let value = '';
    const quote = text[at];
    if (quote === '"' || quote === "'") {
      const quoted = closeQuote(text, at + 1, quote);
      if (quoted) { value = quoted.value; at = quoted.end + 1; } else { value = quote; at++; }
    }
    while (at < text.length && !/\s/.test(text[at])) value += text[at++];
    args.push(value);
  }
  return args;
}

function closeQuote(text: string, start: number, quote: string): { value: string; end: number } | undefined {
  let value = '';
  for (let at = start; at < text.length; at++) {
    if (text[at] === '\\' && text[at + 1] === quote) { value += quote; at++; continue; }
    if (text[at] === quote) return { value, end: at };
    value += text[at];
  }
  return undefined;
}

// `${N:-default}`, `${@:-default}`, `${ARGUMENTS:-default}`, `${@:N}`, `${@:N:L}`, then `$ARGUMENTS`, `$@`, `$N`.
const PLACEHOLDER = /\$\{(\d+|@|ARGUMENTS):-([^}]*)\}|\$\{@:(\d+)(?::(\d+))?\}|\$(ARGUMENTS|@|\d+)/g;

/**
 * Fills a template's placeholders from its arguments in one pass, so an argument that
 * itself reads `$1` stays as written. Positions count from 1; `${@:0}` means all.
 */
export function substitute(body: string, args: string[]): string {
  const all = args.join(' ');
  return body.replace(PLACEHOLDER, (_whole, target?: string, fallback?: string, from?: string, count?: string, plain?: string) => {
    if (target !== undefined) {
      const value = target === '@' || target === 'ARGUMENTS' ? all : args[Number(target) - 1];
      return value || fallback!;
    }
    if (from !== undefined) {
      const start = Math.max(Number(from) - 1, 0);
      return (count === undefined ? args.slice(start) : args.slice(start, start + Number(count))).join(' ');
    }
    return plain === '@' || plain === 'ARGUMENTS' ? all : args[Number(plain) - 1] ?? '';
  });
}

/**
 * `/name args` becomes the template's body with the arguments filled in. Other text,
 * unknown names, and a template whose frontmatter Pi could not read pass unchanged.
 */
export async function expandTemplate(text: string, entries: TemplateEntry[]): Promise<string> {
  const match = /^\/(\S+)(?:\s+([\s\S]*))?$/.exec(text);
  const entry = match && entries.find(candidate => candidate.name === match[1]);
  if (!entry) return text;
  // The helper refuses a link at any point below the folder found at discovery.
  const { text: content } = await fileAction<{ text: string }>(entry.root, 'read', entry.file, undefined, undefined, entry.identity);
  if (content.length > TEMPLATE_LIMIT) fail('permission_denied', `Prompt template ${entry.name} exceeds the size Plait sends in a message.`);
  let body: string;
  try { body = parseFrontmatter(content).body; } catch { return text; }
  return substitute(body, parseArgs(match![2] ?? ''));
}

/**
 * The description and argument hint Pi would show, read for the Settings list only.
 * The file is opened without following a link and must be a bounded regular file.
 */
export async function describeTemplate(path: string): Promise<{ description: string; argumentHint?: string }> {
  const handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  let content: string;
  try {
    const stat = await handle.stat();
    if (!stat.isFile()) throw new Error('Not a regular file.');
    if (stat.size > TEMPLATE_LIMIT) throw new Error(`Larger than ${TEMPLATE_LIMIT} bytes, so it is not offered.`);
    content = await handle.readFile('utf8');
  } finally { await handle.close(); }
  const { frontmatter, body } = parseFrontmatter(content);
  let description = typeof frontmatter.description === 'string' ? frontmatter.description : '';
  if (!description) {
    const first = body.split('\n').find(line => line.trim()) ?? '';
    description = first.length > FIRST_LINE ? first.slice(0, FIRST_LINE) + '...' : first;
  }
  const hint = frontmatter['argument-hint'];
  return typeof hint === 'string' ? { description, argumentHint: hint } : { description };
}
