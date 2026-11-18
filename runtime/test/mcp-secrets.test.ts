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
    assert.equal(expand(value, env, new Secrets()), pi.resolveConfigValue(value, env), value);
  }
  assert.throws(() => expand('${MCP_MISSING_FIXTURE}', {}, new Secrets()), /unavailable/);
});

test('MCP redaction handles nested JSON, escaped strings, headers and URL encoding', () => {
  const secrets = new Secrets(); secrets.add('Bearer fixture-token'); secrets.add('quoted"secret\nline');
  const value = { 'fixture-token': ['fixture-token', 'quoted"secret\nline', 'quoted%22secret%0Aline'], count: 3, flag: true };
  const clean = secrets.json(value);
  assert.doesNotMatch(JSON.stringify(clean), /fixture-token|quoted/);
  assert.equal(clean.count, 3); assert.equal(clean.flag, true);
});
