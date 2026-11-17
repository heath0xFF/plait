import assert from 'node:assert/strict';
import { mkdir, rm, symlink, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { discover, expandInput, LIST_LIMIT, report } from '../src/skills.ts';
import { expandTemplate, parseArgs, substitute } from '../src/templates.ts';
import { setup } from './support/safety.ts';

/** Pi's own parser and substitution, loaded by file path, to check Plait's against. */
async function pi() {
  const entry = fileURLToPath(import.meta.resolve('@earendil-works/pi-coding-agent'));
  return await import(pathToFileURL(join(dirname(entry), 'core', 'prompt-templates.js')).href) as {
    parseCommandArgs(text: string): string[]; substituteArgs(body: string, args: string[]): string;
  };
}

async function fixture(t: Parameters<typeof setup>[0]) {
  const { root, base } = await setup(t);
  const home = process.env.HOME;
  process.env.HOME = join(base, 'home');
  await mkdir(process.env.HOME);
  t.after(async () => { process.env.HOME = home; });
  const agent = join(base, 'agent');
  await mkdir(join(agent, 'prompts'), { recursive: true });
  await writeFile(join(agent, 'prompts', 'review.md'), '---\ndescription: Review a change\nargument-hint: <path> [focus]\n---\nReview $1 for ${2:-correctness}.\n');
  await writeFile(join(agent, 'prompts', 'plain.md'), '\nSummarize the open threads in this checkout, then propose what to do next in order.\n');
  await mkdir(join(root, '.pi', 'prompts'), { recursive: true });
  await writeFile(join(root, '.pi', 'prompts', 'ship.md'), 'Ship it: $@');
  return { root, agent, base, home: process.env.HOME! };
}

test('arguments split and fill exactly as Pi does', async () => {
  const { parseCommandArgs, substituteArgs } = await pi();
  const inputs = ['', 'a b  c', '"a b" c', "'a b' c", '"say \\"hi\\""', 'it\'s fine', '"a"b c', '"unclosed rest', 'x "', '"" \'\' z',
    'tab\tsep\nline', '"\\\\" end', 'a"b"', "'x\\'y'", '"ab\\"'];
  for (const input of inputs) assert.deepEqual(parseArgs(input), parseCommandArgs(input), input);
  const bodies = ['$1 and $2', '$@|$ARGUMENTS', '${1:-one} ${3:-three}', '${@:-none}', '${ARGUMENTS:-none}', '${@:2}', '${@:2:1}', '${@:0}',
    '$10 $1', 'cost $5 $', '${@:1:0}', '$1$2', 'keep ${x} and $ARG'];
  for (const args of [[], ['a'], ['a', 'b', 'c'], ['$1', '$@'], ['', 'b']]) {
    for (const body of bodies) assert.equal(substitute(body, args), substituteArgs(body, args), `${body} ${JSON.stringify(args)}`);
  }
});

test('user templates load; project templates need a trusted project; descriptions only on request', async t => {
  const { root, agent } = await fixture(t);
  const names = async (trusted: boolean) => (await discover(root, trusted, { agentDir: agent })).templates.map(entry => entry.name).sort();
  assert.deepEqual(await names(false), ['plain', 'review']);
  assert.deepEqual(await names(true), ['plain', 'review', 'ship']);
  const quick = await discover(root, false, { agentDir: agent });
  assert.ok(quick.templates.every(entry => entry.description === undefined), 'a run reads no template file');
  const described = await discover(root, false, { agentDir: agent, describe: true });
  const review = described.templates.find(entry => entry.name === 'review')!;
  assert.equal(review.description, 'Review a change');
  assert.equal(review.argumentHint, '<path> [focus]');
  assert.equal(review.origin, 'user');
  assert.equal(described.templates.find(entry => entry.name === 'plain')!.description, 'Summarize the open threads in this checkout, then propose wh...');
});

test('/name expands the template through the host helper; other text passes unchanged', async t => {
  const { root, agent } = await fixture(t);
  const { templates } = await discover(root, true, { agentDir: agent });
  assert.equal(await expandTemplate('/review src/main.rs', templates), 'Review src/main.rs for correctness.');
  assert.equal(await expandTemplate('/review "a b.rs" speed', templates), 'Review a b.rs for speed.');
  assert.equal(await expandTemplate('/ship now please', templates), 'Ship it: now please');
  assert.equal(await expandTemplate('/unknown go', templates), '/unknown go');
  assert.equal(await expandTemplate('review', templates), 'review');
  assert.equal(await expandInput('/review x', { templates }), 'Review x for correctness.');
  assert.equal(await expandInput('/skill:review x', { templates }), '/skill:review x', 'a skill command never falls through to a template');
});

test('a template swapped for a link after discovery is never sent', async t => {
  const { root, agent } = await fixture(t);
  await writeFile(join(agent, 'auth.json'), 'FAKE_AUTH_SECRET');
  const { templates } = await discover(root, false, { agentDir: agent });
  await rm(join(agent, 'prompts', 'review.md'));
  await symlink(join(agent, 'auth.json'), join(agent, 'prompts', 'review.md'));
  await assert.rejects(expandTemplate('/review x', templates), error => !String(error).includes('FAKE_AUTH_SECRET'));
});

test('a linked template is followed once, to Markdown outside credential folders', async t => {
  const { root, agent, home } = await fixture(t);
  const elsewhere = join(home, 'dotfiles'); await mkdir(elsewhere);
  await writeFile(join(elsewhere, 'linked.md'), 'Linked body $1');
  await symlink(join(elsewhere, 'linked.md'), join(agent, 'prompts', 'linked.md'));
  await writeFile(join(agent, 'auth.json'), 'FAKE_AUTH_SECRET');
  await symlink(join(agent, 'auth.json'), join(agent, 'prompts', 'auth.md'));
  await mkdir(join(home, '.ssh')); await writeFile(join(home, '.ssh', 'key.md'), 'FAKE_SSH_SECRET');
  await symlink(join(home, '.ssh', 'key.md'), join(agent, 'prompts', 'key.md'));
  const found = await discover(root, false, { agentDir: agent, describe: true });
  assert.deepEqual(found.templates.map(entry => entry.name).sort(), ['linked', 'plain', 'review']);
  assert.equal(await expandTemplate('/linked x', found.templates), 'Linked body x');
  const refused = found.problems.filter(problem => problem.message.startsWith('Not offered')).map(problem => problem.path).sort();
  assert.deepEqual(refused, [join(agent, 'prompts', 'auth.md'), join(agent, 'prompts', 'key.md')]);
  assert.ok(!JSON.stringify(report(root, false, found)).includes('SECRET'));
});

test('the report lists what a run gets, bounded, without file contents', async t => {
  const { root, agent } = await fixture(t);
  await mkdir(join(agent, 'skills', 'notes'), { recursive: true });
  await writeFile(join(agent, 'skills', 'notes', 'SKILL.md'), `---\nname: notes\ndescription: ${'d'.repeat(2000)}\ndisable-model-invocation: true\n---\n\nSKILL_BODY_TEXT\n`);
  await writeFile(join(agent, 'AGENTS.md'), 'USER_INSTRUCTIONS_TEXT');
  const reply = report(root, false, await discover(root, false, { agentDir: agent, describe: true }));
  assert.equal(reply.trusted, false);
  assert.deepEqual(reply.instructions, { path: join(agent, 'AGENTS.md'), bytes: 22 });
  assert.deepEqual(reply.skills.map(skill => [skill.name, skill.origin, skill.model_invocation, skill.description.length]), [['notes', 'user', false, 1024]]);
  assert.deepEqual(reply.templates.find(template => template.name === 'review'),
    { name: 'review', description: 'Review a change', argument_hint: '<path> [focus]', origin: 'user', path: join(agent, 'prompts', 'review.md') });
  const wire = JSON.stringify(reply);
  for (const text of ['SKILL_BODY_TEXT', 'USER_INSTRUCTIONS_TEXT', 'Review $1']) assert.ok(!wire.includes(text), text);
  const template = { path: '', root: '', file: '', identity: { dev: '0', ino: '0' }, origin: 'user' as const };
  const many = { skills: [], templates: Array.from({ length: LIST_LIMIT + 3 }, (_, index) => ({ ...template, name: `t${index}` })), problems: [] };
  const capped = report(root, false, many);
  assert.equal(capped.templates.length, LIST_LIMIT);
  assert.match(capped.problems[0].message, /first 500 of 503 templates/);
  // Long descriptions in every list still fit the client's 1 MiB reply frame.
  const long = 'é'.repeat(5000);
  const skill = (index: number) => ({ skill: { name: `s${index}`, description: long, filePath: `/p/${index}`, baseDir: '/p', sourceInfo: {} as never, disableModelInvocation: false }, root: '', identity: template.identity, file: '', folder: true, origin: 'user' as const });
  const full = report(root, false, {
    skills: Array.from({ length: 600 }, (_, index) => skill(index)),
    templates: Array.from({ length: 600 }, (_, index) => ({ ...template, name: `t${index}`, description: long, argumentHint: long })),
    problems: Array.from({ length: 600 }, () => ({ path: '/p', message: long })),
  });
  assert.ok(Buffer.byteLength(JSON.stringify({ ok: true, result: full })) < 1024 * 1024);
  assert.ok(full.skills.length > 100 && full.problems.length > 10);
  assert.ok(full.problems.some(problem => /^Only the first \d+ of 600 templates/.test(problem.message)));
});

test('an oversized context file is a problem in the report, but still fails a run', async t => {
  const { root, agent } = await fixture(t);
  await writeFile(join(agent, 'AGENTS.md'), 'x'.repeat(40000));
  await assert.rejects(discover(root, false, { agentDir: agent }), /exceeds the instruction bound/);
  const found = await discover(root, false, { agentDir: agent, describe: true });
  assert.equal(found.instructions, undefined);
  assert.match(found.problems[0].message, /exceeds the instruction bound/);
});
