// OpenAI-compatible mock that behaves like a model using a tool, for
// scripts/smoke-connect.sh: a request without a tool result gets a `bash`
// call with MOCK_COMMAND, a request with one gets the answer "fertig". A
// user message that starts with "!" names the command itself, one that
// starts with "?" a URL for `web_fetch` instead, "?search <terms>" calls
// `web_search`, "?read <terms>" searches and then reads the hit on
// example.org, "@remember <text>" calls `remember`, and "#plan a; b; c"
// calls `progress` with those steps, the
// first one done and the second active. A request without tools (pi
// summarizing its history) gets a
// summary. It also answers like SearXNG (`/search?format=json`).
// Every answer ends with a usage that carries a cost, like OpenRouter's.
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

/** What every answer costs, in dollars; the chat should show it as billed. */
const COST = 0.000123;

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
      if (req.method === 'GET' && req.url.startsWith('/search?')) {
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(
          JSON.stringify({
            results: [
              { title: 'Kaffee rösten', url: 'https://example.com/roesten', content: 'Bei etwa 200 °C' },
              { title: 'Beispiel', url: 'https://example.org/', content: 'Eine Beispielseite' },
            ],
          }),
        );
        return;
      }
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
      // A search, then reading the hit on example.org, then the answer.
      const lastUser = messages.findLastIndex((message) => message.role === 'user');
      const results = messages.slice(lastUser).filter((message) => message.role === 'tool').length;
      const reading = /(?:^|\n)\?read (.+)$/.exec(lastUserText(messages).trim())?.[1];
      res.writeHead(200, { 'content-type': 'text/event-stream' });
      if (request.tools?.length && reading && results < 2) {
        const [name, input] = results === 0 ? ['web_search', { query: reading }] : ['web_fetch', { url: 'https://example.org/' }];
        const call = { index: 0, id: `call_${messages.length}`, type: 'function', function: { name, arguments: '' } };
        res.write(chunk({ role: 'assistant', tool_calls: [call] }));
        res.write(chunk({ tool_calls: [{ index: 0, function: { arguments: JSON.stringify(input) } }] }));
        res.write(chunk({}, 'tool_calls'));
      } else if (!request.tools?.length) {
        res.write(chunk({ role: 'assistant', content: '## Ziel\nMock-Projekt\n\n## Stand\nagent.txt angelegt.' }));
        res.write(chunk({}, 'stop'));
      } else if (answered) {
        res.write(chunk({ role: 'assistant', content: 'fertig' }));
        res.write(chunk({}, 'stop'));
      } else {
        const text = lastUserText(messages).trim();
        const query = /(?:^|\n)\?search (.+)$/.exec(text)?.[1];
        const url = /(?:^|\n)\?(\S+)$/.exec(text)?.[1];
        const wanted = /(?:^|\n)!(.+)$/s.exec(text)?.[1];
        const plan = /(?:^|\n)#plan (.+)$/.exec(text)?.[1];
        const memory = /(?:^|\n)@remember (.+)$/.exec(text)?.[1];
        const steps = plan?.split(';').map((step, index) => ({ text: step.trim(), state: index === 0 ? 'done' : index === 1 ? 'active' : 'pending' }));
        const [name, input] = memory
          ? ['remember', { text: memory }]
          : steps
          ? ['progress', { items: steps }]
          : query
            ? ['web_search', { query }]
            : url
              ? ['web_fetch', { url }]
              : ['bash', { command: wanted ?? COMMAND }];
        const call = { index: 0, id: `call_${messages.length}`, type: 'function', function: { name, arguments: '' } };
        res.write(chunk({ role: 'assistant', tool_calls: [call] }));
        res.write(chunk({ tool_calls: [{ index: 0, function: { arguments: JSON.stringify(input) } }] }));
        res.write(chunk({}, 'tool_calls'));
      }
      // Tokens as OpenAI counts them; the server matches the bill to pi's
      // report of the call by these.
      const usage = { prompt_tokens: Math.ceil(body.length / 4), completion_tokens: 7, cost: COST };
      res.write(`data: ${JSON.stringify({ id: 'mock', object: 'chat.completion.chunk', created: 0, model: request.model, choices: [], usage })}\n\n`);
      res.end('data: [DONE]\n\n');
    });
  })
  .listen(PORT, '127.0.0.1');
