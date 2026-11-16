import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';
import { childPids, delay, exec, request, setup, until } from './support/host.ts';
import { stream, v3 } from './support/stream.ts';

type Item = { type: string; id: string; role?: string; text?: string; reasoning?: string; name?: string; status?: string; input?: string; output?: string;
  more?: { field: string; ref: string; offset: number; length: number }[] };
type Page = { items: Item[]; before: string | null };
type Queued = { id: string; request_id: string; text: string; mode: string; approval: string };
type Thread = {
  id: string; status: string; title: string; created_at: number; updated_at: number; archived: boolean; thinking_level: string;
  model: { provider: string; model_id: string }; approval_mode: string; queued: Queued[]; live_reasoning: string;
  usage: null | { total: number; context_used: number; context_window: number; input: number; output: number };
  retry: null | { attempt: number; at: number }; error: null | { code: string; message: string };
  pending_approval: null | { id: string; tool_id: string };
  messages: { id: string; role: string; text: string; reasoning: string }[];
  tools: { id: string; status: string; after?: string | null }[];
};
type Summary = { id: string; checkout: string; status: string; title: string; created_at: number; updated_at: number; archived: boolean; pending_approval: boolean };
type Fixture = Awaited<ReturnType<typeof setup>>;

const watch3 = async (f: Fixture, id: string) => (await v3<{ thread: Thread }>(f.store, { op: 'watch', thread_id: id })).result.thread;
const submit3 = (f: Fixture, id: string, request_id: string, text: string, extra: Record<string, unknown> = {}) =>
  v3<{ accepted: boolean }>(f.store, { op: 'submit', thread_id: id, request_id, text, ...extra });
const settled = (f: Fixture, id: string, request_id: string) => until(async () => JSON.parse(await readFile(join(f.store, `${id}.settled`), 'utf8')),
  marker => marker.request_id === request_id);
async function history(f: Fixture, id: string, limit = 500) {
  const pages: Item[][] = []; let before: string | undefined;
  for (;;) {
    const reply = await v3<Page>(f.store, { op: 'history', thread_id: id, limit, ...(before ? { before } : {}) });
    assert.equal(reply.ok, true, JSON.stringify(reply));
    pages.unshift(reply.result.items);
    if (reply.result.before === null) return pages;
    before = reply.result.before;
  }
}

test('history returns the whole conversation with the snapshot IDs and never starts work', { timeout: 40000 }, async t => {
  const f = await setup(t, 'effect'); const thread = await f.create();
  assert.deepEqual((await v3<Page>(f.store, { op: 'history', thread_id: thread.id })).result, { items: [], before: null });
  assert.equal((await v3(f.store, { op: 'history', thread_id: 'missing' })).error?.code, 'not_found');
  await f.submit(thread.id);
  const approval = await until(() => f.watch(thread.id), value => !!value.pending_approval);
  // While the run is active, history reads through the live conversation.
  const live = (await history(f, thread.id)).flat();
  assert.deepEqual(live.map(item => [item.id, item.status]), [['request-submit-1', undefined], ['effect-1', 'running']]);
  await f.approve(thread.id, approval.pending_approval!.id);
  await until(() => readFile(join(f.repo, 'attempts.txt'), 'utf8'), value => value === 'effect\n');
  const [node] = await childPids(f.host.pid!);
  process.kill(node, 'SIGKILL');
  await until(async () => f.host.exitCode !== null, value => value);
  await f.start('answer');
  const interrupted = await watch3(f, thread.id);
  assert.equal(interrupted.status, 'interrupted');

  // The conversation store holds an unfinished tool task. Reading it must not run or settle anything.
  for (let round = 0; round < 3; round++) {
    const items = (await history(f, thread.id, round + 1)).flat();
    assert.deepEqual(items.map(item => item.id), ['request-submit-1', 'effect-1']);
    assert.equal(items[1].input, JSON.stringify({ path: 'notes.txt', old_text: 'before', new_text: 'after' }));
  }
  await delay(600);
  assert.deepEqual(await watch3(f, thread.id), interrupted);
  assert.equal(await readFile(join(f.repo, 'attempts.txt'), 'utf8'), 'effect\n');
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'after\n');

  // A later run still recovers and answers normally, and history then matches the snapshot.
  await f.submit(thread.id, 'resume', 'Inspect the interrupted result.');
  const done = await until(() => watch3(f, thread.id), value => value.status === 'idle');
  const items = (await history(f, thread.id)).flat();
  assert.deepEqual(items.filter(item => item.type === 'message').map(item => item.id), done.messages.map(message => message.id));
  assert.deepEqual(items.filter(item => item.type === 'tool').map(item => item.id), done.tools.map(tool => tool.id));
  assert.deepEqual((await history(f, thread.id, 1)).flat(), items);
  assert.equal(await readFile(join(f.repo, 'attempts.txt'), 'utf8'), 'effect\n');
});

test('a reply longer than any frame budget is delivered in full', { timeout: 40000 }, async t => {
  const f = await setup(t, 'v3-long'); const thread = await f.create();
  const question = 'Write the long answer. ' + 'é'.repeat(20000);
  assert.equal((await submit3(f, thread.id, 'long', question)).ok, true);
  const done = await until(() => watch3(f, thread.id), value => value.status === 'idle' || value.status === 'error', 30000);
  assert.equal(done.status, 'idle', JSON.stringify(done.error));
  const expected = 'BEGIN ' + '0123456789'.repeat(20000) + ' END';
  // The snapshot stays the bounded tail.
  assert.equal(done.messages[1].text.length, 4096);
  const [user, answer] = (await history(f, thread.id)).flat();
  assert.equal(user.text, question);
  assert.equal(answer.id, done.messages[1].id);
  assert(answer.text!.startsWith('BEGIN 0123') && answer.text!.length === 65536);
  const more = answer.more!.find(part => part.field === 'text')!;
  assert.deepEqual([more.offset, more.length], [65536, expected.length]);
  let text = answer.text!;
  for (let offset: number | null = more.offset; offset !== null;) {
    const part: { ok: boolean; result: { text: string; next_offset: number | null }; error?: unknown } = await v3(f.store, { op: 'history_text', thread_id: thread.id, ref: more.ref, offset });
    assert.equal(part.ok, true, JSON.stringify(part.error));
    text += part.result.text; offset = part.result.next_offset;
  }
  assert.equal(text, expected);
  assert.equal((await v3(f.store, { op: 'history_text', thread_id: thread.id, ref: 'e999999.0.text', offset: 0 })).error?.code, 'not_found');
});

test('threads carry titles and timestamps, and list across checkouts with paging and archive', { timeout: 40000 }, async t => {
  const f = await setup(t);
  const other = join(f.base, 'other');
  await exec('/usr/bin/git', ['init', '-q', other]);
  const create = async (checkout: string, request_id: string) => (await v3<{ thread: Thread }>(f.store, { op: 'create_thread', checkout, provider: 'faux', model_id: 'faux-1', request_id })).result.thread;
  const list = async (extra: Record<string, unknown> = {}) => (await v3<{ threads: Summary[]; next_cursor: string | null }>(f.store, { op: 'list_threads', ...extra })).result;
  const started = Date.now();
  const first = await create(f.repo, 'c1');
  assert.deepEqual([first.title, first.archived, first.thinking_level, first.queued, first.usage], ['', false, 'off', [], null]);
  assert(first.created_at >= started && first.updated_at === first.created_at);
  await delay(5);
  const second = await create(other, 'c2');
  await delay(5);
  const third = await create(f.repo, 'c3');
  assert.deepEqual((await list()).threads.map(thread => thread.id), [third.id, second.id, first.id]);
  assert.deepEqual((await list({ checkout: other })).threads.map(thread => thread.id), [second.id]);
  assert.deepEqual(Object.keys((await list()).threads[0]).sort(), ['archived', 'checkout', 'created_at', 'id', 'model', 'pending_approval', 'status', 'title', 'updated_at']);

  await submit3(f, first.id, 's1', '  Fix\n the\tparser 🙂  ');
  const done = await until(() => watch3(f, first.id), value => value.status === 'idle');
  assert.equal(done.title, 'Fix the parser 🙂');
  assert(done.updated_at > first.updated_at && done.created_at === first.created_at);
  assert(done.usage && done.usage.total > 0 && done.usage.context_used === done.usage.total && done.usage.context_window === 128000);
  // Activity moves the thread to the front; the first message, not a later one, names it.
  assert.deepEqual((await list()).threads.map(thread => [thread.id, thread.title]), [[first.id, 'Fix the parser 🙂'], [third.id, ''], [second.id, '']]);
  await submit3(f, first.id, 's2', 'A later message');
  assert.equal((await until(() => watch3(f, first.id), value => value.status === 'idle')).title, 'Fix the parser 🙂');

  const renamed = await v3<{ thread: Thread }>(f.store, { op: 'rename_thread', thread_id: third.id, title: '  Parser\n work  ' });
  assert.equal(renamed.result.thread.title, 'Parser work');
  await submit3(f, third.id, 's3', 'This must not rename it');
  assert.equal((await until(() => watch3(f, third.id), value => value.status === 'idle')).title, 'Parser work');
  assert.equal((await v3(f.store, { op: 'rename_thread', thread_id: third.id, title: '\u0007' })).error?.code, 'invalid_request');
  assert.equal((await v3(f.store, { op: 'rename_thread', thread_id: 'missing', title: 'x' })).error?.code, 'not_found');

  // Pages follow the same order and meet at the cursor.
  const page = await list({ limit: 2 });
  assert.equal(page.threads.length, 2);
  const rest = await list({ limit: 2, cursor: page.next_cursor });
  assert.deepEqual([...page.threads, ...rest.threads].map(thread => thread.id), (await list()).threads.map(thread => thread.id));
  assert.equal(rest.next_cursor, null);

  const archived = await v3<{ thread: Thread }>(f.store, { op: 'archive_thread', thread_id: second.id, archived: true });
  assert.equal(archived.result.thread.archived, true);
  assert(!(await list()).threads.some(thread => thread.id === second.id));
  assert.equal((await list({ archived: true })).threads.find(thread => thread.id === second.id)?.archived, true);
  // Older clients have no archive; their checkout listing is unchanged.
  assert.equal((await request(f.store, { op: 'list_threads', checkout: other })).result.threads.length, 1);
  await v3(f.store, { op: 'archive_thread', thread_id: second.id, archived: false });
  assert.equal((await list()).threads.length, 3);
  // Metadata survives a restart.
  f.host.kill('SIGKILL'); await delay(400); await f.start();
  assert.deepEqual((await list()).threads.map(thread => thread.title).sort(), ['', 'Fix the parser 🙂', 'Parser work']);
});

test('watch_threads delivers summary changes for every thread and nothing for streamed text', { timeout: 40000 }, async t => {
  const started = Date.now();
  const f = await setup(t, 'edit');
  const first = await f.create();
  type Frame = { ok?: boolean; result?: { threads: Summary[] }; event?: string; threads?: Summary[]; removed?: string[] };
  const threads = stream<Frame>(f.store, { op: 'watch_threads', protocol_version: 3 });
  t.after(async () => { threads.socket.destroy(); });
  await until(async () => threads.frames.length, count => count === 1);
  assert.deepEqual(threads.frames[0].result!.threads.map(thread => thread.id), [first.id]);
  const events = () => threads.frames.slice(1);
  const latest = (id: string) => events().flatMap(frame => frame.threads ?? []).filter(thread => thread.id === id).at(-1);

  const second = await f.create('create-2');
  await until(async () => latest(second.id), Boolean);
  assert(events().every(frame => frame.event === 'threads' && Array.isArray(frame.removed)));

  await f.submit(first.id, 'submit-1', 'Watch this thread');
  await until(async () => latest(first.id), value => value?.pending_approval === true);
  assert.deepEqual([latest(first.id)!.status, latest(first.id)!.title], ['awaiting_approval', 'Watch this thread']);
  const pending = await f.watch(first.id);
  await f.approve(first.id, pending.pending_approval!.id);
  await until(async () => latest(first.id), value => value?.status === 'idle');
  assert.equal(latest(first.id)!.pending_approval, false);
  assert(latest(first.id)!.updated_at >= started);
  // The answer streamed in many snapshots; the list changed only with status, title, or approval.
  const changes = events().filter(frame => frame.threads?.some(thread => thread.id === first.id)).length;
  assert(changes >= 3 && changes <= 8, `thread list events: ${changes}`);
  assert(events().every(frame => frame.threads!.every(thread => thread.id === first.id || thread.id === second.id)));

  await v3(f.store, { op: 'rename_thread', thread_id: second.id, title: 'Second' });
  await until(async () => latest(second.id), value => value?.title === 'Second');
  await v3(f.store, { op: 'archive_thread', thread_id: second.id, archived: true });
  await until(async () => events().at(-1), frame => !!frame?.removed?.includes(second.id));
  // A watcher that asked for archived threads keeps receiving them.
  const all = stream<Frame>(f.store, { op: 'watch_threads', protocol_version: 3, archived: true });
  t.after(async () => { all.socket.destroy(); });
  await until(async () => all.frames.length, count => count === 1);
  assert.equal(all.frames[0].result!.threads.find(thread => thread.id === second.id)?.archived, true);
  // Disconnecting a watcher stops nothing, and an older request cannot open one.
  threads.socket.destroy();
  assert.equal((await request(f.store, { op: 'watch_threads' })).error?.code, 'invalid_request');
  assert.equal((await f.watch(first.id)).status, 'idle');
});

test('a thread changes model and thinking level between runs, and reasoning is kept in order', { timeout: 40000 }, async t => {
  const f = await setup(t, 'v3-thinking'); const thread = await f.create();
  const status = (await v3<{ protocol_version: number; models: Record<string, unknown>[] }>(f.store, { op: 'status' })).result;
  assert.equal(status.protocol_version, 3);
  assert.deepEqual(status.models, [
    { provider: 'faux', model_id: 'faux-1', name: 'Faux Model', authenticated: true, reasoning: false, thinking_levels: ['off'], context_window: 128000, input: ['text', 'image'] },
    { provider: 'faux', model_id: 'faux-think', name: 'Faux Thinking', authenticated: true, reasoning: true, thinking_levels: ['off', 'minimal', 'low', 'medium', 'high'], context_window: 64000, input: ['text', 'image'] },
  ]);
  const configure = (extra: Record<string, unknown>) => v3<{ thread: Thread }>(f.store, { op: 'configure_thread', thread_id: thread.id, ...extra });
  assert.equal((await configure({ thinking: 'high' })).error?.code, 'invalid_request');
  assert.equal((await configure({ provider: 'faux', model_id: 'missing' })).error?.code, 'model_unavailable');
  assert.equal((await configure({ provider: 'faux', model_id: 'faux-think', thinking: 'max' })).error?.code, 'invalid_request');
  const changed = (await configure({ provider: 'faux', model_id: 'faux-think', thinking: 'high' })).result.thread;
  assert.deepEqual([changed.model, changed.thinking_level], [{ provider: 'faux', model_id: 'faux-think' }, 'high']);
  // An older client sees the new model on the same thread.
  assert.deepEqual((await f.watch(thread.id)).model, { provider: 'faux', model_id: 'faux-think' });

  const live = stream<{ result?: { thread: Thread }; thread?: Thread }>(f.store, { op: 'watch', thread_id: thread.id, protocol_version: 3 });
  t.after(async () => { live.socket.destroy(); });
  await submit3(f, thread.id, 's1', 'Read the notes.');
  assert.equal((await configure({ thinking: 'low' })).error?.code, 'busy');
  const done = await until(() => watch3(f, thread.id), value => value.status === 'idle');
  assert.equal(done.usage?.context_window, 64000);
  assert.deepEqual(done.messages.map(message => [message.role, message.text, message.reasoning.slice(0, 27)]), [
    ['user', 'Read the notes.', ''],
    ['assistant', '', 'Plan: read the notes first.'],
    ['assistant', 'The notes say before.', 'The file says before.'],
  ]);
  assert.deepEqual(done.tools.map(tool => [tool.id, tool.after, tool.status]), [['think-read', done.messages[1].id, 'done']]);
  assert.equal(done.live_reasoning, '');
  // Reasoning streamed while the model was thinking.
  const frames = live.frames.map(frame => frame.thread ?? frame.result!.thread);
  assert(frames.some(frame => frame.live_reasoning.startsWith('Plan: read')), 'no live reasoning frame');
  // Older clients keep text messages only, with the tool after the last one they can see.
  const second = (await request(f.store, { op: 'watch', thread_id: thread.id, protocol_version: 2 })).result.thread;
  assert.deepEqual(second.messages.map(message => Object.keys(message).sort().join()), ['id,role,text', 'id,role,text']);
  assert.equal(second.tools[0].after, 'request-s1');
  const items = (await history(f, thread.id)).flat();
  assert.deepEqual(items.map(item => [item.type, item.id]), [
    ['message', 'request-s1'], ['message', done.messages[1].id], ['tool', 'think-read'], ['message', done.messages[2].id]]);
  assert.equal(items[1].reasoning, 'Plan: read the notes first. '.repeat(20).trim());
  assert.equal(items[2].output, 'before\n');

  // Returning to a model without reasoning moves the level to one it supports; the setting survives restart.
  const back = (await configure({ provider: 'faux', model_id: 'faux-1' })).result.thread;
  assert.deepEqual([back.model.model_id, back.thinking_level], ['faux-1', 'off']);
  await configure({ provider: 'faux', model_id: 'faux-think', thinking: 'medium' });
  f.host.kill('SIGKILL'); await delay(400); await f.start('v3-thinking');
  const restored = await watch3(f, thread.id);
  assert.deepEqual([restored.model.model_id, restored.thinking_level], ['faux-think', 'medium']);
});

test('a follow-up queued during a run starts its own run in its own approval mode', { timeout: 40000 }, async t => {
  const f = await setup(t, 'separate-edits'); const thread = await f.create();
  // The first run approves automatically. The queued one asked to be prompted, and is.
  assert.equal((await submit3(f, thread.id, 's1', 'First task.', { approval: 'auto' })).ok, true);
  const text = 'Follow up: ' + 'x'.repeat(5000);
  assert.deepEqual((await submit3(f, thread.id, 'f1', text, { when_busy: 'follow_up', approval: 'ask' })).result, { thread_id: thread.id, request_id: 'f1', accepted: true });
  const busy = await watch3(f, thread.id);
  assert.deepEqual(busy.queued, [{ id: 'request-f1', request_id: 'f1', text: text.slice(-4096), mode: 'follow_up', approval: 'ask' }]);
  assert(!busy.messages.some(message => message.id === 'request-f1'));
  // Request IDs deduplicate queued inputs as they do accepted ones.
  assert.equal((await submit3(f, thread.id, 'f1', text, { when_busy: 'follow_up', approval: 'ask' })).ok, true);
  assert.equal((await submit3(f, thread.id, 'f1', 'changed', { when_busy: 'follow_up' })).error?.code, 'conflict');
  assert.equal((await watch3(f, thread.id)).queued.length, 1);
  // Without when_busy the answer is the same `busy` as before, and older clients see no queue.
  assert.equal((await f.submit(thread.id, 'plain', 'Not queued.')).error?.code, 'busy');
  assert(!('queued' in (await request(f.store, { op: 'watch', thread_id: thread.id, protocol_version: 2 })).result.thread));

  const pending = await until(() => watch3(f, thread.id), value => !!value.pending_approval, 20000);
  assert.equal(pending.pending_approval!.tool_id, 'second-edit');
  assert.deepEqual([pending.approval_mode, pending.queued], ['ask', []]);
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'after\n');
  assert.deepEqual(pending.messages.map(message => message.id).filter(id => id.startsWith('request-')), ['request-s1', 'request-f1']);
  assert.equal((await f.approve(thread.id, pending.pending_approval!.id)).ok, true);
  const done = await until(() => watch3(f, thread.id), value => value.status === 'idle');
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'followup\n');
  assert.equal((await settled(f, thread.id, 'f1')).status, 'idle');
  // The full queued text is in history once it runs.
  assert.equal((await history(f, thread.id)).flat().find(item => item.id === 'request-f1')?.text, text);
  assert.equal(done.messages.filter(message => message.role === 'user').length, 2);
});

test('a queued follow-up can approve automatically after a run that asked', { timeout: 40000 }, async t => {
  const f = await setup(t, 'separate-edits'); const thread = await f.create();
  await submit3(f, thread.id, 's1', 'First task.');
  const pending = await until(() => watch3(f, thread.id), value => !!value.pending_approval);
  assert.equal((await submit3(f, thread.id, 'f1', 'Second task.', { when_busy: 'follow_up', approval: 'auto' })).ok, true);
  assert.equal((await submit3(f, thread.id, 'f2', 'Third task.', { when_busy: 'follow_up' })).ok, true);
  assert.deepEqual((await watch3(f, thread.id)).queued.map(item => [item.request_id, item.approval]), [['f1', 'auto'], ['f2', 'ask']]);
  // Cancelling removes one input; a second cancel finds nothing.
  const cancelled = await v3<{ thread: Thread }>(f.store, { op: 'cancel_queued', thread_id: thread.id, request_id: 'f2' });
  assert.deepEqual(cancelled.result.thread.queued.map(item => item.request_id), ['f1']);
  assert.equal((await v3(f.store, { op: 'cancel_queued', thread_id: thread.id, request_id: 'f2' })).error?.code, 'not_found');
  assert.equal((await v3(f.store, { op: 'cancel_queued', thread_id: thread.id, request_id: 's1' })).error?.code, 'not_found');
  // The automatic mode waits with its input: the active run still asks.
  assert.equal((await watch3(f, thread.id)).approval_mode, 'ask');
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'before\n');
  await f.approve(thread.id, pending.pending_approval!.id);
  await settled(f, thread.id, 'f1');
  const done = await until(() => watch3(f, thread.id), value => value.status === 'idle');
  assert.deepEqual([done.approval_mode, done.pending_approval, done.queued], ['auto', null, []]);
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'followup\n');
  assert(!done.messages.some(message => message.id === 'request-f2'));
  // A cancelled input's ID stays used, and repeating it starts nothing.
  assert.equal((await submit3(f, thread.id, 'f2', 'Third task.', { when_busy: 'follow_up' })).ok, true);
  await delay(300);
  assert.deepEqual((await watch3(f, thread.id)).messages.map(message => message.id), done.messages.map(message => message.id));
});

test('Stop clears the queue, and the queue is bounded', { timeout: 40000 }, async t => {
  const f = await setup(t, 'edit'); const thread = await f.create();
  await submit3(f, thread.id, 's1', 'First task.');
  await until(() => watch3(f, thread.id), value => !!value.pending_approval);
  for (let index = 0; index < 16; index++) {
    assert.equal((await submit3(f, thread.id, `q${index}`, `Queued ${index}`, { when_busy: index % 2 ? 'steer' : 'follow_up', approval: 'auto' })).ok, true);
  }
  assert.equal((await submit3(f, thread.id, 'q16', 'One too many', { when_busy: 'follow_up' })).error?.code, 'busy');
  assert.equal((await watch3(f, thread.id)).queued.length, 16);
  const stopped = await v3<{ thread: Thread }>(f.store, { op: 'stop', thread_id: thread.id });
  assert.deepEqual([stopped.result.thread.status, stopped.result.thread.queued], ['stopped', []]);
  assert.equal((await settled(f, thread.id, 's1')).status, 'stopped');
  await delay(400);
  const after = await watch3(f, thread.id);
  assert.deepEqual([after.status, after.queued], ['stopped', []]);
  assert.deepEqual(after.messages.map(message => message.id), ['request-s1']);
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'before\n');
  // A message sent after Stop runs as soon as cleanup allows.
  assert.equal((await submit3(f, thread.id, 'next', 'After the stop.', { when_busy: 'follow_up' })).ok, true);
  const done = await until(() => watch3(f, thread.id), value => value.messages.some(message => message.id === 'request-next') && (value.status === 'idle' || !!value.pending_approval));
  assert(!done.messages.some(message => message.id.startsWith('request-q')));
});

test('queued inputs survive a restart, stay held, and resume in order', { timeout: 40000 }, async t => {
  const f = await setup(t, 'edit'); const thread = await f.create();
  await submit3(f, thread.id, 's1', 'First task.');
  await until(() => watch3(f, thread.id), value => !!value.pending_approval);
  await submit3(f, thread.id, 'f1', 'Queued before the crash.', { when_busy: 'follow_up' });
  f.host.kill('SIGKILL'); await delay(400); await f.start('answer');
  const held = await watch3(f, thread.id);
  assert.deepEqual([held.status, held.queued.map(item => item.request_id)], ['interrupted', ['f1']]);
  await delay(500);
  // Nothing starts by itself after a restart.
  assert.deepEqual(await watch3(f, thread.id), held);
  // Joining the held queue resumes it from its head, each input as its own run.
  assert.equal((await submit3(f, thread.id, 'f2', 'Sent after the restart.', { when_busy: 'follow_up' })).ok, true);
  await settled(f, thread.id, 'f2');
  const done = await until(() => watch3(f, thread.id), value => value.status === 'idle');
  assert.deepEqual(done.queued, []);
  assert.deepEqual(done.messages.map(message => message.role === 'user' ? message.id : 'assistant'), ['request-s1', 'request-f1', 'assistant', 'request-f2', 'assistant']);
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'before\n');
});

test('a steering input joins the active run under that run\'s authorization', { timeout: 40000 }, async t => {
  const f = await setup(t, 'edit'); const thread = await f.create();
  await submit3(f, thread.id, 's1', 'First task.');
  const pending = await until(() => watch3(f, thread.id), value => !!value.pending_approval);
  assert.equal((await submit3(f, thread.id, 'st1', 'Also keep it short.', { when_busy: 'steer' })).ok, true);
  assert.deepEqual((await watch3(f, thread.id)).queued.map(item => [item.request_id, item.mode]), [['st1', 'steer']]);
  // Steering does not answer the pending approval or change the file.
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'before\n');
  assert.equal((await watch3(f, thread.id)).pending_approval!.id, pending.pending_approval!.id);
  await f.approve(thread.id, pending.pending_approval!.id);
  const done = await until(() => watch3(f, thread.id), value => value.status === 'idle');
  // One run took it: no second run was started for the steering input.
  assert.equal((await settled(f, thread.id, 's1')).status, 'idle');
  await delay(300);
  assert.equal(JSON.parse(await readFile(join(f.store, `${thread.id}.settled`), 'utf8')).request_id, 's1');
  assert.deepEqual(done.queued, []);
  const items = (await history(f, thread.id)).flat();
  assert.deepEqual(items.map(item => item.type === 'message' && item.role === 'assistant' ? 'assistant' : item.id),
    ['request-s1', 'read-1', 'edit-1', 'request-st1', 'assistant']);
  assert.deepEqual(done.messages.map(message => message.role === 'user' ? message.id : 'assistant'), ['request-s1', 'request-st1', 'assistant']);
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'after\n');
  // It can no longer be cancelled, and repeating its ID confirms it without a second copy.
  assert.equal((await v3(f.store, { op: 'cancel_queued', thread_id: thread.id, request_id: 'st1' })).error?.code, 'not_found');
  assert.equal((await submit3(f, thread.id, 'st1', 'Also keep it short.', { when_busy: 'steer' })).ok, true);
  assert.equal((await watch3(f, thread.id)).messages.filter(message => message.id === 'request-st1').length, 1);
});

test('a transient provider error retries the model request and never repeats a tool effect', { timeout: 40000 }, async t => {
  const f = await setup(t, 'v3-retry'); const thread = await f.create();
  const live = stream<{ result?: { thread: Thread }; thread?: Thread }>(f.store, { op: 'watch', thread_id: thread.id, protocol_version: 3 });
  t.after(async () => { live.socket.destroy(); });
  await submit3(f, thread.id, 's1', 'Edit, then answer.', { approval: 'auto' });
  const done = await until(() => watch3(f, thread.id), value => value.status === 'idle' || value.status === 'error');
  assert.equal(done.status, 'idle', JSON.stringify(done.error));
  assert.deepEqual(done.tools.map(tool => [tool.id, tool.status]), [['retry-edit', 'done']]);
  assert.equal(done.messages.at(-1)!.text, 'Recovered after a transient provider error.');
  assert.equal(await readFile(join(f.store, 'attempts.txt'), 'utf8'), 'effect\n');
  assert.equal(await readFile(join(f.repo, 'notes.txt'), 'utf8'), 'after\n');
  assert.deepEqual([done.retry, done.error], [null, null]);
  const frames = live.frames.map(frame => frame.thread ?? frame.result!.thread);
  assert(frames.some(frame => frame.retry?.attempt === 1 && frame.retry.at > 0), 'no retry frame');
  assert(!JSON.stringify(live.frames).includes('SECRET_PROVIDER_TOKEN'));
  assert(!JSON.stringify(await history(f, thread.id)).includes('SECRET_PROVIDER_TOKEN'));
});

test('retries are bounded and a persistent provider error stays private', { timeout: 40000 }, async t => {
  const f = await setup(t, 'v3-retry-exhausted'); const thread = await f.create();
  await submit3(f, thread.id, 's1', 'This will fail.');
  const failed = await until(() => watch3(f, thread.id), value => value.status === 'error');
  assert.equal(failed.error!.code, 'runtime_error');
  assert.match(failed.error!.message, /model_error/);
  assert(!JSON.stringify(failed).includes('SECRET_PROVIDER_TOKEN'));
  // Three requests were made: the first and two retries. The next run gets the fourth scripted error.
  await submit3(f, thread.id, 's2', 'Again.');
  await settled(f, thread.id, 's2');
  await submit3(f, thread.id, 's3', 'And again.');
  const answered = await until(() => watch3(f, thread.id), value => value.status === 'idle' || value.messages.some(message => message.role === 'assistant'), 20000);
  assert(answered.messages.some(message => message.role === 'assistant'));
});

test('the turn limit pauses the run with a distinct error and the next message continues', { timeout: 40000 }, async t => {
  const f = await setup(t, 'v3-turns'); const thread = await f.create();
  await submit3(f, thread.id, 's1', 'Read repeatedly.');
  const paused = await until(() => watch3(f, thread.id), value => value.status === 'error');
  assert.equal(paused.error!.code, 'turn_limit');
  assert.deepEqual(paused.tools.map(tool => [tool.id, tool.status]), [['turn-0', 'done'], ['turn-1', 'done']]);
  const older = (await request(f.store, { op: 'watch', thread_id: thread.id, protocol_version: 2 })).result.thread;
  assert.deepEqual(older.error, { code: 'runtime_error', message: 'Agent ended without an answer (turn_limit). Inspect the transcript before retrying.' });
  await settled(f, thread.id, 's1');
  await submit3(f, thread.id, 's2', 'Continue.');
  const again = await until(() => watch3(f, thread.id), value => value.status === 'error' && value.tools.length === 4);
  assert.equal(again.error!.code, 'turn_limit');
});
