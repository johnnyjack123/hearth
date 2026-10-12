// Minimal OpenAI-compatible endpoint for end-to-end tests: lists one model,
// streams a fixed Markdown reply and answers title requests non-streamed.
import http from 'node:http';

const PORT = Number(process.env.MOCK_PORT ?? 8091);
const REPLY = 'Hallo! Ich bin ein **Testmodell** und antworte mit Markdown.\n\n```ts\nconst answer: number = 42;\n```\n\n- Punkt eins\n- Punkt zwei\n';
let requestCount = 0;

const chunk = (delta, finish = null, usage) =>
  `data: ${JSON.stringify({
    id: 'mock',
    object: 'chat.completion.chunk',
    created: 0,
    model: 'mock-model',
    choices: [{ index: 0, delta, finish_reason: finish }],
    ...(usage ? { usage } : {}),
  })}\n\n`;

http
  .createServer((req, res) => {
    let body = '';
    req.on('data', (part) => (body += part));
    req.on('end', async () => {
      if (req.method === 'GET' && req.url.endsWith('/models')) {
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ object: 'list', data: [{ id: 'mock-model', object: 'model' }] }));
        return;
      }
      if (req.method === 'POST' && req.url.endsWith('/chat/completions')) {
        requestCount += 1;
        const payload = JSON.parse(body || '{}');
        console.log(`[mock] #${requestCount} stream=${Boolean(payload.stream)} messages=${payload.messages?.length}`);
        if (!payload.stream) {
          res.writeHead(200, { 'content-type': 'application/json' });
          res.end(
            JSON.stringify({
              id: 'mock',
              object: 'chat.completion',
              created: 0,
              model: 'mock-model',
              choices: [{ index: 0, message: { role: 'assistant', content: 'Testgespräch mit Mock' }, finish_reason: 'stop' }],
              usage: { prompt_tokens: 5, completion_tokens: 3, total_tokens: 8 },
            }),
          );
          return;
        }
        res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' });
        res.write(chunk({ role: 'assistant', content: '' }));
        for (const word of REPLY.split(/(?<= )/)) {
          res.write(chunk({ content: word }));
          await new Promise((resolve) => setTimeout(resolve, 15));
        }
        res.write(chunk({}, 'stop', { prompt_tokens: 12, completion_tokens: 30, total_tokens: 42 }));
        res.end('data: [DONE]\n\n');
        return;
      }
      res.writeHead(404).end();
    });
  })
  .listen(PORT, '127.0.0.1', () => console.log(`[mock] listening on ${PORT}`));
