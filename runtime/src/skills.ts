// What the person set up for their Pi agent, offered to Plait threads: enabled
// skills, prompt templates, and user-level instructions. Discovery uses Pi's own package
// resolution and skill loader, so a skill is visible here exactly when Pi would
// load it, but never installs or updates a package.
import { readFile, realpath } from 'node:fs/promises';
import { homedir } from 'node:os';
import { basename, dirname, isAbsolute, join, relative, sep } from 'node:path';
import { DefaultPackageManager, SettingsManager, formatSkillsForPrompt, getAgentDir, loadSkills, stripFrontmatter } from '@earendil-works/pi-coding-agent';
import type { Skill } from '@earendil-works/pi-coding-agent';
import { describeTemplate, expandTemplate } from './templates.ts';
import type { Origin, TemplateEntry } from './templates.ts';
import { fail } from './protocol.ts';
import { fileAction, rootIdentity } from './safety.ts';
import type { FileIdentity } from './safety.ts';

/** User-level instructions share the project file's bound. */
export const INSTRUCTIONS_LIMIT = 32768;
/** A skill's own instructions, when `/skill:name` places them in a message. */
export const SKILL_TEXT_LIMIT = 65536;
/** Pi's precedence for the agent directory's context file. */
const CONTEXT_FILES = ['AGENTS.override.md', 'AGENTS.md', 'AGENTS.MD', 'CLAUDE.md', 'CLAUDE.MD'];

/**
 * A discovered skill. `root` is its resolved folder and `identity` that folder's
 * device and inode at discovery; reads go through the host's no-symlink helper
 * rooted there. A skill with its own folder (a `SKILL.md`) grants that folder;
 * a standalone Markdown skill grants only its own file, never the folder it sits in.
 */
export type SkillEntry = { skill: Skill; root: string; identity: FileIdentity; file: string; folder: boolean; origin: Origin };
export type DiscoverOptions = {
  agentDir?: string;
  /** More folders a skill folder may neither be nor contain, such as the host store. */
  protect?: string[];
  /** For the Settings list: read template descriptions, and report rather than fail. */
  describe?: boolean;
};
/** Something Pi or Plait could not use, said once with the file it concerns. */
export type Problem = { path?: string; message: string };
/** Credential folders under the home directory that no skill folder may be in or hold. */
const CREDENTIALS = ['.ssh', '.gnupg', '.aws', '.azure', '.kube', '.docker', '.config/gh', 'Library/Keychains'];
export type Resources = {
  skills: SkillEntry[];
  templates: TemplateEntry[];
  instructions?: { path: string; text: string };
  problems: Problem[];
};
type Resolved = Awaited<ReturnType<DefaultPackageManager['resolve']>>;
type Guard = { holds: string[]; credentials: string[] };

/**
 * Enabled skills, prompt templates, and the user-level context file. Project
 * resources (`.pi/skills`, `.agents/skills`, `.pi/prompts`, and project settings)
 * load only for a trusted project. For a run, discovery problems leave the thread
 * without skills or templates rather than failing it; an oversized context file
 * fails it, since its instructions would be silently missing.
 */
export async function discover(checkout: string, trusted: boolean, options: DiscoverOptions = {}): Promise<Resources> {
  const agentDir = options.agentDir ?? getAgentDir();
  const problems: Problem[] = [];
  const found = await resources(checkout, trusted, agentDir, options, problems).catch(() => {
    problems.push({ message: 'Pi\'s settings or packages could not be read, so no skills or templates are offered.' });
    return { skills: [], templates: [] };
  });
  let context: Resources['instructions'];
  try { context = await instructions(agentDir); } catch (error) {
    if (!options.describe) throw error;
    problems.push({ path: agentDir, message: error instanceof Error ? error.message : 'The context file could not be read.' });
  }
  return { ...found, instructions: context, problems };
}

async function resources(checkout: string, trusted: boolean, agentDir: string, options: DiscoverOptions, problems: Problem[]) {
  const settingsManager = SettingsManager.create(checkout, agentDir, { projectTrusted: trusted });
  const packages = new DefaultPackageManager({ cwd: checkout, agentDir, settingsManager });
  // Missing packages are skipped: a run never installs anything.
  const resolved = await packages.resolve(async () => 'skip');
  const guard = await guarded(agentDir, options.protect ?? []);
  return {
    skills: await skills(resolved, checkout, agentDir, guard, problems),
    templates: await templates(resolved, guard, options.describe === true, problems),
  };
}

/**
 * A skill folder becomes readable to the read tool and the command sandbox, so one
 * that holds the home, Pi agent, or host store directory, or that sits in or holds
 * a credential folder, is skipped. A template may not live in a credential folder.
 */
async function guarded(agentDir: string, protect: string[]): Promise<Guard> {
  const resolve = (path: string) => realpath(path).catch(() => path);
  const home = await resolve(homedir());
  return {
    holds: await Promise.all([home, agentDir, ...protect].map(resolve)),
    credentials: await Promise.all([...CREDENTIALS.map(path => join(home, path)), ...protect].map(resolve)),
  };
}

const SKIPPED = 'Not offered: this folder is, holds, or sits in a protected folder (home, Pi agent, credentials, or Plait\'s data).';

async function skills(resolved: Resolved, checkout: string, agentDir: string, guard: Guard, problems: Problem[]): Promise<SkillEntry[]> {
  const enabled = resolved.skills.filter(resource => resource.enabled);
  const loaded = loadSkills({ cwd: checkout, agentDir, skillPaths: enabled.map(resource => resource.path), includeDefaults: false });
  for (const diagnostic of loaded.diagnostics) problems.push({ path: diagnostic.path, message: diagnostic.message });
  const entries: SkillEntry[] = [];
  for (const skill of loaded.skills) {
    // The host's file helper walks the root without following symlinks, so it gets the resolved folder.
    let root: string; let identity: FileIdentity;
    try { root = await realpath(skill.baseDir); identity = await rootIdentity(root); } catch {
      problems.push({ path: skill.filePath, message: 'The skill folder could not be opened.' });
      continue;
    }
    const folder = basename(skill.filePath) === 'SKILL.md';
    const file = relative(skill.baseDir, skill.filePath);
    if (!file || file.split(sep).includes('..')) continue;
    if (folder && (guard.holds.some(path => contains(root, path)) || guard.credentials.some(path => contains(path, root) || contains(root, path)))) {
      problems.push({ path: skill.baseDir, message: SKIPPED });
      continue;
    }
    entries.push({ skill, root, identity, file, folder, origin: originOf(enabled, skill.filePath) });
  }
  return entries;
}

/**
 * Templates in Pi's order; the first of a name wins, as in Pi. A link is followed
 * once, here, to a Markdown file outside credential folders; every later read is
 * rooted at the folder it resolved to.
 */
async function templates(resolved: Resolved, guard: Guard, describe: boolean, problems: Problem[]): Promise<TemplateEntry[]> {
  const enabled = resolved.prompts.filter(resource => resource.enabled);
  const entries: TemplateEntry[] = [];
  const names = new Set<string>();
  for (const { path } of enabled) {
    const name = basename(path).replace(/\.md$/, '');
    let real: string;
    try { real = await realpath(path); } catch {
      problems.push({ path, message: 'The template could not be opened.' });
      continue;
    }
    if (!real.endsWith('.md') || guard.credentials.some(folder => contains(folder, real))) {
      problems.push({ path, message: 'Not offered: it links to a file that is not Markdown or is in a protected folder.' });
      continue;
    }
    if (names.has(name)) {
      problems.push({ path, message: `Another template named ${name} comes first, so this one is not used.` });
      continue;
    }
    let entry: TemplateEntry;
    try {
      const root = dirname(real);
      entry = { name, path, root, file: basename(real), identity: await rootIdentity(root), origin: originOf(enabled, path) };
      if (describe) Object.assign(entry, await describeTemplate(real));
    } catch (error) {
      problems.push({ path, message: error instanceof Error ? `Not offered: ${error.message}` : 'Not offered: the template could not be read.' });
      continue;
    }
    names.add(name);
    entries.push(entry);
  }
  return entries;
}

/** Where a resource came from, by the most specific resolved path that holds it. */
function originOf(enabled: Resolved['skills'], path: string): Origin {
  let best: Resolved['skills'][number] | undefined;
  for (const resource of enabled) {
    if (contains(resource.path, path) && (!best || resource.path.length > best.path.length)) best = resource;
  }
  const metadata = best?.metadata;
  if (!metadata) return 'path';
  if (metadata.origin === 'package') return 'package';
  if (metadata.scope === 'project') return 'project';
  return metadata.source === 'auto' ? 'user' : 'path';
}

/** Folders sandboxed commands may read: only skills with a folder of their own. */
export function readableFolders(entries: SkillEntry[]) {
  return entries.filter(entry => entry.folder).map(entry => entry.root);
}

/** Whether `path` is `folder` or inside it. */
export function contains(folder: string, path: string) {
  return path === folder || path.startsWith(folder.endsWith(sep) ? folder : folder + sep);
}

async function instructions(agentDir: string): Promise<Resources['instructions']> {
  for (const name of CONTEXT_FILES) {
    const path = join(agentDir, name);
    let text: string;
    try { text = await readFile(path, 'utf8'); } catch (error) {
      if ((error as NodeJS.ErrnoException).code === 'ENOENT') continue;
      throw error;
    }
    if (text.length > INSTRUCTIONS_LIMIT) fail('permission_denied', `${name} in the Pi agent directory exceeds the instruction bound.`);
    return { path, text };
  }
  return undefined;
}

/** The skills block of the instructions, in Pi's format. Empty without skills. */
export function skillsPrompt(entries: SkillEntry[]) {
  return formatSkillsForPrompt(entries.map(entry => entry.skill), 'read');
}

/**
 * Where an absolute `read` path lands: inside a skill's own folder, or exactly a
 * standalone skill's file. Gives the resolved root, its identity, and the path
 * below it; `undefined` for anything else.
 */
export function skillFile(entries: SkillEntry[], path: string): { root: string; path: string; identity: FileIdentity } | undefined {
  if (!isAbsolute(path)) return undefined;
  for (const { skill, root, identity, file, folder } of entries) {
    if (!folder) {
      if (path === skill.filePath) return { root, path: file, identity };
      continue;
    }
    const base = skill.baseDir.endsWith(sep) ? skill.baseDir : skill.baseDir + sep;
    if (!path.startsWith(base)) continue;
    const below = relative(skill.baseDir, path);
    // `..` and empty paths are refused again by the helper; this keeps the error specific.
    if (!below || below.split(sep).includes('..')) return undefined;
    return { root, path: below, identity };
  }
  return undefined;
}

/** A typed message as the model receives it: a skill or template command expanded. */
export function expandInput(text: string, found: { skills?: SkillEntry[]; templates?: TemplateEntry[] }) {
  return text.startsWith('/skill:') ? expandSkill(text, found.skills ?? []) : expandTemplate(text, found.templates ?? []);
}

/**
 * `/skill:name args` becomes the skill's instructions followed by the request,
 * in the block Pi itself sends, so a skill behaves the same in both. Other
 * text, and unknown skills, pass through unchanged.
 */
export async function expandSkill(text: string, entries: SkillEntry[]): Promise<string> {
  if (!text.startsWith('/skill:')) return text;
  const space = text.search(/\s/);
  const name = space === -1 ? text.slice(7) : text.slice(7, space);
  const args = space === -1 ? '' : text.slice(space + 1).trim();
  const entry = entries.find(candidate => candidate.skill.name === name);
  if (!entry) return text;
  const { skill } = entry;
  // Through the no-symlink helper, rooted at the folder found at discovery: a skill
  // file swapped for a link to a credential cannot be sent.
  const { text: content } = await fileAction<{ text: string }>(entry.root, 'read', entry.file, undefined, undefined, entry.identity);
  if (content.length > SKILL_TEXT_LIMIT) fail('permission_denied', `Skill ${name} exceeds the size Plait sends in a message.`);
  const block = `<skill name="${skill.name}" location="${skill.filePath}">\nReferences are relative to ${skill.baseDir}.\n\n${stripFrontmatter(content).trim()}\n</skill>`;
  return args ? `${block}\n\n${args}` : block;
}

/** Bounds on `agent_resources`: entries per list and characters per description. */
export const LIST_LIMIT = 500;
export const DESCRIPTION_LIMIT = 1024;
/** The whole reply stays under the client's 1 MiB frame; problems keep a share of it. */
const REPLY_BUDGET = 960 * 1024;
const PROBLEM_SHARE = 96 * 1024;
const clipped = (text: string) => text.length > DESCRIPTION_LIMIT ? text.slice(0, DESCRIPTION_LIMIT - 1) + '…' : text;

/** The leading items that fit both the count limit and what is left of the byte budget. */
function bounded<T>(items: T[], kind: string, budget: { left: number }, reserve: number, problems: Problem[]) {
  const kept: T[] = [];
  for (const item of items) {
    const size = Buffer.byteLength(JSON.stringify(item)) + 1;
    if (kept.length === LIST_LIMIT || size > budget.left - reserve) break;
    budget.left -= size; kept.push(item);
  }
  if (kept.length < items.length) problems.push({ message: `Only the first ${kept.length} of ${items.length} ${kind} are listed.` });
  return kept;
}

/**
 * The `agent_resources` reply: what a run in this checkout would be offered, with
 * where each came from and what could not be used. Never any file's contents.
 */
export function report(checkout: string, trusted: boolean, found: Resources) {
  // What was left out is said first, so it survives when the problems list is cut too.
  const notices: Problem[] = [];
  const budget = { left: REPLY_BUDGET };
  const skills = bounded(found.skills.map(({ skill, origin }) => ({
    name: skill.name, description: clipped(skill.description), origin, path: skill.filePath,
    model_invocation: !skill.disableModelInvocation,
  })), 'skills', budget, PROBLEM_SHARE, notices);
  const templates = bounded(found.templates.map(template => ({
    name: template.name, description: clipped(template.description ?? ''), argument_hint: template.argumentHint ? clipped(template.argumentHint) : null,
    origin: template.origin, path: template.path,
  })), 'templates', budget, PROBLEM_SHARE, notices);
  const shown = [...notices, ...found.problems].map(problem => ({ path: problem.path ?? null, message: clipped(problem.message) }));
  return {
    checkout, trusted,
    instructions: found.instructions ? { path: found.instructions.path, bytes: Buffer.byteLength(found.instructions.text) } : null,
    skills, templates,
    problems: bounded(shown, 'problems', budget, 0, []),
  };
}
