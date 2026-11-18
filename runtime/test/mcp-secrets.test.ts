import assert from 'node:assert/strict';
import { test } from 'node:test';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { Secrets, expand } from '../src/mcp/secrets.ts';

test('MCP variable templates match pinned Pi without shell execution', async () => {
  const entry = fileURLToPath(import.meta.resolve('@earendil-works/pi-coding-agent'));
  const pi = await import(pathToFileURL(join(dirname(entry), 'core/resolve-config-value.js')).href);
  const env = { MCP_FIXTURE: 'fixture-value' };
  for (const value of ['literal', '${MCP_FIXTURE}', '$MCP_FIXTURE', 'Bearer ${MCP_FIXTURE}', '$${MCP_FIXTURE}', '$!secret', '${not-valid}', '${unclosed']) {
    assert.equal(expand(value, env), pi.resolveConfigValue(value, env), value);
  }
  assert.throws(() => expand('${MCP_MISSING_FIXTURE}', {}), /unavailable/);
});

test('MCP redaction handles nested JSON, escaped strings, headers and URL encoding', () => {
  const secrets = new Secrets(); secrets.add('Bearer fixture-token'); secrets.add('quoted"secret\nline');
  const value = { values: ['fixture-token', 'quoted"secret\nline', 'quoted%22secret%0Aline'], count: 3, flag: true };
  const clean = secrets.json(value);
  assert.doesNotMatch(JSON.stringify(clean), /fixture-token|quoted/);
  assert.equal(clean.count, 3); assert.equal(clean.flag, true);
  assert.throws(() => secrets.schema({ type: 'object', properties: { 'fixture-token': { type: 'string' } } }));
  const schema = { type: 'object', properties: { run: { type: 'string', description: 'fixture-token' } }, required: ['run'] };
  assert.deepEqual(secrets.schema(schema), { ...schema, properties: { run: { type: 'string', description: '[redacted]' } } });
  secrets.add('1'); secrets.add('runner'); secrets.add('Bearer tiny');
  assert.equal(secrets.text('Found 12 results by runner with tiny'), 'Found 12 results by runner with tiny');
  assert.deepEqual(secrets.json({ 'fixture-token': 'fixture-token' }), { 'fixture-token': '[redacted]' }, 'Argument keys are never rewritten');
});

test('untrusted JSON has a depth bound without recursion or whole-list serialization', async () => {
  const { boundJson } = await import('../src/mcp/json.ts');
  const secrets = new Secrets(); const deep = JSON.parse('['.repeat(4000) + '0' + ']'.repeat(4000));
  for (const inspect of [() => secrets.json(deep), () => secrets.schema(deep)]) assert.throws(inspect, /64-level depth limit/);
  let serialized = false; const page = { tools: [], large: 'x'.repeat(2 * 1024 * 1024) };
  Object.defineProperty(page, 'toJSON', { value: () => { serialized = true; throw new Error('must not serialize'); } });
  assert.throws(() => boundJson(page, 1024 * 1024), /byte limit/); assert.equal(serialized, false);
  assert.throws(() => expand('${toString}', {}), /unavailable/);
  const own = JSON.parse('{"__proto__":"kept","constructor":"kept"}'); assert.deepEqual(secrets.json(own), own);
});

test('pipe budget limits message bursts and bytes before parsing and replenishes at a fixed rate', async () => {
  const { PipeBudget, MCP_PIPE_BYTES, MCP_PIPE_MESSAGES } = await import('../src/mcp/pipe.ts');
  const messages = new PipeBudget(); const now = performance.now();
  assert.equal(messages.accept(Buffer.from('\n'.repeat(MCP_PIPE_MESSAGES)), true, now), true);
  assert.equal(messages.accept(Buffer.from('\n'), true, now), false);
  assert.equal(messages.accept(Buffer.from('\n'), true, now + 1000), true);
  const bytes = new PipeBudget(); assert.equal(bytes.accept(Buffer.alloc(MCP_PIPE_BYTES), false, now), true);
  assert.equal(bytes.accept(Buffer.alloc(1), false, now), false);
});
