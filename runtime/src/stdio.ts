import { once } from 'node:events';
import type { Runtime } from './runtime.ts';
import { notificationChannel } from './notifications.ts';
import type { ChangeSource } from './notifications.ts';

export async function serve(runtime: Pick<Runtime, 'reply'> & ChangeSource) {
  const stopNotifications = notificationChannel(runtime);
  let pending = Buffer.alloc(0);
  try {
    for await (const chunk of process.stdin) {
      pending = Buffer.concat([pending, chunk]);
      if (pending.length > 65536) throw new Error('Host frame exceeds limit');
      let end: number;
      while ((end = pending.indexOf(10)) >= 0) {
        const frame = pending.subarray(0, end); pending = pending.subarray(end + 1);
        let reply;
        try { reply = await runtime.reply(JSON.parse(frame.toString('utf8'))); }
        catch { reply = { ok: false, error: { code: 'invalid_request', message: 'Invalid JSON.' } }; }
        let output = JSON.stringify(reply) + '\n';
        if (Buffer.byteLength(output) > 1048576) output = JSON.stringify({ ok: false, error: { code: 'internal', message: 'Response exceeds the safe output bound.' } }) + '\n';
        if (!process.stdout.write(output)) await once(process.stdout, 'drain');
      }
    }
  } finally { stopNotifications(); }
}
