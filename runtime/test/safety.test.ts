import assert from 'node:assert/strict';
import { mkdir, writeFile, readFile, rm, symlink, link, chmod, stat, rename } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileAction, dirtyPaths, identities, safePath, acquireCheckout, rootIdentity, checkRoot } from '../src/safety.ts';
import { prepareBuildCache } from '../src/build-cache.ts';
import { validate } from '../src/protocol.ts';
import type { Snapshot } from '../src/protocol.ts';
import { boundSnapshot } from '../src/transcript.ts';
import { restoreOwned } from '../src/provenance.ts';
import { createHash, randomUUID } from 'node:crypto';
import { exec, setup } from './support/safety.ts';

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

test('read-only originals retain mode and identity on a rejected edit', async t => {
  const { root } = await setup(t);
  const path = join(root, 'readonly.txt');
  await writeFile(path, 'before\n');
  await chmod(path, 0o444);
  const before = await stat(path, { bigint: true });
  const image = (await fileAction(root, 'read', 'readonly.txt')).postimage;
  await assert.rejects(fileAction(root, 'edit', 'readonly.txt', { expected: 'not present', content: 'bad', expected_image: image }), /old_text must match exactly once/);
  assert.equal((await stat(path, { bigint: true })).ctimeNs, before.ctimeNs);
  await fileAction(root, 'edit', 'readonly.txt', { expected: 'before', content: 'after', expected_image: image });
  assert.equal(await readFile(path, 'utf8'), 'after\n');
  assert.equal((await stat(path)).mode & 0o777, 0o444);
});
