import test from 'node:test';
import assert from 'node:assert/strict';
import { approvalFor, approvalReply, declineUnsupported } from '../src/codex/approval.ts';
import type { FileChangeItem } from '../src/codex/protocol.ts';

const base = { threadId: 'thread-1', turnId: 'turn-1', itemId: 'item-1', startedAtMs: 1 };

test('a command approval shows the command line, cwd, and reason', () => {
  const approval = approvalFor('item/commandExecution/requestApproval', {
    ...base, kind: 'command', environmentId: null, command: 'cargo test', cwd: '/work', reason: 'Needs network',
  });
  assert.equal(approval?.kind, 'command');
  assert.equal(approval?.summary, 'cargo test');
  assert.equal(approval?.details, 'In /work\nReason: Needs network');
  assert.deepEqual(approval?.paths, []);
  assert.match(approval!.risk, /Only this request is approved/);
  const stdin = approvalFor('item/commandExecution/requestApproval', { ...base, kind: 'writeStdin', environmentId: null });
  assert.equal(stdin?.summary, 'Send input to a running command');
  assert.equal(stdin?.details, '');
});

test('an edit approval lists the started item\'s files and a bounded diff', () => {
  const item: FileChangeItem = { type: 'fileChange', id: 'item-1', status: 'inProgress', changes: [
    { path: '/work/a.ts', kind: { type: 'update', move_path: null }, diff: '-a\n+b\n' },
    { path: '/work/big.ts', kind: { type: 'add' }, diff: `+${'x'.repeat(20000)}\n` },
    { path: '/work/old.ts', kind: { type: 'delete' }, diff: '' },
  ] };
  const approval = approvalFor('item/fileChange/requestApproval', { ...base, reason: 'Refactor' }, item)!;
  assert.equal(approval.kind, 'edit');
  assert.equal(approval.summary, 'Edit 3 files');
  assert.deepEqual(approval.paths, ['/work/a.ts', '/work/big.ts', '/work/old.ts']);
  assert.ok(approval.details.startsWith('Reason: Refactor\nUpdate /work/a.ts\nAdd /work/big.ts\nDelete /work/old.ts\n-a\n+b\n'));
  assert.ok(approval.details.length <= 8192);
  assert.ok(approval.details.endsWith('…'));
  const one = approvalFor('item/fileChange/requestApproval', base, { ...item, changes: item.changes.slice(0, 1) })!;
  assert.equal(one.summary, 'Edit 1 file');
  assert.equal(approvalFor('item/fileChange/requestApproval', base)!.summary, 'Edit files');
});

test('other methods are not approvals', () => {
  assert.equal(approvalFor('item/permissions/requestApproval', base), undefined);
  assert.equal(approvalFor('execCommandApproval', base), undefined);
});

test('replies use the generated decision values', () => {
  assert.deepEqual(approvalReply('item/commandExecution/requestApproval', true), { decision: 'accept' });
  assert.deepEqual(approvalReply('item/commandExecution/requestApproval', false), { decision: 'decline' });
  assert.deepEqual(approvalReply('item/fileChange/requestApproval', true), { decision: 'accept' });
  assert.deepEqual(approvalReply('item/fileChange/requestApproval', false), { decision: 'decline' });
  assert.throws(() => approvalReply('item/tool/requestUserInput', true), /not an approval/);
});

test('unsupported requests are refused with a safe result', () => {
  assert.deepEqual(declineUnsupported('item/permissions/requestApproval'), { permissions: {}, scope: 'turn' });
  assert.deepEqual(declineUnsupported('item/tool/requestUserInput'), { answers: {} });
  assert.deepEqual(declineUnsupported('mcpServer/elicitation/request'), { action: 'decline', content: null, _meta: null });
  assert.deepEqual(declineUnsupported('item/tool/call'), { contentItems: [{ type: 'inputText', text: 'Plait does not support this request.' }], success: false });
  assert.deepEqual(declineUnsupported('execCommandApproval'), { decision: { denied: { rejection: 'Plait does not support this request.' } } });
  assert.deepEqual(declineUnsupported('applyPatchApproval'), { decision: { denied: { rejection: 'Plait does not support this request.' } } });
  assert.equal(declineUnsupported('account/chatgptAuthTokens/refresh'), undefined);
  assert.equal(declineUnsupported('attestation/generate'), undefined);
  assert.equal(declineUnsupported('item/commandExecution/requestApproval'), undefined);
});
