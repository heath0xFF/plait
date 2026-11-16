import { connect } from 'node:net';
import { join } from 'node:path';
import type { Snapshot } from '../../src/protocol.ts';

type Frame<T> = { ok?: boolean; result?: { thread: T }; event?: string; thread?: T };
export function watch<T = Snapshot>(store: string, id: string) {
  const socket = connect(join(store, 'host.sock'));
  const frames: Frame<T>[] = [];
  let pending = '';
  let error: Error | undefined;
  socket.once('connect', () => socket.write(JSON.stringify({ op: 'watch', thread_id: id }) + '\n'));
  socket.on('error', value => { error = value; });
  socket.on('data', data => {
    pending += data.toString(); let end: number;
    while ((end = pending.indexOf('\n')) >= 0) {
      frames.push(JSON.parse(pending.slice(0, end))); pending = pending.slice(end + 1);
    }
  });
  return {
    socket, frames,
    latest() {
      if (error) throw error;
      const last = frames.at(-1);
      return last?.thread ?? last?.result?.thread;
    },
  };
}
