// OpenAI-compatible mock that behaves like a model using a tool, for
// scripts/smoke-connect.sh: a request without a tool result gets a `bash`
// call with MOCK_COMMAND, a request with one gets the answer "fertig". A
// user message that starts with "!" names the command itself, one that
// starts with "?" a URL for `web_fetch` instead.
// Every request body is appended to MOCK_LOG if set.
import fs from 'node:fs';
import http from 'node:http';

const PORT = Number(process.env.MOCK_PORT ?? 8096);
const LOG = process.env.MOCK_LOG;
const COMMAND = process.env.MOCK_COMMAND ?? 'ls';

/** Text of the newest user message; pi may send it as parts. */
function lastUserText(messages) {
  const content = messages.findLast((message) => message.role === 'user')?.content ?? '';
  return typeof content === 'string' ? content : content.map((part) => part.text ?? '').join('');
}

const chunk = (delta, finish = null) =>
  `data: ${JSON.stringify({
    id: 'mock',
    object: 'chat.completion.chunk',
    created: 0,
    model: 'mock-model',
    choices: [{ index: 0, delta, finish_reason: finish }],
  })}\n\n`;

http
  .createServer((req, res) => {
    let body = '';
    req.on('data', (part) => {
      body += part;
    });
    req.on('end', () => {
      if (req.method === 'GET' && req.url.endsWith('/models')) {
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ object: 'list', data: [{ id: 'mock-model', object: 'model' }] }));
        return;
      }
      if (LOG) {
        fs.appendFileSync(LOG, `${body}\n`);
      }
      const request = JSON.parse(body || '{}');
      const messages = request.messages ?? [];
      // Answer once the newest message is a tool result; a new user message
      // starts the next tool call.
      const answered = messages.at(-1)?.role === 'tool';
      res.writeHead(200, { 'content-type': 'text/event-stream' });
      if (answered) {
        res.write(chunk({ role: 'assistant', content: 'fertig' }));
        res.write(chunk({}, 'stop'));
      } else {
        const text = lastUserText(messages).trim();
        const url = /(?:^|\n)\?(\S+)$/.exec(text)?.[1];
        const wanted = /(?:^|\n)!(.+)$/s.exec(text)?.[1];
        const [name, input] = url ? ['web_fetch', { url }] : ['bash', { command: wanted ?? COMMAND }];
        const call = { index: 0, id: `call_${messages.length}`, type: 'function', function: { name, arguments: '' } };
        res.write(chunk({ role: 'assistant', tool_calls: [call] }));
        res.write(chunk({ tool_calls: [{ index: 0, function: { arguments: JSON.stringify(input) } }] }));
        res.write(chunk({}, 'tool_calls'));
      }
      res.end('data: [DONE]\n\n');
    });
  })
  .listen(PORT, '127.0.0.1');
