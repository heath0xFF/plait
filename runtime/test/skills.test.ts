import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';
import { contains, discover, expandSkill, INSTRUCTIONS_LIMIT, skillFile, skillsPrompt } from '../src/skills.ts';
import { fileAction } from '../src/safety.ts';
import { setup } from './support/safety.ts';

async function skill(directory: string, name: string, description: string, body = 'Do the thing.') {
  await mkdir(directory, { recursive: true });
  await writeFile(join(directory, 'SKILL.md'), `---\nname: ${name}\ndescription: ${description}\n---\n\n${body}\n`);
}

/** A checkout and a Pi agent directory, both disposable. */
async function fixture(t: Parameters<typeof setup>[0]) {
  const { root, base } = await setup(t);
  // Pi also reads ~/.agents/skills; a scratch HOME keeps the person's own skills out.
  const home = process.env.HOME;
  process.env.HOME = join(base, 'home');
  await mkdir(process.env.HOME);
  t.after(async () => { process.env.HOME = home; });
  const agent = join(base, 'agent');
  await skill(join(agent, 'skills', 'release-notes'), 'release-notes', 'Write release notes. Use when tagging a release.');
  await skill(join(root, '.pi', 'skills', 'project-only'), 'project-only', 'A skill that ships inside the repository.');
  return { root, agent };
}

test('user skills load; project skills need a trusted project', async t => {
  const { root, agent } = await fixture(t);
  const names = async (trusted: boolean) => (await discover(root, trusted, agent)).skills.map(entry => entry.skill.name).sort();
  assert.deepEqual(await names(false), ['release-notes']);
  assert.deepEqual(await names(true), ['project-only', 'release-notes']);
});

test('the skills block names each skill and where to read it', async t => {
  const { root, agent } = await fixture(t);
  const { skills } = await discover(root, false, agent);
  const prompt = skillsPrompt(skills);
  assert.match(prompt, /<name>release-notes<\/name>/);
  assert.match(prompt, /Use the read tool/);
  assert.ok(prompt.includes(skills[0].skill.filePath));
  assert.equal(skillsPrompt([]), '');
});

test('user instructions follow Pi precedence and the size bound', async t => {
  const { root, agent } = await fixture(t);
  assert.equal((await discover(root, false, agent)).instructions, undefined);
  await writeFile(join(agent, 'CLAUDE.md'), 'claude');
  assert.equal((await discover(root, false, agent)).instructions?.text, 'claude');
  await writeFile(join(agent, 'AGENTS.md'), 'agents');
  assert.equal((await discover(root, false, agent)).instructions?.text, 'agents');
  await writeFile(join(agent, 'AGENTS.override.md'), 'override');
  assert.equal((await discover(root, false, agent)).instructions?.text, 'override');
  await writeFile(join(agent, 'AGENTS.override.md'), 'x'.repeat(INSTRUCTIONS_LIMIT + 1));
  await assert.rejects(discover(root, false, agent), /exceeds the instruction bound/);
});

test('reads map into a skill folder only, and stay bounded by the host helper', async t => {
  const { root, agent } = await fixture(t);
  const { skills } = await discover(root, false, agent);
  const base = skills[0].skill.baseDir;
  await mkdir(join(base, 'references'));
  await writeFile(join(base, 'references', 'format.md'), 'Format notes.');
  assert.deepEqual(skillFile(skills, join(base, 'references', 'format.md')), { root: skills[0].root, path: join('references', 'format.md') });
  assert.equal(skillFile(skills, 'references/format.md'), undefined, 'relative paths are checkout paths');
  assert.equal(skillFile(skills, base + '-evil/SKILL.md'), undefined, 'a sibling with the same prefix is not inside');
  assert.equal(skillFile(skills, join(base, '..', 'other', 'SKILL.md')), undefined);
  assert.equal(skillFile(skills, join(agent, 'auth.json')), undefined);
  const mapped = skillFile(skills, join(base, 'references', 'format.md'))!;
  const page = await fileAction<{ text: string }>(mapped.root, 'read_range', mapped.path, { offset: 0, limit: 8192 });
  assert.equal(page.text, 'Format notes.');
});

test('/skill:name sends the skill in Pi\'s block, then the request', async t => {
  const { root, agent } = await fixture(t);
  const { skills } = await discover(root, false, agent);
  const { skill } = skills[0];
  const expanded = await expandSkill('/skill:release-notes for v1.2', skills);
  assert.equal(expanded, `<skill name="release-notes" location="${skill.filePath}">\nReferences are relative to ${skill.baseDir}.\n\nDo the thing.\n</skill>\n\nfor v1.2`);
  assert.equal(await expandSkill('/skill:release-notes', skills), expanded.split('\n\nfor v1.2')[0]);
  assert.equal(await expandSkill('/skill:unknown go', skills), '/skill:unknown go');
  assert.equal(await expandSkill('plain text', skills), 'plain text');
});

test('folders that hold the home or agent directory are never readable', () => {
  assert.ok(contains('/a/b', '/a/b'));
  assert.ok(contains('/a/b', '/a/b/c'));
  assert.ok(!contains('/a/b', '/a/bc'));
  assert.ok(!contains('/a/b/c', '/a/b'));
});
