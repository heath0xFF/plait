import type { ChildProcess } from 'node:child_process';
import type { Writable } from 'node:stream';

/** No user-controlled loader variables may affect the unsandboxed guardian. */
export const GUARDIAN_ENV = Object.freeze({ PATH: '/usr/bin:/bin:/usr/sbin:/sbin' });
export const ENVIRONMENT_BYTES = 1024 * 1024;
export function writeEnvironment(child: ChildProcess, env: NodeJS.ProcessEnv) {
  const pipe = (child.stdio as (Writable | null | undefined)[])[6]!;
  pipe.on('error', () => {});
  const data = JSON.stringify(env);
  // The host also enforces this bound before applying any environment.
  pipe.end(Buffer.byteLength(data) <= ENVIRONMENT_BYTES ? data : '{}!');
}
