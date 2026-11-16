import { createServer } from 'node:http';
import type { ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';

export const replayKey = 'PLAIT_REPLAY_FIXTURE_NOT_A_SECRET';
export const replayModel = 'plait-replay-fixture';
type Response = { text: string } | { tool: { id: string; name: string; input: Record<string, unknown> } };
export type ReplayRequest = { method?: string; path?: string; key?: string | string[]; body: Record<string, unknown> };

function event(response: ServerResponse, type: string, fields: Record<string, unknown> = {}) {
  response.write(`event: ${type}\ndata: ${JSON.stringify({ type, ...fields })}\n\n`);
}
function stream(response: ServerResponse, answer: Response, index: number) {
  response.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache' });
  event(response, 'message_start', { message: {
    id: `msg_fixture_${index}`, type: 'message', role: 'assistant', model: replayModel,
    content: [], stop_reason: null, stop_sequence: null, usage: { input_tokens: 10, output_tokens: 0 },
  } });
  if ('text' in answer) {
    event(response, 'content_block_start', { index: 0, content_block: { type: 'text', text: '' } });
    event(response, 'content_block_delta', { index: 0, delta: { type: 'text_delta', text: answer.text } });
  } else {
    event(response, 'content_block_start', { index: 0, content_block: { type: 'tool_use', ...answer.tool, input: {} } });
    event(response, 'content_block_delta', { index: 0, delta: { type: 'input_json_delta', partial_json: JSON.stringify(answer.tool.input) } });
  }
  event(response, 'content_block_stop', { index: 0 });
  event(response, 'message_delta', { delta: { stop_reason: 'text' in answer ? 'end_turn' : 'tool_use', stop_sequence: null }, usage: { output_tokens: 5 } });
  event(response, 'message_stop');
  response.end();
}

// This loopback fixture exercises the real provider decoder, not a replacement Models object.
export async function replayServer(answers: Response[]) {
  const requests: ReplayRequest[] = [];
  const server = createServer((request, response) => {
    void (async () => {
      let body = '';
      for await (const chunk of request) {
        body += chunk.toString();
        if (body.length > 1024 * 1024) throw new Error('Fixture request exceeded its bound.');
      }
      requests.push({ method: request.method, path: request.url, key: request.headers['x-api-key'], body: JSON.parse(body) });
      const answer = answers[requests.length - 1];
      if (request.method !== 'POST' || request.url?.split('?')[0] !== '/v1/messages' || !answer) {
        response.writeHead(400).end('Unexpected fixture request.');
        return;
      }
      stream(response, answer, requests.length);
    })().catch(() => { response.writeHead(500).end('Invalid fixture request.'); });
  });
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  return {
    baseUrl: `http://127.0.0.1:${(server.address() as AddressInfo).port}`,
    requests,
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
    },
  };
}
