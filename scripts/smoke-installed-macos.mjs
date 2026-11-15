import assert from 'node:assert/strict';
import { execFile, spawn } from 'node:child_process';
import { mkdtemp, readFile, writeFile, symlink, rm } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { promisify } from 'node:util';
import { test } from 'node:test';

if (process.platform !== 'darwin' || process.env.GITHUB_ACTIONS !== 'true') {
  throw new Error('Installed-package smoke tests run only on disposable GitHub macOS runners.');
}
const exec = promisify(execFile);
const repository = resolve(import.meta.dirname, '..');
const release = JSON.parse(await readFile(process.env.PLAIT_RELEASE_MANIFEST, 'utf8'));
assert.match(release.host.build_id, /^[0-9]+\.[0-9]+\.[0-9]+-[0-9a-f]{20}$/);
const installed = join('/Library/Application Support/Plait/Host/releases', release.host.build_id);
const binary = join(installed, 'bin/plait-host');
const node = join(installed, 'bin/node');
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const support = await import(pathToFileURL(join(repository, 'runtime/test/support/host.ts')));
const { until, request } = support;

// The fixture imports installed runtime code and dependencies, not checkout code.
const fixtureRoot = await mkdtemp('/tmp/plait-installed-fixture-');
await symlink(join(installed, 'runtime/node_modules'), join(fixtureRoot, 'node_modules'));
let fixture = await readFile(join(repository, 'runtime/test/fixture.ts'), 'utf8');
fixture = fixture.replaceAll("'../src/", "'" + pathToFileURL(join(installed, 'runtime/src')).href + '/')
  .replace('sleep 30 & wait', 'sleep 300 & wait');
const fixturePath = join(fixtureRoot, 'fixture.ts');
await writeFile(fixturePath, fixture);
process.env.PLAIT_HOST_BINARY = binary;
process.env.PLAIT_TEST_RUNTIME = fixturePath;

// This module was loaded before the fixture override; start the full suite in a child.
test('installed production host starts without developer Node or source runtime paths', { timeout: 30000 }, async t => {
  const base = await mkdtemp('/tmp/plait-installed-host-');
  const store = join(base, 'store');
  const host = spawn(binary, ['run', '--store', store], {
    stdio: 'ignore', env: { ...process.env, PATH: '/usr/bin:/bin:/usr/sbin:/sbin', NODE_OPTIONS: '--require=/must-not-load', NODE_PATH: '/must-not-load' },
  });
  t.after(async () => { host.kill('SIGTERM'); await delay(500); await rm(base, { recursive: true, force: true }); });
  const status = await until(() => request(store, { op: 'status' }), value => value.ok);
  assert.equal(status.result.protocol_version, 1);
  assert.equal((await exec(node, ['--version'])).stdout.trim(), 'v' + release.host.node_version);
  const version = JSON.parse((await exec(binary, ['version'])).stdout);
  assert.equal(version.protocol_version, 1);
  assert.equal(version.version, release.host.version);
  assert(version.revision.startsWith(release.revision));
});

test('installed runtime passes the full macOS safety suite', { timeout: 240000 }, async () => {
  await new Promise((resolve, reject) => {
    const child = spawn(node, ['test/run.mjs', '--require-command-sandbox'], {
      cwd: join(repository, 'runtime'), stdio: 'inherit', env: process.env,
    });
    child.once('error', reject);
    child.once('exit', code => code === 0 ? resolve() : reject(new Error(`Installed runtime suite failed: ${code}`)));
  });
});

test('client update and staged host upgrade preserve an active command and durable state', { timeout: 240000 }, async () => {
  // Import in a child so the shared test support reads the installed overrides once.
  const program = `
    import assert from 'node:assert/strict';
    import { execFile, spawn } from 'node:child_process';
    import { readFile } from 'node:fs/promises';
    import { promisify } from 'node:util';
    import { setup, until, request, delay } from ${JSON.stringify(pathToFileURL(join(repository, 'runtime/test/support/host.ts')).href)};
    const cleanup=[];
    try {
      const f=await setup({after: fn=>cleanup.push(fn)}, 'crash');
      const thread=await f.create(); await f.submit(thread.id);
      const pending=await until(()=>f.watch(thread.id), value=>!!value.pending_approval);
      await f.approve(thread.id,pending.pending_approval.id);
      const running=await until(()=>f.watch(thread.id), value=>value.tools.some(tool=>/READY \\d+/.test(tool.output)));
      const oldManifest=await readFile(${JSON.stringify(join(installed, 'component.json'))},'utf8');
      for (const [index,path] of JSON.parse(process.env.PLAIT_UPDATE_PACKAGES).entries()) {
        const install=()=>promisify(execFile)('/usr/bin/sudo',['-n','/usr/sbin/installer','-pkg',path,'-target','/'],{timeout:90000});
        if(index===1) await assert.rejects(install); else await install();
        assert.equal(await readFile(${JSON.stringify(join(installed, 'component.json'))},'utf8'),oldManifest);
        process.kill(f.host.pid,0);
        const current=await f.watch(thread.id);
        assert.equal(current.status,'running');
        assert.deepEqual(current.tools.map(tool=>tool.id),running.tools.map(tool=>tool.id));
      }
      await request(f.store,{op:'stop',thread_id:thread.id});
      const stopped=await f.watch(thread.id); assert.equal(stopped.status,'stopped');
      f.host.kill('SIGTERM'); await until(async()=>f.host.signalCode!==null,value=>value); await delay(500);
      const next=JSON.parse(await readFile('/Library/Application Support/Plait/Host/installed.json','utf8'));
      assert.equal(next.fixture,'disposable upgrade simulation, not a release');
      assert.notEqual(next.build_id,JSON.parse(oldManifest).build_id);
      const upgraded=spawn('/Library/Application Support/Plait/Host/releases/'+next.build_id+'/bin/plait-host',['run','--store',f.store],{stdio:'ignore'});
      cleanup.push(async()=>{upgraded.kill('SIGTERM'); await delay(500);});
      const restored=await until(()=>f.watch(thread.id),value=>!!value);
      assert.equal(restored.status,'stopped');
      assert.equal(restored.pending_approval,null);
      assert.deepEqual(restored.tools.map(tool=>tool.id),running.tools.map(tool=>tool.id));
      assert.deepEqual(restored.messages.map(message=>message.id),stopped.messages.map(message=>message.id));
    } finally { for (const fn of cleanup.reverse()) await fn(); }
  `;
  const packages = ['Client', 'Host'].map(component => join(repository, 'dist', release.artifacts.find(item => item.name.startsWith(`Plait-${component}-`)).name));
  packages.push(join(repository, 'dist/installer-test/host-update.pkg'));
  await new Promise((resolve, reject) => {
    const child = spawn(node, ['--input-type=module', '-e', program], {
      cwd: join(repository, 'runtime'), stdio: 'inherit',
      env: { ...process.env, PLAIT_UPDATE_PACKAGES: JSON.stringify(packages) },
    });
    child.once('error', reject);
    child.once('exit', code => code === 0 ? resolve() : reject(new Error(`Independent reinstall failed: ${code}`)));
  });
});

process.once('beforeExit', async () => { await rm(fixtureRoot, { recursive: true, force: true }); });
