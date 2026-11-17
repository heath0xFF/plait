import { lstat, mkdir, readlink, readdir, realpath, symlink } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fail } from '../protocol.ts';
import { fileAction, sharedGitDir } from '../safety.ts';
import { prepareBuildCache } from '../build-cache.ts';
import type { BuildCache } from '../build-cache.ts';
import { appleToolchain, DEVELOPER, SDK } from '../apple-toolchain.ts';

export function sandboxPolicy(root: string, scratch: string, readOnly: string[] = [], cacheRoots: string[] = []) {
  const quote = (value: string) => JSON.stringify(value);
  return `(version 1) (deny default file-link)
    (allow process-exec process-fork sysctl-read)
    (allow signal (target same-sandbox))
    (allow network-bind network-outbound (subpath ${quote(scratch)}))
    (allow file-read-metadata)
    (allow file-read-data (literal "/"))
    (allow file-read* (subpath ${quote(root)}) (subpath ${quote(scratch)})
      (subpath "/System") (subpath "/usr") (subpath "/bin") (subpath "/sbin")
      (subpath "/Library/Developer") (subpath "/private/var/db")
      (subpath "/Applications/Xcode.app") (subpath "/opt/homebrew")
      ${[...readOnly, ...cacheRoots].map(path => `(subpath ${quote(path)})`).join(' ')}
      (literal "/private/etc/ssl/openssl.cnf")
      (literal "/dev/null") (literal "/dev/urandom") (literal "/dev/random"))
    (allow file-write* (subpath ${quote(scratch)}) (literal "/dev/null")
      ${cacheRoots.map(path => `(subpath ${quote(path)})`).join(' ')})
    ${cacheRoots.length ? `(deny file-write-unlink file-write-mode file-write-owner ${cacheRoots.map(path => `(literal ${quote(path)})`).join(' ')})` : ''}
    (deny file-write* (require-all (regex #"(^|/)[.][gG][iI][tT](/|$)") (require-not (subpath ${quote(scratch)}))))`;
}
async function buildEnvironment(root: string, scratch: string, signal: AbortSignal, cache?: BuildCache, readable: string[] = []) {
  const home = process.env.HOME ?? '/nonexistent';
  if (cache && (cache.store === root || cache.store.startsWith(root + '/'))) fail('permission_denied', 'Build cache store must be outside the checkout.');
  const paths = cache ? await prepareBuildCache(cache) : { cargo: join(scratch, 'cargo'), target: join(scratch, 'target') };
  const cacheRoots = cache ? [paths.cargo, paths.target] : [];
  if (cacheRoots.some(path => root === path || root.startsWith(path + '/'))) fail('permission_denied', 'Checkout must be outside the build cache.');
  const cargoHome = paths.cargo;
  if (!cache) await mkdir(cargoHome);
  // Skill folders, already resolved: their scripts and references can be read and run, never written.
  const readOnly: string[] = [...readable];
  // Git in a worktree reads the main repository's objects; the sandbox still denies writes under .git.
  const shared = await sharedGitDir(root);
  if (shared) readOnly.push(shared);
  for (const cache of ['registry', 'git']) {
    try {
      const path = await realpath(join(home, '.cargo', cache));
      readOnly.push(path);
      const alias = join(cargoHome, cache);
      try { await symlink(path, alias); }
      catch (error) {
        if ((error as { code?: string }).code !== 'EEXIST') throw error;
        if (await readlink(alias) !== path) fail('permission_denied', 'Cargo source-cache link changed.');
      }
    } catch (error) { if ((error as { code?: string }).code !== 'ENOENT') throw error; }
  }
  let channel = '1.98.1';
  try {
    await lstat(join(root, 'rust-toolchain.toml'));
    const { text: config } = await fileAction(root, 'read', 'rust-toolchain.toml');
    const match = config.match(/^\s*channel\s*=\s*"([A-Za-z0-9_.-]+)"/m);
    if (!match) fail('permission_denied', 'Cannot safely select the pinned Rust toolchain.');
    channel = match[1];
  } catch (error) { if ((error as { code?: string }).code !== 'ENOENT') throw error; }
  let toolchainBin = '';
  try {
    const directory = join(home, '.rustup', 'toolchains');
    const installed = (await readdir(directory)).find(name => name === channel || name.startsWith(`${channel}-`));
    if (installed) {
      const path = await realpath(join(directory, installed));
      readOnly.push(path); toolchainBin = join(path, 'bin') + ':';
    }
  } catch (error) { if ((error as { code?: string }).code !== 'ENOENT') throw error; }
  const apple = await appleToolchain(scratch, signal);
  // Installed hosts carry Node outside the developer shell's PATH.
  const node = await realpath(process.execPath);
  return { readOnly: [...readOnly, ...apple.readOnly, node], cacheRoots, env: {
    PATH: apple.bin + dirname(node) + ':' + toolchainBin + `${DEVELOPER}/Toolchains/XcodeDefault.xctoolchain/usr/bin:${DEVELOPER}/usr/bin:/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin:/opt/homebrew/bin`,
    SDKROOT: SDK, DEVELOPER_DIR: DEVELOPER,
    HOME: scratch, TMPDIR: scratch, CARGO_HOME: cargoHome, CARGO_TARGET_DIR: paths.target, CARGO_NET_OFFLINE: 'true', GIT_OPTIONAL_LOCKS: '0', OPENSSL_CONF: '/dev/null',
  } };
}

export async function prepareCommand(root: string, scratch: string, signal: AbortSignal, cache?: BuildCache, readable: string[] = []) {
  const environment = await buildEnvironment(root, scratch, signal, cache, readable);
  return { env: environment.env, policy: sandboxPolicy(root, scratch, environment.readOnly, environment.cacheRoots) };
}
