import assert from 'node:assert/strict';
import { test } from 'node:test';
import { DatabaseSync } from 'node:sqlite';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { setup, request, until, delay } from './support/host.ts';

test('a failed command has durable paged diagnostics after host restart without rerunning it', { timeout: 30000 }, async t => {
  const f = await setup(t);
  const script = (steps: unknown[]) => writeFile(join(f.store, 'script.json'), JSON.stringify(steps));
  await script([
    { name: 'command', args: { command: "awk 'BEGIN {print \"EARLY_DIAGNOSTIC\"; for (i=0;i<25000;i++) print \"output line\"; print \"FINAL_DIAGNOSTIC\"; exit 3}'" }, id: 'long-build' },
    { text: 'The scripted build failed.' },
  ]);
  f.host.kill('SIGKILL'); await delay(400); const first = await f.start('self-host-script');
  const thread = await f.create();
  assert.equal((await request(f.store, { op: 'submit', thread_id: thread.id, request_id: 'build', text: 'Run the build.', protocol_version: 3, approval: 'auto' })).ok, true);
  const done = await until(() => f.watch(thread.id), value => value.status === 'idle');
  assert.equal(done.tools.find(tool => tool.id === 'long-build')?.status, 'error');
  const output = done.tools.find(tool => tool.id === 'long-build')!.output;
  assert.match(output, /FINAL_DIAGNOSTIC/);
  const id = output.match(/Command output: ([0-9a-f-]+)/)?.[1]; assert(id, output);
  const db = new DatabaseSync(join(f.store, 'host.sqlite'), { readOnly: true });
  const bytes = Number(db.prepare('SELECT bytes FROM command_outputs WHERE id=?').get(id)?.bytes);
  assert(bytes > 250000); db.close();
  await script([
    { name: 'read_output', args: { id, offset: 0, limit: 12000 }, id: 'early-log' },
    { name: 'read_output', args: { id, offset: bytes - 100 }, id: 'late-log' },
    { text: 'Retrieved the original diagnostics.' },
  ]);
  first.kill('SIGKILL'); await delay(400); await f.start('self-host-script');
  await f.submit(thread.id, 'inspect-log', 'Read the previous build output.');
  await until(() => f.watch(thread.id), value => value.status === 'idle' && value.tools.some(tool => tool.id === 'late-log'));
  const history = await request(f.store, { op: 'history', thread_id: thread.id, protocol_version: 3 });
  const items = (history.result as unknown as { items: { type: string; id: string; output?: string }[] }).items;
  assert.match(items.find(item => item.id === 'early-log')?.output ?? '', /^EARLY_DIAGNOSTIC/);
  assert.match(items.find(item => item.id === 'late-log')?.output ?? '', /FINAL_DIAGNOSTIC/);
  const reopened = new DatabaseSync(join(f.store, 'host.sqlite'), { readOnly: true });
  assert.equal(reopened.prepare('SELECT count(*) AS n FROM command_outputs').get()?.n, 1);
  reopened.close();
});
