import { writeFileSync, existsSync } from 'node:fs';
import { setTimeout as delay } from 'node:timers/promises';
import { spawn } from 'node:child_process';
import { join } from 'node:path';

writeFileSync(join(process.argv[3], 'started'), String(process.pid));
switch (process.argv[5]) {
  case 'output':
    process.stdout.write('x'.repeat(65536));
    process.stdout.write('FINAL_OUTPUT');
    break;
  case 'unicode': {
    process.stdout.write(Buffer.concat([Buffer.from('STDOUT_READY'), Buffer.from([0xe2])]));
    process.stderr.write(Buffer.concat([Buffer.from('STDERR_READY'), Buffer.from([0xf0, 0x9f])]));
    // A parent acknowledgement separates these writes without a timing assumption.
    const deadline = Date.now() + 3000;
    while (!['STDOUT_READY', 'STDERR_READY'].every(name => existsSync(join(process.argv[3], name)))) {
      if (Date.now() >= deadline) throw new Error('Fixture output was not acknowledged');
      await delay(5);
    }
    process.stdout.write(Buffer.from([0x82, 0xac]));
    process.stderr.write(Buffer.from([0x9a, 0x80]));
    break;
  }
  case 'unicode-bounds':
    process.stdout.write('🚀' + 'a'.repeat(12286) + '🚀' + 'b'.repeat(4095));
    break;
  case 'incomplete-utf8':
    process.stdout.write(Buffer.from([0xe2, 0x82]));
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
