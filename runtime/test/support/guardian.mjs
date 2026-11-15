import { writeFileSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { join } from 'node:path';

writeFileSync(join(process.argv[3], 'started'), String(process.pid));
switch (process.argv[5]) {
  case 'output':
    process.stdout.write('x'.repeat(65536));
    process.stdout.write('FINAL_OUTPUT');
    break;
  case 'failure':
    process.stderr.write('FIXTURE_FAILURE');
    process.exitCode = 3;
    break;
  case 'wait':
    process.stdout.write('READY');
    process.stdin.resume();
    process.stdin.once('end', () => {
      process.stdout.write('EOF_SEEN');
      process.exitCode = 1;
    });
    break;
  case 'detached': {
    const child = spawn(process.execPath, ['-e', 'setTimeout(() => {}, 10000)'], {
      detached: true, stdio: ['ignore', 'inherit', 'inherit'],
    });
    child.unref();
    process.stdout.write(`DETACHED ${child.pid}`);
    break;
  }
  default:
    throw new Error('Unknown test guardian mode');
}
