// What the person set up for their Pi agent, offered to Plait threads: enabled
// skills and user-level instructions. Discovery uses Pi's own package
// resolution and skill loader, so a skill is visible here exactly when Pi would
// load it, but never installs or updates a package.
import { readFile, realpath } from 'node:fs/promises';
import { homedir } from 'node:os';
import { isAbsolute, join, relative, sep } from 'node:path';
import { DefaultPackageManager, SettingsManager, formatSkillsForPrompt, getAgentDir, loadSkills, stripFrontmatter } from '@earendil-works/pi-coding-agent';
import type { Skill } from '@earendil-works/pi-coding-agent';
import { fail } from './protocol.ts';

/** User-level instructions share the project file's bound. */
export const INSTRUCTIONS_LIMIT = 32768;
/** A skill's own instructions, when `/skill:name` places them in a message. */
export const SKILL_TEXT_LIMIT = 65536;
/** Pi's precedence for the agent directory's context file. */
const CONTEXT_FILES = ['AGENTS.override.md', 'AGENTS.md', 'AGENTS.MD', 'CLAUDE.md', 'CLAUDE.MD'];

/** A discovered skill, with the resolved folder its files are read from. */
export type SkillEntry = { skill: Skill; root: string };
export type Resources = {
  skills: SkillEntry[];
  instructions?: { path: string; text: string };
};

/**
 * Enabled skills and the user-level context file. Project skills (`.pi/skills`,
 * `.agents/skills`, and project settings) load only for a trusted project.
 * Discovery problems leave a thread without skills rather than failing it.
 */
export async function discover(checkout: string, trusted: boolean, agentDir = getAgentDir()): Promise<Resources> {
  return { skills: await skills(checkout, trusted, agentDir).catch(() => []), instructions: await instructions(agentDir) };
}

async function skills(checkout: string, trusted: boolean, agentDir: string): Promise<SkillEntry[]> {
  const settingsManager = SettingsManager.create(checkout, agentDir, { projectTrusted: trusted });
  const packages = new DefaultPackageManager({ cwd: checkout, agentDir, settingsManager });
  // Missing packages are skipped: a run never installs anything.
  const resolved = await packages.resolve(async () => 'skip');
  const paths = resolved.skills.filter(resource => resource.enabled).map(resource => resource.path);
  const { skills } = loadSkills({ cwd: checkout, agentDir, skillPaths: paths, includeDefaults: false });
  // A skill's folder becomes readable to the read tool and the command sandbox, so
  // one that holds the home or Pi agent directory (and their credentials) is skipped.
  const guarded = await Promise.all([homedir(), agentDir].map(path => realpath(path).catch(() => path)));
  const entries: SkillEntry[] = [];
  for (const skill of skills) {
    // The host's file helper walks the root without following symlinks, so it gets the resolved folder.
    let root: string;
    try { root = await realpath(skill.baseDir); } catch { continue; }
    if (guarded.some(path => contains(root, path))) continue;
    entries.push({ skill, root });
  }
  return entries;
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
 * Where an absolute `read` path lands inside a skill's folder: the resolved
 * root and the path below it. `undefined` when it is in no skill's folder.
 */
export function skillFile(entries: SkillEntry[], path: string): { root: string; path: string } | undefined {
  if (!isAbsolute(path)) return undefined;
  for (const { skill, root } of entries) {
    const base = skill.baseDir.endsWith(sep) ? skill.baseDir : skill.baseDir + sep;
    if (!path.startsWith(base)) continue;
    const below = relative(skill.baseDir, path);
    // `..` and empty paths are refused again by the helper; this keeps the error specific.
    if (!below || below.split(sep).includes('..')) return undefined;
    return { root, path: below };
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
  const content = await readFile(skill.filePath, 'utf8');
  if (content.length > SKILL_TEXT_LIMIT) fail('permission_denied', `Skill ${name} exceeds the size Plait sends in a message.`);
  const block = `<skill name="${skill.name}" location="${skill.filePath}">\nReferences are relative to ${skill.baseDir}.\n\n${stripFrontmatter(content).trim()}\n</skill>`;
  return args ? `${block}\n\n${args}` : block;
}
