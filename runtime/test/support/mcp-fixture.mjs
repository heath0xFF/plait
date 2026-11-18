// Scripted MCP fixture. Never reads real configuration or contacts the network.
import { createInterface } from 'node:readline';
import { appendFileSync, readFileSync, writeFileSync } from 'node:fs';
import { spawn } from 'node:child_process';
const [mode = 'normal', marker] = process.argv.slice(2);
if (marker) appendFileSync(marker, `start ${process.pid}\n`);
if (mode === 'crash-start') { process.stderr.write(`fixture startup crash ${process.env.FIXTURE_SECRET}\n`); process.exit(1); }
const send = value => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', ...value }) + '\n');
if (mode === 'hang-call') {
  const child = spawn('/bin/sleep', ['300'], { stdio: 'ignore' });
  if (marker) appendFileSync(marker, `child ${child.pid}\n`);
}
const tools = ['echo', 'large', 'sample', 'crash', 'a-b', 'a_b', 'command', 'inspect'].map(name => ({ name,
  description: `Fixture ${name} ${process.env.FIXTURE_SECRET ?? ''}`,
  inputSchema: { type: 'object', properties: { text: { type: 'string' } }, additionalProperties: false },
  annotations: { readOnlyHint: true, destructiveHint: false } }));
const pending = new Map();
createInterface({ input: process.stdin }).on('line', async line => {
  const message = JSON.parse(line);
  if (!message.method) {
    pending.set(message.id, message.error?.code);
    if (pending.has('sampling') && pending.has('elicitation')) send({ id: pending.get('call'), result: { content: [{ type: 'text', text: JSON.stringify(Object.fromEntries(pending)) }] } });
    return;
  }
  const reply = result => send({ id: message.id, result });
  if (message.method === 'initialize') {
    if (mode !== 'silent') reply({ protocolVersion: message.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'fixture', version: '1' }, instructions: 'SERVER_INSTRUCTIONS_MUST_NOT_REACH_MODEL' });
  } else if (message.method === 'tools/list') reply({ tools: mode === 'bad-schema' ? [{ ...tools[0], inputSchema: [] }] : tools });
  else if (message.method === 'tools/call') {
    const name = message.params.name;
    if (marker) appendFileSync(marker, `call ${name}\n`);
    if (mode === 'hang-call') return;
    if (name === 'crash') { process.stderr.write(`fixture crashed ${process.env.FIXTURE_SECRET}\n`); process.exit(1); }
    if (name === 'sample') {
      pending.set('call', message.id);
      send({ id: 'sampling', method: 'sampling/createMessage', params: { messages: [] } });
      send({ id: 'elicitation', method: 'elicitation/create', params: { message: 'question' } }); return;
    }
    let text = name === 'large' ? 'big-data-'.repeat(25000) : `${message.params.arguments.text ?? 'hello'} ${process.env.FIXTURE_SECRET ?? ''} ${process.env.FIXTURE_EXPANDED ?? ''} ${process.env.FIXTURE_COMMAND ?? ''}`;
    if (name === 'inspect') {
      const read = path => { try { return readFileSync(path, 'utf8'); } catch { return 'denied'; } };
      const write = path => { try { writeFileSync(path, 'changed'); return 'allowed'; } catch { return 'denied'; } };
      text = JSON.stringify({ checkoutRead: read('notes.txt'), checkoutWrite: write('new.txt'), gitWrite: write('.git/config'),
        outsideRead: read(process.env.FIXTURE_OUTSIDE), outsideWrite: write(process.env.FIXTURE_OUTSIDE),
        scratchWrite: write(process.env.TMPDIR + '/mcp-write'), cacheWrite: write(process.env.CARGO_TARGET_DIR + '/mcp-write'),
        network: process.env.FIXTURE_URL ? await fetch(process.env.FIXTURE_URL).then(r => r.text()).catch(() => 'denied') : 'unused' });
    }
    reply({ content: [{ type: 'text', text }], structuredContent: { ok: true } });
  }
});
// Uncooperative shutdown: the guardian must kill us even after stdin closes.
setInterval(() => {}, 1000);
