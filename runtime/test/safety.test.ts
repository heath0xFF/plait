import assert from 'node:assert/strict';
import { execFile, spawn } from 'node:child_process';
import { mkdtemp, mkdir, writeFile, readFile, realpath, rm, symlink, link, chmod, stat, rename, readdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { promisify } from 'node:util';
import { test } from 'node:test';
import { createServer } from 'node:http';
import { createServer as unixServer } from 'node:net';
import { command, fileAction, dirtyPaths, identities, safePath, acquireCheckout, rootIdentity, checkRoot, COMMAND_TIMEOUT_MS } from '../src/safety.ts';
import { prepareBuildCache } from '../src/build-cache.ts';
import { validate } from '../src/protocol.ts';
import type { Snapshot } from '../src/protocol.ts';
import { boundSnapshot } from '../src/transcript.ts';
import { restoreOwned } from '../src/provenance.ts';
import { createHash, randomUUID } from 'node:crypto';
const exec = promisify(execFile);
process.env.PLAIT_HOST_EXE = resolve('../target/debug/plait-host');

async function setup(t: { after: (fn: () => Promise<void>) => void }) {
  const base = await realpath(await mkdtemp(join(tmpdir(), 'plait-sandbox-')));
  const root = join(base, 'repo'); await mkdir(root);
  t.after(async () => { await rm(base, { recursive: true, force: true }); });
  await exec('/usr/bin/git', ['init', '-q', root]);
  return { root, base };
}

test('snapshot budget counts UTF-8 and JSON escapes without clipping approval arguments', () => {
  const snapshot: Snapshot = { id: 'x', checkout: '/repo', model: { provider: 'faux', model_id: 'faux-1' }, status: 'idle',
    messages: Array.from({ length: 100 }, (_, index) => ({ id: String(index), role: 'assistant', text: '─'.repeat(4096) })),
    tools: Array.from({ length: 50 }, (_, index) => ({ id: String(index), name: 'read', status: 'done', input: '\u0000'.repeat(4096), output: '─'.repeat(4096) })),
    pending_approval: { id: 'a', tool_id: 't', kind: 'edit', summary: 'change', details: '─'.repeat(4000), dirty_paths: [], risk: 'exact' }, error: null, truncated: false, live_text: '─'.repeat(16384) };
  const details = snapshot.pending_approval!.details;
  boundSnapshot(snapshot);
  assert(Buffer.byteLength(JSON.stringify(snapshot)) <= 524288);
  assert(snapshot.truncated); assert.equal(snapshot.pending_approval!.details, details);
});

test('strict request admission', () => {
  for (const input of [null, [], {}, { op: 'watch' }, { op: 'status', unknown: 1 }, { op: 'submit', thread_id: '../bad', request_id: 'x', text: 'hello' }]) {
    assert.throws(() => validate(input));
  }
  assert.deepEqual(validate({ op: 'status' }), { op: 'status' });
});

test('sandbox reuses compiled artifacts across commands in a private thread cache', { timeout: 30000 }, async t => {
  const { root, base } = await setup(t);
  const store = join(base, 'store'); await mkdir(store, { mode: 0o700 });
  const cache = { store, thread: randomUUID() };
  await mkdir(join(root, 'src'));
  await writeFile(join(root, 'Cargo.toml'), '[package]\nname="plait-sandbox-proof"\nversion="0.1.0"\nedition="2024"\n[dependencies]\nitoa="=1.0.19"\n');
  await writeFile(join(root, 'rust-toolchain.toml'), '[toolchain]\nchannel="1.98.1"\n');
  await writeFile(join(root, 'src/lib.rs'), '#[test] fn works() { assert_eq!(2 + 2, 4); }\n');
  await exec('/Users/heath/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin/cargo', ['generate-lockfile', '--offline'], { cwd: root });
  const run = (text: string) => command(root, text, [], new AbortController().signal, () => {}, undefined, undefined, cache);
  assert.match(await run('cargo build --offline --locked && cargo test --offline --locked'), /1 passed/);
  const { target } = await prepareBuildCache(cache);
  const deps = join(target, 'debug', 'deps');
  const artifact = join(deps, (await readdir(deps)).find(name => name.startsWith('libitoa-') && name.endsWith('.rlib'))!);
  const before = await stat(artifact, { bigint: true });
  const warm = await run('cargo test --offline --locked --verbose');
  assert.match(warm, /Fresh itoa/); assert.doesNotMatch(warm, /Compiling/);
  assert.equal((await stat(artifact, { bigint: true })).mtimeNs, before.mtimeNs);
});

test('thread caches deny sibling/store access, symlink escapes and root replacement', { timeout: 30000 }, async t => {
  const { root, base } = await setup(t);
  const store = join(base, 'store'); await mkdir(store, { mode: 0o700 });
  const cache = { store, thread: randomUUID() };
  const first = await prepareBuildCache(cache);
  const sibling = await prepareBuildCache({ store, thread: randomUUID() });
  const secret = join(store, 'auth.json'); await writeFile(secret, 'private-fixture');
  const other = join(sibling.target, 'keep'); await writeFile(other, 'keep');
  const run = (text: string) => command(root, text, [], new AbortController().signal, () => {}, undefined, undefined, cache);
  await run('printf cached > "$CARGO_TARGET_DIR/own"');
  assert.equal(await readFile(join(first.target, 'own'), 'utf8'), 'cached');
  for (const path of [secret, other]) {
    await assert.rejects(run(`cat '${path}'`));
    await assert.rejects(run(`printf bad > '${path}'`));
  }
  await run(`ln -s '${sibling.target}' "$CARGO_TARGET_DIR/escape"`);
  await assert.rejects(run('printf bad > "$CARGO_TARGET_DIR/escape/keep"'));
  await assert.rejects(run('mv "$CARGO_TARGET_DIR" "$TMPDIR/stolen"'));
  await assert.rejects(run('mv "$CARGO_HOME" "$TMPDIR/stolen"'));
  await assert.rejects(run('chmod 777 "$CARGO_HOME"'));
  assert.equal(await readFile(other, 'utf8'), 'keep');
  assert.equal(await readFile(secret, 'utf8'), 'private-fixture');
});

test('cache preparation rejects symlink roots, insecure ownership modes and path IDs', async t => {
  const { base } = await setup(t);
  const store = join(base, 'store'); await mkdir(store, { mode: 0o700 });
  const cache = { store, thread: randomUUID() };
  await assert.rejects(prepareBuildCache({ store, thread: '../other' }));
  const { target } = await prepareBuildCache(cache);
  await rm(target, { recursive: true }); await symlink(base, target);
  await assert.rejects(prepareBuildCache(cache), /symlink/);
  await rm(target); await mkdir(target, { mode: 0o755 });
  await assert.rejects(prepareBuildCache(cache), /private/);
  await symlink(store, join(base, 'alias'));
  await assert.rejects(prepareBuildCache({ store: join(base, 'alias'), thread: randomUUID() }), /symlink/);
});

test('Stop kills a command promptly under the fifteen-minute cap and leaves cache usable', { timeout: 15000 }, async t => {
  const { root, base } = await setup(t);
  const store = join(base, 'store'); await mkdir(store, { mode: 0o700 });
  const cache = { store, thread: randomUUID() };
  assert.equal(COMMAND_TIMEOUT_MS, 900000);
  const controller = new AbortController();
  let pid = 0;
  await assert.rejects(command(root, 'echo READY $$; sleep 120', [], controller.signal, chunk => {
    const match = chunk.match(/READY (\d+)/);
    if (match) { pid = Number(match[1]); controller.abort(); }
  }, undefined, undefined, cache));
  assert(pid > 0); assert.throws(() => process.kill(pid, 0));
  assert.match(await command(root, 'printf usable', [], new AbortController().signal, () => {}, undefined, undefined, cache), /usable/);
});

test('sandbox compiles and links Metal with installed tools and no discovery services', { timeout: 30000 }, async t => {
  const { root } = await setup(t);
  const result = await command(root, `printf '#include <metal_stdlib>\\nusing namespace metal;\\nkernel void noop() {}\\n' > "$TMPDIR/input.metal" && xcrun -sdk "$SDKROOT" metal -c "$TMPDIR/input.metal" -o "$TMPDIR/out.air" && xcrun -sdk "$SDKROOT" metallib "$TMPDIR/out.air" -o "$TMPDIR/out.metallib" && test -s "$TMPDIR/out.metallib" && printf METAL_OK`, [], new AbortController().signal, () => {});
  assert.match(result, /METAL_OK/);
});

test('sandbox denies outside/symlink writes, Git writes, network and baseline dirty commands', { timeout: 15000 }, async t => {
  const { root, base } = await setup(t);
  const outside = join(base, 'protected.txt'); await writeFile(outside, 'keep');
  await symlink(base, join(root, 'escape'));
  await writeFile(join(root, 'source.txt'), 'source');
  const run = (text: string, baseline: string[] = []) => command(root, text, baseline, new AbortController().signal, () => {});
  await assert.rejects(run(`printf bad > '${outside}'`));
  await assert.rejects(run('printf bad > escape/protected.txt'));
  await assert.rejects(run('printf bad > .git/config'));
  await assert.rejects(run('printf bad > .GIT/config'));
  await assert.rejects(run('printf bad > .Git/config'));
  await assert.rejects(run('ln source.txt "$TMPDIR/linked" && printf bad > "$TMPDIR/linked"'));
  assert.equal(await readFile(join(root, 'source.txt'), 'utf8'), 'source');
  await assert.rejects(run('printf bad > anything', ['notes.txt']));
  assert.match(await run('printf safe', ['notes.txt']), /safe/);
  assert.equal(await readFile(outside, 'utf8'), 'keep');
  let hits = 0;
  const server = createServer((_request, response) => { hits++; response.end('unsafe'); });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  try {
    const port = (server.address() as { port: number }).port;
    await assert.rejects(run(`/usr/bin/curl --max-time 2 http://127.0.0.1:${port}`));
    assert.equal(hits, 0);
  } finally { await new Promise<void>(resolve => server.close(() => resolve())); }
});

test('scratch Git and child signals work without checkout writes or external signals', { timeout: 15000 }, async t => {
  const { root } = await setup(t);
  const sentinel = spawn('/bin/sleep', ['30'], { stdio: 'ignore' });
  t.after(() => { sentinel.kill('SIGKILL'); });
  const run = (text: string) => command(root, text, [], new AbortController().signal, () => {});
  assert.match(await run('git init -q "$TMPDIR/fixture" && git -C "$TMPDIR/fixture" status --porcelain && printf SCRATCH_GIT_OK'), /SCRATCH_GIT_OK/);
  await assert.rejects(run(`ln -s '${root}/.git' "$TMPDIR/git-alias"; printf bad > "$TMPDIR/git-alias/config"`));
  await assert.rejects(run(`kill -TERM ${sentinel.pid}`));
  process.kill(sentinel.pid!, 0);
  assert.match(await run('sleep 5 & pid=$!; kill -TERM "$pid"; wait "$pid"; test $? -gt 128 && printf CHILD_STOPPED'), /CHILD_STOPPED/);
});

test('Unix IPC stays in scratch and cannot connect to host or sibling cache sockets', { timeout: 15000 }, async t => {
  const { root } = await setup(t);
  const store = await realpath(await mkdtemp('/tmp/p-ipc-'));
  t.after(async () => { await rm(store, { recursive: true, force: true }); });
  const cache = { store, thread: randomUUID() };
  const sibling = await prepareBuildCache({ store, thread: randomUUID() });
  const run = (text: string) => command(root, text, [], new AbortController().signal, () => {}, undefined, undefined, cache);
  const script = 'const net=require("net");const d=process.env.TMPDIR+"/plait-client-"+process.pid;require("fs").mkdirSync(d);const p=d+"/host.sock";const s=net.createServer(c=>{c.end("LOCAL_IPC_OK");s.close()});s.listen(p,()=>{net.connect(p).on("data",d=>process.stdout.write(d))});';
  assert.match(await run(`'${process.execPath}' -e '${script}'`), /LOCAL_IPC_OK/);
  for (const path of [join(store, 'host.sock'), join(sibling.target, 's')]) {
    let hits = 0;
    const server = unixServer(socket => { hits++; socket.destroy(); });
    await new Promise<void>((resolve, reject) => { server.once('error', reject); server.listen(path, resolve); });
    try {
      await assert.rejects(run(`/usr/bin/nc -z -w 1 -U '${path}'`));
      assert.equal(hits, 0);
    } finally { await new Promise<void>(resolve => server.close(() => resolve())); }
  }
});

test('file helpers reject hard links, symlink parents, duplicate replacement and overwrite', async t => {
  const { root, base } = await setup(t);
  await writeFile(join(root, 'file.txt'), 'same same');
  await assert.rejects(fileAction(root, 'edit', 'file.txt', { expected: 'same', content: 'bad' }));
  await assert.rejects(fileAction(root, 'write', 'file.txt', { content: 'bad' }));
  await link(join(root, 'file.txt'), join(base, 'hard.txt'));
  await assert.rejects(fileAction(root, 'read', 'file.txt'));
  await symlink(base, join(root, 'parent'));
  await assert.rejects(fileAction(root, 'read', 'parent/hard.txt'));
  assert.equal(await readFile(join(root, 'file.txt'), 'utf8'), 'same same');
});

test('edit opens originals read-only and reports fixed mismatch diagnostics', async t => {
  const { root } = await setup(t);
  const path = join(root, 'readonly.txt');
  await writeFile(path, 'before\n');
  await exec('/usr/bin/xattr', ['-w', 'com.plait.test', 'read-only-original', path]);
  await chmod(path, 0o444);
  const before = await stat(path, { bigint: true });
  const image = (await fileAction(root, 'read', 'readonly.txt')).postimage;
  await assert.rejects(fileAction(root, 'edit', 'readonly.txt', { expected: 'not present', content: 'bad', expected_image: image }), /old_text must match exactly once/);
  assert.equal((await stat(path, { bigint: true })).ctimeNs, before.ctimeNs);
  await fileAction(root, 'edit', 'readonly.txt', { expected: 'before', content: 'after', expected_image: image });
  assert.equal(await readFile(path, 'utf8'), 'after\n');
  assert.equal((await stat(path)).mode & 0o777, 0o444);
});

test('case and Unicode aliases cannot bypass dirty identities or Git metadata checks', async t => {
  const { root } = await setup(t);
  for (const alias of ['.GIT/config', '.Git/config', '.git/config']) await assert.rejects(fileAction(root, 'read', alias));
  await writeFile(join(root, 'notes.txt'), 'dirty');
  const protectedFiles = await identities(root, ['notes.txt']);
  assert.throws(() => safePath('NOTES.TXT', ['notes.txt']));
  await assert.rejects(fileAction(root, 'edit', 'NOTES.TXT', { expected: 'dirty', content: 'bad', protected: protectedFiles }));
  const nfc = 'caf\u00e9.txt'; const nfd = nfc.normalize('NFD');
  await writeFile(join(root, nfc), 'dirty');
  assert.throws(() => safePath(nfd, [nfc]));
  await assert.rejects(fileAction(root, 'edit', nfd, { expected: 'dirty', content: 'bad', protected: await identities(root, [nfc]) }));
  assert.equal(await readFile(join(root, nfc), 'utf8'), 'dirty');
  assert.equal(await readFile(join(root, 'notes.txt'), 'utf8'), 'dirty');
});

test('atomic edits preserve mode and reject changed approval content; replaced checkout is rejected', async t => {
  const { root, base } = await setup(t);
  await writeFile(join(root, 'code.sh'), 'before'); await chmod(join(root, 'code.sh'), 0o755);
  const before = await stat(join(root, 'code.sh'));
  const result = await fileAction(root, 'edit', 'code.sh', { expected: 'before', content: 'after', expected_file: 'before' });
  assert.equal(result.postimage.sha256, createHash('sha256').update('after').digest('hex'));
  assert.equal(result.postimage.path, 'code.sh');
  assert(!Object.hasOwn(result.postimage, 'atime'));
  const after = await stat(join(root, 'code.sh'));
  assert.notEqual(before.ino, after.ino); assert.equal(after.mode & 0o777, 0o755);
  await assert.rejects(fileAction(root, 'edit', 'code.sh', { expected: 'after', content: 'bad', expected_file: 'wrong' }));
  assert.equal(await readFile(join(root, 'code.sh'), 'utf8'), 'after');
  const saved = await rootIdentity(root);
  await rename(root, join(base, 'old')); await mkdir(root);
  await writeFile(join(root, 'code.sh'), 'replacement');
  await assert.rejects(checkRoot(root, saved), /identity changed/);
  await assert.rejects(fileAction(root, 'read', 'code.sh', undefined, undefined, saved));
});

test('provenance never exempts ignored parents and never adopts a later unrelated file state', async t => {
  const { root } = await setup(t); await mkdir(join(root, 'generated'));
  const identity = await rootIdentity(root);
  const result = await fileAction(root, 'write', 'generated/new.txt', { content: 'known' }, undefined, identity);
  let restored = await restoreOwned(root, await dirtyPaths(root), [result.postimage], identity);
  assert.equal(restored.size, 1);
  await writeFile(join(root, '.gitignore'), 'generated/\n');
  restored = await restoreOwned(root, await dirtyPaths(root), [result.postimage], identity);
  assert.equal(restored.size, 0);
  await rm(join(root, '.gitignore'));
  await writeFile(join(root, 'generated/new.txt'), 'unknown later state');
  restored = await restoreOwned(root, await dirtyPaths(root), [result.postimage], identity);
  assert.equal(restored.size, 0);
});

test('checkout writer lease excludes different hosts and releases on close', async t => {
  const { root } = await setup(t);
  const first = await acquireCheckout(root);
  await assert.rejects(acquireCheckout(root), /already has a host writer/);
  await first.close();
  const second = await acquireCheckout(root); await second.close();
});

test('Git dirty baseline includes staged, untracked, rename source without running filters', async t => {
  const { root, base } = await setup(t);
  await writeFile(join(root, 'original.txt'), 'before');
  await exec('/usr/bin/git', ['-C', root, 'add', '.']);
  await exec('/usr/bin/git', ['-C', root, '-c', 'user.name=Test', '-c', 'user.email=test@invalid', 'commit', '-qm', 'fixture']);
  await exec('/usr/bin/git', ['-C', root, 'mv', 'original.txt', 'renamed.txt']);
  await writeFile(join(root, 'untracked.txt'), 'keep');
  await writeFile(join(root, '.gitattributes'), '*.txt filter=evil\n');
  await writeFile(join(root, '.gitignore'), '.env\n'); await writeFile(join(root, '.env'), 'private');
  await exec('/usr/bin/git', ['-C', root, 'config', 'filter.evil.clean', `touch '${join(base, 'BAD')}'`]);
  const dirty = await dirtyPaths(root);
  assert(dirty.includes('original.txt')); assert(dirty.includes('renamed.txt')); assert(dirty.includes('untracked.txt')); assert(dirty.includes('.env'));
  await assert.rejects(readFile(join(base, 'BAD')));
});
