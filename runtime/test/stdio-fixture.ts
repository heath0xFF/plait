import { serve } from '../src/stdio.ts';
let requests = 0;
await serve({ async reply() {
  requests++;
  if (requests === 2 && process.env.PLAIT_TEST_SCENARIO === 'stall') { setInterval(() => {}, 1000); await new Promise(() => {}); }
  if (requests === 2 && process.env.PLAIT_TEST_SCENARIO === 'oversize') return { ok: true, result: '─'.repeat(500000) };
  return { ok: true, result: { protocol_version: 1, models: [], default_model: null } };
} });
