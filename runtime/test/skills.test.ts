import assert from 'node:assert/strict';
import { mkdir, rm, symlink, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';
import { contains, discover, expandSkill, INSTRUCTIONS_LIMIT, readableFolders, skillFile, skillsPrompt } from '../src/skills.ts';
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
  return { root, agent, home: process.env.HOME! };
}

test('user skills load; project skills need a trusted project', async t => {
  const { root, agent } = await fixture(t);
  const names = async (trusted: boolean) => (await discover(root, trusted, { agentDir: agent })).skills.map(entry => entry.skill.name).sort();
  assert.deepEqual(await names(false), ['release-notes']);
  assert.deepEqual(await names(true), ['project-only', 'release-notes']);
});

test('the skills block names each skill and where to read it', async t => {
  const { root, agent } = await fixture(t);
  const { skills } = await discover(root, false, { agentDir: agent });
  const prompt = skillsPrompt(skills);
  assert.match(prompt, /<name>release-notes<\/name>/);
  assert.match(prompt, /Use the read tool/);
  assert.ok(prompt.includes(skills[0].skill.filePath));
  assert.equal(skillsPrompt([]), '');
});

test('user instructions follow Pi precedence and the size bound', async t => {
  const { root, agent } = await fixture(t);
  assert.equal((await discover(root, false, { agentDir: agent })).instructions, undefined);
  await writeFile(join(agent, 'CLAUDE.md'), 'claude');
  assert.equal((await discover(root, false, { agentDir: agent })).instructions?.text, 'claude');
  await writeFile(join(agent, 'AGENTS.md'), 'agents');
  assert.equal((await discover(root, false, { agentDir: agent })).instructions?.text, 'agents');
  await writeFile(join(agent, 'AGENTS.override.md'), 'override');
  assert.equal((await discover(root, false, { agentDir: agent })).instructions?.text, 'override');
  await writeFile(join(agent, 'AGENTS.override.md'), 'x'.repeat(INSTRUCTIONS_LIMIT + 1));
  await assert.rejects(discover(root, false, { agentDir: agent }), /exceeds the instruction bound/);
});

test('reads map into a skill folder only, and stay bounded by the host helper', async t => {
  const { root, agent } = await fixture(t);
  const { skills } = await discover(root, false, { agentDir: agent });
  const base = skills[0].skill.baseDir;
  await mkdir(join(base, 'references'));
  await writeFile(join(base, 'references', 'format.md'), 'Format notes.');
  assert.deepEqual(skillFile(skills, join(base, 'references', 'format.md')), { root: skills[0].root, path: join('references', 'format.md'), identity: skills[0].identity });
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
  const { skills } = await discover(root, false, { agentDir: agent });
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

test('a skill file swapped for a link after discovery is never sent', async t => {
  const { root, agent } = await fixture(t);
  await writeFile(join(agent, 'auth.json'), 'FAKE_AUTH_SECRET');
  const { skills } = await discover(root, false, { agentDir: agent });
  const file = skills[0].skill.filePath;
  await rm(file);
  await symlink(join(agent, 'auth.json'), file);
  await assert.rejects(expandSkill('/skill:release-notes', skills));
  await assert.rejects(expandSkill('/skill:release-notes go', skills), error => !String(error).includes('FAKE_AUTH_SECRET'));
});

test('a standalone skill grants its own file, never the folder it sits in', async t => {
  const { root, agent, home } = await fixture(t);
  const folder = join(home, '.ssh');
  await mkdir(folder);
  await writeFile(join(folder, 'deploy.md'), '---\nname: deploy\ndescription: Deploy the site. Use when shipping.\n---\n\nRun the deploy.\n');
  await writeFile(join(folder, 'id_ed25519'), 'FAKE_SSH_SECRET');
  await writeFile(join(agent, 'settings.json'), JSON.stringify({ skills: [join(folder, 'deploy.md')] }));
  const { skills } = await discover(root, false, { agentDir: agent });
  const deploy = skills.find(entry => entry.skill.name === 'deploy');
  assert.ok(deploy, 'the standalone skill still loads');
  assert.equal(deploy.folder, false);
  assert.ok(skillFile(skills, join(folder, 'deploy.md')));
  assert.equal(skillFile(skills, join(folder, 'id_ed25519')), undefined);
  assert.ok(!readableFolders(skills).some(path => path.endsWith('.ssh')));
  assert.match(await expandSkill('/skill:deploy', skills), /Run the deploy/);
});

test('skill folders in or holding credential folders or the host store are skipped', async t => {
  const { root, agent, home } = await fixture(t);
  const store = join(home, 'store');
  await skill(join(home, '.ssh', 'tool'), 'in-ssh', 'Sits inside a credential folder.');
  await skill(store, 'is-store', 'Is the host store.');
  await writeFile(join(agent, 'settings.json'), JSON.stringify({ skills: [join(home, '.ssh', 'tool'), store] }));
  const names = (await discover(root, false, { agentDir: agent, protect: [store] })).skills.map(entry => entry.skill.name);
  assert.deepEqual(names.sort(), ['release-notes']);
});
