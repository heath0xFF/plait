import test from 'node:test';
import assert from 'node:assert/strict';
import { allowReply, approvalFor, canUseToolFrom, DECLINED, denyReply, mcpName, unsupportedReply } from '../src/claude/approval.ts';
import type { ClaudeApproval } from '../src/claude/approval.ts';

test('a Bash request shows the command, its description, and why Claude Code asked', () => {
  const approval = approvalFor('Bash', { command: 'cargo test', description: 'Run the tests' }, { decisionReason: 'Not in your allow rules' })!;
  assert.equal(approval.kind, 'command');
  assert.equal(approval.summary, 'cargo test');
  assert.equal(approval.details, 'Run the tests\nReason: Not in your allow rules');
  assert.deepEqual(approval.paths, []);
  assert.match(approval.risk, /outside Plait's sandbox\. Only this request is approved\./);
  assert.equal(approvalFor('Bash', {})!.summary, 'Run a command');
  assert.match(approvalFor('Bash', { command: 'sleep 9', run_in_background: true })!.details, /Runs in the background/);
});

test('edit requests list their file and a bounded diff', () => {
  const edit = approvalFor('Edit', { file_path: '/work/a.ts', old_string: 'one\ntwo', new_string: 'three' })!;
  assert.equal(edit.kind, 'edit');
  assert.equal(edit.summary, 'Edit /work/a.ts');
  assert.equal(edit.details, '-one\n-two\n+three');
  assert.deepEqual(edit.paths, ['/work/a.ts']);
  assert.equal(approvalFor('Edit', { file_path: '/work/a.ts', old_string: 'a', new_string: 'b', replace_all: true })!.summary, 'Edit /work/a.ts (every match)');

  const write = approvalFor('Write', { file_path: '/work/big.txt', content: 'x'.repeat(50_000) })!;
  assert.equal(write.summary, 'Write /work/big.txt');
  assert.ok(write.details.length <= 8192);
  assert.ok(write.details.endsWith('…'));

  const notebook = approvalFor('NotebookEdit', { notebook_path: '/work/n.ipynb', new_source: 'print(1)', edit_mode: 'delete' })!;
  assert.equal(notebook.summary, 'Delete a cell in /work/n.ipynb');
  assert.deepEqual(notebook.paths, ['/work/n.ipynb']);

  const multi = approvalFor('MultiEdit', { file_path: '/work/m.ts', edits: [{ old_string: 'a', new_string: 'b' }, { old_string: 'c', new_string: 'd' }] })!;
  assert.equal(multi.summary, 'Edit /work/m.ts in 2 places');
  assert.equal(multi.details, '-a\n+b\n-c\n+d');
  assert.deepEqual(approvalFor('Write', {})!.paths, []);
});

test('web, MCP, plan, and other tools read plainly', () => {
  const fetch = approvalFor('WebFetch', { url: 'https://example.com/docs', prompt: 'Summarize' })!;
  assert.equal(fetch.summary, 'Fetch https://example.com/docs');
  assert.equal(fetch.details, 'Summarize');
  assert.match(fetch.risk, /network/);
  assert.equal(approvalFor('WebSearch', { query: 'node test runner' })!.summary, 'Search the web for node test runner');

  assert.deepEqual(mcpName('mcp__github__create_issue'), { server: 'github', tool: 'create_issue' });
  assert.deepEqual(mcpName('mcp__my__server__run'), { server: 'my__server', tool: 'run' });
  assert.equal(mcpName('Bash'), undefined);
  const mcp = approvalFor('mcp__github__create_issue', { title: 'Bug' }, { mcpServer: { name: 'github\u001b[31m', source: 'user' } })!;
  assert.equal(mcp.summary, 'Use create_issue from github[31m');
  assert.match(mcp.details, /"title": "Bug"/);
  assert.match(mcp.risk, /server from your Claude Code configuration/);

  const plan = approvalFor('ExitPlanMode', { plan: '1. Edit a.ts\n2. Test' })!;
  assert.equal(plan.summary, 'Leave plan mode and start on this plan');
  assert.equal(plan.details, '1. Edit a.ts\n2. Test');

  const other = approvalFor('Glob', { pattern: '**/*.ts' }, { title: 'Claude wants to search for files' })!;
  assert.equal(other.summary, 'Claude wants to search for files');
  assert.equal(approvalFor('Glob', { pattern: '*' })!.summary, 'Use Glob');
  // Bidi controls from untrusted text never reach the approval.
  assert.equal(approvalFor('Bash', { command: 'echo ‮evil' })!.summary, 'echo evil');
  assert.ok(approvalFor('Bash', { command: 'y'.repeat(5000) })!.summary.length <= 1024);
});

test('questions are declined with a note for the model; replies approve one call only', () => {
  assert.equal(approvalFor('AskUserQuestion', { questions: [] }), undefined);
  const refusal = unsupportedReply('AskUserQuestion');
  assert.equal(refusal.behavior, 'deny');
  assert.match((refusal as { message: string }).message, /Ask it in your reply instead/);
  const input = { command: 'ls' };
  assert.deepEqual(allowReply(input), { behavior: 'allow', updatedInput: input });
  assert.deepEqual(denyReply(), { behavior: 'deny', message: DECLINED });
  assert.equal('updatedPermissions' in allowReply(input), false);
});

test('canUseToolFrom asks once per request and declines on refusal, error, or abort', async () => {
  const asked: ClaudeApproval[] = [];
  const options = (signal = new AbortController().signal) => ({ signal, toolUseID: 'toolu_1', requestId: 'r1', suggestions: [] });
  const yes = canUseToolFrom(async approval => { asked.push(approval); return true; });
  assert.deepEqual(await yes('Bash', { command: 'ls' }, options()), { behavior: 'allow', updatedInput: { command: 'ls' } });
  assert.equal(asked[0].summary, 'ls');
  const no = canUseToolFrom(async () => false);
  assert.deepEqual(await no('Bash', { command: 'ls' }, options()), { behavior: 'deny', message: DECLINED });
  const broken = canUseToolFrom(async () => { throw new Error('gone'); });
  assert.equal((await broken('Bash', { command: 'ls' }, options()))?.behavior, 'deny');
  const controller = new AbortController();
  const late = canUseToolFrom(async () => { controller.abort(); return true; });
  assert.equal((await late('Bash', { command: 'ls' }, options(controller.signal)))?.behavior, 'deny');
  let called = false;
  const question = canUseToolFrom(async () => { called = true; return true; });
  assert.equal((await question('AskUserQuestion', {}, options()))?.behavior, 'deny');
  assert.equal(called, false);
});
