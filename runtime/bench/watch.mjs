import { spawn, execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { once } from 'node:events';
import { mkdtemp, readFile, realpath, rm } from 'node:fs/promises';
import { connect } from 'node:net';
import { platform, arch } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';
import { setTimeout as delay } from 'node:timers/promises';

const root = fileURLToPath(new URL('../../', import.meta.url));
const binary = process.env.PLAIT_HOST_BINARY ?? resolve(root, 'target/release/plait-host');
const fixture = resolve(root, 'runtime/test/transport-fixture.ts');
const sha256 = async path => createHash('sha256').update(await readFile(resolve(root, path))).digest('hex');
const store = await realpath(await mkdtemp('/tmp/plait-watch-bench-'));
const host = spawn(binary, ['run', '--store', store, '--runtime', fixture], { stdio: ['ignore', 'ignore', 'inherit'] });
const sockets = new Set();
let startupError;
host.once('error', error => { startupError = error; });

function open(request, receive) {
  const socket = connect(join(store, 'host.sock'));
  sockets.add(socket);
  socket.once('close', () => sockets.delete(socket));
  socket.once('connect', () => socket.write(JSON.stringify(request) + '\n'));
  let pending = '';
  socket.on('data', data => {
    pending += data.toString();
    let end;
    while ((end = pending.indexOf('\n')) >= 0) {
      const frame = pending.slice(0, end); pending = pending.slice(end + 1);
      try { receive(JSON.parse(frame)); } catch (error) { socket.destroy(error); }
    }
  });
  return socket;
}
function request(value) {
  return new Promise((resolve, reject) => {
    const socket = open(value, reply => { socket.destroy(); resolve(reply.result); });
    socket.setTimeout(5000, () => socket.destroy(new Error('Request timeout')));
    socket.once('error', reject);
    socket.once('end', () => reject(new Error('Host closed before reply')));
  });
}
async function until(read, accept, timeout = 5000) {
  const deadline = performance.now() + timeout;
  while (performance.now() < deadline) {
    if (startupError) throw startupError;
    if (host.exitCode !== null || host.signalCode !== null) throw new Error('Host exited');
    try { const value = await read(); if (accept(value)) return value; } catch {}
    await delay(5);
  }
  throw new Error('Probe timed out');
}
function percentile(values, fraction) {
  return [...values].sort((a, b) => a - b)[Math.ceil(values.length * fraction) - 1];
}
async function measure(count) {
  const watchers = [];
  let received = 0;
  let revision = 0;
  const samples = [];
  for (let index = 0; index < count; index++) {
    const socket = open({ op: 'watch', thread_id: `fixture-${index}` }, reply => {
      if (reply.ok) received++;
      if (index === 0 && reply.event === 'snapshot') {
        revision = reply.thread.revision;
        samples.push(Number(process.hrtime.bigint() - BigInt(reply.thread.changed_ns)) / 1e6);
      }
    });
    socket.on('error', error => { startupError = error; });
    watchers.push(socket);
  }
  await until(async () => received, value => value === count);
  const before = await request({ op: 'status' });
  const idleStart = performance.now();
  await delay(2000);
  const after = await request({ op: 'status' });
  const idleMs = performance.now() - idleStart;
  for (let index = 0; index < 40; index++) {
    // Vary the interval so a polling implementation cannot lock to the probe.
    await delay(7 + (index * 37) % 91);
    const changed = await request({ op: 'submit', thread_id: 'fixture-0' });
    await until(async () => revision, value => value === changed.thread.revision);
  }
  for (const socket of watchers) socket.destroy();
  await delay(200);
  return {
    watchers: count, idle_ms: idleMs,
    idle_watch_requests: after.watch_requests - before.watch_requests,
    samples_ms: samples,
    delivery_ms: { p50: percentile(samples, 0.5), p95: percentile(samples, 0.95), max: Math.max(...samples) },
  };
}
try {
  await until(() => request({ op: 'status' }), value => value.protocol_version === 1, 15000);
  const results = [];
  for (const count of [1, 20]) results.push(await measure(count));
  console.log(JSON.stringify({
    kind: 'SCRIPTED_TRANSPORT_PROBE_NOT_ADEMARK', measured_at: new Date().toISOString(),
    commit: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
    dirty: execFileSync('git', ['status', '--porcelain'], { cwd: root, encoding: 'utf8' }).trim().length > 0,
    binary_sha256: await sha256(binary),
    sidecar_sha256: Object.fromEntries(await Promise.all([
      'runtime/test/transport-fixture.ts', 'runtime/src/stdio.ts', 'runtime/src/notifications.ts',
    ].map(async path => [path, await sha256(path)]))),
    platform: platform(), arch: arch(), node: process.version, binary, results,
  }, null, 2));
} finally {
  for (const socket of sockets) socket.destroy();
  if (host.exitCode === null && host.signalCode === null && host.pid) {
    const exited = once(host, 'exit'); host.kill('SIGKILL'); await exited;
  }
  await delay(200);
  await rm(store, { recursive: true, force: true });
}
