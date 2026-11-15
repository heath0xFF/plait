import { execFile } from 'node:child_process';
import { lstat, mkdir, realpath, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { promisify } from 'node:util';
import { fail } from './protocol.ts';

const execute = promisify(execFile);
export const DEVELOPER = '/Applications/Xcode.app/Contents/Developer';
export const SDK = `${DEVELOPER}/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk`;
const shellQuote = (text: string) => `'${text.replaceAll("'", "'\\''")}'`;

export function trustedMetalRoot(path: string, name: string, developer: string): string | null {
  if (name !== 'metal' && name !== 'metallib') return null;
  const filenames = name === 'metal' ? ['metal'] : ['metallib', 'air-lld'];
  const toolchain = `${developer}/Toolchains/XcodeDefault.xctoolchain`;
  if (path.startsWith(toolchain + '/')) {
    const relative = path.slice(toolchain.length + 1);
    // Xcode 16 installs Metal below usr/metal/<version>; newer Xcode uses cryptexes.
    if (filenames.some(file => relative === `usr/bin/${file}`
      || (relative.endsWith('/' + file) && /^usr\/metal\/(?:[0-9]+(?:\.[0-9]+)*|current)\/bin\/[^/]+$/.test(relative)))) return toolchain;
  }
  const root = dirname(dirname(dirname(path)));
  if (/^\/private\/var\/run\/com\.apple\.security\.cryptexd\/mnt\/com\.apple\.MobileAsset\.MetalToolchain-[A-Za-z0-9.-]+\/Metal\.xctoolchain$/.test(root)
    && filenames.some(file => path === join(root, 'usr', 'bin', file))) return root;
  return null;
}

export async function appleToolchain(scratch: string, signal: AbortSignal) {
  let developer: string;
  try { developer = await realpath(DEVELOPER); }
  catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return { bin: '', readOnly: [] as string[] }; throw error; }
  if (!developer.startsWith('/Applications/') || !developer.endsWith('.app/Contents/Developer')) {
    fail('permission_denied', 'Xcode developer directory is not a trusted installed application.');
  }
  // Xcode.app may be a system-managed alias to a versioned Xcode application.
  const tools: string[] = []; const roots = new Set<string>([developer]);
  for (const name of ['metal', 'metallib']) {
    let output: string;
    try {
      // Resolve only fixed installed tools, never arguments from an approved command.
      output = (await execute('/usr/bin/xcrun', ['--sdk', SDK, '--find', name], {
        env: { PATH: '/usr/bin:/bin', DEVELOPER_DIR: DEVELOPER, HOME: scratch, TMPDIR: scratch },
        timeout: 10000, maxBuffer: 4096, signal,
      })).stdout.trim();
    } catch { return { bin: '', readOnly: [] as string[] }; }
    const path = await realpath(output); const root = trustedMetalRoot(path, name, developer);
    if (!root) fail('permission_denied', 'Metal compiler is not in a trusted installed Apple toolchain.');
    const info = await lstat(path); const directory = await lstat(root);
    if (!info.isFile() || info.uid !== 0 || directory.uid !== 0 || ((info.mode | directory.mode) & 0o022) !== 0) {
      fail('permission_denied', 'Metal compiler is not in a trusted installed Apple toolchain.');
    }
    tools.push(path); roots.add(root);
  }
  const bin = join(scratch, 'bin'); await mkdir(bin);
  // Avoid xcodebuild discovery services, not license checks or the command sandbox.
  await writeFile(join(bin, 'xcrun'), `#!/bin/sh
if [ "$#" -ge 3 ] && [ "$1" = "-sdk" ] && { [ "$2" = "$SDKROOT" ] || [ "$2" = "macosx" ]; }; then
  case "$3" in
    metal) shift 3; exec ${shellQuote(tools[0])} "$@" ;;
    metallib) shift 3; exec ${shellQuote(tools[1])} "$@" ;;
  esac
fi
exec /usr/bin/xcrun "$@"
`, { mode: 0o700, flag: 'wx' });
  return { bin: bin + ':', readOnly: [...roots] };
}
