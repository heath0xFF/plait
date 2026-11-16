import assert from 'node:assert/strict';
import { test } from 'node:test';
import { trustedMetalRoot } from '../src/apple-toolchain.ts';

test('Metal lookup accepts only fixed tools in known installed layouts', () => {
  const developer = '/Applications/Xcode_16.4.app/Contents/Developer';
  const root = developer + '/Toolchains/XcodeDefault.xctoolchain';
  for (const directory of ['usr/bin', 'usr/metal/32023.620/bin', 'usr/metal/current/bin']) {
    assert.equal(trustedMetalRoot(`${root}/${directory}/metal`, 'metal', developer), root);
    assert.equal(trustedMetalRoot(`${root}/${directory}/air-lld`, 'metallib', developer), root);
  }
  const cryptex = '/private/var/run/com.apple.security.cryptexd/mnt/com.apple.MobileAsset.MetalToolchain-v17.0.ABC/Metal.xctoolchain';
  assert.equal(trustedMetalRoot(`${cryptex}/usr/bin/metal`, 'metal', developer), cryptex);
  for (const path of [
    '/tmp/metal', `${root}/usr/bin/clang`, `${root}/usr/metal/../bin/metal`,
    `${root}/other/metal`, `${root}-other/usr/bin/metal`, `${cryptex}/usr/bin/air-lld`,
  ]) assert.equal(trustedMetalRoot(path, 'metal', developer), null, path);
  assert.equal(trustedMetalRoot(`${root}/usr/bin/metal`, 'other', developer), null);
});
