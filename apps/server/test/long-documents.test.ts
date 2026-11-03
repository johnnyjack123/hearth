import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import type { ChatFile, ConversationSummary, ProviderCredential } from '@harness/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { createScriptedLlm, createTestServer, ORIGIN, readChunks, type Step, type TestServer } from './helpers.ts';

/** A handbook of ~30,000 characters with one fact far down. */
function handbook(): string {
  const chapters = Array.from({ length: 24 }, (_, index) => {
    const body = `Allgemeiner Text über Kaffee, Kapitel ${index + 1}. `.repeat(28);
    return `## Kapitel ${index + 1}\n\n${body}`;
  });
  chapters.splice(19, 0, '## Röstung\n\nDie Röstung dauert 14 Minuten bei 210 °C. Danach kühlt der Kaffee vier Minuten ab.');
  return `# Handbuch\n\n${chapters.join('\n\n')}`;
}

describe('long documents', () => {
  let server: TestServer;
  let llm: ReturnType<typeof createScriptedLlm>;
  let script: Step[] = [];
  let cookie: string;
  let credential: ProviderCredential;

  beforeEach(async () => {
    llm = createScriptedLlm(() => script);
    server = await createTestServer('sqlite', { llm });
    cookie = await server.signUp('alice@example.com');
    credential = (await (
      await server.request('POST', '/api/providers', { cookie, body: { kind: 'openai', label: 'Work', apiKey: 'sk-test-0123456789abcdef' } })
    ).json()) as ProviderCredential;
  });

  afterEach(async () => {
    await server.close();
  });

  async function chat(conversationId: string, parts: unknown[]) {
    const response = await server.app.request(`/api/conversations/${conversationId}/chat`, {
      method: 'POST',
      headers: { origin: ORIGIN, cookie, 'content-type': 'application/json' },
      body: JSON.stringify({ message: { id: `m-${Math.random().toString(36).slice(2)}`, role: 'user', parts }, credentialId: credential.id, modelId: 'mock-1' }),
    });
    expect(response.status).toBe(200);
    return readChunks(response);
  }

  it('reach the model as start and outline, and it reads the parts it needs', async () => {
    const response = await server.app.request('/api/files', {
      method: 'POST',
      headers: { origin: ORIGIN, cookie, 'content-type': 'application/octet-stream', 'x-file-name': 'handbuch.md' },
      body: new TextEncoder().encode(handbook()),
    });
    const file = (await response.json()) as ChatFile;
    const ref = file.id.slice(-8);
    const { id } = (await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary;

    script = [
      { tool: 'read_document', input: { file: ref, focus: 'Wie lange dauert die Röstung?' } },
      { tool: 'read_document', input: { file: 'handbuch.md', grep: 'Minuten' } },
      { text: '14 Minuten.' },
    ];
    await chat(id, [
      { type: 'text', text: 'Wie lange wird geröstet?' },
      { type: 'file', url: `/api/files/${file.id}`, mediaType: 'text/plain', filename: 'handbuch.md' },
    ]);

    const first = JSON.stringify(llm.calls[0]?.prompt);
    expect(llm.calls[0]?.tools).toContain('read_document');
    // Start and outline instead of the whole text.
    expect(first).toContain(`read_document tool, file \\"${ref}\\"`);
    expect(first).toContain('Röstung');
    expect(first).not.toContain('Die Röstung dauert 14 Minuten');
    expect(first.length).toBeLessThan(12_000);
    // The focus finds the section; grep the line.
    expect(JSON.stringify(llm.calls[1]?.prompt)).toContain('Die Röstung dauert 14 Minuten bei 210 °C');
    expect(JSON.stringify(llm.calls[2]?.prompt)).toContain('Danach kühlt der Kaffee vier Minuten ab.');
    // The first line says how: without an OpenRouter key by keywords only.
    expect(JSON.stringify(llm.calls[1]?.prompt)).toMatch(/sections [\d, ]+ of \d+, chosen by keywords/);
    expect(JSON.stringify(llm.calls[2]?.prompt)).toContain('1 matching lines for');

    // A wrong name says which documents there are.
    script = [{ tool: 'read_document', input: { file: 'nichts.pdf' } }, { text: 'Ok.' }];
    await chat(id, [{ type: 'text', text: 'Und sonst?' }]);
    expect(JSON.stringify(llm.calls.at(-1)?.prompt)).toContain(`Its documents: \\"${ref}\\" (handbuch.md)`);
  });

  it('embeds a document once and searches all of its sections by meaning', async () => {
    // The embedding model: questions and the one passage on storage point
    // one way, everything else the other.
    const requests: string[][] = [];
    const embeddings = createServer((request, response) => {
      let body = '';
      request.on('data', (chunk) => (body += chunk));
      request.on('end', () => {
        response.writeHead(200, { 'content-type': 'application/json' });
        if (!request.url?.endsWith('/embeddings')) {
          response.end(JSON.stringify({ data: [] }));
          return;
        }
        const { input } = JSON.parse(body) as { input: string[] };
        requests.push(input);
        const data = input.map((text, index) => ({ index, embedding: text.startsWith('Instruct:') || text.includes('luftdicht') ? [1, 0] : [0, 1] }));
        response.end(JSON.stringify({ data, usage: { prompt_tokens: input.length * 100, cost: input.length * 0.000001 } }));
      });
    });
    await new Promise<void>((resolve) => embeddings.listen(0, '127.0.0.1', resolve));
    await server.close();
    server = await createTestServer('sqlite', {
      llm,
      helpers: { apiUrl: `http://127.0.0.1:${(embeddings.address() as AddressInfo).port}/api/v1`, timeoutMs: 2_000 },
    });
    cookie = await server.signUp('alice@example.com');
    credential = (await (
      await server.request('POST', '/api/providers', { cookie, body: { kind: 'openai', label: 'Work', apiKey: 'sk-test-0123456789abcdef' } })
    ).json()) as ProviderCredential;
    await server.request('POST', '/api/providers', { cookie, body: { kind: 'openrouter', label: 'OpenRouter', apiKey: 'sk-or-test-0123456789abcdef' } });
    try {
      // 260 chapters on beans, and far down, past the 200 a web page would
      // embed, the answer in other words.
      const chapters = Array.from({ length: 260 }, (_, index) => `## Kapitel ${index + 1}\n\n${'Kaffeebohnen lagert man so oder so. '.repeat(20)}`);
      chapters.splice(240, 0, '## Aufbewahrung\n\nIn einer luftdicht verschlossenen Dose, dunkel und kühl.');
      const response = await server.app.request('/api/files', {
        method: 'POST',
        headers: { origin: ORIGIN, cookie, 'content-type': 'application/octet-stream', 'x-file-name': 'bohnen.md' },
        body: new TextEncoder().encode(chapters.join('\n\n')),
      });
      const file = (await response.json()) as ChatFile;
      const ref = file.id.slice(-8);
      const { id } = (await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary;

      script = [{ tool: 'read_document', input: { file: ref, focus: 'Wie lagert man Kaffeebohnen?' } }, { text: 'Luftdicht.' }];
      const chunks = await chat(id, [
        { type: 'text', text: 'Wie lagere ich Bohnen?' },
        { type: 'file', url: `/api/files/${file.id}`, mediaType: 'text/markdown', filename: 'bohnen.md' },
      ]);
      expect(JSON.stringify(llm.calls[1]?.prompt)).toContain('luftdicht verschlossenen Dose');
      expect(JSON.stringify(llm.calls[1]?.prompt)).toMatch(/sections [\d, ]+ of 261, chosen by meaning and keywords/);
      // Every section once, in batches of 32, and the question on its own.
      const sections = requests.filter((input) => !input[0]?.startsWith('Instruct:')).flat();
      expect(sections).toHaveLength(261);
      expect(requests.filter((input) => input[0]?.startsWith('Instruct: Given a question, retrieve the passages of a document'))).toHaveLength(1);
      expect(chunks.some((chunk) => chunk.type === 'finish')).toBe(true);

      // The next search embeds only its question.
      requests.length = 0;
      script = [{ tool: 'read_document', input: { file: ref, focus: 'Wo bewahre ich sie auf?' } }, { text: 'Dunkel.' }];
      await chat(id, [{ type: 'text', text: 'Und wo?' }]);
      expect(requests).toEqual([[expect.stringMatching(/^Instruct: .*\nQuery: Wo bewahre ich sie auf\?$/)]]);
      expect(JSON.stringify(llm.calls.at(-1)?.prompt)).toContain('luftdicht verschlossenen Dose');

      // Deleting the chat takes the vectors along.
      expect((await server.request('DELETE', `/api/conversations/${id}`, { cookie })).status).toBe(204);
      expect(await server.database.db.selectFrom('fileVectors').selectAll().execute()).toEqual([]);
    } finally {
      await new Promise((resolve) => embeddings.close(resolve));
    }
  });

  it('leaves the tool out of chats with short documents only', async () => {
    const response = await server.app.request('/api/files', {
      method: 'POST',
      headers: { origin: ORIGIN, cookie, 'content-type': 'application/octet-stream', 'x-file-name': 'notiz.txt' },
      body: new TextEncoder().encode('Bohnen kaufen.'),
    });
    const file = (await response.json()) as ChatFile;
    const { id } = (await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary;
    script = [{ text: 'Ok.' }];
    await chat(id, [{ type: 'file', url: `/api/files/${file.id}`, mediaType: 'text/plain', filename: 'notiz.txt' }]);
    expect(llm.calls[0]?.tools).not.toContain('read_document');
    expect(JSON.stringify(llm.calls[0]?.prompt)).toContain('Bohnen kaufen.');
  });
});
