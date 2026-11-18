import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { createModels } from '@earendil-works/pi-ai/models';
import { LinearService } from '../src/linear/service.ts';
import { LINEAR_COMMENT_BODY_MAX_BYTES } from '../src/linear/types.ts';
import { Runtime } from '../src/runtime.ts';
import { FakeLinearServer } from './support/linear-server.ts';

async function setup(t: test.TestContext) {
  const dir = await mkdtemp(join(tmpdir(), 'plait-linear-writes-'));
  const storeDir = join(dir, 'store');
  await mkdir(storeDir, { recursive: true, mode: 0o700 });
  const fake = new FakeLinearServer();
  const endpoint = await fake.start();
  let now = 1_763_500_000_000;
  let runtime = new Runtime(storeDir, createModels(), new Set());
  const attach = (target: Runtime) => {
    target.linear = new LinearService(target.store, storeDir, { endpoint, timeoutMs: 1000, now: () => now });
  };
  attach(runtime);
  t.after(async () => {
    await runtime.close();
    await fake.close();
    await rm(dir, { recursive: true, force: true });
  });
  const transcript: string[] = [];
  const call = async (input: unknown): Promise<any> => {
    const reply = await runtime.reply(input);
    transcript.push(JSON.stringify(reply));
    return reply;
  };
  const restart = async () => {
    await runtime.close();
    runtime = new Runtime(storeDir, createModels(), new Set());
    attach(runtime);
  };
  return {
    storeDir, fake, call, transcript, restart,
    get runtime() { return runtime; },
    advance(ms: number) { now += ms; },
  };
}

test('linear_post_comment posts once, invalidates cache, deduplicates retries (including after restart and failed refresh), and rejects conflicts', async t => {
  const h = await setup(t);
  assert.equal((await h.call({ op: 'linear_set_key', key: h.fake.validKey })).ok, true);

  // Prime list and detail caches first.
  assert.equal((await h.call({ op: 'linear_list_issues', team_id: 'team-pla', filter: 'active' })).ok, true);
  assert.equal((await h.call({ op: 'linear_get_issue', issue_id: 'issue-142' })).result.issue.comments.length, 1);

  const mutationsBefore = h.fake.requests.filter(r => r.query.includes('PlaitCommentCreate')).length;
  const posted = await h.call({
    op: 'linear_post_comment',
    issue_id: 'issue-142',
    body: 'Investigated the backoff behavior.\nReady for review.',
    request_id: 'cmt-req-1',
  });
  assert.equal(posted.ok, true, JSON.stringify(posted));
  assert.equal(typeof posted.result.comment_id, 'string');
  assert.equal(posted.result.issue.comments.length, 2);
  assert.equal(posted.result.issue.comments[1].body, 'Investigated the backoff behavior.\nReady for review.');
  assert.equal(h.fake.requests.filter(r => r.query.includes('PlaitCommentCreate')).length, mutationsBefore + 1);

  // Team issue list page cache was invalidated, so next list hits Linear fresh.
  const listAfter = await h.call({ op: 'linear_list_issues', team_id: 'team-pla', filter: 'active' });
  assert.equal(listAfter.result.cached, false);

  // Retrying the same request_id returns the saved result without posting a second comment.
  const retry1 = await h.call({
    op: 'linear_post_comment',
    issue_id: 'issue-142',
    body: 'Investigated the backoff behavior.\nReady for review.',
    request_id: 'cmt-req-1',
  });
  assert.deepEqual(retry1, posted);
  assert.equal(h.fake.requests.filter(r => r.query.includes('PlaitCommentCreate')).length, mutationsBefore + 1);

  // Survives host restart without posting again.
  await h.restart();
  const retryAfterRestart = await h.call({
    op: 'linear_post_comment',
    issue_id: 'issue-142',
    body: 'Investigated the backoff behavior.\nReady for review.',
    request_id: 'cmt-req-1',
  });
  assert.deepEqual(retryAfterRestart, posted);
  assert.equal(h.fake.requests.filter(r => r.query.includes('PlaitCommentCreate')).length, mutationsBefore + 1);

  // Same request_id with different body returns conflict and does not post.
  const conflict = await h.call({
    op: 'linear_post_comment',
    issue_id: 'issue-142',
    body: 'Different body',
    request_id: 'cmt-req-1',
  });
  assert.equal(conflict.ok, false);
  assert.equal(conflict.error.code, 'conflict');
  assert.equal(h.fake.requests.filter(r => r.query.includes('PlaitCommentCreate')).length, mutationsBefore + 1);

  // If the post-write refresh fails (e.g. transient 503), retrying the same request_id
  // refreshes the issue WITHOUT posting a second comment.
  const beforeSecond = h.fake.requests.filter(r => r.query.includes('PlaitCommentCreate')).length;
  let sawComment = false;
  h.fake.enqueueFault(() => { sawComment = true; return false; });
  h.fake.enqueueFault((_req, res) => { res.writeHead(503); res.end(); return true; });
  const partialPost = await h.call({
    op: 'linear_post_comment',
    issue_id: 'issue-142',
    body: 'Second comment with flaky refresh',
    request_id: 'cmt-req-2',
  });
  assert.equal(sawComment, true);
  assert.equal(partialPost.ok, true);
  assert.equal(partialPost.result.issue, null);
  assert.equal(h.fake.requests.filter(r => r.query.includes('PlaitCommentCreate')).length, beforeSecond + 1);

  const retrySecond = await h.call({
    op: 'linear_post_comment',
    issue_id: 'issue-142',
    body: 'Second comment with flaky refresh',
    request_id: 'cmt-req-2',
  });
  assert.equal(retrySecond.ok, true);
  assert.equal(retrySecond.result.comment_id, partialPost.result.comment_id);
  assert.equal(retrySecond.result.issue.comments.length, 3);
  assert.equal(h.fake.requests.filter(r => r.query.includes('PlaitCommentCreate')).length, beforeSecond + 1);

  // Concurrent identical retries with the same request_id send only one mutation.
  let releaseConcurrent!: () => void;
  const concurrentStarted = new Promise<void>(resolve => {
    h.fake.enqueueFault(async () => {
      resolve();
      await new Promise<void>(done => { releaseConcurrent = done; });
      return false;
    });
  });
  const beforeConcurrent = h.fake.requests.filter(r => r.query.includes('PlaitCommentCreate')).length;
  const p1 = h.call({ op: 'linear_post_comment', issue_id: 'issue-142', body: 'Once only', request_id: 'cmt-req-3' });
  await concurrentStarted;
  const p2 = h.call({ op: 'linear_post_comment', issue_id: 'issue-142', body: 'Once only', request_id: 'cmt-req-3' });
  releaseConcurrent();
  const [r1, r2] = await Promise.all([p1, p2]);
  assert.deepEqual(r1, r2);
  assert.equal(h.fake.requests.filter(r => r.query.includes('PlaitCommentCreate')).length, beforeConcurrent + 1);
});

test('linear_update_issue_state checks team states before sending, invalidates cache, and deduplicates retries', async t => {
  const h = await setup(t);
  assert.equal((await h.call({ op: 'linear_set_key', key: h.fake.validKey })).ok, true);
  assert.equal((await h.call({ op: 'linear_list_teams' })).ok, true);
  assert.equal((await h.call({ op: 'linear_list_issues', team_id: 'team-pla', filter: 'active' })).ok, true);
  assert.equal((await h.call({ op: 'linear_get_issue', issue_id: 'issue-142' })).ok, true);

  // State from another team (team-ops) is refused BEFORE sending any mutation to Linear.
  const wrongTeam = await h.call({
    op: 'linear_update_issue_state',
    issue_id: 'issue-142',
    state_id: 'state-ops-done',
    request_id: 'state-req-bad',
  });
  assert.equal(wrongTeam.ok, false);
  assert.equal(wrongTeam.error.code, 'invalid_request');
  assert.equal(h.fake.requests.filter(r => r.query.includes('PlaitIssueUpdateState')).length, 0);

  // Valid state belonging to team-pla succeeds, invalidates caches, and returns refreshed issue.
  const moved = await h.call({
    op: 'linear_update_issue_state',
    issue_id: 'issue-142',
    state_id: 'state-done',
    request_id: 'state-req-1',
  });
  assert.equal(moved.ok, true, JSON.stringify(moved));
  assert.equal(moved.result.issue.state.id, 'state-done');
  assert.equal(moved.result.issue.state.name, 'Done');
  assert.equal(h.fake.requests.filter(r => r.query.includes('PlaitIssueUpdateState')).length, 1);

  // Active list cache was invalidated and now reflects that PLA-142 left active states.
  const activeAfter = await h.call({ op: 'linear_list_issues', team_id: 'team-pla', filter: 'active' });
  assert.equal(activeAfter.result.cached, false);
  assert.equal(activeAfter.result.issues.length, 0);

  // Retry with same request_id returns saved reply without repeating the mutation.
  const retry = await h.call({
    op: 'linear_update_issue_state',
    issue_id: 'issue-142',
    state_id: 'state-done',
    request_id: 'state-req-1',
  });
  assert.deepEqual(retry, moved);
  assert.equal(h.fake.requests.filter(r => r.query.includes('PlaitIssueUpdateState')).length, 1);

  // Different state with same request_id returns conflict.
  const conflict = await h.call({
    op: 'linear_update_issue_state',
    issue_id: 'issue-142',
    state_id: 'state-todo',
    request_id: 'state-req-1',
  });
  assert.equal(conflict.ok, false);
  assert.equal(conflict.error.code, 'conflict');
});

test('read-only key refuses both writes safely and never leaks the key into replies or host.sqlite', async t => {
  const h = await setup(t);
  assert.equal((await h.call({ op: 'linear_set_key', key: h.fake.readOnlyKey })).ok, true);
  assert.equal((await h.call({ op: 'linear_list_teams' })).ok, true);

  h.fake.readOnlyMode = 'graphql_forbidden';
  const commentRefused = await h.call({
    op: 'linear_post_comment',
    issue_id: 'issue-142',
    body: 'Should fail on read-only key',
    request_id: 'ro-1',
  });
  assert.equal(commentRefused.ok, false);
  assert.equal(commentRefused.error.code, 'linear_read_only_key');

  h.fake.readOnlyMode = 'http403';
  const stateRefused = await h.call({
    op: 'linear_update_issue_state',
    issue_id: 'issue-142',
    state_id: 'state-done',
    request_id: 'ro-2',
  });
  assert.equal(stateRefused.ok, false);
  assert.equal(stateRefused.error.code, 'linear_read_only_key');

  // Verify the key never appears in any reply or in host.sqlite bytes.
  assert(!h.transcript.join('\n').includes(h.fake.readOnlyKey));
  for (const suffix of ['', '-wal', '-shm']) {
    try {
      const bytes = await readFile(join(h.storeDir, `host.sqlite${suffix}`));
      assert(!bytes.includes(h.fake.readOnlyKey));
    } catch (error) {
      if ((error as { code?: string }).code !== 'ENOENT') throw error;
    }
  }
});

test('write operations enforce bounds, reject malformed inputs, and are not exposed as agent tools', async t => {
  const h = await setup(t);
  assert.equal((await h.call({ op: 'linear_set_key', key: h.fake.validKey })).ok, true);

  // Exact 16 KiB body is accepted; 16 KiB + 1 byte is rejected.
  const exact = 'a'.repeat(LINEAR_COMMENT_BODY_MAX_BYTES);
  const okMax = await h.call({ op: 'linear_post_comment', issue_id: 'issue-142', body: exact, request_id: 'max-ok' });
  assert.equal(okMax.ok, true);
  const tooBig = 'a'.repeat(LINEAR_COMMENT_BODY_MAX_BYTES + 1);
  const badMax = await h.call({ op: 'linear_post_comment', issue_id: 'issue-142', body: tooBig, request_id: 'max-bad' });
  assert.equal(badMax.ok, false);
  assert.equal(badMax.error.code, 'invalid_request');

  for (const bad of [
    { op: 'linear_post_comment', issue_id: 'issue-142', body: '   ', request_id: 'r1' },
    { op: 'linear_post_comment', issue_id: 'issue-142', body: 'hi\u0000there', request_id: 'r2' },
    { op: 'linear_post_comment', issue_id: 'bad/id', body: 'hi', request_id: 'r3' },
    { op: 'linear_post_comment', issue_id: 'issue-142', body: 'hi', request_id: 'bad id' },
    { op: 'linear_post_comment', issue_id: 'issue-142', body: 'hi', request_id: 'r4', protocol_version: 6 },
    { op: 'linear_update_issue_state', issue_id: 'issue-142', state_id: 'bad/state', request_id: 'r5' },
    { op: 'linear_update_issue_state', issue_id: 'issue-142', state_id: 'state-done', request_id: 'r6', extra: true },
  ]) {
    const reply = await h.call(bad);
    assert.equal(reply.ok, false, JSON.stringify(bad));
    assert.equal(reply.error.code, 'invalid_request', JSON.stringify(bad));
  }

  // No agent tool in Runtime's tool registry touches Linear.
  const toolNames = h.runtime.registry({} as never).snapshot().tools().map(({ tool }) => tool.name);
  assert.deepEqual(toolNames.sort(), ['command', 'edit', 'read', 'read_output', 'write']);
});
