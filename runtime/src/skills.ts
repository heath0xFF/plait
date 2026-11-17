// What the person set up for their Pi agent, offered to Plait threads: enabled
// skills and user-level instructions. Discovery uses Pi's own package
// resolution and skill loader, so a skill is visible here exactly when Pi would
// load it, but never installs or updates a package.
import { readFile, realpath } from 'node:fs/promises';
import { homedir } from 'node:os';
import { basename, isAbsolute, join, relative, sep } from 'node:path';
import { DefaultPackageManager, SettingsManager, formatSkillsForPrompt, getAgentDir, loadSkills, stripFrontmatter } from '@earendil-works/pi-coding-agent';
import type { Skill } from '@earendil-works/pi-coding-agent';
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
export type SkillEntry = { skill: Skill; root: string; identity: FileIdentity; file: string; folder: boolean };
export type DiscoverOptions = {
  agentDir?: string;
  /** More folders a skill folder may neither be nor contain, such as the host store. */
  protect?: string[];
};
/** Credential folders under the home directory that no skill folder may be in or hold. */
const CREDENTIALS = ['.ssh', '.gnupg', '.aws', '.azure', '.kube', '.docker', '.config/gh', 'Library/Keychains'];
export type Resources = {
  skills: SkillEntry[];
  instructions?: { path: string; text: string };
};

/**
 * Enabled skills and the user-level context file. Project skills (`.pi/skills`,
 * `.agents/skills`, and project settings) load only for a trusted project.
 * Discovery problems leave a thread without skills rather than failing it.
 */
export async function discover(checkout: string, trusted: boolean, options: DiscoverOptions = {}): Promise<Resources> {
  const agentDir = options.agentDir ?? getAgentDir();
  return { skills: await skills(checkout, trusted, agentDir, options.protect ?? []).catch(() => []), instructions: await instructions(agentDir) };
}

async function skills(checkout: string, trusted: boolean, agentDir: string, protect: string[]): Promise<SkillEntry[]> {
  const settingsManager = SettingsManager.create(checkout, agentDir, { projectTrusted: trusted });
  const packages = new DefaultPackageManager({ cwd: checkout, agentDir, settingsManager });
  // Missing packages are skipped: a run never installs anything.
  const resolved = await packages.resolve(async () => 'skip');
  const paths = resolved.skills.filter(resource => resource.enabled).map(resource => resource.path);
  const { skills } = loadSkills({ cwd: checkout, agentDir, skillPaths: paths, includeDefaults: false });
  // A skill folder becomes readable to the read tool and the command sandbox, so one
  // that holds the home, Pi agent, or host store directory, or that sits in or holds
  // a credential folder, is skipped.
  const resolve = (path: string) => realpath(path).catch(() => path);
  const home = await resolve(homedir());
  const holds = await Promise.all([home, agentDir, ...protect].map(resolve));
  const credentials = await Promise.all([...CREDENTIALS.map(path => join(home, path)), ...protect].map(resolve));
  const entries: SkillEntry[] = [];
  for (const skill of skills) {
    // The host's file helper walks the root without following symlinks, so it gets the resolved folder.
    let root: string; let identity: FileIdentity;
    try { root = await realpath(skill.baseDir); identity = await rootIdentity(root); } catch { continue; }
    const folder = basename(skill.filePath) === 'SKILL.md';
    const file = relative(skill.baseDir, skill.filePath);
    if (!file || file.split(sep).includes('..')) continue;
    if (folder && (holds.some(path => contains(root, path)) || credentials.some(path => contains(path, root) || contains(root, path)))) continue;
    entries.push({ skill, root, identity, file, folder });
  }
  return entries;
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
