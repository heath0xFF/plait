import assert from 'node:assert/strict';
import { mkdir, open, readFile, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { userInfo } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { loginEnvironment } from '../src/command/login-environment.ts';
import type { Account } from '../src/command/login-environment.ts';
import { trustedCommand } from '../src/command/trusted.ts';
import type { TrustedRun } from '../src/command/trusted.ts';
import { COMMAND_TIMEOUT_MS } from '../src/command/supervisor.ts';
import { fileAction, pathKey, rootIdentity, sameImage } from '../src/safety.ts';
import { exec, setup } from './support/safety.ts';

// These commands really run unsandboxed. Each uses a disposable checkout and a
// disposable HOME, and the developer's SSH agent stays out of their environment.
delete process.env.SSH_AUTH_SOCK;

const git = (root: string, ...args: string[]) => exec('/usr/bin/git', ['-C', root, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@invalid', ...args],
  { env: { PATH: '/usr/bin:/bin', HOME: '/nonexistent', GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null' } });
const alive = (pid: number) => { try { process.kill(pid, 0); return true; } catch { return false; } };
async function gone(pid: number) {
  for (let attempt = 0; attempt < 100 && alive(pid); attempt++) await new Promise(resolve => setTimeout(resolve, 20));
  return !alive(pid);
}

async function fixture(t: { after: (fn: () => Promise<void>) => void }) {
  const { root, base } = await setup(t);
  const home = join(base, 'home'); await mkdir(home);
  await writeFile(join(home, '.gitconfig'), '[user]\n\tname = Trusted Fixture\n\temail = trusted@invalid\n');
  const account: Account = { home, shell: '/bin/zsh', user: userInfo().username };
  const run: TrustedRun = { snapshot: { checkout: root }, baseline: [], owned: new Map(), rootIdentity: await rootIdentity(root) };
  const execute = (text: string, signal = new AbortController().signal, output: (chunk: string) => void = () => {}, timeoutMs?: number) =>
    trustedCommand(run, text, signal, output, { account, timeoutMs });
  return { root, base, home, account, run, execute };
}

test('trusted command writes the checkout, commits with fixture Git identity, and reaches loopback', { timeout: 30000 }, async t => {
  const f = await fixture(t);
  assert.equal((await f.execute('pwd')).trim(), f.root);
  assert.equal((await f.execute('printf %s "$HOME"')), f.home);
  const log = await f.execute('printf "made by a command\\n" > created.txt && git add created.txt && git commit -qm "trusted commit" && git log -1 --format="%an <%ae> %s"');
  assert.equal(log.trim(), 'Trusted Fixture <trusted@invalid> trusted commit');
  assert.equal(await readFile(join(f.root, 'created.txt'), 'utf8'), 'made by a command\n');
  assert.equal((await git(f.root, 'status', '--porcelain')).stdout, '');
  let hits = 0;
  const server = createServer((_request, response) => { hits++; response.end('LOOPBACK_OK'); });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    const port = (server.address() as { port: number }).port;
    assert.match(await f.execute(`/usr/bin/curl --silent --max-time 5 http://127.0.0.1:${port}`), /LOOPBACK_OK/);
    assert.equal(hits, 1);
  } finally { await new Promise<void>(resolve => server.close(() => resolve())); }
});

test('login-shell startup files supply the environment; host plumbing and descriptors stay out', { timeout: 30000 }, async t => {
  const f = await fixture(t);
  for (const [directory, tool] of [['login-bin', 'login-tool'], ['rc-bin', 'rc-tool']]) {
    await mkdir(join(f.home, directory));
    await writeFile(join(f.home, directory, tool), `#!/bin/sh\nprintf '%s ' ${tool.toUpperCase()}\n`, { mode: 0o700 });
  }
  await writeFile(join(f.home, '.zprofile'), 'export PATH="$HOME/login-bin:$PATH"\n');
  // Noise and a failing last line must not disturb the capture.
  await writeFile(join(f.home, '.zshrc'), 'export PATH="$HOME/rc-bin:$PATH"\nexport FIXTURE_FROM_RC=present\necho STARTUP NOISE\nfalse\n');
  const lease = await open(f.root, 'r');
  t.after(async () => { await lease.close(); });
  f.run.lease = lease;
  process.env.PLAIT_NOTIFY_FD = '9';
  t.after(async () => { delete process.env.PLAIT_NOTIFY_FD; });
  const result = await f.execute('login-tool && rc-tool && env && fd=3; while [ $fd -lt 64 ]; do if [ -e /dev/fd/$fd ]; then echo "OPEN_DESCRIPTOR $fd"; fi; fd=$((fd+1)); done; echo DESCRIPTORS_CHECKED');
  assert.match(result, /DESCRIPTORS_CHECKED/);
  assert.match(result, /LOGIN-TOOL RC-TOOL /);
  assert.match(result, /^FIXTURE_FROM_RC=present$/m);
  assert.match(result, new RegExp(`^HOME=${f.home}$`, 'm'));
  assert.doesNotMatch(result, /PLAIT_|NODE_TEST_CONTEXT|STARTUP NOISE|OPEN_DESCRIPTOR/);
  // The environment is read once per run, not once per command.
  assert.equal((await f.run.loginEnvironment)!.source, 'login-shell');
  await writeFile(join(f.home, '.zshrc'), 'export FIXTURE_FROM_RC=changed\n');
  assert.match(await f.execute('printf %s "$FIXTURE_FROM_RC"'), /^present$/);
});

test('an unreadable login shell falls back to a fixed PATH and says so', { timeout: 30000 }, async t => {
  const f = await fixture(t);
  const broken = { ...f.account, shell: '/usr/bin/false' };
  const environment = await loginEnvironment(broken, new AbortController().signal);
  assert.equal(environment.source, 'fallback');
  assert.equal(environment.env.PATH, `${f.home}/.cargo/bin:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin`);
  await writeFile(join(f.home, '.zshrc'), 'sleep 30\n');
  const slow = await loginEnvironment(f.account, new AbortController().signal, 300);
  assert.equal(slow.source, 'fallback');
  const result = await trustedCommand(f.run, 'printf ran', new AbortController().signal, () => {}, { account: broken });
  assert.equal(result, 'Plait: the login-shell environment could not be read; using a minimal PATH.\nran');
});

test('Stop kills a long trusted command and its children', { timeout: 30000 }, async t => {
  const f = await fixture(t);
  const controller = new AbortController();
  let pids: number[] = [];
  await assert.rejects(f.execute('sleep 120 & printf "READY %s %s\\n" "$$" "$!"; wait', controller.signal, chunk => {
    const match = chunk.match(/READY (\d+) (\d+)/);
    if (match) { pids = [Number(match[1]), Number(match[2])]; controller.abort(); }
  }), { code: 'permission_denied' });
  assert.equal(pids.length, 2);
  for (const pid of pids) assert(await gone(pid), `process ${pid} survived Stop`);
  // A stopped run does not start another command.
  await assert.rejects(f.execute('printf late > late.txt', controller.signal), /Run stopped/);
  await assert.rejects(readFile(join(f.root, 'late.txt')), { code: 'ENOENT' });
});

test('the deadline and output bounds apply to trusted commands', { timeout: 30000 }, async t => {
  const f = await fixture(t);
  assert.equal(COMMAND_TIMEOUT_MS, 900000);
  let pid = 0;
  await assert.rejects(f.execute('printf "READY %s\\n" "$$"; sleep 120', undefined, chunk => {
    const match = chunk.match(/READY (\d+)/);
    if (match) pid = Number(match[1]);
  }, 700), /Command ended without success/);
  assert(pid > 0); assert(await gone(pid));
  const chunks: string[] = [];
  const script = 'process.stdout.write("🚀"+"x".repeat(70000)+"🚀FINAL")';
  const result = await f.execute(`'${process.execPath}' -e '${script}'`, undefined, chunk => chunks.push(chunk));
  assert(result.endsWith('🚀FINAL'));
  assert(result.length <= 16384);
  for (const text of [result, ...chunks]) assert.equal(Buffer.from(text).toString(), text);
  assert(chunks.every(chunk => chunk.length <= 4096));
  const split = 'process.stdout.write(Buffer.from([0xe2]));process.stderr.write(Buffer.from([0xf0,0x9f]));setTimeout(()=>{process.stdout.write(Buffer.from([0x82,0xac]));process.stderr.write(Buffer.from([0x9a,0x80]));},30);';
  const decoded = await f.execute(`'${process.execPath}' -e '${split}'`);
  assert(decoded.includes('€')); assert(decoded.includes('🚀')); assert(!decoded.includes('�'));
});

/** Commits the named files, then makes the thread their owner as the edit tool would. */
async function own(f: Awaited<ReturnType<typeof fixture>>, names: string[]) {
  const recorded: string[] = []; const forgotten: string[] = [];
  f.run.record = image => { recorded.push(image.path); };
  f.run.forget = path => { forgotten.push(path); };
  for (const name of names) await writeFile(join(f.root, name), 'before\n');
  await git(f.root, 'add', '.'); await git(f.root, 'commit', '-qm', 'fixture');
  for (const name of names) {
    const current = await fileAction(f.root, 'read', name);
    const written = await fileAction(f.root, 'edit', name, { expected: 'before', content: 'thread', expected_image: current.postimage });
    f.run.owned.set(pathKey(name), written.postimage);
  }
  return { recorded, forgotten };
}
const image = async (root: string, name: string) => (await fileAction(root, 'read', name)).postimage;

test('a file this thread owned stays owned, with a fresh record, when its trusted command changes it', { timeout: 30000 }, async t => {
  const f = await fixture(t);
  const { recorded, forgotten } = await own(f, ['formatted.txt', 'kept.txt', 'removed.txt']);
  const before = new Map(f.run.owned);
  await f.execute('printf "by command\\n" >> formatted.txt && rm removed.txt && printf "by command\\n" > generated.txt');
  // The untouched record is the same one; the removed file and the command's new file are not owned.
  assert.deepEqual([...f.run.owned.keys()].sort(), ['formatted.txt', 'kept.txt']);
  assert.equal(f.run.owned.get('kept.txt'), before.get('kept.txt'));
  const fresh = f.run.owned.get('formatted.txt')!;
  assert.notEqual(fresh.sha256, before.get('formatted.txt')!.sha256);
  assert(sameImage(fresh, await image(f.root, 'formatted.txt')));
  assert.deepEqual(recorded, ['formatted.txt']);
  assert.deepEqual(forgotten.sort(), ['formatted.txt', 'removed.txt']);
  // The fresh record is what the next edit is checked against, and the old one no longer passes.
  await assert.rejects(fileAction(f.root, 'edit', 'formatted.txt', { expected: 'by command', content: 'stale', expected_image: before.get('formatted.txt') }));
  await fileAction(f.root, 'edit', 'formatted.txt', { expected: 'by command', content: 'by thread', expected_image: fresh });
  assert.equal(await readFile(join(f.root, 'formatted.txt'), 'utf8'), 'thread\nby thread\n');
  // A failing command and an atomic replacement are refreshed the same way.
  await assert.rejects(f.execute('printf "replaced\\n" > kept.tmp && mv kept.tmp kept.txt; exit 3'), /Command ended without success \(1\)/);
  const replaced = f.run.owned.get('kept.txt')!;
  assert.notEqual(replaced.ino, before.get('kept.txt')!.ino);
  assert(sameImage(replaced, await image(f.root, 'kept.txt')));
});

test('a file that changed before the command started is dropped, not refreshed', { timeout: 30000 }, async t => {
  const f = await fixture(t);
  const { recorded, forgotten } = await own(f, ['outside.txt', 'outside-then-command.txt', 'command-only.txt']);
  await writeFile(join(f.root, 'dirty.txt'), 'user work\n');
  f.run.baseline = ['dirty.txt'];
  // Something other than this thread's command changes two owned files first.
  await writeFile(join(f.root, 'outside.txt'), 'someone else\n');
  await writeFile(join(f.root, 'outside-then-command.txt'), 'someone else\n');
  await f.execute('for name in outside-then-command.txt command-only.txt dirty.txt; do printf "by command\\n" >> "$name"; done');
  assert.deepEqual([...f.run.owned.keys()], ['command-only.txt']);
  assert.deepEqual(recorded, ['command-only.txt']);
  assert.deepEqual(forgotten.sort(), ['command-only.txt', 'outside-then-command.txt', 'outside.txt']);
  assert.equal(await readFile(join(f.root, 'outside-then-command.txt'), 'utf8'), 'someone else\nby command\n');
  // A later command cannot bring a dropped file, or a pre-existing dirty one, into ownership.
  await f.execute('printf "again\\n" >> outside.txt && printf "again\\n" >> dirty.txt');
  assert.deepEqual([...f.run.owned.keys()], ['command-only.txt']);
  assert.deepEqual(recorded, ['command-only.txt']);
});

test('a trusted command that changes a pre-existing dirty path is reported, not prevented', { timeout: 30000 }, async t => {
  const f = await fixture(t);
  await writeFile(join(f.root, 'dirty.txt'), 'user work\n');
  await writeFile(join(f.root, 'other.txt'), 'user work\n');
  f.run.baseline = ['dirty.txt', 'other.txt'];
  assert.equal(await f.execute('cat dirty.txt'), 'user work\n');
  const result = await f.execute('printf "by command\\n" >> dirty.txt && printf done');
  assert.equal(result, 'Plait: paths that were dirty before this run changed during this command: dirty.txt.\ndone');
  await assert.rejects(f.execute('rm other.txt; exit 2'), (error: Error) =>
    error.message.startsWith('Plait: paths that were dirty before this run changed during this command: other.txt.\nCommand ended without success'));
});
