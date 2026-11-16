import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { performance } from 'node:perf_hooks';
import { exec, request, until } from '../runtime/test/support/host.ts';

const READY_MS = 15000;

async function diagnostic(store) {
  try { return JSON.parse(await readFile(join(store, 'startup.json'), 'utf8')); }
  catch (error) { if (error.code === 'ENOENT') return undefined; throw error; }
}

function exited(pid) {
  try { process.kill(pid, 0); return false; }
  catch (error) { if (error.code === 'ESRCH') return true; throw error; }
}

async function sample(binary, processType, round) {
  const base = await mkdtemp('/tmp/plait-launch-startup-');
  const home = join(base, 'home');
  const store = join(base, 'store');
  const plist = join(base, 'agent.plist');
  const label = `dev.plait.startup-check.${randomUUID()}`;
  const service = `gui/${process.getuid()}/${label}`;
  let loaded = false;
  let startup;
  try {
    await mkdir(home, { mode: 0o700 });
    await writeFile(plist, JSON.stringify({
      Label: label, ProgramArguments: [binary, 'run', '--store', store],
      RunAtLoad: true, KeepAlive: false, ProcessType: processType,
      EnvironmentVariables: {
        HOME: home, PATH: '/usr/bin:/bin:/usr/sbin:/sbin',
        PI_CODING_AGENT_DIR: join(home, '.pi/agent'),
      },
    }), { mode: 0o600 });
    await exec('/usr/bin/plutil', ['-convert', 'xml1', plist], { timeout: 5000 });
    const started = performance.now();
    await exec('/bin/launchctl', ['bootstrap', `gui/${process.getuid()}`, plist], { timeout: READY_MS });
    loaded = true;
    let reply;
    let waitError;
    try {
      reply = await until(() => request(store, { op: 'status' }), value => value.ok,
        Math.max(1, READY_MS - (performance.now() - started)));
    } catch (error) { waitError = error; }
    const elapsed = Math.round(performance.now() - started);
    startup = await diagnostic(store);
    const result = { process_type: processType, round, ready: !!reply?.ok, elapsed_ms: elapsed, startup };
    console.log('ISOLATED_LAUNCH_AGENT_STARTUP', result);
    if (processType === 'Standard') {
      // The shipped policy must pass on its first process, within the existing bound.
      if (waitError) throw waitError;
      assert(elapsed <= READY_MS, `Standard startup exceeded ${READY_MS} ms: ${elapsed}`);
      assert.equal(startup?.phase, 'ready');
    }
    if (reply) {
      assert.equal(reply.result.protocol_version, 1);
      assert(reply.result.models.every(model => model.authenticated === false), 'Startup probe must use an empty account.');
    }
    return result;
  } finally {
    startup ??= await diagnostic(store);
    try { await exec('/bin/launchctl', ['bootout', service], { timeout: 10000 }); }
    catch (error) { if (loaded) throw error; }
    // Only fixture jobs are stopped. Wait for their sidecars before removing stores.
    for (const pid of [startup?.pid, startup?.sidecar_pid].filter(pid => Number.isSafeInteger(pid) && pid > 0)) {
      await until(async () => exited(pid), value => value, 10000);
    }
    await rm(base, { recursive: true, force: true });
  }
}

export async function verifyInstalledStartup(binary) {
  if (process.platform !== 'darwin' || process.env.GITHUB_ACTIONS !== 'true') {
    throw new Error('LaunchAgent startup checks require a disposable Mac CI runner.');
  }
  const { stdout } = await exec('/usr/bin/plutil', ['-convert', 'json', '-o', '-', '/Library/LaunchAgents/dev.plait.host.plist']);
  assert.equal(JSON.parse(stdout).ProcessType, 'Standard');
  const results = [];
  for (let round = 1; round <= 3; round++) {
    // A control measures the retired policy; it is not a passing acceptance case.
    if (process.env.PLAIT_COMPARE_STARTUP_POLICIES === '1' && round % 2 === 1) {
      results.push(await sample(binary, 'Background', round));
    }
    results.push(await sample(binary, 'Standard', round));
    if (process.env.PLAIT_COMPARE_STARTUP_POLICIES === '1' && round % 2 === 0) {
      results.push(await sample(binary, 'Background', round));
    }
  }
  console.log('INSTALLED_STARTUP_SAMPLES', results.map(({ startup, ...result }) => result));
}
