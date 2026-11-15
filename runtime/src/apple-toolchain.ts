import { execFile } from 'node:child_process';
import { lstat, mkdir, realpath, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { promisify } from 'node:util';
import { fail } from './protocol.ts';

const execute = promisify(execFile);
export const DEVELOPER = '/Applications/Xcode.app/Contents/Developer';
export const SDK = `${DEVELOPER}/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk`;
const shellQuote = (text: string) => `'${text.replaceAll("'", "'\\''")}'`;

export async function appleToolchain(scratch: string, signal: AbortSignal) {
  const tools: string[] = []; const roots = new Set<string>();
  for (const name of ['metal', 'metallib']) {
    let output: string;
    try {
      // Resolve only fixed installed tools, never arguments from an approved command.
      output = (await execute('/usr/bin/xcrun', ['--sdk', SDK, '--find', name], {
        env: { PATH: '/usr/bin:/bin', DEVELOPER_DIR: DEVELOPER, HOME: scratch, TMPDIR: scratch },
        timeout: 10000, maxBuffer: 4096, signal,
      })).stdout.trim();
    } catch { return { bin: '', readOnly: [] as string[] }; }
    const path = await realpath(output); const root = dirname(dirname(dirname(path)));
    const installed = root === `${DEVELOPER}/Toolchains/XcodeDefault.xctoolchain`
      || /^\/private\/var\/run\/com\.apple\.security\.cryptexd\/mnt\/com\.apple\.MobileAsset\.MetalToolchain-[A-Za-z0-9.-]+\/Metal\.xctoolchain$/.test(root);
    const info = await lstat(path); const directory = await lstat(root);
    const executable = path === join(root, 'usr', 'bin', name)
      || (name === 'metallib' && path === join(root, 'usr', 'bin', 'air-lld'));
    if (!installed || !executable || !info.isFile() || info.uid !== 0 || directory.uid !== 0 || ((info.mode | directory.mode) & 0o022) !== 0) {
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
