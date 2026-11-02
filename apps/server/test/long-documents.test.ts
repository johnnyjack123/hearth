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

    // A wrong name says which documents there are.
    script = [{ tool: 'read_document', input: { file: 'nichts.pdf' } }, { text: 'Ok.' }];
    await chat(id, [{ type: 'text', text: 'Und sonst?' }]);
    expect(JSON.stringify(llm.calls.at(-1)?.prompt)).toContain(`Its documents: \\"${ref}\\" (handbuch.md)`);
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
