import { execFile, spawn } from 'node:child_process';
import { mkdtemp, mkdir, readFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { promisify } from 'node:util';

if (process.platform !== 'darwin' || process.env.GITHUB_ACTIONS !== 'true') {
  throw new Error('Startup diagnostics require a disposable Mac CI runner.');
}
const exec = promisify(execFile);
const metadata = JSON.parse(await readFile(process.env.PLAIT_RELEASE_MANIFEST, 'utf8'));
const installed = join('/Library/Application Support/Plait/Host/releases', metadata.host.build_id);
try {
  const { stdout } = await exec('/bin/launchctl', ['print', `gui/${process.getuid()}/dev.plait.host`]);
  console.log(stdout.split('\n').filter(line => /^\s*(state|pid|program|path|last exit code|username|group|uid) =/.test(line)).join('\n'));
  const pid = stdout.match(/\bpid\s*=\s*(\d+)/)?.[1];
  if (pid) console.log((await exec('/bin/ps', ['-ww', '-p', pid, '-o', 'uid=,pid=,ppid=,command='])).stdout);
} catch (error) { console.error('Managed service inspection:', error.message); }

// No real home, credentials, inherited provider keys, or model request is used.
const fixture = await mkdtemp('/tmp/plait-startup-diagnostic-');
const home = join(fixture, 'home'); const store = join(fixture, 'store');
await mkdir(home, { mode: 0o700 }); await mkdir(store, { mode: 0o700 });
try {
  const child = spawn(join(installed, 'bin/node'), ['--disable-warning=ExperimentalWarning', join(installed, 'runtime/src/main.ts'), store], {
    cwd: '/', env: { HOME: home, PATH: '/usr/bin:/bin:/usr/sbin:/sbin', PLAIT_HOST_EXE: join(installed, 'bin/plait-host') },
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  let output = ''; let errors = '';
  child.stdout.on('data', chunk => { if (output.length < 1048576) output += chunk.toString(); });
  child.stderr.on('data', chunk => { errors = (errors + chunk.toString()).slice(-8192); });
  const deadline = setTimeout(() => child.kill('SIGKILL'), 15000);
  child.stdin.end('{"op":"status"}\n');
  const code = await new Promise((resolve, reject) => { child.once('exit', resolve); child.once('error', reject); });
  clearTimeout(deadline);
  let status; try { const reply = JSON.parse(output); status = { ok: reply.ok, protocol: reply.result?.protocol_version, models: reply.result?.models?.length }; } catch { status = 'no status JSON'; }
  console.log('EMPTY_ACCOUNT_STARTUP_DIAGNOSTIC', { code, status, stderr: errors });
} finally { await rm(fixture, { recursive: true, force: true }); }
