import { serve } from '../src/stdio.ts';

// This fixture measures transport only. No model, tools, or durable run exists.
let watchRequests = 0;
const snapshots = new Map<string, { id: string; revision: number; changed_ns: string }>();
const listeners = new Set<(id: string) => void>();
const runtime = {
  subscribe(listener: (id: string) => void) {
    listeners.add(listener);
    return () => { listeners.delete(listener); };
  },
  async reply(input: unknown) {
    const request = input as { op: string; thread_id: string };
    if (request.op === 'status') return { ok: true, result: { protocol_version: 1, watch_requests: watchRequests } };
    let snapshot = snapshots.get(request.thread_id);
    if (!snapshot) {
      snapshot = { id: request.thread_id, revision: 0, changed_ns: '0' };
      snapshots.set(request.thread_id, snapshot);
    }
    if (request.op === 'watch') watchRequests++;
    if (request.op === 'submit') {
      snapshot.revision++;
      snapshot.changed_ns = process.hrtime.bigint().toString();
      for (const listener of listeners) listener(snapshot.id);
    }
    return { ok: true, result: { thread: snapshot } };
  },
};
await serve(runtime);
