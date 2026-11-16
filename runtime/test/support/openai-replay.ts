import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';

export const replayKey = 'PLAIT_CARGO_FIXTURE_NOT_A_SECRET';
export const replayModel = 'plait-cargo-fixture';
type Answer = { text: string } | { tool: { id: string; name: string; input: Record<string, unknown> } };
type Captured = { key?: string; body: Record<string, unknown> };

// Use the public chat-completions wire protocol so tests exercise Pi's real decoder.
export async function replayServer(answers: Answer[]) {
  const requests: Captured[] = [];
  const server = createServer((request, response) => {
    void (async () => {
      let body = '';
      for await (const chunk of request) {
        body += chunk.toString();
        if (Buffer.byteLength(body) > 1024 * 1024) throw new Error('Fixture input exceeds its bound.');
      }
      if (request.method !== 'POST' || request.url !== '/v1/chat/completions'
          || request.headers.authorization !== `Bearer ${replayKey}`) {
        response.writeHead(400).end('Unexpected fixture route or key.'); return;
      }
      requests.push({ key: request.headers.authorization, body: JSON.parse(body) });
      const answer = answers[requests.length - 1];
      if (!answer) { response.writeHead(400).end('Fixture has no further answers.'); return; }
      response.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache' });
      const event = (delta: object, finish_reason: string | null = null) => {
        response.write(`data: ${JSON.stringify({
          id: `fixture_${requests.length}`, object: 'chat.completion.chunk', created: 1, model: replayModel,
          choices: [{ index: 0, delta, finish_reason }],
        })}\n\n`);
      };
      event({ role: 'assistant' });
      if ('text' in answer) event({ content: answer.text });
      else event({ tool_calls: [{ index: 0, id: answer.tool.id, type: 'function', function: {
        name: answer.tool.name, arguments: JSON.stringify(answer.tool.input),
      } }] });
      event({}, 'text' in answer ? 'stop' : 'tool_calls');
      response.end('data: [DONE]\n\n');
    })().catch(() => { response.writeHead(500).end('Invalid fixture input.'); });
  });
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject); server.listen(0, '127.0.0.1', resolve);
  });
  return {
    baseUrl: `http://127.0.0.1:${(server.address() as AddressInfo).port}/v1`, requests,
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
    },
  };
}
