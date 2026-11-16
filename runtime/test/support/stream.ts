import { connect } from 'node:net';
import { join } from 'node:path';
import { request } from './host.ts';

/** One version 3 request and its reply, as wire objects. */
export async function v3<T = Record<string, unknown>>(store: string, value: Record<string, unknown>) {
  return await request(store, { ...value, protocol_version: 3 }) as unknown as { ok: boolean; result: T; error?: { code: string; message: string } };
}

/** A subscription connection: every frame the host sends, in order. */
export function stream<T = Record<string, unknown>>(store: string, value: unknown) {
  const socket = connect(join(store, 'host.sock'));
  const frames: T[] = [];
  let pending = '';
  let closed = false;
  socket.setEncoding('utf8');
  socket.once('connect', () => socket.write(JSON.stringify(value) + '\n'));
  socket.on('error', () => { closed = true; });
  socket.on('close', () => { closed = true; });
  socket.on('data', data => {
    pending += data.toString(); let end: number;
    while ((end = pending.indexOf('\n')) >= 0) {
      frames.push(JSON.parse(pending.slice(0, end))); pending = pending.slice(end + 1);
    }
  });
  return { socket, frames, closed: () => closed };
}
